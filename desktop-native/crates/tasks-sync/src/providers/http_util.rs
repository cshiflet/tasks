//! Shared HTTP helpers for the four provider modules.
//!
//! At the moment this is just a body-size cap: none of the REST /
//! CalDAV servers we talk to should ever hand us a response larger
//! than a handful of MiB, but `reqwest::Response::text()` will
//! happily buffer the entire body into memory with no cap. An
//! attacker-controlled server could therefore wedge the desktop by
//! streaming gigabytes at us. [`read_body_capped`] streams the
//! response body through `bytes_stream()` and fails loudly once the
//! running total exceeds `limit_bytes` (M-2).

use futures_util::StreamExt;
use reqwest::Response;

use crate::provider::{SyncError, SyncResult};

/// Default cap used by all four providers. 64 MiB is far larger
/// than any real CalDAV multistatus or Graph paging envelope
/// (Graph lists cap at 1000 tasks per page, which is a few MiB;
/// CalDAV multistatus responses are bound by the calendar size),
/// so callers hitting the cap are almost certainly looking at
/// misbehaviour rather than a legitimate payload.
pub const DEFAULT_BODY_CAP: usize = 64 * 1024 * 1024;

/// Read `resp.bytes_stream()` into a UTF-8 string, failing with
/// [`SyncError::Protocol`] if the accumulated body size exceeds
/// `limit_bytes`. Drop-in replacement for `resp.text().await`.
///
/// Behaviour detail: we fail on the first chunk that *would* push
/// the total past the cap; the already-buffered prefix is dropped
/// along with the stream.
pub async fn read_body_capped(resp: Response, limit_bytes: usize) -> SyncResult<String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| SyncError::Network(format!("body stream: {e}")))?;
        if buf.len().saturating_add(chunk.len()) > limit_bytes {
            return Err(SyncError::Protocol(format!(
                "response body exceeded {} MiB",
                limit_bytes / (1024 * 1024)
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|e| SyncError::Protocol(format!("body not UTF-8: {e}")))
}

/// Pure-logic half of [`read_body_capped`], split out so tests
/// exercise the overflow branch without needing to spin up an HTTP
/// server. Callers feed the sequence of chunks the stream would
/// have produced; the function returns the same result shape.
#[cfg(test)]
pub(crate) fn accumulate_capped<I>(chunks: I, limit_bytes: usize) -> SyncResult<String>
where
    I: IntoIterator<Item = Vec<u8>>,
{
    let mut buf: Vec<u8> = Vec::new();
    for chunk in chunks {
        if buf.len().saturating_add(chunk.len()) > limit_bytes {
            return Err(SyncError::Protocol(format!(
                "response body exceeded {} MiB",
                limit_bytes / (1024 * 1024)
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|e| SyncError::Protocol(format!("body not UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_capped_accepts_short_body() {
        let out = accumulate_capped(
            vec![b"hello ".to_vec(), b"world".to_vec()],
            DEFAULT_BODY_CAP,
        )
        .unwrap();
        assert_eq!(out, "hello world");
    }

    #[test]
    fn accumulate_capped_rejects_oversize_body() {
        // 3 MiB cap, feed 4 MiB.
        let cap = 3 * 1024 * 1024;
        let chunks = vec![vec![b'x'; 2 * 1024 * 1024], vec![b'x'; 2 * 1024 * 1024]];
        let err = accumulate_capped(chunks, cap).unwrap_err();
        assert!(matches!(err, SyncError::Protocol(m) if m.contains("exceeded")));
    }

    #[test]
    fn accumulate_capped_fails_on_non_utf8() {
        let chunks = vec![vec![0xFF, 0xFE, 0xFD]];
        let err = accumulate_capped(chunks, DEFAULT_BODY_CAP).unwrap_err();
        assert!(matches!(err, SyncError::Protocol(m) if m.contains("UTF-8")));
    }

    #[test]
    fn accumulate_capped_fails_at_exact_boundary() {
        // chunks that together exactly equal cap+1 should trip it.
        let cap = 10;
        let chunks = vec![vec![b'a'; 5], vec![b'a'; 6]];
        let err = accumulate_capped(chunks, cap).unwrap_err();
        assert!(matches!(err, SyncError::Protocol(_)));
    }
}
