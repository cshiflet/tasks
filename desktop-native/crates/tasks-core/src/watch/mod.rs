//! Filesystem watcher that emits a debounced signal when the database file
//! (or its -wal / -shm siblings) changes on disk.
//!
//! The typical deployment model is that the Android app writes the SQLite
//! file on a sync folder (Syncthing, iCloud Drive, OneDrive) and the desktop
//! client reads it. We watch the parent directory rather than the file
//! itself so atomic renames during WAL checkpoints don't orphan the watch.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};

use crate::error::Result;

/// How long to coalesce back-to-back filesystem notifications before
/// signalling a reload. 250 ms matches the plan and keeps the UI from
/// thrashing during a burst of WAL activity.
const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct DatabaseWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, FileIdMap>,
    pub events: Receiver<DebounceEventResult>,
    target: PathBuf,
}

impl DatabaseWatcher {
    /// Start watching the directory containing `db_path`. The returned
    /// receiver yields one event per debounced batch.
    pub fn start(db_path: impl AsRef<Path>) -> Result<Self> {
        let target = db_path.as_ref().to_path_buf();
        let parent = target
            .parent()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "db path has no parent directory",
                )
            })?
            .to_path_buf();

        let (tx, rx) = channel();
        let mut debouncer = new_debouncer(DEBOUNCE, None, move |res| {
            // Drop if the consumer hung up.
            let _ = tx.send(res);
        })?;
        debouncer
            .watcher()
            .watch(&parent, RecursiveMode::NonRecursive)?;

        Ok(DatabaseWatcher {
            _debouncer: debouncer,
            events: rx,
            target,
        })
    }

    pub fn target(&self) -> &Path {
        &self.target
    }
}
