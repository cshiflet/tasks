//! Write helpers.
//!
//! Milestone 2 opens the door to mutating the desktop-native database.
//! We deliberately keep writes out of the [`crate::db::Database`] type —
//! that handle stays read-only so the query path can't accidentally
//! issue an UPDATE. Instead each helper here opens its own short-lived
//! read-write SQLite connection, runs a single transaction, and
//! closes. A handful of invariants fall out of that structure:
//!
//! * The read-only handle the GUI is holding stays valid throughout.
//!   SQLite's per-connection locking lets a writer take
//!   RESERVED/EXCLUSIVE while the reader idles, so there's no
//!   coordination beyond the default `busy_timeout`.
//! * Each write is its own transaction. Callers that need to batch
//!   multiple mutations should add a batch-shaped helper here rather
//!   than threading `rusqlite::Transaction` through the bridge.
//! * Failures propagate as [`crate::error::CoreError`] so the bridge
//!   can surface them in the status bar unchanged.
//!
//! Recurring-task rescheduling (advancing `dueDate` to the next
//! occurrence on complete) lives in a later M2 step; for now
//! completing a recurring task marks the current instance done and
//! leaves the next one to the user.

use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};

use crate::error::Result;

/// Open a writable connection to `path` with the same defensive
/// tuning the read-only path uses (short `busy_timeout`, no shared
/// cache). Callers should drop the connection as soon as the write
/// completes so the GUI's read-only handle reclaims the lock.
fn open_rw(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)?;
    // One second is enough for any realistic transient contention
    // (the only other writer is ourselves, during import).
    conn.busy_timeout(std::time::Duration::from_millis(1_000))?;
    Ok(conn)
}

/// Toggle a task's completion state.
///
/// `completed = true` stamps `tasks.completed = now_ms` — the Android
/// client treats any non-zero completion timestamp as "done", so the
/// exact value just needs to be monotonic. `completed = false` clears
/// it back to `0`, restoring the task to the active list. Either way,
/// `tasks.modified` is bumped so any downstream observer that sorts
/// by modification time (sync, recently-modified filter) notices.
///
/// Returns `Ok(true)` when a row was updated, `Ok(false)` when
/// `task_id` didn't match any row (caller can surface "not found").
pub fn set_task_completion(
    path: &Path,
    task_id: i64,
    completed: bool,
    now_ms: i64,
) -> Result<bool> {
    let conn = open_rw(path)?;
    let completed_at = if completed { now_ms } else { 0 };
    let rows = conn.execute(
        "UPDATE tasks SET completed = ?1, modified = ?2 WHERE _id = ?3",
        params![completed_at, now_ms, task_id],
    )?;
    Ok(rows > 0)
}

/// Soft-delete a task. Mirrors the Android client's semantics: writes
/// `tasks.deleted = now_ms` rather than `DELETE FROM tasks`, which
/// lets a future sync layer propagate the tombstone and also keeps
/// the row available for undo. The active-list query filter
/// (`tasks.deleted = 0`) hides the row immediately.
///
/// Returns `Ok(true)` when a row was updated.
pub fn set_task_deleted(path: &Path, task_id: i64, now_ms: i64) -> Result<bool> {
    let conn = open_rw(path)?;
    let rows = conn.execute(
        "UPDATE tasks SET deleted = ?1, modified = ?1 WHERE _id = ?2",
        params![now_ms, task_id],
    )?;
    Ok(rows > 0)
}
