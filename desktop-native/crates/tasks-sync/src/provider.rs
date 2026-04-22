//! The [`Provider`] trait every sync backend implements, plus the
//! shared value types (`RemoteTask`, `RemoteCalendar`, credentials,
//! error enum) each provider speaks.
//!
//! The trait is intentionally sync-over-async: every method is
//! `async` so the HTTP-heavy providers (CalDAV, Google, Microsoft)
//! can drive the reactor directly, and the FFI-bound EteSync
//! provider can wrap its blocking calls in `spawn_blocking` without
//! leaking that detail to callers.
//!
//! Nothing in this file performs I/O. It's the contract the four
//! `providers::*` modules implement.

use async_trait::async_trait;
use thiserror::Error;

/// Uniform error shape for every provider.
#[derive(Debug, Error)]
pub enum SyncError {
    /// A provider method hasn't been implemented yet. Every stub in
    /// the skeleton returns this so callers fail loudly instead of
    /// silently returning empty data.
    #[error("{provider}: {method} is not yet implemented")]
    NotYetImplemented {
        provider: &'static str,
        method: &'static str,
    },

    /// HTTP / transport error.
    #[error("network error: {0}")]
    Network(String),

    /// Authentication (bad credentials, expired token, revoked app).
    #[error("authentication failed: {0}")]
    Auth(String),

    /// Server responded but the payload didn't match what we
    /// expected (malformed XML/JSON, missing required fields).
    #[error("unexpected server response: {0}")]
    Protocol(String),

    /// Local side error (DB access, file I/O) while merging
    /// remote changes back into the Room schema.
    #[error("local error: {0}")]
    Local(String),

    /// Provider-specific fallback.
    #[error("{0}")]
    Other(String),
}

pub type SyncResult<T> = Result<T, SyncError>;

/// Tag identifying which concrete backend a [`Provider`] is. Used
/// by the UI to persist the account type next to the credentials
/// and to render provider-specific affordances (e.g. the EteSync
/// password field's "this is *not* your server password" hint).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    CalDav,
    GoogleTasks,
    MicrosoftToDo,
    EteSync,
}

impl ProviderKind {
    pub fn display_name(self) -> &'static str {
        match self {
            ProviderKind::CalDav => "CalDAV",
            ProviderKind::GoogleTasks => "Google Tasks",
            ProviderKind::MicrosoftToDo => "Microsoft To Do",
            ProviderKind::EteSync => "EteSync",
        }
    }
}

/// Credentials bundle handed to each provider on construction.
/// Semantics are provider-specific:
/// * CalDAV: `server_url` + `username` + `password` (Basic or
///   Digest), or an OAuth token when the host is Fastmail / iCloud.
/// * Google / Microsoft: `oauth_access_token` + `oauth_refresh_token`;
///   `server_url` is unused.
/// * EteSync: `server_url` + `username` + `password` (used to
///   derive the login + encryption keys — never sent verbatim).
#[derive(Debug, Clone, Default)]
pub struct AccountCredentials {
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub oauth_access_token: Option<String>,
    pub oauth_refresh_token: Option<String>,
}

/// A remote calendar / task list as the provider knows it. Maps
/// onto `caldav_lists` rows locally (`cdl_uuid` = `remote_id`,
/// `cdl_name` = `name`, `cdl_url` = `url`, `cdl_color` = `color`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCalendar {
    pub remote_id: String,
    pub name: String,
    pub url: Option<String>,
    pub color: Option<i32>,
    /// Provider-set change tag ("ctag" for CalDAV, "updated" for
    /// Google). A subsequent `list_tasks` can be skipped when this
    /// is unchanged since the last pull.
    pub change_tag: Option<String>,
    pub read_only: bool,
}

/// A remote task row. We translate to/from `tasks` + `caldav_tasks`
/// rows at the provider boundary — the trait surface speaks this
/// normalised shape so the UI layer doesn't have to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTask {
    pub remote_id: String,
    pub calendar_remote_id: String,
    /// Provider-set per-task change tag ("etag" for CalDAV,
    /// "etag" for Google). Drives incremental push: we only PUT
    /// when the local row has changed since this value was
    /// recorded.
    pub etag: Option<String>,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub due_ms: i64,
    pub completed_ms: i64,
    pub priority: i32,
    pub recurrence: Option<String>,
    pub parent_remote_id: Option<String>,
    /// iCalendar VTODO serialization (CalDAV only) — held so
    /// partial-update PUTs can merge fields back without losing
    /// attachments or alarms the desktop can't yet edit.
    pub raw_vtodo: Option<String>,
}

/// Summary of what a sync round trip accomplished.
#[derive(Debug, Clone, Default)]
pub struct SyncOutcome {
    pub calendars_pulled: usize,
    pub tasks_pulled: usize,
    pub tasks_pushed: usize,
    pub conflicts: usize,
}

/// Every sync backend implements this. The UI layer holds a
/// `Box<dyn Provider>` per connected account and invokes
/// `sync_once()` on a timer (or when the user clicks Sync Now).
#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    /// Human-readable account label (e.g. the user's email, or the
    /// CalDAV server's hostname).
    fn account_label(&self) -> &str;

    /// Verify credentials + discover the service endpoint. Called
    /// once when an account is added.
    async fn connect(&mut self) -> SyncResult<()>;

    /// Fetch the list of calendars / task lists the authenticated
    /// account has access to.
    async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>>;

    /// Fetch every task in the given calendar. Providers that
    /// support incremental pull (via `change_tag`) can skip on an
    /// unchanged ctag; callers check before invoking.
    async fn list_tasks(&mut self, calendar_remote_id: &str) -> SyncResult<Vec<RemoteTask>>;

    /// Push a single task. `task.etag` is the etag the local DB
    /// remembers from the last pull; providers use it as an
    /// `If-Match` to detect concurrent edits. Returns the new etag.
    async fn push_task(&mut self, task: &RemoteTask) -> SyncResult<Option<String>>;

    /// Delete a task remotely.
    async fn delete_task(&mut self, calendar_remote_id: &str, remote_id: &str) -> SyncResult<()>;

    /// Run a full sync cycle (pull changes, push local deltas,
    /// reconcile). The exact algorithm is provider-specific;
    /// callers just want the summary.
    async fn sync_once(&mut self) -> SyncResult<SyncOutcome>;
}
