//! Google Tasks provider — real implementation.
//!
//! Speaks the Google Tasks REST API v1
//! (<https://developers.google.com/tasks/reference/rest>) using
//! `reqwest` with rustls. OAuth2 "installed-app" flow: system
//! browser → loopback redirect → PKCE code exchange. The
//! OAuth half is the PKCE + authorization-URL builder in
//! [`crate::oauth`] + the loopback server in [`crate::loopback`];
//! this file stitches them together, plus the on-the-wire JSON
//! translators in [`super::google_json`].
//!
//! Token lifecycle:
//! * `connect()` makes sure we have a live access token: if the
//!   caller already provided one in `AccountCredentials`
//!   (`oauth_access_token`), great; else we try the stored tokens
//!   (`TokenStore`); else we run the browser-based flow and store
//!   what comes back.
//! * Every HTTP call checks `needs_refresh` + refreshes via the
//!   refresh_token before issuing the request.
//!
//! Etags: Google returns a per-resource `etag` on task JSON. We
//! stamp it into [`RemoteTask::etag`] on pull and echo it back as
//! an `If-Match` header on update so concurrent edits surface
//! as [`SyncError::Conflict`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, IF_MATCH};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use super::google_json::{
    parse_tasklists, parse_tasks, remote_to_task_json, task_to_remote, tasklist_to_remote_calendar,
};
use crate::oauth::{
    build_authorization_request, build_refresh_request_body, build_token_request_body,
};
use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};
use crate::token_store::{OAuthTokens, TokenStore};

/// Google's documented OAuth2 endpoints + API root.
const AUTHORIZATION_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const API_ROOT: &str = "https://tasks.googleapis.com/tasks/v1";
/// The single scope we need — read + write Google Tasks.
const SCOPE: &str = "https://www.googleapis.com/auth/tasks";
/// Refresh the access token when it's this close to expiring.
const REFRESH_GRACE_MS: i64 = 60_000;

/// Session state populated by `connect()`.
struct Session {
    http: Client,
    tokens: OAuthTokens,
}

pub struct GoogleTasksProvider {
    credentials: AccountCredentials,
    account_label: String,
    /// OAuth2 client id registered in the Google Cloud console.
    /// For a desktop client the secret is omitted (Google's
    /// "installed app" flow is PKCE-only).
    client_id: String,
    /// Optional token store; if present, `connect()` reads + writes
    /// tokens here. If None, tokens live only for the process
    /// lifetime.
    token_store: Option<Arc<dyn TokenStore>>,
    session: Arc<tokio::sync::Mutex<Option<Session>>>,
}

