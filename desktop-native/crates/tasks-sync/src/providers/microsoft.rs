//! Microsoft To Do provider — real implementation.
//!
//! Speaks Microsoft Graph's `/me/todo` surface
//! (<https://learn.microsoft.com/graph/api/resources/todo-overview>)
//! with `reqwest` + rustls. OAuth2 loopback-PKCE flow against
//! the Azure AD v2 endpoints, same shape as [`super::google`]
//! but different URLs + paging + verb for updates:
//! * auth: `https://login.microsoftonline.com/common/oauth2/v2.0/authorize`
//! * token: `https://login.microsoftonline.com/common/oauth2/v2.0/token`
//! * api root: `https://graph.microsoft.com/v1.0/me/todo`
//! * pagination: OData `@odata.nextLink` (absolute URL, not a token)
//! * update: PATCH (Graph; PUT replaces the whole resource, which
//!   we don't want).
//!
//! Scopes: `Tasks.ReadWrite` for the API + `offline_access` so the
//! token endpoint returns a refresh_token.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, IF_MATCH};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use super::microsoft_json::{
    parse_task_lists, parse_tasks, remote_to_task_json, task_list_to_remote_calendar,
    task_to_remote,
};
use crate::oauth::{
    build_authorization_request, build_refresh_request_body, build_token_request_body,
};
use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};
use crate::token_store::{OAuthTokens, TokenStore};

const AUTHORIZATION_ENDPOINT: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const API_ROOT: &str = "https://graph.microsoft.com/v1.0/me/todo";
/// Fixed origin all Graph calls must stay on. `@odata.nextLink` is
/// an absolute URL the server picks, so we clamp every page URL
/// before attaching the Bearer token — otherwise a compromised
/// Graph edge could bounce the request at a third-party host and
/// leak the token. Scheme/host/port match is exact.
const GRAPH_HOST: &str = "graph.microsoft.com";
const GRAPH_SCHEME: &str = "https";
/// `Tasks.ReadWrite` covers todo read + write; `offline_access`
/// is what makes the token endpoint hand us a refresh_token.
const SCOPES: &[&str] = &["Tasks.ReadWrite", "offline_access"];
const REFRESH_GRACE_MS: i64 = 60_000;

struct Session {
    http: Client,
    tokens: OAuthTokens,
}

pub struct MicrosoftToDoProvider {
    credentials: AccountCredentials,
    account_label: String,
    /// Azure AD application (client) id.
    client_id: String,
    token_store: Option<Arc<dyn TokenStore>>,
    session: Arc<tokio::sync::Mutex<Option<Session>>>,
}

impl MicrosoftToDoProvider {
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

    async fn ensure_fresh(&self) -> SyncResult<HeaderValue> {
        let mut guard = self.session.lock().await;
        let s = guard
            .as_mut()
            .ok_or_else(|| SyncError::Auth("Microsoft To Do: connect() first".into()))?;
        if s.tokens.needs_refresh(now_ms(), REFRESH_GRACE_MS) {
            let refresh = s.tokens.refresh_token.clone().ok_or_else(|| {
                SyncError::Auth("access token expired and no refresh_token stored".into())
            })?;
            let new = refresh_access_token(&s.http, &self.client_id, &refresh).await?;
            // Azure AD usually rotates refresh tokens but isn't
            // contractually required to; preserve the previous one
            // if the response omits it.
            let rotated_refresh = new.refresh_token.clone().or(Some(refresh));
            s.tokens = OAuthTokens {
                access_token: new.access_token,
                refresh_token: rotated_refresh,
                expires_at_ms: new.expires_at_ms,
            };
            if let Some(store) = &self.token_store {
                let _ = store.put(ProviderKind::MicrosoftToDo, &self.account_label, &s.tokens);
            }
        }
        HeaderValue::from_str(&format!("Bearer {}", s.tokens.access_token))
            .map_err(|e| SyncError::Auth(format!("bad bearer header: {e}")))
    }
}

