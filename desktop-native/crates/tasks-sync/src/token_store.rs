//! OAuth token persistence.
//!
//! Abstracts the OS-specific secret stores we eventually want to
//! plug in (libsecret on Linux, Keychain on macOS, Credential
//! Manager on Windows) behind a plain trait, so the sync
//! plumbing can read/write tokens without caring about where
//! they live. Ships one concrete impl: [`InMemoryTokenStore`],
//! suitable for tests, debugging, and as a fallback on systems
//! where the OS store is unavailable.
//!
//! An OS-native impl lands alongside the first real OAuth
//! provider (Google or Microsoft). The trait surface is stable
//! enough that the impl swap is additive.

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;

use crate::provider::ProviderKind;

/// The stuff a token endpoint hands back. Matches the `expires_in`
/// conversion the HTTP layer does at the point of receipt —
/// callers store an absolute `expires_at_ms` rather than a
/// relative TTL so a paused app doesn't come back thinking its
/// token is still fresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Milliseconds since Unix epoch at which the access token
    /// expires. `0` = unknown (rare — Google/MS both return an
    /// `expires_in`).
    pub expires_at_ms: i64,
}

impl OAuthTokens {
    /// True when the access token is within `grace_ms` of its
    /// expiry (or already past it). Callers use this to decide
    /// whether to refresh before issuing a request.
    pub fn needs_refresh(&self, now_ms: i64, grace_ms: i64) -> bool {
        self.expires_at_ms > 0 && self.expires_at_ms - grace_ms <= now_ms
    }
}

#[derive(Debug, Error)]
pub enum TokenStoreError {
    /// Backing store rejected the operation (e.g., keychain access
    /// denied, secret-service not running).
    #[error("token store backend error: {0}")]
    Backend(String),
}

pub type TokenStoreResult<T> = Result<T, TokenStoreError>;

/// Read/write OAuth tokens per-provider per-account. Keyed on
/// `(ProviderKind, account_label)` because a single user may
/// have, e.g., two Google accounts connected.
pub trait TokenStore: Send + Sync {
    fn get(&self, provider: ProviderKind, account: &str) -> Option<OAuthTokens>;
    fn put(
        &self,
        provider: ProviderKind,
        account: &str,
        tokens: &OAuthTokens,
    ) -> TokenStoreResult<()>;
    fn delete(&self, provider: ProviderKind, account: &str) -> TokenStoreResult<()>;
}

/// Test/dev implementation. All state is in-process; nothing
/// survives a restart. Used directly by tests and by
/// `--no-keychain` runs where the OS store isn't wanted.
pub struct InMemoryTokenStore {
    inner: Mutex<HashMap<(ProviderKind, String), OAuthTokens>>,
}

impl Default for InMemoryTokenStore {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl InMemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStore for InMemoryTokenStore {
    fn get(&self, provider: ProviderKind, account: &str) -> Option<OAuthTokens> {
        self.inner
            .lock()
            .ok()
            .and_then(|m| m.get(&(provider, account.to_string())).cloned())
    }
    fn put(
        &self,
        provider: ProviderKind,
        account: &str,
        tokens: &OAuthTokens,
    ) -> TokenStoreResult<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| TokenStoreError::Backend(format!("mutex poisoned: {e}")))?;
        guard.insert((provider, account.to_string()), tokens.clone());
        Ok(())
    }
    fn delete(&self, provider: ProviderKind, account: &str) -> TokenStoreResult<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| TokenStoreError::Backend(format!("mutex poisoned: {e}")))?;
        guard.remove(&(provider, account.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OAuthTokens {
        OAuthTokens {
            access_token: "AT".into(),
            refresh_token: Some("RT".into()),
            expires_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn put_get_round_trip() {
        let store = InMemoryTokenStore::new();
        store
            .put(ProviderKind::GoogleTasks, "alice", &sample())
            .unwrap();
        let got = store.get(ProviderKind::GoogleTasks, "alice").unwrap();
        assert_eq!(got, sample());
    }

    #[test]
    fn accounts_are_independent_by_provider_and_label() {
        let store = InMemoryTokenStore::new();
        store
            .put(ProviderKind::GoogleTasks, "alice", &sample())
            .unwrap();
        store
            .put(
                ProviderKind::MicrosoftToDo,
                "alice",
                &OAuthTokens {
                    access_token: "other".into(),
                    refresh_token: None,
                    expires_at_ms: 0,
                },
            )
            .unwrap();
        assert!(store.get(ProviderKind::GoogleTasks, "bob").is_none());
        assert_eq!(
            store
                .get(ProviderKind::MicrosoftToDo, "alice")
                .unwrap()
                .access_token,
            "other"
        );
    }

    #[test]
    fn delete_removes_key() {
        let store = InMemoryTokenStore::new();
        store
            .put(ProviderKind::GoogleTasks, "alice", &sample())
            .unwrap();
        store.delete(ProviderKind::GoogleTasks, "alice").unwrap();
        assert!(store.get(ProviderKind::GoogleTasks, "alice").is_none());
        // Delete on a missing key is a no-op.
        store.delete(ProviderKind::GoogleTasks, "bob").unwrap();
    }

    #[test]
    fn needs_refresh_respects_grace_window() {
        let t = OAuthTokens {
            access_token: "x".into(),
            refresh_token: None,
            expires_at_ms: 1000,
        };
        // Well before expiry: no refresh.
        assert!(!t.needs_refresh(500, 100));
        // Inside the grace window (1000 - 100 == 900, now >= 900): refresh.
        assert!(t.needs_refresh(900, 100));
        // Past expiry: definitely refresh.
        assert!(t.needs_refresh(1500, 100));
        // Unknown expiry: never auto-refresh.
        let unknown = OAuthTokens {
            expires_at_ms: 0,
            ..t
        };
        assert!(!unknown.needs_refresh(1_000_000, 100));
    }
}
