//! Microsoft To Do provider (Milestone 4).
//!
//! Same shape as Google Tasks: OAuth2 loopback flow, JSON over
//! HTTPS. Different API surface (Microsoft Graph), different
//! scopes, different rate-limit headers — but the trait surface
//! is identical.
//!
//! Skeleton only; methods return `NotYetImplemented`.

use async_trait::async_trait;

use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};

pub struct MicrosoftToDoProvider {
    credentials: AccountCredentials,
    account_label: String,
}

impl MicrosoftToDoProvider {
    pub fn new(credentials: AccountCredentials, account_label: impl Into<String>) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
        }
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
        tracing::debug!(
            "MicrosoftToDoProvider::connect stub, oauth_present={}",
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
        provider: "Microsoft To Do",
        method,
    })
}
