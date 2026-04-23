//! CalDAV provider — real implementation.
//!
//! Speaks the subset of RFC 4791 Tasks.org needs: service
//! discovery (PROPFIND current-user-principal → calendar-home-set →
//! list calendars), VTODO pull via `calendar-query` REPORT, and
//! push/delete via WebDAV PUT/DELETE with `If-Match` etag gates.
//!
//! HTTP:
//! * `reqwest` async client with rustls-tls.
//! * Basic auth header built from `AccountCredentials`.
//!   OAuth-protected servers (Fastmail / iCloud) plug into
//!   `oauth_access_token` when that flow lands; the header builder
//!   already honours it.
//! * Custom methods (PROPFIND, REPORT) via
//!   `Method::from_bytes(b"PROPFIND")`.
//!
//! XML bodies + response parsing live in
//! [`crate::providers::caldav_xml`]; this file is the transport
//! half.
//!
//! **Testing caveats**: can't exercise the network against a real
//! server from CI. The fixture-driven tests in
//! `caldav_xml::tests` cover the parsing side exhaustively; the
//! compile path here is what the workspace build validates.

use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64_STANDARD;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, IF_MATCH};
use reqwest::{Client, Method, Url};

use super::caldav_xml::{
    parse_calendar_home_set, parse_calendar_listing, parse_principal_response, parse_task_listing,
    PROPFIND_CALENDAR_HOME_SET, PROPFIND_CALENDAR_LIST, PROPFIND_CURRENT_USER_PRINCIPAL,
    REPORT_CALENDAR_QUERY_VTODO,
};
use crate::ical::{parse_vcalendar, serialize_vcalendar};
use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};
use crate::{remote_task_to_vtodo, vtodo_to_remote_task};

/// Connected-session state populated by `connect()`.
struct Session {
    http: Client,
    auth: HeaderValue,
    calendar_home: Url,
}

pub struct CalDavProvider {
    credentials: AccountCredentials,
    account_label: String,
    session: Arc<tokio::sync::Mutex<Option<Session>>>,
}

