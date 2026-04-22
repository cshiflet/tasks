//! Google Tasks provider (Milestone 4).
//!
//! Planned stack:
//! * `reqwest` with rustls for HTTPS.
//! * `oauth2` crate for the loopback-redirect flow (system
//!   browser opens, redirect to `http://127.0.0.1:<port>`).
//! * JSON via `serde_json` for the REST payloads.
//! * Token persistence via the shared `TokenStore` trait (M4
//!   will land a `secret-service` / Keychain / Credential Manager
//!   adapter).
//!
//! Skeleton: every network method returns
//! `NotYetImplemented` so the crate compiles cleanly today.

use async_trait::async_trait;

use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};

pub struct GoogleTasksProvider {
    credentials: AccountCredentials,
    account_label: String,
}

impl GoogleTasksProvider {
    pub fn new(credentials: AccountCredentials, account_label: impl Into<String>) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
        }
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
        tracing::debug!(
            "GoogleTasksProvider::connect stub, oauth_present={}",
            self.credentials.oauth_access_token.is_some()
        );
        not_yet("connect")
    }

    async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
        not_yet("list_calendars")
    }

    async fn list_tasks(&mut self, _calendar_remote_id: &str) -> SyncResult<Vec<RemoteTask>> {
        not_yet("list_tasks")
    }

    async fn push_task(&mut self, _task: &RemoteTask) -> SyncResult<Option<String>> {
        not_yet("push_task")
    }

    async fn delete_task(&mut self, _calendar_remote_id: &str, _remote_id: &str) -> SyncResult<()> {
        not_yet("delete_task")
    }

    async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
        not_yet("sync_once")
    }
}

fn not_yet<T>(method: &'static str) -> SyncResult<T> {
    Err(SyncError::NotYetImplemented {
        provider: "Google Tasks",
        method,
    })
}
