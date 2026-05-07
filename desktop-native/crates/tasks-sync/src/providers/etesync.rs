//! EteSync provider — real implementation.
//!
//! Speaks to an Etebase server (typically <https://api.etebase.com>
//! or a self-hosted install) via the `etebase` crate. Collections
//! of type `etebase.vtodo` map onto CalDAV-style calendars; items
//! inside each collection carry iCalendar VTODO content that our
//! [`crate::ical`] parser already understands.
//!
//! The `etebase` crate is synchronous and blocking (it wraps
//! `sodiumoxide` for the client-side crypto), so every provider
//! method wraps its real work in `tokio::task::spawn_blocking`.
//! The desktop UI drives this from its Qt event loop → Rust
//! async path, so the Tokio runtime is already running.
//!
//! **Testing reality**: the actual network / crypto path cannot
//! be exercised from CI (needs a live Etebase account). The
//! compile path *is* validated by the workspace build; the per-
//! method logic (credential validation, collection-type filter,
//! VTODO-to-RemoteTask conversion via the shared [`crate::convert`]
//! module) is straight-line code the compiler catches typing
//! mistakes in. End-to-end smoke testing is the user's job.

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use etebase::{
    Account, Client, Collection, CollectionAccessLevel, FetchOptions, Item, ItemMetadata,
};
use secrecy::ExposeSecret;

use crate::ical::{parse_vcalendar, serialize_vcalendar};
use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};
use crate::{remote_task_to_vtodo, vtodo_to_remote_task};

/// EteSync collections we're interested in.
const COLLECTION_TYPE_VTODO: &str = "etebase.vtodo";

/// Convenience: wrap Etebase's error type into our `SyncError`.
fn map_err<T: std::fmt::Display>(label: &'static str, e: T) -> SyncError {
    SyncError::Network(format!("{label}: {e}"))
}

/// Take the session mutex, turning the `PoisonError` into a
/// `SyncError` instead of panicking (M-6). A panic on the holding
/// thread would otherwise propagate here via `.unwrap()` and crash
/// whichever worker happened to call next.
fn lock_session(state: &Mutex<Option<Account>>) -> SyncResult<MutexGuard<'_, Option<Account>>> {
    state
        .lock()
        .map_err(|_| SyncError::Other("etesync session poisoned".into()))
}

pub struct EteSyncProvider {
    credentials: AccountCredentials,
    account_label: String,
    // When `true`, `connect()` falls back to `Account::signup` if
    // login fails — useful against test servers running with
    // `AUTO_SIGNUP=true` so the desktop can materialise its test
    // user on first connect. Off by default; turn on per-account
    // via `with_signup_fallback(true)` when the server URL points
    // at a known auto-signup deployment.
    allow_signup: bool,
    // Held behind Arc<Mutex<_>> so spawn_blocking closures can
    // clone the handle and take the lock on the worker thread.
    // Account itself isn't Clone; mutex access is the cheapest
    // way to share across async boundaries without tokio::sync
    // crossing into blocking code.
    state: Arc<Mutex<Option<Account>>>,
}

impl EteSyncProvider {
    pub fn new(credentials: AccountCredentials, account_label: impl Into<String>) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
            allow_signup: false,
            state: Arc::new(Mutex::new(None)),
        }
    }

    /// Builder-style toggle for the auto-signup fallback. The
    /// bridge passes `true` for known test-server URLs (e.g.
    /// `http://127.0.0.1:3735`) so first-time connect against the
    /// docker-compose stack creates the account on the fly.
    pub fn with_signup_fallback(mut self, allow: bool) -> Self {
        self.allow_signup = allow;
        self
    }
}