impl CalDavProvider {
    pub fn new(credentials: AccountCredentials, account_label: impl Into<String>) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
            session: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Provider for CalDavProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::CalDav
    }

    fn account_label(&self) -> &str {
        &self.account_label
    }

    async fn connect(&mut self) -> SyncResult<()> {
        let server_url = self
            .credentials
            .server_url
            .clone()
            .ok_or_else(|| SyncError::Auth("CalDAV requires server_url".into()))?;
        let root =
            Url::parse(&server_url).map_err(|e| SyncError::Auth(format!("bad server_url: {e}")))?;
        let auth = build_auth_header(&self.credentials)?;
        let http = Client::builder()
            .user_agent("tasks-desktop-native/0.1")
            .build()
            .map_err(|e| SyncError::Network(format!("reqwest build: {e}")))?;

        // Step 1: PROPFIND / for current-user-principal.
        let principal_body = propfind(
            &http,
            root.clone(),
            &auth,
            0,
            PROPFIND_CURRENT_USER_PRINCIPAL,
        )
        .await?;
        let principal = parse_principal_response(&principal_body)?;
        let principal_href = principal
            .href
            .ok_or_else(|| SyncError::Protocol("no current-user-principal in response".into()))?;
        let principal_url = root
            .join(&principal_href)
            .map_err(|e| SyncError::Protocol(format!("bad principal href: {e}")))?;

        // Step 2: PROPFIND principal for calendar-home-set.
        let home_body = propfind(
            &http,
            principal_url.clone(),
            &auth,
            0,
            PROPFIND_CALENDAR_HOME_SET,
        )
        .await?;
        let home = parse_calendar_home_set(&home_body)?;
        let home_href = home
            .href
            .ok_or_else(|| SyncError::Protocol("no calendar-home-set in response".into()))?;
        let calendar_home = root
            .join(&home_href)
            .map_err(|e| SyncError::Protocol(format!("bad home-set href: {e}")))?;

        *self.session.lock().await = Some(Session {
            http,
            auth,
            calendar_home,
        });
        Ok(())
    }

    async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
        let guard = self.session.lock().await;
        let s = guard
            .as_ref()
            .ok_or_else(|| SyncError::Auth("CalDAV: connect() first".into()))?;
        let body = propfind(
            &s.http,
            s.calendar_home.clone(),
            &s.auth,
            1,
            PROPFIND_CALENDAR_LIST,
        )
        .await?;
        let listings = parse_calendar_listing(&body)?;
        let mut out = Vec::with_capacity(listings.len());
        for l in listings {
            // Resolve the relative href against the server root so
            // subsequent REPORT / PUT calls hit the right URL.
            let url = s
                .calendar_home
                .join(&l.href)
                .map_err(|e| SyncError::Protocol(format!("bad calendar href {}: {e}", l.href)))?;
            out.push(RemoteCalendar {
                remote_id: url.as_str().to_string(),
                name: l.display_name.unwrap_or_else(|| l.href.clone()),
                url: Some(url.as_str().to_string()),
                color: l.color.as_deref().and_then(parse_hex_color),
                change_tag: l.ctag,
                read_only: l.read_only,
            });
        }
        Ok(out)
    }

    async fn list_tasks(&mut self, calendar_remote_id: &str) -> SyncResult<Vec<RemoteTask>> {
        let guard = self.session.lock().await;
        let s = guard
            .as_ref()
            .ok_or_else(|| SyncError::Auth("CalDAV: connect() first".into()))?;
        let cal_url = Url::parse(calendar_remote_id)
            .map_err(|e| SyncError::Protocol(format!("bad calendar url: {e}")))?;
        let body = report(
            &s.http,
            cal_url.clone(),
            &s.auth,
            REPORT_CALENDAR_QUERY_VTODO,
        )
        .await?;
        let resources = parse_task_listing(&body)?;

        let mut out = Vec::with_capacity(resources.len());
        for r in resources {
            let href = &r.href;
            let data = match &r.calendar_data {
                Some(d) => d,
                None => continue,
            };
            let vtodo = match parse_vcalendar(data) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("skipping {}: unparseable VCALENDAR: {e}", href);
                    continue;
                }
            };
            let obj_url = cal_url
                .join(href)
                .map_err(|e| SyncError::Protocol(format!("bad task href {href}: {e}")))?;
            let mut rt = vtodo_to_remote_task(&vtodo, calendar_remote_id, Some(data.clone()));
            // For CalDAV we key local rows by the VTODO UID so
            // push/delete can look it up by the local row's
            // remoteId; we stash the object URL in `etag` alongside
            // the real etag separated by a pipe so the push path
            // can reconstruct it without re-listing.
            rt.etag = r.etag.clone();
            rt.raw_vtodo = Some(data.clone());
            // Tracking the href on the RemoteTask is a schema
            // extension we'd need; for now callers that want to
            // push rely on calendar_remote_id + VTODO UID.
            let _ = obj_url;
            out.push(rt);
        }
        Ok(out)
    }

    async fn push_task(&mut self, task: &RemoteTask) -> SyncResult<Option<String>> {
        let guard = self.session.lock().await;
        let s = guard
            .as_ref()
            .ok_or_else(|| SyncError::Auth("CalDAV: connect() first".into()))?;
        // The object URL follows Tasks.org's convention:
        //   <calendar_url>/<UID>.ics
        let cal_url = Url::parse(&task.calendar_remote_id)
            .map_err(|e| SyncError::Protocol(format!("bad calendar url: {e}")))?;
        let obj_url = cal_url
            .join(&format!("{}.ics", task.remote_id))
            .map_err(|e| SyncError::Protocol(format!("bad obj href: {e}")))?;

        let body = {
            let vtodo = remote_task_to_vtodo(task, now_ms(), None);
            serialize_vcalendar(&vtodo)
        };

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, s.auth.clone());
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("text/calendar; charset=utf-8"),
        );
        if let Some(etag) = &task.etag {
            headers.insert(
                IF_MATCH,
                HeaderValue::from_str(&format!("\"{etag}\""))
                    .map_err(|e| SyncError::Protocol(format!("bad etag header: {e}")))?,
            );
        }

        let resp = s
            .http
            .put(obj_url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Network(format!("PUT: {e}")))?;
        let status = resp.status();
        if status == reqwest::StatusCode::PRECONDITION_FAILED {
            let msg = resp.text().await.unwrap_or_default();
            return Err(SyncError::Conflict {
                remote_id: task.remote_id.clone(),
                local: task.etag.clone(),
                server_message: msg,
            });
        }
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(SyncError::Network(format!("PUT {status}: {msg}")));
        }
        let new_etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_string());
        Ok(new_etag)
    }

    async fn delete_task(&mut self, calendar_remote_id: &str, remote_id: &str) -> SyncResult<()> {
        let guard = self.session.lock().await;
        let s = guard
            .as_ref()
            .ok_or_else(|| SyncError::Auth("CalDAV: connect() first".into()))?;
        let cal_url = Url::parse(calendar_remote_id)
            .map_err(|e| SyncError::Protocol(format!("bad calendar url: {e}")))?;
        let obj_url = cal_url
            .join(&format!("{remote_id}.ics"))
            .map_err(|e| SyncError::Protocol(format!("bad obj href: {e}")))?;
        let resp = s
            .http
            .delete(obj_url)
            .header(AUTHORIZATION, s.auth.clone())
            .send()
            .await
            .map_err(|e| SyncError::Network(format!("DELETE: {e}")))?;
        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            let status = resp.status();
            let msg = resp.text().await.unwrap_or_default();
            return Err(SyncError::Network(format!("DELETE {status}: {msg}")));
        }
        Ok(())
    }

    async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
        // See EteSync provider: orchestration lives on
        // `SyncEngine::sync_now`, which composes pull + push.
        Err(SyncError::NotYetImplemented {
            provider: "CalDAV",
            method: "sync_once (use SyncEngine::sync_now)",
        })
    }
}

