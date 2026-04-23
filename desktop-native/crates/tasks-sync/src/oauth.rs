//! OAuth2 desktop / native flow with PKCE (RFC 7636).
//!
//! Google Tasks and Microsoft To Do both want a "native app" OAuth
//! dance: system browser opens the authorization URL, the user
//! approves, the server redirects to `http://127.0.0.1:<port>/…`
//! with a `code` param, and we exchange that code for access +
//! refresh tokens at the provider's token endpoint.
//!
//! This module owns the pure-logic half of that dance — PKCE
//! challenge generation, authorization-URL construction, redirect
//! parsing, and token-exchange body assembly. The loopback HTTP
//! server and the `reqwest` POST live in the eventual network
//! commit; both are trivial wrappers on top of this surface.
//!
//! Nothing here does I/O beyond `getrandom`, so every behaviour
//! round-trips through tests.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OAuthError {
    #[error("random source failed: {0}")]
    Random(String),
    #[error("authorization server returned error: {0}")]
    AuthorizationDenied(String),
    #[error("redirect state mismatch — expected {expected}, got {actual}")]
    StateMismatch { expected: String, actual: String },
    #[error("redirect did not contain the expected parameter: {0}")]
    MissingParam(&'static str),
    #[error("redirect URL is malformed: {0}")]
    MalformedRedirect(String),
}

pub type OAuthResult<T> = Result<T, OAuthError>;

/// A PKCE verifier + its derived challenge, plus the method
/// identifier. The verifier stays on the client; the challenge
/// is what's sent to the authorization endpoint. On token
/// exchange, the client proves possession by sending the raw
/// verifier and the server re-hashes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceChallenge {
    pub verifier: String,
    pub challenge: String,
    pub method: &'static str,
}

impl PkceChallenge {
    /// Generate a fresh PKCE pair. Verifier is 32 URL-safe bytes
    /// (matches the 43-char base64url output RFC 7636 recommends).
    pub fn generate() -> OAuthResult<Self> {
        let mut raw = [0u8; 32];
        getrandom::getrandom(&mut raw).map_err(|e| OAuthError::Random(e.to_string()))?;
        let verifier = URL_SAFE_NO_PAD.encode(raw);
        Ok(Self::from_verifier(verifier))
    }

    /// Build from a known verifier. Used by tests to pin against
    /// RFC 7636 test vectors.
    pub fn from_verifier(verifier: String) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(digest);
        PkceChallenge {
            verifier,
            challenge,
            method: "S256",
        }
    }
}

/// Shape the UI hands to the loopback + token-exchange layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    /// URL to open in the system browser. Contains every required
    /// OAuth2 param + the PKCE challenge.
    pub authorization_url: String,
    /// Opaque anti-CSRF token. The loopback handler must verify
    /// the `state` param in the redirect matches this.
    pub state: String,
    /// Verifier to send back to the token endpoint.
    pub pkce_verifier: String,
    /// The redirect URI embedded in the authorization URL.
    /// Loopback server must listen on this exact URL.
    pub redirect_uri: String,
}

/// Parsed result of the browser's redirect to our loopback server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectParams {
    pub code: String,
    pub state: String,
}

/// Build an `authorization_url` ready to hand to
/// `QDesktopServices::openUrl()`. The `state` + `pkce_verifier`
/// in the returned struct are the client's half of the dance —
/// keep them in memory until the redirect arrives, then use them
/// to validate and exchange.
pub fn build_authorization_request(
    authorization_endpoint: &str,
    client_id: &str,
    scopes: &[&str],
    redirect_uri: &str,
) -> OAuthResult<AuthorizationRequest> {
    let pkce = PkceChallenge::generate()?;
    let state = generate_random_state()?;
    let scope_str = scopes.join(" ");

    let separator = if authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    let url = format!(
        "{authorization_endpoint}{separator}\
         response_type=code\
         &client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &scope={scope}\
         &state={state}\
         &code_challenge={challenge}\
         &code_challenge_method={method}",
        client_id = percent_encode(client_id),
        redirect_uri = percent_encode(redirect_uri),
        scope = percent_encode(&scope_str),
        state = percent_encode(&state),
        challenge = percent_encode(&pkce.challenge),
        method = pkce.method,
    );

    Ok(AuthorizationRequest {
        authorization_url: url,
        state,
        pkce_verifier: pkce.verifier,
        redirect_uri: redirect_uri.to_string(),
    })
}