#[async_trait]
impl Provider for EteSyncProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::EteSync
    }

    fn account_label(&self) -> &str {
        &self.account_label
    }

    async fn connect(&mut self) -> SyncResult<()> {
        // Etebase requires libsodium init on first use per process.
        // Calling it more than once is harmless — the crate guards.
        etebase::init().map_err(|e| map_err("etebase::init", e))?;

        let server_url = self
            .credentials
            .server_url
            .clone()
            .unwrap_or_else(|| "https://api.etebase.com".to_string());
        let username = self
            .credentials
            .username
            .clone()
            .ok_or_else(|| SyncError::Auth("EteSync requires a username".into()))?;
        let password = self
            .credentials
            .password
            .clone()
            .ok_or_else(|| SyncError::Auth("EteSync requires a password".into()))?;

        let state = self.state.clone();
        let allow_signup = self.allow_signup;
        let joined = tokio::task::spawn_blocking(move || -> SyncResult<()> {
            let client = Client::new("tasks-desktop-native", &server_url)
                .map_err(|e| map_err("Client::new", e))?;
            // Single spot where the password leaves its secret
            // wrapper and crosses into the `etebase` FFI; the
            // wrapper stays live until this closure returns.
            let pwd = password.expose_secret().to_string();
            let account = match Account::login(client, &username, &pwd) {
                Ok(a) => a,
                Err(login_err) if allow_signup => {
                    // The server's `AUTO_SIGNUP=true` flag opens
                    // `/api/v1/authentication/signup/`; clients can
                    // pre-create the account at first connect rather
                    // than requiring an out-of-band setup step.
                    // Build a fresh client + try signup with the
                    // same credentials; on any failure surface the
                    // *original* login error so the user sees the
                    // primary problem rather than a secondary
                    // signup-already-exists noise.
                    let client2 = Client::new("tasks-desktop-native", &server_url)
                        .map_err(|e| map_err("Client::new (signup retry)", e))?;
                    let user = etebase::User::new(
                        &username,
                        // Etebase requires a non-empty email; if we
                        // don't have one, synthesize a deterministic
                        // localhost address rather than rejecting
                        // the signup. Real deployments should set a
                        // real email via the Accounts pane.
                        &format!("{username}@localhost"),
                    );
                    Account::signup(client2, &user, &pwd).map_err(|signup_err| {
                        SyncError::Auth(format!(
                            "login: {login_err} (signup fallback also failed: {signup_err})"
                        ))
                    })?
                }
                Err(e) => return Err(SyncError::Auth(format!("login: {e}"))),
            };
            *lock_session(&state)? = Some(account);
            Ok(())
        })
        .await
        .map_err(|e| SyncError::Other(format!("spawn_blocking: {e}")))?;
        joined
    }

    async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || -> SyncResult<Vec<RemoteCalendar>> {
            let guard = lock_session(&state)?;
            let account = guard
                .as_ref()
                .ok_or_else(|| SyncError::Auth("EteSync: connect() first".into()))?;
            let col_mgr = account
                .collection_manager()
                .map_err(|e| map_err("collection_manager", e))?;
            let opts = FetchOptions::new();
            let resp = col_mgr
                .list(COLLECTION_TYPE_VTODO, Some(&opts))
                .map_err(|e| map_err("collections.list", e))?;
            let mut out = Vec::with_capacity(resp.data().len());
            for col in resp.data() {
                out.push(collection_to_remote_calendar(col)?);
            }
            Ok(out)
        })
        .await
        .map_err(|e| SyncError::Other(format!("spawn_blocking: {e}")))?
    }

    async fn list_tasks(&mut self, calendar_remote_id: &str) -> SyncResult<Vec<RemoteTask>> {
        let state = self.state.clone();
        let cal_uid = calendar_remote_id.to_string();
        tokio::task::spawn_blocking(move || -> SyncResult<Vec<RemoteTask>> {
            let guard = lock_session(&state)?;
            let account = guard
                .as_ref()
                .ok_or_else(|| SyncError::Auth("EteSync: connect() first".into()))?;
            let col_mgr = account
                .collection_manager()
                .map_err(|e| map_err("collection_manager", e))?;
            let col = col_mgr
                .fetch(&cal_uid, None)
                .map_err(|e| map_err("collection.fetch", e))?;
            let item_mgr = col_mgr
                .item_manager(&col)
                .map_err(|e| map_err("item_manager", e))?;
            let opts = FetchOptions::new();
            let resp = item_mgr
                .list(Some(&opts))
                .map_err(|e| map_err("items.list", e))?;

            let mut out = Vec::with_capacity(resp.data().len());
            for item in resp.data() {
                if item.is_deleted() {
                    // EteSync surfaces deletes as tombstones in the
                    // item list. The sync engine's
                    // tombstone-missing-tasks path will pick those
                    // up on the next pull; we just skip them here.
                    continue;
                }
                let content_bytes = item.content().map_err(|e| map_err("item.content", e))?;
                let content = String::from_utf8(content_bytes).map_err(|e| {
                    SyncError::Protocol(format!("item {} content not UTF-8: {e}", item.uid()))
                })?;
                // EteSync items *can* carry a non-VCALENDAR payload
                // (raw JSON for custom types). For `etebase.vtodo`,
                // every item is a VCALENDAR wrapping one VTODO.
                let vtodo = match parse_vcalendar(&content) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("skipping item {}: unparseable VCALENDAR: {e}", item.uid());
                        continue;
                    }
                };
                let mut rt = vtodo_to_remote_task(&vtodo, &cal_uid, Some(content));
                // EteSync items are keyed by their own UID, not
                // the VTODO UID. Keep both — use the item UID as
                // our `remote_id` so push/delete round-trips
                // address the right item.
                rt.remote_id = item.uid().to_string();
                rt.etag = Some(item.uid().to_string());
                out.push(rt);
            }
            Ok(out)
        })
        .await
        .map_err(|e| SyncError::Other(format!("spawn_blocking: {e}")))?
    }

    async fn push_task(&mut self, task: &RemoteTask) -> SyncResult<Option<String>> {
        let state = self.state.clone();
        let cal_uid = task.calendar_remote_id.clone();
        let remote_id = task.remote_id.clone();
        let vtodo_bytes = {
            // Build the VCALENDAR bytes on the async thread so we
            // don't need to Send the full `&RemoteTask` into the
            // blocking closure.
            let now_ms = now_ms();
            let vtodo = remote_task_to_vtodo(task, now_ms, None);
            serialize_vcalendar(&vtodo).into_bytes()
        };
        let now_ms = now_ms();

        tokio::task::spawn_blocking(move || -> SyncResult<Option<String>> {
            let guard = lock_session(&state)?;
            let account = guard
                .as_ref()
                .ok_or_else(|| SyncError::Auth("EteSync: connect() first".into()))?;
            let col_mgr = account
                .collection_manager()
                .map_err(|e| map_err("collection_manager", e))?;
            let col = col_mgr
                .fetch(&cal_uid, None)
                .map_err(|e| map_err("collection.fetch", e))?;
            let item_mgr = col_mgr
                .item_manager(&col)
                .map_err(|e| map_err("item_manager", e))?;

            // Find existing item by uid (our `remote_id` is the
            // Etebase item uid). If it doesn't exist yet, create.
            let existing = item_mgr.fetch(&remote_id, None).ok();
            let mut meta = ItemMetadata::new();
            meta.set_item_type(Some("task"));
            meta.set_mtime(Some(now_ms));

            match existing {
                Some(mut item) => {
                    item.set_meta(&meta)
                        .map_err(|e| map_err("item.set_meta", e))?;
                    item.set_content(&vtodo_bytes)
                        .map_err(|e| map_err("item.set_content", e))?;
                    item_mgr
                        .batch(std::iter::once(&item), None)
                        .map_err(|e| map_err("item_mgr.batch", e))?;
                    Ok(Some(item.uid().to_string()))
                }
                None => {
                    let item: Item = item_mgr
                        .create(&meta, &vtodo_bytes)
                        .map_err(|e| map_err("item_mgr.create", e))?;
                    item_mgr
                        .batch(std::iter::once(&item), None)
                        .map_err(|e| map_err("item_mgr.batch (create)", e))?;
                    Ok(Some(item.uid().to_string()))
                }
            }
        })
        .await
        .map_err(|e| SyncError::Other(format!("spawn_blocking: {e}")))?
    }

    async fn delete_task(&mut self, calendar_remote_id: &str, remote_id: &str) -> SyncResult<()> {
        let state = self.state.clone();
        let cal_uid = calendar_remote_id.to_string();
        let item_uid = remote_id.to_string();
        tokio::task::spawn_blocking(move || -> SyncResult<()> {
            let guard = lock_session(&state)?;
            let account = guard
                .as_ref()
                .ok_or_else(|| SyncError::Auth("EteSync: connect() first".into()))?;
            let col_mgr = account
                .collection_manager()
                .map_err(|e| map_err("collection_manager", e))?;
            let col = col_mgr
                .fetch(&cal_uid, None)
                .map_err(|e| map_err("collection.fetch", e))?;
            let item_mgr = col_mgr
                .item_manager(&col)
                .map_err(|e| map_err("item_manager", e))?;
            let mut item = item_mgr
                .fetch(&item_uid, None)
                .map_err(|e| map_err("item.fetch", e))?;
            item.delete().map_err(|e| map_err("item.delete", e))?;
            item_mgr
                .batch(std::iter::once(&item), None)
                .map_err(|e| map_err("item_mgr.batch (delete)", e))?;
            Ok(())
        })
        .await
        .map_err(|e| SyncError::Other(format!("spawn_blocking: {e}")))?
    }

    async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
        // Higher-level orchestration (pull + push + reconcile)
        // lives on `crate::engine::SyncEngine`. Provider's own
        // `sync_once` stays as a "not needed yet" marker because
        // the engine already composes pull_all + push_dirty.
        Err(SyncError::NotYetImplemented {
            provider: "EteSync",
            method: "sync_once (use SyncEngine::sync_now)",
        })
    }

    async fn create_calendar(
        &mut self,
        name: &str,
        color: Option<i32>,
    ) -> SyncResult<RemoteCalendar> {
        let state = self.state.clone();
        let name_owned = name.to_string();
        tokio::task::spawn_blocking(move || -> SyncResult<RemoteCalendar> {
            let guard = lock_session(&state)?;
            let account = guard
                .as_ref()
                .ok_or_else(|| SyncError::Auth("EteSync: connect() first".into()))?;
            let col_mgr = account
                .collection_manager()
                .map_err(|e| map_err("collection_manager", e))?;
            let mut meta = ItemMetadata::new();
            meta.set_name(Some(&name_owned));
            meta.set_mtime(Some(now_ms_blocking()));
            // Apple-style #RRGGBBAA serialization keeps parity with
            // the CalDAV provider's MKCALENDAR body. 0 (or None) =
            // "no colour".
            if let Some(c) = color {
                let argb = c as u32;
                let alpha = ((argb >> 24) & 0xff) as u8;
                let red = ((argb >> 16) & 0xff) as u8;
                let green = ((argb >> 8) & 0xff) as u8;
                let blue = (argb & 0xff) as u8;
                meta.set_color(Some(&format!("#{red:02X}{green:02X}{blue:02X}{alpha:02X}")));
            }
            let collection = col_mgr
                .create(COLLECTION_TYPE_VTODO, &meta, b"")
                .map_err(|e| map_err("collection_manager.create", e))?;
            col_mgr
                .upload(&collection, None)
                .map_err(|e| map_err("collection_manager.upload", e))?;
            collection_to_remote_calendar(&collection)
        })
        .await
        .map_err(|e| SyncError::Other(format!("spawn_blocking: {e}")))?
    }
}