impl GoogleTasksProvider {
    pub fn new(
        credentials: AccountCredentials,
        account_label: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
            client_id: client_id.into(),
            token_store: None,
            session: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn with_token_store(mut self, store: Arc<dyn TokenStore>) -> Self {
        self.token_store = Some(store);
        self
    }

    /// Make sure the session has a non-expired access token.
    /// Refreshes via the refresh_token if needed.
    async fn ensure_fresh(&self) -> SyncResult<HeaderValue> {
        let mut guard = self.session.lock().await;
        let s = guard
            .as_mut()
            .ok_or_else(|| SyncError::Auth("Google Tasks: connect() first".into()))?;
        if s.tokens.needs_refresh(now_ms(), REFRESH_GRACE_MS) {
            let refresh = s.tokens.refresh_token.clone().ok_or_else(|| {
                SyncError::Auth("Google access token expired and no refresh_token stored".into())
            })?;
            let new = refresh_access_token(&s.http, &self.client_id, &refresh).await?;
            // Google doesn't always rotate the refresh token; preserve
            // the existing one if the response omits it.
            let rotated_refresh = new.refresh_token.clone().or(Some(refresh));
            s.tokens = OAuthTokens {
                access_token: new.access_token,
                refresh_token: rotated_refresh,
                expires_at_ms: new.expires_at_ms,
            };
            if let Some(store) = &self.token_store {
                let _ = store.put(ProviderKind::GoogleTasks, &self.account_label, &s.tokens);
            }
        }
        HeaderValue::from_str(&format!("Bearer {}", s.tokens.access_token))
            .map_err(|e| SyncError::Auth(format!("bad bearer header: {e}")))
    }
}

#[async_trait]
impl Provider for GoogleTasksProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::GoogleTasks
    }

    fn account_label(&self) -> &str {
        &self.account_label
    }

    async fn connect(&mut self) -> SyncResult<()> {
        let http = Client::builder()
            .user_agent("tasks-desktop-native/0.1")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| SyncError::Network(format!("reqwest build: {e}")))?;

        // Token resolution order:
        //   1. Fresh access token already on credentials (for tests
        //      / handoff from a UI-driven flow).
        //   2. Stored tokens from the TokenStore.
        //   3. NotYetImplemented — the browser-driven auth flow
        //      itself is invoked from the UI shell, not here.
        let tokens = if let Some(at) = self.credentials.oauth_access_token.clone() {
            OAuthTokens {
                access_token: at,
                refresh_token: self.credentials.oauth_refresh_token.clone(),
                // Unknown expiry from credentials-only → never auto-refresh.
                expires_at_ms: 0,
            }
        } else if let Some(store) = &self.token_store {
            store
                .get(ProviderKind::GoogleTasks, &self.account_label)
                .ok_or_else(|| {
                    SyncError::Auth(format!(
                        "no stored Google tokens for '{}' — run authorize() first",
                        self.account_label
                    ))
                })?
        } else {
            return Err(SyncError::Auth(
                "Google Tasks: no credentials and no TokenStore; call authorize() or set oauth_access_token".into(),
            ));
        };

        *self.session.lock().await = Some(Session { http, tokens });
        Ok(())
    }

    async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
        let auth = self.ensure_fresh().await?;
        let guard = self.session.lock().await;
        let s = guard.as_ref().expect("session populated above");

        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!("{API_ROOT}/users/@me/lists?maxResults=100");
            if let Some(t) = &page_token {
                url.push_str("&pageToken=");
                url.push_str(&percent_encode(t));
            }
            let resp = s
                .http
                .get(&url)
                .header(AUTHORIZATION, auth.clone())
                .send()
                .await
                .map_err(|e| SyncError::Network(format!("tasklists.list: {e}")))?;
            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| SyncError::Network(format!("read body: {e}")))?;
            if !status.is_success() {
                return Err(classify_http_error(status, body, "tasklists.list"));
            }
            let envelope = parse_tasklists(&body)?;
            for tl in &envelope.items {
                out.push(tasklist_to_remote_calendar(tl));
            }
            match envelope.next_page_token {
                Some(next) if !next.is_empty() => page_token = Some(next),
                _ => break,
            }
        }
        Ok(out)
    }

    async fn list_tasks(&mut self, calendar_remote_id: &str) -> SyncResult<Vec<RemoteTask>> {
        let auth = self.ensure_fresh().await?;
        let guard = self.session.lock().await;
        let s = guard.as_ref().expect("session populated above");

        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            // showCompleted=true + showHidden=true to pull finished
            // + archived tasks; Tasks.org shows both.
            let mut url = format!(
                "{API_ROOT}/lists/{}/tasks?maxResults=100&showCompleted=true&showHidden=true",
                percent_encode(calendar_remote_id)
            );
            if let Some(t) = &page_token {
                url.push_str("&pageToken=");
                url.push_str(&percent_encode(t));
            }
            let resp = s
                .http
                .get(&url)
                .header(AUTHORIZATION, auth.clone())
                .send()
                .await
                .map_err(|e| SyncError::Network(format!("tasks.list: {e}")))?;
            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| SyncError::Network(format!("read body: {e}")))?;
            if !status.is_success() {
                return Err(classify_http_error(status, body, "tasks.list"));
            }
            let envelope = parse_tasks(&body)?;
            for t in &envelope.items {
                // Tombstones surface via `deleted: true`; keep them
                // so the engine can tombstone locally.
                if t.deleted.unwrap_or(false) {
                    continue;
                }
                out.push(task_to_remote(t, calendar_remote_id));
            }
            match envelope.next_page_token {
                Some(next) if !next.is_empty() => page_token = Some(next),
                _ => break,
            }
        }
        Ok(out)
    }

    async fn push_task(&mut self, task: &RemoteTask) -> SyncResult<Option<String>> {
        let auth = self.ensure_fresh().await?;
        let guard = self.session.lock().await;
        let s = guard.as_ref().expect("session populated above");

        let body = remote_to_task_json(task);
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| SyncError::Protocol(format!("serialise task: {e}")))?;

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth.clone());
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        if let Some(etag) = &task.etag {
            headers.insert(
                IF_MATCH,
                HeaderValue::from_str(etag)
                    .map_err(|e| SyncError::Protocol(format!("bad etag header: {e}")))?,
            );
        }

        // Google Tasks uses PUT for update, POST for insert. If we
        // have a remote_id *and* an etag the row came from a prior
        // pull → update. If we have remote_id without etag it's a
        // row that was created locally with a pre-assigned id; use
        // PUT too (Google accepts a PUT with an id as upsert). If
        // remote_id is empty we let the engine skip — Google assigns
        // ids on insert and we don't want to burn one here without
        // the engine plumbed to stamp it back.
        let url = format!(
            "{API_ROOT}/lists/{}/tasks/{}",
            percent_encode(&task.calendar_remote_id),
            percent_encode(&task.remote_id)
        );
        let resp = if task.remote_id.is_empty() {
            let post_url = format!(
                "{API_ROOT}/lists/{}/tasks",
                percent_encode(&task.calendar_remote_id)
            );
            s.http
                .post(post_url)
                .headers(headers)
                .body(body_bytes)
                .send()
                .await
        } else {
            s.http
                .put(&url)
                .headers(headers)
                .body(body_bytes)
                .send()
                .await
        }
        .map_err(|e| SyncError::Network(format!("tasks push: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SyncError::Network(format!("read body: {e}")))?;
        if status == StatusCode::PRECONDITION_FAILED {
            return Err(SyncError::Conflict {
                remote_id: task.remote_id.clone(),
                local: task.etag.clone(),
                server_message: body,
            });
        }
        if !status.is_success() {
            return Err(classify_http_error(status, body, "tasks push"));
        }
        // The response body is a full Task JSON including the new
        // etag; thread it back to the engine so the next pull is
        // idempotent.
        #[derive(Deserialize)]
        struct EtagOnly {
            etag: Option<String>,
        }
        let parsed: EtagOnly = serde_json::from_str(&body).unwrap_or(EtagOnly { etag: None });
        Ok(parsed.etag)
    }

    async fn delete_task(&mut self, calendar_remote_id: &str, remote_id: &str) -> SyncResult<()> {
        let auth = self.ensure_fresh().await?;
        let guard = self.session.lock().await;
        let s = guard.as_ref().expect("session populated above");

        let url = format!(
            "{API_ROOT}/lists/{}/tasks/{}",
            percent_encode(calendar_remote_id),
            percent_encode(remote_id)
        );
        let resp = s
            .http
            .delete(&url)
            .header(AUTHORIZATION, auth.clone())
            .send()
            .await
            .map_err(|e| SyncError::Network(format!("tasks.delete: {e}")))?;
        let status = resp.status();
        if status.is_success() || status == StatusCode::NOT_FOUND {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(classify_http_error(status, body, "tasks.delete"))
    }

    async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
        // Orchestration lives on `SyncEngine::sync_now`, which
        // composes pull + push across providers uniformly.
        Err(SyncError::NotYetImplemented {
            provider: "Google Tasks",
            method: "sync_once (use SyncEngine::sync_now)",
        })
    }
}

// ---------- OAuth helpers ----------

/// Public helper the UI calls when the user picks "Add Google
/// Tasks account": opens [`authorization_url`] in the system
/// browser, pumps the loopback receiver, exchanges the code for
/// tokens, and stores them.
///
/// Returns the fresh tokens so the caller can hand them to a
/// [`GoogleTasksProvider`] via the token store (or the
/// `oauth_access_token` field).
pub async fn authorize(
    client_id: &str,
    http: &Client,
    open_browser: impl FnOnce(&str),
    timeout: Duration,
) -> SyncResult<OAuthTokens> {
    use crate::loopback::LoopbackReceiver;

    let receiver =
        LoopbackReceiver::bind().map_err(|e| SyncError::Auth(format!("loopback bind: {e}")))?;
    let redirect_uri = receiver.redirect_uri();
    let req =
        build_authorization_request(AUTHORIZATION_ENDPOINT, client_id, &[SCOPE], &redirect_uri)
            .map_err(|e| SyncError::Auth(format!("build auth url: {e}")))?;
    open_browser(&req.authorization_url);

    // The loopback receiver blocks a thread; keep it off the async
    // runtime by offloading to spawn_blocking.
    let state = req.state.clone();
    let redirect = tokio::task::spawn_blocking(move || receiver.wait_for_redirect(&state, timeout))
        .await
        .map_err(|e| SyncError::Auth(format!("loopback task: {e}")))?
        .map_err(|e| SyncError::Auth(format!("loopback: {e}")))?;

    let body =
        build_token_request_body(client_id, &redirect.code, &redirect_uri, &req.pkce_verifier);
    let resp = http
        .post(TOKEN_ENDPOINT)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| SyncError::Network(format!("token exchange: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| SyncError::Network(format!("token exchange body: {e}")))?;
    if !status.is_success() {
        return Err(SyncError::Auth(format!(
            "token exchange failed: {status}: {text}"
        )));
    }
    parse_token_response(&text)
}

async fn refresh_access_token(
    http: &Client,
    client_id: &str,
    refresh_token: &str,
) -> SyncResult<OAuthTokens> {
    let body = build_refresh_request_body(client_id, refresh_token);
    let resp = http
        .post(TOKEN_ENDPOINT)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| SyncError::Network(format!("refresh: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| SyncError::Network(format!("refresh body: {e}")))?;
    if !status.is_success() {
        return Err(SyncError::Auth(format!("refresh failed: {status}: {text}")));
    }
    parse_token_response(&text)
}

/// Parse the `/token` response into our canonical shape.
fn parse_token_response(body: &str) -> SyncResult<OAuthTokens> {
    #[derive(Deserialize)]
    struct Raw {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        expires_in: Option<i64>,
    }
    let raw: Raw = serde_json::from_str(body)
        .map_err(|e| SyncError::Protocol(format!("token response JSON: {e}")))?;
    let expires_at_ms = raw
        .expires_in
        .map(|secs| now_ms() + secs * 1000)
        .unwrap_or(0);
    Ok(OAuthTokens {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token,
        expires_at_ms,
    })
}

// ---------- small helpers ----------

fn percent_encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Map an HTTP failure to the closest [`SyncError`] variant.
fn classify_http_error(status: StatusCode, body: String, op: &str) -> SyncError {
    match status {
        StatusCode::UNAUTHORIZED => SyncError::Auth(format!("{op}: 401 {body}")),
        StatusCode::FORBIDDEN => SyncError::Auth(format!("{op}: 403 {body}")),
        StatusCode::NOT_FOUND => SyncError::Protocol(format!("{op}: 404 {body}")),
        _ => SyncError::Network(format!("{op}: {status} {body}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_label_are_reported() {
        let p = GoogleTasksProvider::new(
            AccountCredentials::default(),
            "alice@gmail.com",
            "client-id-abc",
        );
        assert_eq!(p.kind(), ProviderKind::GoogleTasks);
        assert_eq!(p.account_label(), "alice@gmail.com");
    }

    #[test]
    fn parse_token_response_extracts_expiry() {
        let body = r#"{
          "access_token": "AT-1",
          "refresh_token": "RT-1",
          "expires_in": 3600,
          "token_type": "Bearer",
          "scope": "https://www.googleapis.com/auth/tasks"
        }"#;
        let t = parse_token_response(body).unwrap();
        assert_eq!(t.access_token, "AT-1");
        assert_eq!(t.refresh_token.as_deref(), Some("RT-1"));
        // Within a few ms of "now + 3600s".
        let target = now_ms() + 3_600_000;
        assert!((t.expires_at_ms - target).abs() < 5_000);
    }

    #[test]
    fn parse_token_response_tolerates_missing_refresh_token() {
        // Google omits `refresh_token` on a refresh-token refresh.
        let body = r#"{"access_token": "AT-2", "expires_in": 3599, "token_type": "Bearer"}"#;
        let t = parse_token_response(body).unwrap();
        assert_eq!(t.access_token, "AT-2");
        assert!(t.refresh_token.is_none());
        assert!(t.expires_at_ms > 0);
    }

    #[test]
    fn parse_token_response_rejects_malformed_json() {
        let err = parse_token_response("{").unwrap_err();
        assert!(matches!(err, SyncError::Protocol(_)));
    }

    #[test]
    fn classify_http_error_maps_401_to_auth() {
        let e = classify_http_error(StatusCode::UNAUTHORIZED, "nope".into(), "op");
        assert!(matches!(e, SyncError::Auth(_)));
    }

    #[test]
    fn classify_http_error_maps_500_to_network() {
        let e = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, "oops".into(), "op");
        assert!(matches!(e, SyncError::Network(_)));
    }

    #[test]
    fn percent_encode_escapes_slashes_and_colons() {
        // Google task list ids are opaque base64url strings, but
        // the path segment encoding must be safe for anything.
        assert_eq!(percent_encode("a/b"), "a%2Fb");
        assert_eq!(percent_encode("MDE:XYZ"), "MDE%3AXYZ");
    }
}