/// Parse the loopback redirect URL for the authorization `code` +
/// matching `state`. Returns `Err(AuthorizationDenied)` if the
/// server redirected with an `error` param instead of `code`.
///
/// L-2: pair-level validation fails loudly on empty keys, pairs
/// with no `=`, and decoded values containing CR/LF. The CR/LF
/// guard matters because the decoded `code` / `state` eventually
/// crosses into HTTP header / log formats, where embedded CRLF is
/// a header-injection primitive.
pub fn parse_redirect(url: &str, expected_state: &str) -> OAuthResult<RedirectParams> {
    // We accept either a full URL ("http://127.0.0.1:12345/?code=…")
    // or just the query string ("code=…"). Loopback servers
    // typically hand us the full request line; either shape works.
    let query = url
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or(url)
        .trim_end_matches(&['#', '/'][..]);

    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    let mut error: Option<String> = None;

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| OAuthError::MalformedRedirect(format!("no '=' in pair {pair:?}")))?;
        if k.is_empty() {
            return Err(OAuthError::MalformedRedirect(format!(
                "empty key in pair {pair:?}"
            )));
        }
        let decoded = percent_decode(v);
        if decoded.contains('\r') || decoded.contains('\n') {
            // Prevents header-injection where the redirect URL
            // smuggles CR/LF into what we later format into a log
            // line or an HTTP header.
            return Err(OAuthError::MalformedRedirect(format!(
                "CR/LF in value for {k:?}"
            )));
        }
        match k {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            "error" => error = Some(decoded),
            _ => {}
        }
    }

    if let Some(err) = error {
        return Err(OAuthError::AuthorizationDenied(err));
    }

    let code = code.ok_or(OAuthError::MissingParam("code"))?;
    let state = state.ok_or(OAuthError::MissingParam("state"))?;

    if state != expected_state {
        return Err(OAuthError::StateMismatch {
            expected: expected_state.to_string(),
            actual: state,
        });
    }
    Ok(RedirectParams { code, state })
}