fn now_ms_blocking() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn collection_to_remote_calendar(col: &Collection) -> SyncResult<RemoteCalendar> {
    let meta = col.meta().map_err(|e| map_err("collection.meta", e))?;
    let name = meta.name().unwrap_or_default().to_string();
    let color = meta.color().and_then(parse_hex_color);
    let read_only = matches!(col.access_level(), CollectionAccessLevel::ReadOnly);
    Ok(RemoteCalendar {
        remote_id: col.uid().to_string(),
        name,
        url: None,
        color,
        change_tag: col.stoken().map(String::from),
        read_only,
    })
}

/// "#rrggbb" → signed 32-bit integer (Android-parity: ARGB with
/// FF alpha). Returns None on any shape we don't recognise.
fn parse_hex_color(s: &str) -> Option<i32> {
    let s = s.trim().trim_start_matches('#');
    let bytes = u32::from_str_radix(s, 16).ok()?;
    // Add opaque alpha if user provided a plain #RRGGBB.
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
        let creds = AccountCredentials::new_password("https://api.etebase.com", "alice", "hunter2");
        let p = EteSyncProvider::new(creds, "alice@etebase");
        assert_eq!(p.kind(), ProviderKind::EteSync);
        assert_eq!(p.account_label(), "alice@etebase");
    }

    #[test]
    fn parse_hex_color_handles_common_shapes() {
        assert_eq!(parse_hex_color("#ff0000"), Some(0xFFFF_0000_u32 as i32));
        assert_eq!(parse_hex_color("00ff00"), Some(0xFF00_FF00_u32 as i32));
        // With alpha prefix.
        assert_eq!(parse_hex_color("#AA112233"), Some(0xAA11_2233_u32 as i32));
        assert!(parse_hex_color("not-a-color").is_none());
    }

    // Network-dependent tests (login / list collections / round-trip
    // a VTODO) live in a separate binary that the user runs against
    // their real account — the automated suite can't exercise them
    // without credentials.
}
