//! CalDAV provider (Milestone 3).
//!
//! Planned stack when this lands:
//! * `reqwest` with rustls-tls for the HTTP client.
//! * `quick-xml` for PROPFIND / REPORT / mkcol payloads.
//! * A lightweight iCalendar VTODO parser — either a port of the
//!   relevant bits of `libical`, or a direct pull-parser that
//!   handles the subset Tasks.org emits.
//! * OAuth flow for Fastmail / iCloud (Milestone 3.x); plain
//!   Basic / Digest otherwise.
//!
//! See README milestone 3 for the full task list.
//!
//! This module is currently a *skeleton*: every method returns
//! [`SyncError::NotYetImplemented`] so the compilable crate surface
//! stays true to the trait.

use async_trait::async_trait;

use crate::provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};

/// CalDAV provider skeleton.
pub struct CalDavProvider {
    credentials: AccountCredentials,
    account_label: String,
}

impl CalDavProvider {
    pub fn new(credentials: AccountCredentials, account_label: impl Into<String>) -> Self {
        Self {
            credentials,
            account_label: account_label.into(),
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
        tracing::debug!(
            "CalDavProvider::connect stub for {:?}",
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
        provider: "CalDAV",
        method,
    })
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

    #[tokio::test]
    async fn stub_methods_report_not_yet_implemented() {
        let mut p = CalDavProvider::new(AccountCredentials::default(), "x");
        let err = p.connect().await.unwrap_err();
        assert!(matches!(
            err,
            SyncError::NotYetImplemented {
                provider: "CalDAV",
                method: "connect",
            }
        ));
    }
}