#[async_trait]
impl Provider for MicrosoftToDoProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::MicrosoftToDo
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

        let tokens = if let Some(at) = self.credentials.oauth_access_token.clone() {
            OAuthTokens {
                access_token: at,
                refresh_token: self.credentials.oauth_refresh_token.clone(),
                expires_at_ms: 0,
            }
        } else if let Some(store) = &self.token_store {
            store
                .get(ProviderKind::MicrosoftToDo, &self.account_label)
                .ok_or_else(|| {
                    SyncError::Auth(format!(
                        "no stored Microsoft tokens for '{}' — run authorize() first",
                        self.account_label
                    ))
                })?
        } else {
            return Err(SyncError::Auth(
                "Microsoft To Do: no credentials and no TokenStore".into(),
            ));
        };

        *self.session.lock().await = Some(Session { http, tokens });
        Ok(())
    }

    async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
        let auth = self.ensure_fresh().await?;
        let guard = self.session.lock().await;
        let s = guard.as_ref().expect("session populated above");

        let mut url = format!("{API_ROOT}/lists");
        let mut out = Vec::new();
        loop {
            // Clamp every URL (first page + each `@odata.nextLink`)
            // to the Graph origin before attaching the Bearer token.
            check_graph_origin(&url)?;
            let resp = s
                .http
                .get(&url)
                .header(AUTHORIZATION, auth.clone())
                .send()
                .await
                .map_err(|e| SyncError::Network(format!("lists.list: {e}")))?;
            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| SyncError::Network(format!("read body: {e}")))?;
            if !status.is_success() {
                return Err(classify_http_error(status, body, "lists.list"));
            }
            let envelope = parse_task_lists(&body)?;
            for l in &envelope.value {
                out.push(task_list_to_remote_calendar(l));
            }
            match envelope.next_link {
                Some(next) if !next.is_empty() => url = next,
                _ => break,
            }
        }
        Ok(out)
    }

    async fn list_tasks(&mut self, calendar_remote_id: &str) -> SyncResult<Vec<RemoteTask>> {
        let auth = self.ensure_fresh().await?;
        let guard = self.session.lock().await;
        let s = guard.as_ref().expect("session populated above");

        let mut url = format!(
            "{API_ROOT}/lists/{}/tasks",
            percent_encode(calendar_remote_id)
        );
        let mut out = Vec::new();
        loop {
            // Clamp the first page + every `@odata.nextLink` to the
            // Graph origin; see H-1 for the token-leak rationale.
            check_graph_origin(&url)?;
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
            for t in &envelope.value {
                out.push(task_to_remote(t, calendar_remote_id));
            }
            match envelope.next_link {
                Some(next) if !next.is_empty() => url = next,
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

        // Graph uses PATCH (not PUT) for update and POST for create.
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
            let url = format!(
                "{API_ROOT}/lists/{}/tasks/{}",
                percent_encode(&task.calendar_remote_id),
                percent_encode(&task.remote_id)
            );
            s.http
                .patch(&url)
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
        // Graph returns the updated resource with the fresh
        // `@odata.etag` — thread it back so the next pull is idempotent.
        #[derive(Deserialize)]
        struct EtagOnly {
            #[serde(rename = "@odata.etag")]
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
        Err(SyncError::NotYetImplemented {
            provider: "Microsoft To Do",
            method: "sync_once (use SyncEngine::sync_now)",
        })
    }
}

// ---------- OAuth helpers ----------

/// Browser-based authorize: same shape as the Google helper but
/// against Azure AD v2. Caller provides a function that opens a
/// URL in the user's default browser; the loopback receiver picks
/// up the redirect and we exchange for tokens.
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
    let req = build_authorization_request(AUTHORIZATION_ENDPOINT, client_id, SCOPES, &redirect_uri)
        .map_err(|e| SyncError::Auth(format!("build auth url: {e}")))?;
    open_browser(&req.authorization_url);

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

/// Ensure `url` is `https://graph.microsoft.com/...`; reject any
/// other origin before issuing an authenticated request.
fn check_graph_origin(url: &str) -> SyncResult<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| SyncError::Protocol(format!("bad follow-up URL {url}: {e}")))?;
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme == GRAPH_SCHEME && host == GRAPH_HOST {
        Ok(())
    } else {
        Err(SyncError::Protocol(format!(
            "off-origin redirect to {host}"
        )))
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn classify_http_error(status: StatusCode, body: String, op: &str) -> SyncError {
    match status {
        StatusCode::UNAUTHORIZED => SyncError::Auth(format!("{op}: 401 {body}")),
        StatusCode::FORBIDDEN => SyncError::Auth(format!("{op}: 403 {body}")),
        StatusCode::NOT_FOUND => SyncError::Protocol(format!("{op}: 404 {body}")),
        StatusCode::TOO_MANY_REQUESTS => SyncError::Network(format!("{op}: 429 {body}")),
        _ => SyncError::Network(format!("{op}: {status} {body}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_label_are_reported() {
        let p = MicrosoftToDoProvider::new(
            AccountCredentials::default(),
            "alice@outlook.com",
            "client-id",
        );
        assert_eq!(p.kind(), ProviderKind::MicrosoftToDo);
        assert_eq!(p.account_label(), "alice@outlook.com");
    }

    #[test]
    fn parse_token_response_extracts_expiry_and_refresh() {
        let body = r#"{
          "token_type": "Bearer",
          "scope": "Tasks.ReadWrite offline_access",
          "expires_in": 3599,
          "ext_expires_in": 3599,
          "access_token": "AT-1",
          "refresh_token": "RT-1"
        }"#;
        let t = parse_token_response(body).unwrap();
        assert_eq!(t.access_token, "AT-1");
        assert_eq!(t.refresh_token.as_deref(), Some("RT-1"));
        let target = now_ms() + 3_599_000;
        assert!((t.expires_at_ms - target).abs() < 5_000);
    }

    #[test]
    fn parse_token_response_without_refresh_token() {
        let body = r#"{"access_token": "AT-2", "expires_in": 3600, "token_type": "Bearer"}"#;
        let t = parse_token_response(body).unwrap();
        assert!(t.refresh_token.is_none());
    }

    #[test]
    fn parse_token_response_rejects_garbage() {
        let err = parse_token_response("not-json").unwrap_err();
        assert!(matches!(err, SyncError::Protocol(_)));
    }

    #[test]
    fn classify_http_error_buckets_auth_vs_network_vs_protocol() {
        assert!(matches!(
            classify_http_error(StatusCode::UNAUTHORIZED, "x".into(), "op"),
            SyncError::Auth(_)
        ));
        assert!(matches!(
            classify_http_error(StatusCode::FORBIDDEN, "x".into(), "op"),
            SyncError::Auth(_)
        ));
        assert!(matches!(
            classify_http_error(StatusCode::NOT_FOUND, "x".into(), "op"),
            SyncError::Protocol(_)
        ));
        assert!(matches!(
            classify_http_error(StatusCode::TOO_MANY_REQUESTS, "x".into(), "op"),
            SyncError::Network(_)
        ));
        assert!(matches!(
            classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, "x".into(), "op"),
            SyncError::Network(_)
        ));
    }

    #[test]
    fn percent_encode_safeguards_path_segments() {
        assert_eq!(percent_encode("AAMkA/lis"), "AAMkA%2Flis");
    }

    #[test]
    fn check_graph_origin_rejects_off_origin_nextlink() {
        // Baseline: the real Graph host is accepted.
        assert!(check_graph_origin("https://graph.microsoft.com/v1.0/me/todo/lists").is_ok());
        // A malicious `@odata.nextLink` pointing elsewhere is rejected.
        let err = check_graph_origin("https://evil.example.org/v1.0/me/todo/lists").unwrap_err();
        assert!(matches!(err, SyncError::Protocol(m) if m.contains("evil.example.org")));
        // Scheme downgrade → rejected.
        assert!(check_graph_origin("http://graph.microsoft.com/v1.0/me/todo/lists").is_err());
        // Subdomain shenanigans → rejected (exact host match only).
        assert!(check_graph_origin("https://graph.microsoft.com.evil.example.org/v1.0").is_err());
    }
}
