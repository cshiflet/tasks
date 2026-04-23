//! Loopback HTTP receiver for the OAuth2 desktop flow.
//!
//! Companion to [`crate::oauth`]: binds `127.0.0.1:0` (random
//! port), hands the caller the `http://127.0.0.1:<port>/cb`
//! redirect URI to embed in the authorization URL, and then
//! blocks until the browser hits us with the `?code=…&state=…`
//! callback. Responds with a canned HTML "you can close this
//! window" page.
//!
//! Uses `std::net` directly — no tokio, no reqwest — so it
//! drops cleanly into any thread (GUI's or a background worker).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use crate::oauth::{parse_redirect, OAuthError, RedirectParams};

/// Single-use receiver. Call [`bind`] to open the socket, embed
/// [`redirect_uri`] in the authorization URL, then
/// [`wait_for_redirect`] once the user clicks through.
pub struct LoopbackReceiver {
    listener: TcpListener,
    addr: SocketAddr,
}

impl LoopbackReceiver {
    /// Bind to a random high port on the loopback interface.
    /// Returns the bound port for embedding in the redirect URI.
    pub fn bind() -> Result<Self, OAuthError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| OAuthError::Random(format!("bind loopback: {e}")))?;
        let addr = listener
            .local_addr()
            .map_err(|e| OAuthError::Random(format!("local_addr: {e}")))?;
        Ok(Self { listener, addr })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/cb", self.addr.port())
    }

    /// Block until the browser hits us. The first valid HTTP
    /// request whose query contains `state` matching
    /// `expected_state` is parsed and returned; any request we
    /// can't parse (or whose state doesn't match) gets a 400
    /// response and the listener accepts again until `timeout`
    /// elapses.
    ///
    /// Returns `Err(OAuthError::MalformedRedirect)` on timeout.
    pub fn wait_for_redirect(
        self,
        expected_state: &str,
        timeout: Duration,
    ) -> Result<RedirectParams, OAuthError> {
        self.listener
            .set_nonblocking(false)
            .map_err(|e| OAuthError::Random(format!("set_nonblocking: {e}")))?;
        let deadline = std::time::Instant::now() + timeout;

        // We can't use TcpListener::accept_timeout directly; poll
        // set_read_timeout on a peer stream instead. The idiom:
        // set an incoming-accept timeout via try_accept loop with
        // a short set_nonblocking window.
        self.listener
            .set_nonblocking(true)
            .map_err(|e| OAuthError::Random(format!("set_nonblocking: {e}")))?;

        loop {
            if std::time::Instant::now() >= deadline {
                return Err(OAuthError::MalformedRedirect(
                    "loopback receiver timed out".into(),
                ));
            }
            match self.listener.accept() {
                Ok((stream, _peer)) => match handle_stream(stream, expected_state) {
                    Ok(params) => return Ok(params),
                    Err(OAuthError::MalformedRedirect(msg)) => {
                        tracing::debug!("ignoring malformed loopback hit: {msg}");
                        // Keep accepting until timeout.
                        continue;
                    }
                    Err(other) => return Err(other),
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
                Err(e) => {
                    return Err(OAuthError::Random(format!("accept: {e}")));
                }
            }
        }
    }
}

fn handle_stream(
    mut stream: TcpStream,
    expected_state: &str,
) -> Result<RedirectParams, OAuthError> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    // Read enough of the request to get the first line + maybe
    // some headers. Browsers don't send request bodies on the
    // redirect GET, so 8 KiB is plenty.
    let mut buf = [0u8; 8192];
    let mut read_total = 0usize;
    while read_total < buf.len() {
        match stream.read(&mut buf[read_total..]) {
            Ok(0) => break,
            Ok(n) => {
                read_total += n;
                if buf[..read_total].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                return Err(OAuthError::MalformedRedirect(format!("read: {e}")));
            }
        }
    }
    let text = String::from_utf8_lossy(&buf[..read_total]);
    let first_line = text
        .lines()
        .next()
        .ok_or_else(|| OAuthError::MalformedRedirect("empty request".into()))?;

    // "GET /cb?code=…&state=… HTTP/1.1"
    let mut parts = first_line.split_whitespace();
    let _method = parts.next();
    let path = parts
        .next()
        .ok_or_else(|| OAuthError::MalformedRedirect("no path in request line".into()))?;

    let params = parse_redirect(path, expected_state)?;

    // Respond with a friendly page so the user can close the tab.
    let body = concat!(
        "<!doctype html><html><head><meta charset=\"utf-8\">",
        "<title>Signed in</title></head><body style=\"font-family:",
        "-apple-system,Segoe UI,Roboto,sans-serif;padding:2em;",
        "text-align:center\"><h1>You're signed in.</h1>",
        "<p>You can close this tab and return to Tasks.</p>",
        "</body></html>",
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn bind_returns_a_usable_port() {
        let r = LoopbackReceiver::bind().unwrap();
        let uri = r.redirect_uri();
        assert!(uri.starts_with("http://127.0.0.1:"));
        assert!(uri.ends_with("/cb"));
        assert!(r.port() > 1024);
    }

    #[test]
    fn wait_for_redirect_parses_callback() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let port = receiver.port();

        // Client thread: fire the redirect-style GET.
        let handle = std::thread::spawn(move || {
            // Give the receiver a brief moment to start accepting.
            std::thread::sleep(Duration::from_millis(50));
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /cb?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .unwrap();
            // Drain the response so the server's write_all returns.
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
        });

        let params = receiver
            .wait_for_redirect("xyz", Duration::from_secs(2))
            .unwrap();
        assert_eq!(params.code, "abc");
        assert_eq!(params.state, "xyz");
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_redirect_times_out_on_silent_port() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let err = receiver
            .wait_for_redirect("anything", Duration::from_millis(200))
            .unwrap_err();
        assert!(matches!(err, OAuthError::MalformedRedirect(_)));
    }

    #[test]
    fn wait_for_redirect_surfaces_state_mismatch() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let port = receiver.port();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /cb?code=abc&state=wrong HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .unwrap();
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
        });
        let err = receiver
            .wait_for_redirect("expected", Duration::from_secs(2))
            .unwrap_err();
        assert!(matches!(err, OAuthError::StateMismatch { .. }));
        handle.join().unwrap();
    }
}