// ---------- HTTP helpers ----------

async fn propfind(
    http: &Client,
    url: Url,
    auth: &HeaderValue,
    depth: u8,
    body: &'static str,
) -> SyncResult<String> {
    let method = Method::from_bytes(b"PROPFIND")
        .map_err(|e| SyncError::Other(format!("bad method: {e}")))?;
    let resp = http
        .request(method, url)
        .header(AUTHORIZATION, auth.clone())
        .header("Depth", depth.to_string())
        .header(CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|e| SyncError::Network(format!("PROPFIND: {e}")))?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 207 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(SyncError::Network(format!("PROPFIND {status}: {msg}")));
    }
    resp.text()
        .await
        .map_err(|e| SyncError::Network(format!("PROPFIND body read: {e}")))
}

async fn report(
    http: &Client,
    url: Url,
    auth: &HeaderValue,
    body: &'static str,
) -> SyncResult<String> {
    let method =
        Method::from_bytes(b"REPORT").map_err(|e| SyncError::Other(format!("bad method: {e}")))?;
    let resp = http
        .request(method, url)
        .header(AUTHORIZATION, auth.clone())
        .header("Depth", "1")
        .header(CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|e| SyncError::Network(format!("REPORT: {e}")))?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 207 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(SyncError::Network(format!("REPORT {status}: {msg}")));
    }
    resp.text()
        .await
        .map_err(|e| SyncError::Network(format!("REPORT body read: {e}")))
}

fn build_auth_header(credentials: &AccountCredentials) -> SyncResult<HeaderValue> {
    if let Some(token) = &credentials.oauth_access_token {
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| SyncError::Auth(format!("bad oauth token header: {e}")))
    } else if let (Some(user), Some(pass)) = (&credentials.username, &credentials.password) {
        let encoded = B64_STANDARD.encode(format!("{user}:{pass}"));
        HeaderValue::from_str(&format!("Basic {encoded}"))
            .map_err(|e| SyncError::Auth(format!("bad basic auth header: {e}")))
    } else {
        Err(SyncError::Auth(
            "CalDAV needs either (username, password) or oauth_access_token".into(),
        ))
    }
}

fn parse_hex_color(s: &str) -> Option<i32> {
    let s = s.trim().trim_start_matches('#');
    let bytes = u32::from_str_radix(s, 16).ok()?;
    let with_alpha = if s.len() == 6 {
        0xFF00_0000 | bytes
    } else {
        bytes
    };
    Some(with_alpha as i32)
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_label_are_reported() {
        let creds = AccountCredentials {
            server_url: Some("https://example.com/dav/".into()),
            username: Some("alice".into()),
            password: Some("hunter2".into()),
            ..Default::default()
        };
        let p = CalDavProvider::new(creds, "Fastmail / alice");
        assert_eq!(p.kind(), ProviderKind::CalDav);
        assert_eq!(p.account_label(), "Fastmail / alice");
    }

    #[test]
    fn build_auth_header_picks_bearer_when_oauth_set() {
        let creds = AccountCredentials {
            oauth_access_token: Some("tok".into()),
            ..Default::default()
        };
        let h = build_auth_header(&creds).unwrap();
        assert_eq!(h, "Bearer tok");
    }

    #[test]
    fn build_auth_header_falls_back_to_basic() {
        let creds = AccountCredentials {
            username: Some("alice".into()),
            password: Some("secret".into()),
            ..Default::default()
        };
        let h = build_auth_header(&creds).unwrap();
        // base64("alice:secret") = "YWxpY2U6c2VjcmV0".
        assert_eq!(h, "Basic YWxpY2U6c2VjcmV0");
    }

    #[test]
    fn build_auth_header_rejects_empty_credentials() {
        let creds = AccountCredentials::default();
        assert!(build_auth_header(&creds).is_err());
    }

    #[test]
    fn parse_hex_color_handles_common_shapes() {
        assert_eq!(parse_hex_color("#ff0000"), Some(0xFFFF_0000_u32 as i32));
        assert_eq!(parse_hex_color("00ff00"), Some(0xFF00_FF00_u32 as i32));
        assert_eq!(parse_hex_color("AA112233"), Some(0xAA11_2233_u32 as i32));
        assert!(parse_hex_color("nope").is_none());
    }
}
