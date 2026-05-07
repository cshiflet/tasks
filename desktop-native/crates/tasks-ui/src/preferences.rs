//! Persistent preferences for the desktop client.
//!
//! Single small JSON blob in
//! `<dirs::config_dir()>/tasks-desktop/preferences.json`. Holds
//! the pieces of session state we want to survive a restart —
//! currently the appearance theme + the list-default query prefs.
//! Per-list overrides will land alongside this once the right-
//! click-on-list flow is wired (deferred).
//!
//! Failures are non-fatal: a missing file, a malformed JSON, or a
//! permissions error all degrade to "use the defaults"; a write
//! failure logs at warn but never panics. The on-disk format is
//! intentionally easy to inspect by hand.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    /// Material theme override.
    /// 0 = Follow OS, 1 = Light, 2 = Dark.
    #[serde(default)]
    pub theme_mode: i32,
    /// Default sort mode (matches `tasks_core` SortHelper integers).
    #[serde(default)]
    pub sort_mode: i32,
    #[serde(default = "default_true")]
    pub sort_ascending: bool,
    #[serde(default)]
    pub show_completed: bool,
    #[serde(default)]
    pub show_hidden: bool,
    #[serde(default)]
    pub completed_at_bottom: bool,
    #[serde(default = "default_window_width")]
    pub window_width: i32,
    #[serde(default = "default_window_height")]
    pub window_height: i32,
    /// 0 = "no saved position; let the window manager place it".
    #[serde(default)]
    pub window_x: i32,
    #[serde(default)]
    pub window_y: i32,
    #[serde(default)]
    pub window_maximized: bool,
    /// Master toggle for the OS-level reminder notifications fired
    /// by `notifier::AlarmScheduler`. Default-on so an existing
    /// preferences file from before the feature landed picks up
    /// notifications automatically. The General Settings pane
    /// surfaces it as "Show OS notifications for task reminders".
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_window_width() -> i32 {
    1100
}

fn default_window_height() -> i32 {
    720
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme_mode: 0,
            sort_mode: 0,
            sort_ascending: true,
            show_completed: false,
            show_hidden: false,
            completed_at_bottom: false,
            window_width: default_window_width(),
            window_height: default_window_height(),
            window_x: 0,
            window_y: 0,
            window_maximized: false,
            notifications_enabled: true,
        }
    }
}

impl Preferences {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("preferences: ignoring malformed {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!("preferences: read {} failed: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = config_path() else {
            tracing::warn!("preferences: no config dir available; skipping persist");
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(
                    "preferences: create_dir_all {} failed: {e}",
                    parent.display()
                );
                return;
            }
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    tracing::warn!("preferences: write {} failed: {e}", path.display());
                }
            }
            Err(e) => tracing::warn!("preferences: serialize failed: {e}"),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("tasks-desktop");
    p.push("preferences.json");
    Some(p)
}

/// On-disk OAuth client-ID config, separate from the main
/// preferences blob so it can be hand-managed (and
/// `.gitignore`d) without churning the rest of the prefs file.
///
/// Path: `<dirs::config_dir()>/tasks-desktop/oauth.json`. Schema:
///
/// ```json
/// {
///   "google_client_id": "1234.apps.googleusercontent.com",
///   "microsoft_client_id": "abcd-..."
/// }
/// ```
///
/// Both fields optional. Missing file / malformed JSON / unset
/// fields all degrade to None, leaving env-var fallback to
/// fail with a clear status message.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OAuthConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microsoft_client_id: Option<String>,
}

impl OAuthConfig {
    fn load() -> Self {
        let Some(path) = oauth_config_path() else {
            return Self::default();
        };
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<OAuthConfig>(&bytes) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("oauth.json: ignoring malformed {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!("oauth.json: read {} failed: {e}", path.display());
                Self::default()
            }
        }
    }
}

fn oauth_config_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("tasks-desktop");
    p.push("oauth.json");
    Some(p)
}

/// Path to the on-disk OAuth config, used in user-facing error
/// strings so the user knows where to drop their client IDs.
pub fn oauth_config_path_display() -> String {
    oauth_config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<config_dir>/tasks-desktop/oauth.json".to_string())
}

/// Resolve the Google Tasks OAuth client ID. Env var
/// `TASKS_DESKTOP_GOOGLE_CLIENT_ID` wins (deliberate override
/// for CI / dev / one-off use); falls back to the
/// `google_client_id` field in `oauth.json`. Returns `None`
/// when neither source carries a non-empty value, leaving the
/// caller to surface a status message with the path to set.
pub fn google_oauth_client_id() -> Option<String> {
    if let Ok(v) = std::env::var("TASKS_DESKTOP_GOOGLE_CLIENT_ID") {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    OAuthConfig::load()
        .google_client_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Microsoft To Do equivalent of [`google_oauth_client_id`].
/// Env var `TASKS_DESKTOP_MICROSOFT_CLIENT_ID` overrides the
/// `microsoft_client_id` field in `oauth.json`.
pub fn microsoft_oauth_client_id() -> Option<String> {
    if let Ok(v) = std::env::var("TASKS_DESKTOP_MICROSOFT_CLIENT_ID") {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    OAuthConfig::load()
        .microsoft_client_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
