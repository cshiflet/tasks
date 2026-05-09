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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::oauth::{parse_redirect, OAuthError, RedirectParams};

/// Single-use receiver. Call [`bind`] to open the socket, embed
/// [`redirect_uri`] in the authorization URL, then
/// [`wait_for_redirect`] once the user clicks through.
pub struct LoopbackReceiver {
    listener: TcpListener,
    addr: SocketAddr,
    /// Host name advertised in [`redirect_uri`] and validated in
    /// the inbound `Host:` header. Two values matter today:
    /// `"127.0.0.1"` (Google's preference — IP literal sidesteps
    /// DNS-rebinding risk) and `"localhost"` (Microsoft requires
    /// it; their redirect-URI matcher treats `localhost` and
    /// `127.0.0.1` as distinct strings even though both resolve
    /// to the same socket). The bind address is always
    /// `127.0.0.1` regardless — the host string only affects
    /// the URL we hand the authorization server.
    redirect_host: String,
    /// Path component of the redirect URI. Google accepts any
    /// loopback path (`/cb` is conventional); Microsoft requires
    /// the path of the *sent* redirect URI to match the path of
    /// the *registered* redirect URI exactly (case-sensitive).
    /// Microsoft's recommended public-client registration is
    /// `http://localhost` with no path, so the loopback receiver
    /// for that provider has to advertise `/` (root) and accept
    /// requests to `/`.
    redirect_path: String,
}

impl LoopbackReceiver {
    /// Bind to a random high port on the loopback interface and
    /// advertise the redirect URI as `http://127.0.0.1:<port>/cb`.
    pub fn bind() -> Result<Self, OAuthError> {
        Self::bind_with_redirect("127.0.0.1", "/cb")
    }

    /// Bind to a random high port on the loopback interface and
    /// advertise the redirect URI with the given host name. The
    /// path stays `/cb`; for providers that need a different path
    /// use [`bind_with_redirect`].
    pub fn bind_with_host(host: &str) -> Result<Self, OAuthError> {
        Self::bind_with_redirect(host, "/cb")
    }

