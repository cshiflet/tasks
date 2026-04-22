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

use rusqlite::{params, Connection, OpenFlags, Transaction};

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

/// Value bundle for [`update_task_fields`]. Fields match the
/// Android `Task` entity columns the edit dialog exposes.
#[derive(Debug, Clone)]
pub struct TaskEdit<'a> {
    pub title: &'a str,
    pub notes: &'a str,
    /// Milliseconds since the Unix epoch. `0` = no date.
    pub due_ms: i64,
    /// Milliseconds since the Unix epoch. `0` = not hidden.
    pub hide_until_ms: i64,
    /// Raw `Priority` value: HIGH=0 … NONE=3.
    pub priority: i32,
    /// New CalDAV calendar UUID to assign, or `None` to leave the
    /// current assignment alone. Applied by updating
    /// `caldav_tasks.cd_calendar` for the row that already points at
    /// this task; if the task has no caldav_tasks row (local-only
    /// task), this field is a no-op. Reassigning to a *different*
    /// calendar UUID on an existing row is supported; detaching
    /// entirely (moving a CalDAV task back to local) is not in M2
    /// Phase C1 — that's an explicit "delete from calendar" flow in
    /// the Android app and will land alongside CalDAV sync.
    pub caldav_calendar_uuid: Option<&'a str>,
    /// Replace the task's tag join rows (`tags` table) with exactly
    /// this set of `tagdata.remoteId` values. `None` leaves tags
    /// untouched; `Some(&[])` clears every tag. The write helper
    /// DELETEs the existing rows then re-INSERTs with looked-up
    /// tagdata names + the task's remoteId.
    pub tag_uids: Option<&'a [String]>,
    /// Replace the task's alarm rows with exactly this set, keyed
    /// by `(time, alarm_type)`. `None` leaves alarms untouched;
    /// `Some(&[])` clears every alarm. The write helper DELETEs
    /// then INSERTs fresh rows with `repeat` / `interval` both
    /// zero — reminders richer than type + time (random intervals,
    /// nag-till-done) aren't editable from the dialog yet.
    pub alarms: Option<&'a [(i64, i32)]>,
}

/// Edit the core user-visible fields of a task in a single UPDATE.
///
/// `now_ms` is written into `tasks.modified` so any sort or sync
/// observer picks the change up. Recurrence is intentionally not
/// editable from this call — the dialog's "Repeats" row is read-only
/// in M2 Phase B because a full RRULE picker belongs in its own UI.
/// Advancing the recurrence on complete still happens automatically
/// per `tasks.recurrence` when that feature lands.
///
/// Returns `Ok(true)` when a row was updated, `Ok(false)` when
/// `task_id` matched nothing.
/// Replace the tag-join rows (`tags` table) for `task_id` with the
/// passed tagdata UIDs. DELETE all existing rows, INSERT one per UID
/// carrying the looked-up `tagdata.name` and the task's `remoteId`.
/// Missing tagdata rows (stale UID) are silently skipped at debug.
fn replace_task_tags(tx: &Transaction<'_>, task_id: i64, tag_uids: &[String]) -> Result<()> {
    let task_remote_id: Option<String> = tx
        .query_row(
            "SELECT remoteId FROM tasks WHERE _id = ?1",
            params![task_id],
            |r| r.get(0),
        )
        .unwrap_or(None);

    tx.execute("DELETE FROM tags WHERE task = ?1", params![task_id])?;

    if tag_uids.is_empty() {
        return Ok(());
    }

    let mut name_stmt = tx.prepare("SELECT name FROM tagdata WHERE remoteId = ?1 LIMIT 1")?;
    let mut insert_stmt =
        tx.prepare("INSERT INTO tags (task, name, tag_uid, task_uid) VALUES (?1, ?2, ?3, ?4)")?;
    for uid in tag_uids {
        let name: Option<String> = name_stmt
            .query_row(params![uid], |r| r.get(0))
            .unwrap_or(None);
        let Some(name) = name else {
            tracing::debug!("skipping tag uid {uid:?}: no tagdata row");
            continue;
        };
        insert_stmt.execute(params![task_id, name, uid, task_remote_id])?;
    }
    Ok(())
}

/// Replace the alarm rows for `task_id` with the given `(time, type)`
/// pairs. `repeat` and `interval` are zeroed since the edit dialog
/// doesn't offer them yet.
fn replace_task_alarms(tx: &Transaction<'_>, task_id: i64, alarms: &[(i64, i32)]) -> Result<()> {
    tx.execute("DELETE FROM alarms WHERE task = ?1", params![task_id])?;
    if alarms.is_empty() {
        return Ok(());
    }
    let mut stmt = tx.prepare(
        "INSERT INTO alarms (task, time, type, repeat, interval) \
         VALUES (?1, ?2, ?3, 0, 0)",
    )?;
    for (time, alarm_type) in alarms {
        stmt.execute(params![task_id, time, alarm_type])?;
    }
    Ok(())
}

pub fn update_task_fields(
    path: &Path,
    task_id: i64,
    edit: &TaskEdit<'_>,
    now_ms: i64,
) -> Result<bool> {
    let mut conn = open_rw(path)?;
    // Wrap both the tasks UPDATE and the optional caldav_tasks
    // UPDATE in one transaction so a mid-write interrupt leaves
    // the pair consistent. For a one-row edit the overhead is
    // negligible and the safety margin is worth it.
    let tx = conn.transaction()?;

    // Empty notes / title should store as NULL (matching Android's
    // `Task.notes: String?`): otherwise a cleared field persists as
    // the empty string and a sync round-trip can flip-flop between
    // NULL and "".
    let notes_arg: Option<&str> = if edit.notes.is_empty() {
        None
    } else {
        Some(edit.notes)
    };
    let title_arg: Option<&str> = if edit.title.is_empty() {
        None
    } else {
        Some(edit.title)
    };
    let rows = tx.execute(
        "UPDATE tasks SET \
             title = ?1, \
             notes = ?2, \
             dueDate = ?3, \
             hideUntil = ?4, \
             importance = ?5, \
             modified = ?6 \
         WHERE _id = ?7",
        params![
            title_arg,
            notes_arg,
            edit.due_ms,
            edit.hide_until_ms,
            edit.priority,
            now_ms,
            task_id,
        ],
    )?;
    if rows == 0 {
        // Task not found — rolling back leaves the caldav_tasks row
        // alone too, avoiding a stray reassignment.
        tx.rollback()?;
        return Ok(false);
    }

    if let Some(tag_uids) = edit.tag_uids {
        replace_task_tags(&tx, task_id, tag_uids)?;
    }

    if let Some(alarms) = edit.alarms {
        replace_task_alarms(&tx, task_id, alarms)?;
    }

    if let Some(uuid) = edit.caldav_calendar_uuid {
        // Only touch caldav_tasks when the caller passes a non-empty
        // UUID; an empty string from the UI means "no calendar",
        // which in M2 Phase C1 semantics is "don't change anything".
        if !uuid.is_empty() {
            // UPDATE the existing row if one exists. Tasks that have
            // never been synced (local-only) have no caldav_tasks
            // row; the zero-affected-rows case below is a no-op,
            // matching the "this field is a no-op for local tasks"
            // contract in the TaskEdit docs.
            tx.execute(
                "UPDATE caldav_tasks SET cd_calendar = ?1 WHERE cd_task = ?2",
                params![uuid, task_id],
            )?;
        }
    }

    tx.commit()?;
    Ok(true)
}
