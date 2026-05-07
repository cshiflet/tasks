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