/// Build the `application/x-www-form-urlencoded` body for the
/// token endpoint POST. Caller sets the `Content-Type` + POSTs;
/// response parsing lives in the network commit since it's just
/// serde_json over the standard OAuth2 token response shape.
pub fn build_token_request_body(
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> String {
    let mut body = String::new();
    body.push_str("grant_type=authorization_code");
    body.push_str("&client_id=");
    body.push_str(&percent_encode(client_id));
    body.push_str("&code=");
    body.push_str(&percent_encode(code));
    body.push_str("&redirect_uri=");
    body.push_str(&percent_encode(redirect_uri));
    body.push_str("&code_verifier=");
    body.push_str(&percent_encode(code_verifier));
    body
}

/// Build the body for a refresh-token request. Returns the new
/// access token (and possibly a rotated refresh token) from the
/// same token endpoint.
pub fn build_refresh_request_body(client_id: &str, refresh_token: &str) -> String {
    format!(
        "grant_type=refresh_token\
         &client_id={}\
         &refresh_token={}",
        percent_encode(client_id),
        percent_encode(refresh_token),
    )
}

// ---------- helpers ----------

fn generate_random_state() -> OAuthResult<String> {
    let mut raw = [0u8; 16];
    getrandom::getrandom(&mut raw).map_err(|e| OAuthError::Random(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(raw))
}

fn percent_encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

fn percent_decode(s: &str) -> String {
    // percent_encoding 2.x doesn't ship a decoder for
    // application/x-www-form-urlencoded that handles `+` as
    // space; the redirect URL is always a query string, so we
    // handle that one replacement inline.
    let replaced = s.replace('+', " ");
    percent_encoding::percent_decode_str(&replaced)
        .decode_utf8_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_7636_test_vector() {
        // RFC 7636 Appendix B.1: the example verifier
        // "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk" hashes to
        // the challenge "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".
        let pkce =
            PkceChallenge::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(pkce.method, "S256");
    }

    #[test]
    fn generated_pkce_survives_round_trip() {
        let a = PkceChallenge::generate().unwrap();
        let b = PkceChallenge::from_verifier(a.verifier.clone());
        assert_eq!(a, b);
        assert!(!a.verifier.is_empty());
        assert!(!a.challenge.is_empty());
        // verifier length is 32 random bytes → 43 base64url chars.
        assert_eq!(a.verifier.len(), 43);
    }

    #[test]
    fn authorization_url_carries_every_param() {
        let req = build_authorization_request(
            "https://accounts.google.com/o/oauth2/v2/auth",
            "my-client-id",
            &["https://www.googleapis.com/auth/tasks"],
            "http://127.0.0.1:12345/cb",
        )
        .unwrap();
        let url = &req.authorization_url;
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=my%2Dclient%2Did"));
        assert!(
            url.contains("redirect_uri=http%3A%2F%2F127%2E0%2E0%2E1%3A12345%2Fcb"),
            "url: {url}"
        );
        assert!(url.contains("scope=https%3A%2F%2Fwww%2Egoogleapis%2Ecom%2Fauth%2Ftasks"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!(
            "code_challenge={}",
            percent_encode(&PkceChallenge::from_verifier(req.pkce_verifier.clone()).challenge)
        )));
        // state should be random + non-empty.
        assert!(!req.state.is_empty());
    }

    #[test]
    fn authorization_url_appends_with_ampersand_when_endpoint_has_query() {
        let req = build_authorization_request(
            "https://example.com/authorize?tenant=foo",
            "client",
            &["scope-a"],
            "http://127.0.0.1:1/cb",
        )
        .unwrap();
        assert!(
            req.authorization_url
                .starts_with("https://example.com/authorize?tenant=foo&response_type=code"),
            "url: {}",
            req.authorization_url
        );
    }

    #[test]
    fn redirect_parses_code_and_state() {
        let got = parse_redirect("http://127.0.0.1:12345/cb?code=abc&state=xyz", "xyz").unwrap();
        assert_eq!(got.code, "abc");
        assert_eq!(got.state, "xyz");
    }

    #[test]
    fn redirect_rejects_state_mismatch() {
        let err =
            parse_redirect("http://127.0.0.1:1/cb?code=abc&state=wrong", "expected").unwrap_err();
        assert!(matches!(err, OAuthError::StateMismatch { .. }));
    }

    #[test]
    fn redirect_surfaces_error_param() {
        let err =
            parse_redirect("http://127.0.0.1:1/cb?error=access_denied&state=x", "x").unwrap_err();
        assert!(matches!(err, OAuthError::AuthorizationDenied(_)));
    }

    #[test]
    fn redirect_decodes_percent_escapes() {
        let got = parse_redirect(
            "http://127.0.0.1:1/cb?code=a%20b&state=state%2Dvalue",
            "state-value",
        )
        .unwrap();
        assert_eq!(got.code, "a b");
        assert_eq!(got.state, "state-value");
    }

    #[test]
    fn redirect_missing_code_errors() {
        let err = parse_redirect("http://127.0.0.1:1/cb?state=x", "x").unwrap_err();
        assert!(matches!(err, OAuthError::MissingParam("code")));
    }

    /// L-2: a pair with no `=` is rejected outright instead of
    /// silently dropped.
    #[test]
    fn redirect_rejects_pair_without_equals() {
        let err = parse_redirect("http://127.0.0.1:1/cb?code=abc&broken&state=x", "x").unwrap_err();
        assert!(matches!(err, OAuthError::MalformedRedirect(m) if m.contains("broken")));
    }

    /// L-2: an empty key (`=value`) is rejected.
    #[test]
    fn redirect_rejects_empty_key() {
        let err = parse_redirect("http://127.0.0.1:1/cb?code=abc&=evil&state=x", "x").unwrap_err();
        assert!(matches!(err, OAuthError::MalformedRedirect(m) if m.contains("empty key")));
    }

    /// L-2: CR/LF inside a decoded value is a header-injection
    /// primitive once the value crosses into a log line or header
    /// — reject before it ever gets that far.
    #[test]
    fn redirect_rejects_crlf_in_value() {
        // "a%0Db" decodes to "a\rb".
        let err = parse_redirect("http://127.0.0.1:1/cb?code=a%0Db&state=x", "x").unwrap_err();
        assert!(matches!(err, OAuthError::MalformedRedirect(m) if m.contains("CR/LF")));
        // "a%0Ab" decodes to "a\nb".
        let err2 =
            parse_redirect("http://127.0.0.1:1/cb?code=abc&state=a%0Ab", "whatever").unwrap_err();
        assert!(matches!(err2, OAuthError::MalformedRedirect(m) if m.contains("CR/LF")));
    }

    #[test]
    fn token_body_has_every_param() {
        let body = build_token_request_body("cid", "the-code", "http://127.0.0.1:1/cb", "verifier");
        assert!(body.starts_with("grant_type=authorization_code"));
        assert!(body.contains("&client_id=cid"));
        assert!(body.contains("&code=the%2Dcode"));
        assert!(body.contains("&redirect_uri=http%3A%2F%2F127%2E0%2E0%2E1%3A1%2Fcb"));
        assert!(body.contains("&code_verifier=verifier"));
    }

    #[test]
    fn refresh_body_shape() {
        let body = build_refresh_request_body("cid", "refresh-tok-123");
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains("client_id=cid"));
        assert!(body.contains("refresh_token=refresh%2Dtok%2D123"));
    }
}
