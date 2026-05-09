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

/// Cap used when an error response body is going to be embedded
/// in a `SyncError` string that the bridge then displays in the
/// QML status bar. The full body still flows to the tracing
/// sink at `debug` level via the per-provider call sites.
///
/// 256 chars is enough to carry the typical OAuth `error` /
/// `error_description` JSON pair (Google + Microsoft both
/// fit in well under 200) without:
///   1. wedging the status bar with megabytes of HTML when a
///      stuck proxy / DNS-rebind / reachable token endpoint
///      replies with a long page, and
///   2. risking that an OAuth `code` echoed back inside an
///      `error_description` survives intact in tracing
///      output (the token-exchange failure path is when the
///      code is most likely to appear because the AS rejected
///      it but is nonetheless honest about *why*).
pub const ERROR_BODY_DISPLAY_CAP: usize = 256;

/// Truncate `body` to at most [`ERROR_BODY_DISPLAY_CAP`] chars
/// for embedding in a user-visible error string. Newlines are
/// collapsed to spaces so the status bar (a single Qt label)
/// renders cleanly. Truncation is signalled with a `…(truncated)`
/// suffix.
///
/// Walks `char_indices` until either the cap is hit or the input
/// ends, so the work done here is O(min(len(body), cap)) — we
/// never materialise a copy of an attacker-pumped 64 MiB body
/// the way an upfront `body.chars().collect::<String>()` would.
/// The defense against unbounded input is `read_body_capped`'s
/// stream cap; this helper just trims the bounded result for
/// display.
pub fn truncate_for_status(body: &str) -> String {
    // Walk char-by-char up to the cap, replacing CR/LF/TAB with
    // spaces. `more` flags that input remained after we stopped
    // — drives the `…(truncated)` suffix below.
    let mut out = String::with_capacity(ERROR_BODY_DISPLAY_CAP);
    let mut more = false;
    for (i, c) in body.chars().enumerate() {
        if i >= ERROR_BODY_DISPLAY_CAP {
            more = true;
            break;
        }
        out.push(if matches!(c, '\n' | '\r' | '\t') {
            ' '
        } else {
            c
        });
    }
    let trimmed = out.trim();
    if more {
        format!("{trimmed}…(truncated)")
    } else {
        trimmed.to_string()
    }
}

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

    #[test]
    fn truncate_for_status_passes_through_short_strings() {
        assert_eq!(
            truncate_for_status(r#"{"error":"invalid_grant"}"#),
            r#"{"error":"invalid_grant"}"#
        );
    }

    #[test]
    fn truncate_for_status_collapses_newlines_and_trims() {
        // \n, \r, \t each become a single space; outer
        // whitespace is trimmed; inner whitespace is preserved.
        let out = truncate_for_status("\n  one\r\n  two\t three  ");
        // No control chars survive.
        assert!(!out.contains('\n') && !out.contains('\r') && !out.contains('\t'));
        // Trimmed at the edges.
        assert!(!out.starts_with(' ') && !out.ends_with(' '));
        // Words are still separated and in order.
        let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(collapsed, "one two three");
    }

    #[test]
    fn truncate_for_status_caps_long_body_with_marker() {
        // 600-char body should get capped at 256 + the
        // `…(truncated)` suffix so a megabyte of HTML or a long
        // OAuth `error_description` echoing the rejected `code`
        // can't wedge the status bar.
        let body = "x".repeat(600);
        let out = truncate_for_status(&body);
        assert!(out.ends_with("…(truncated)"));
        // 256 chars of payload + the suffix.
        assert_eq!(
            out.chars().count(),
            ERROR_BODY_DISPLAY_CAP + "…(truncated)".chars().count()
        );
    }
}
