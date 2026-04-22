//! EteSync provider (Milestone 5).
//!
//! EteSync is end-to-end encrypted, so the provider never sees
//! plaintext off the wire; the Etebase SDK (libetebase-rs, or the
//! C FFI if the Rust wrapper isn't suitable) handles login, key
//! derivation, collection listing, and item CRUD. Our trait
//! surface is the same — the encryption is plumbing.
//!
//! Skeleton: `NotYetImplemented` everywhere.

use async_trait::async_trait;

use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};

pub struct EteSyncProvider {
    credentials: AccountCredentials,
    account_label: String,
}

impl EteSyncProvider {
    pub fn new(credentials: AccountCredentials, account_label: impl Into<String>) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
        }
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
        tracing::debug!(
            "EteSyncProvider::connect stub for server {:?}",
            self.credentials.server_url
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
        provider: "EteSync",
        method,
    })
}