    /// Bind to a random high port on the loopback interface with
    /// fully-configurable host + path components. The path must
    /// start with `/`; use `"/"` for providers (Microsoft) that
    /// require the redirect URI to have no extra path so it
    /// matches a registration of `http://localhost`.
    ///
    /// `host` MUST be one of `127.0.0.1`, `localhost`, `[::1]`,
    /// or `::1` — the kernel always binds the listener to
    /// `127.0.0.1:0` regardless, but the value advertised in
    /// the OAuth `redirect_uri` (and matched against the
    /// inbound `Host:` header) has to stay loopback. A future
    /// caller passing an attacker-supplied hostname here would
    /// otherwise pull the browser's redirect through arbitrary
    /// DNS and accept a `Host:` header naming that host —
    /// turning a same-machine OAuth flow into a cross-host
    /// callback target.
    pub fn bind_with_redirect(host: &str, path: &str) -> Result<Self, OAuthError> {
        if !is_loopback_host(host) {
            return Err(OAuthError::Random(format!(
                "loopback host must be 127.0.0.1, localhost, ::1, or [::1]; got {host:?}"
            )));
        }
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| OAuthError::Random(format!("bind loopback: {e}")))?;
        let addr = listener
            .local_addr()
            .map_err(|e| OAuthError::Random(format!("local_addr: {e}")))?;
        Ok(Self {
            listener,
            addr,
            redirect_host: host.to_string(),
            redirect_path: path,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn redirect_uri(&self) -> String {
        // Strip the trailing `/` when the path is bare `/` so the
        // sent URI is `http://host:port` rather than
        // `http://host:port/`. Microsoft's matcher is happier
        // with the unrooted form for `http://localhost`-style
        // registrations.
        if self.redirect_path == "/" {
            format!("http://{}:{}", self.redirect_host, self.addr.port())
        } else {
            format!(
                "http://{}:{}{}",
                self.redirect_host,
                self.addr.port(),
                self.redirect_path
            )
        }
    }

    /// Block until the browser hits us. The first valid HTTP
    /// request whose query contains `state` matching
    /// `expected_state` is parsed and returned; any request we
    /// can't parse (or whose state doesn't match) gets a 400
    /// response and the listener accepts again until `timeout`
    /// elapses.
    ///
    /// Returns `Err(OAuthError::MalformedRedirect)` on timeout
    /// or cancel.
    ///
    /// `cancel` is checked at the same 25 ms cadence as the
    /// accept poll. Set it from outside (e.g. from a view-model
    /// Drop) so closing the app while a sign-in is in flight
    /// aborts the wait promptly instead of hanging until the
    /// 120 s timeout fires.
    pub fn wait_for_redirect(
        self,
        expected_state: &str,
        timeout: Duration,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<RedirectParams, OAuthError> {
        self.listener
            .set_nonblocking(false)
            .map_err(|e| OAuthError::Random(format!("set_nonblocking: {e}")))?;
        let deadline = std::time::Instant::now() + timeout;
        let bound_port = self.addr.port();
        let expected_host = format!("{}:{}", self.redirect_host, bound_port);
        let expected_path = self.redirect_path.clone();

        // We can't use TcpListener::accept_timeout directly; poll
        // set_read_timeout on a peer stream instead. The idiom:
        // set an incoming-accept timeout via try_accept loop with
        // a short set_nonblocking window.
        self.listener
            .set_nonblocking(true)
            .map_err(|e| OAuthError::Random(format!("set_nonblocking: {e}")))?;

        loop {
            if let Some(c) = cancel.as_ref() {
                if c.load(Ordering::Relaxed) {
                    return Err(OAuthError::MalformedRedirect(
                        "loopback receiver cancelled".into(),
                    ));
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(OAuthError::MalformedRedirect(
                    "loopback receiver timed out".into(),
                ));
            }
            match self.listener.accept() {
                Ok((stream, peer)) => {
                    // Defence-in-depth: today the bind is hard-
                    // coded to `127.0.0.1:0`, so the kernel won't
                    // hand us an off-link peer. A future change
                    // that drops the bind to `0.0.0.0` (e.g. for
                    // dual-stack experiments) would lose that
                    // guarantee, leaving Host-header pinning as
                    // the only line of defence — a real LAN
                    // attacker can spoof Host. Reject any peer
                    // that isn't a loopback address up front.
                    if !peer.ip().is_loopback() {
                        tracing::warn!(
                            "loopback receiver: rejecting non-loopback peer {}",
                            peer.ip()
                        );
                        drop(stream);
                        continue;
                    }
                    match handle_stream(stream, expected_state, &expected_host, &expected_path) {
                        Ok(params) => return Ok(params),
                        Err(OAuthError::MalformedRedirect(msg)) => {
                            tracing::debug!("ignoring malformed loopback hit: {msg}");
                            // Keep accepting until timeout. A slow
                            // client that never completes its request
                            // within the per-stream 1 s deadline
                            // (H-3 / slow-loris — also covers L-4)
                            // lands here too.
                            continue;
                        }
                        Err(other) => return Err(other),
                    }
                }
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

/// Per-stream read deadline (H-3). A browser redirect is sub-
/// millisecond in practice; 1 second is generous enough to tolerate
/// a sleepy laptop on localhost while still aborting a slow-loris
/// client that dribbles bytes to hold the socket open. This also
/// addresses L-4 — no separate fix needed.
const STREAM_DEADLINE: Duration = Duration::from_secs(1);

/// Allowlist of host strings the OAuth `redirect_uri` is
/// permitted to advertise. Matched against the value passed
/// into [`LoopbackReceiver::bind_with_redirect`] so a future
/// caller can't accidentally point the flow at a public
/// hostname even though the kernel-level bind is loopback.
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

fn handle_stream(
    mut stream: TcpStream,
    expected_state: &str,
    expected_host: &str,
    expected_path: &str,
) -> Result<RedirectParams, OAuthError> {
    stream.set_read_timeout(Some(STREAM_DEADLINE)).ok();
    stream.set_write_timeout(Some(STREAM_DEADLINE)).ok();

    // Read enough of the request to get the first line + maybe
    // some headers. Browsers don't send request bodies on the
    // redirect GET, so 8 KiB is plenty. Enforce a wall-clock
    // deadline across all read()s so a client that sends one byte
    // at a time doesn't park the listener.
    let mut buf = [0u8; 8192];
    let mut read_total = 0usize;
    let start = std::time::Instant::now();
    while read_total < buf.len() {
        if start.elapsed() >= STREAM_DEADLINE {
            // Don't bother with a 408; just drop the connection.
            return Err(OAuthError::MalformedRedirect(
                "request headers incomplete before deadline".into(),
            ));
        }
        match stream.read(&mut buf[read_total..]) {
            Ok(0) => break,
            Ok(n) => {
                read_total += n;
                if buf[..read_total].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // read_timeout fired — drop the slow client.
                return Err(OAuthError::MalformedRedirect(
                    "read timed out on loopback stream".into(),
                ));
            }
            Err(e) => {
                return Err(OAuthError::MalformedRedirect(format!("read: {e}")));
            }
        }
    }
    let text = String::from_utf8_lossy(&buf[..read_total]);
    let mut lines = text.lines();
    let first_line = lines
        .next()
        .ok_or_else(|| OAuthError::MalformedRedirect("empty request".into()))?;

    // "GET /cb?code=…&state=… HTTP/1.1"
    let mut parts = first_line.split_whitespace();
    let _method = parts.next();
    let path = parts
        .next()
        .ok_or_else(|| OAuthError::MalformedRedirect("no path in request line".into()))?;

    // Only accept the exact callback path. Anything else is a 400.
    // Without this any local process (or a browser poking the
    // loopback port during dev-tools auto-discovery) could feed
    // the receiver an attacker-shaped URL.
    let path_only = path.split_once('?').map(|(p, _)| p).unwrap_or(path);
    if path_only != expected_path {
        let _ = write_bad_request(&mut stream);
        return Err(OAuthError::MalformedRedirect(format!(
            "unexpected path {path_only}"
        )));
    }

    // Require the Host header to match the host we advertised in
    // the redirect URI. DNS rebinding and a drive-by browser
    // hitting the same port from a public origin both rely on a
    // Host header the server didn't expect; reject anything that
    // isn't exactly `<advertised_host>:<bound_port>`. The host
    // string is whatever was passed to `bind_with_host` —
    // `127.0.0.1` for Google's IP-literal flow, `localhost` for
    // Microsoft's redirect-URI matcher.
    let host_header = lines
        .clone()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("Host") {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if host_header != expected_host {
        let _ = write_bad_request(&mut stream);
        return Err(OAuthError::MalformedRedirect(format!(
            "unexpected Host header {host_header:?}"
        )));
    }

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

fn write_bad_request(stream: &mut TcpStream) -> std::io::Result<()> {
    let body = "bad request";
    let response = format!(
        "HTTP/1.1 400 Bad Request\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    /// Read from `stream` until EOF or `timeout`, returning everything
    /// the client received as a single `Vec<u8>`. The test threads
    /// previously called `read(&mut buf)` once and asserted on the
    /// resulting buffer, which raced on macOS — `read` would return
    /// `Ok(0)` before the server's response landed in the loopback
    /// receive buffer (Linux + Windows happened to deliver before the
    /// first read returned). Looping until graceful EOF is the
    /// portable shape; a 2-second wall-clock guards against a stuck
    /// connection holding the test thread forever.
    fn read_response(stream: &mut TcpStream) -> Vec<u8> {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    if !out.is_empty() {
                        break;
                    }
                    // macOS occasionally hands back Ok(0) on a fresh
                    // loopback stream before the server's writes have
                    // been flushed through. Yield once and retry; the
                    // outer read_timeout still bounds total wait.
                    std::thread::sleep(Duration::from_millis(20));
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => out.extend_from_slice(&buf[..n]),
                        Err(_) => break,
                    }
                }
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        out
    }

    #[test]
    fn bind_returns_a_usable_port() {
        let r = LoopbackReceiver::bind().unwrap();
        let uri = r.redirect_uri();
        assert!(uri.starts_with("http://127.0.0.1:"));
        assert!(uri.ends_with("/cb"));
        assert!(r.port() > 1024);
    }

    /// Round-N security review F2: `bind_with_redirect` must
    /// reject any host string that isn't a documented loopback
    /// alias. Without this, a future caller (or an accidental
    /// path that lifts a string out of user input) could
    /// advertise a public hostname in the OAuth `redirect_uri`
    /// and accept a matching `Host:` header from the browser —
    /// pulling the callback through arbitrary DNS even though
    /// the kernel-level bind stays loopback.
    #[test]
    fn bind_with_redirect_rejects_non_loopback_hosts() {
        for host in &[
            "example.com",
            "attacker.evil",
            "192.168.1.1",
            "0.0.0.0",
            "",
            // case-sensitive on purpose: the OAuth URI is a
            // text-only contract with the AS; matching is exact.
            "Localhost",
            "127.0.0.2",
        ] {
            let result = LoopbackReceiver::bind_with_redirect(host, "/cb");
            assert!(
                result.is_err(),
                "bind_with_redirect({host:?}) should be rejected"
            );
        }
        // The four allowed forms must succeed.
        for host in &["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(
                LoopbackReceiver::bind_with_redirect(host, "/cb").is_ok(),
                "bind_with_redirect({host:?}) should be accepted"
            );
        }
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
            let req =
                format!("GET /cb?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
            s.write_all(req.as_bytes()).unwrap();
            // Drain the response so the server's write_all returns.
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
        });

        // 5 s deadline (not 2): macOS-latest GH runner is slow
        // enough that the receive-poll-parse cycle can drift past
        // 2 s under load. The test still finishes in well under a
        // second on real hardware; the headroom only matters on
        // virtualised CI.
        let params = receiver
            .wait_for_redirect("xyz", Duration::from_secs(5), None)
            .unwrap();
        assert_eq!(params.code, "abc");
        assert_eq!(params.state, "xyz");
        handle.join().unwrap();
    }

    #[test]
    fn wait_for_redirect_times_out_on_silent_port() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let err = receiver
            .wait_for_redirect("anything", Duration::from_millis(200), None)
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
            let req =
                format!("GET /cb?code=abc&state=wrong HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
            s.write_all(req.as_bytes()).unwrap();
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
        });
        // 5 s deadline — same race shape as
        // `wait_for_redirect_parses_callback`; see that test for
        // why 2 s was tight on the macOS-latest runner.
        let err = receiver
            .wait_for_redirect("expected", Duration::from_secs(5), None)
            .unwrap_err();
        assert!(matches!(err, OAuthError::StateMismatch { .. }));
        handle.join().unwrap();
    }

    /// Cancel flag wakes the receiver mid-wait. The bridge's
    /// view-model Drop sets this so closing the window during an
    /// in-flight OAuth flow aborts the wait promptly instead of
    /// hanging until the 120 s timeout. Verifies (a) the wait
    /// returns an Err immediately, and (b) the elapsed time is
    /// well under the timeout (so we know the cancel really
    /// woke it, not the timeout).
    #[test]
    fn wait_for_redirect_returns_promptly_on_cancel() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let _waker = std::thread::spawn(move || {
            // Give the receiver a moment to start polling.
            std::thread::sleep(Duration::from_millis(50));
            cancel_for_thread.store(true, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let err = receiver
            .wait_for_redirect("state", Duration::from_secs(60), Some(cancel))
            .unwrap_err();
        let elapsed = started.elapsed();
        // Cancelled-on-poll loops at the 25 ms accept cadence;
        // 1 s is generous headroom and well under the 60 s
        // timeout we'd otherwise wait.
        assert!(
            elapsed < Duration::from_secs(1),
            "cancel didn't wake the receiver in time (took {elapsed:?})"
        );
        match err {
            OAuthError::MalformedRedirect(msg) => {
                assert!(
                    msg.contains("cancelled"),
                    "expected cancel-shaped error, got {msg:?}"
                );
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    /// H-3 path allowlist: anything other than `/cb` returns 400
    /// and the receiver keeps accepting. If the deadline fires
    /// without a good request we bubble `MalformedRedirect`.
    #[test]
    fn wrong_path_is_rejected_and_receiver_keeps_going() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let port = receiver.port();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let req = format!("GET /admin HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
            s.write_all(req.as_bytes()).unwrap();
            let response = String::from_utf8_lossy(&read_response(&mut s)).to_string();
            assert!(response.contains("400"), "response: {response}");
        });
        // Receiver stays up until timeout — there's no valid /cb hit.
        let err = receiver
            .wait_for_redirect("state", Duration::from_millis(500), None)
            .unwrap_err();
        assert!(matches!(err, OAuthError::MalformedRedirect(_)));
        handle.join().unwrap();
    }

    /// H-3 Host-header check: a request that spoofs a different
    /// Host lands in 400-territory and the receiver moves on.
    #[test]
    fn wrong_host_is_rejected() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let port = receiver.port();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            // A DNS-rebinding-style request claiming a public host.
            let req = b"GET /cb?code=abc&state=ok HTTP/1.1\r\nHost: attacker.example.com\r\n\r\n";
            s.write_all(req).unwrap();
            let response = String::from_utf8_lossy(&read_response(&mut s)).to_string();
            assert!(response.contains("400"), "response: {response}");
        });
        let err = receiver
            .wait_for_redirect("ok", Duration::from_millis(500), None)
            .unwrap_err();
        assert!(matches!(err, OAuthError::MalformedRedirect(_)));
        handle.join().unwrap();
    }

    /// H-3 / L-4 slow-loris: a client that connects but never
    /// finishes its headers must be dropped within the per-stream
    /// deadline, and the listener must keep accepting after the
    /// drop.
    #[test]
    fn slow_client_is_dropped_within_deadline() {
        let receiver = LoopbackReceiver::bind().unwrap();
        let port = receiver.port();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            // Open the socket, write only a partial request line
            // (no terminating CRLFCRLF), and hold it open past the
            // 1 s deadline.
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = s.write_all(b"GET /cb?co");
            // Sleep past the deadline + a small margin.
            std::thread::sleep(Duration::from_millis(1500));
            // Drop.
            drop(s);
        });
        // The receiver should give up on the slow client inside
        // STREAM_DEADLINE (~1 s) and then time out on its own
        // wall-clock deadline a short while later.
        let start = std::time::Instant::now();
        let err = receiver
            .wait_for_redirect("state", Duration::from_millis(2500), None)
            .unwrap_err();
        let elapsed = start.elapsed();
        assert!(matches!(err, OAuthError::MalformedRedirect(_)));
        // We should have waited about the outer timeout, not
        // significantly longer — confirming we didn't block
        // forever on the slow client.
        assert!(
            elapsed < Duration::from_secs(3),
            "receiver hung past outer deadline: {elapsed:?}"
        );
        handle.join().unwrap();
    }
}
