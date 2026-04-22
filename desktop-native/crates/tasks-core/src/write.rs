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

/// Create a new task with the given title.
///
/// `caldav_calendar_uuid` optionally assigns the new task to a
/// CalDAV list; when non-empty, a fresh `caldav_tasks` row is
/// minted with a new UUID as `cd_remote_id` so the task is
/// syncable from first save.
///
/// Returns the new `tasks._id`. Caller can immediately select the
/// row in the UI for editing.
pub fn create_task(
    path: &Path,
    title: &str,
    now_ms: i64,
    caldav_calendar_uuid: Option<&str>,
) -> Result<i64> {
    let mut conn = open_rw(path)?;
    let tx = conn.transaction()?;

    let task_remote_id = uuid::Uuid::new_v4().to_string();
    let title_arg: Option<&str> = if title.is_empty() { None } else { Some(title) };

    // Minimum-viable row: same defaults the JSON importer sets for
    // a brand-new task with no metadata. Priority = 3 (NONE),
    // everything else zero, collapsed/read_only false.
    tx.execute(
        "INSERT INTO tasks \
         (title, importance, dueDate, hideUntil, created, modified, \
          completed, deleted, estimatedSeconds, elapsedSeconds, \
          timerStart, notificationFlags, lastNotified, repeat_from, \
          collapsed, parent, read_only, remoteId) \
         VALUES (?1, 3, 0, 0, ?2, ?2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, ?3)",
        params![title_arg, now_ms, task_remote_id],
    )?;
    let task_id = tx.last_insert_rowid();

    if let Some(uuid) = caldav_calendar_uuid {
        if !uuid.is_empty() {
            // Mint a fresh cd_remote_id so downstream CalDAV sync
            // has a stable object URL. Tasks.org's Android flow
            // does the same — see CaldavTaskUpdater.
            let object_uid = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO caldav_tasks \
                 (cd_task, cd_calendar, cd_remote_id, cd_last_sync, \
                  cd_deleted, gt_moved, gt_remote_order) \
                 VALUES (?1, ?2, ?3, 0, 0, 0, 0)",
                params![task_id, uuid, object_uid],
            )?;
        }
    }

    tx.commit()?;
    Ok(task_id)
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
    /// Replace the task's geofence rows. `None` = leave alone.
    /// `Some(GeofenceEdit { place_uid: "" , .. })` clears the
    /// existing geofences; non-empty `place_uid` writes exactly one
    /// row with the given arrival/departure flags. Tasks.org's UI
    /// is effectively 1:1 task-to-geofence even though the schema
    /// allows multiple.
    pub geofence: Option<GeofenceEdit<'a>>,
    /// New value for `tasks.parent`. `None` leaves it alone;
    /// `Some(0)` promotes the task to top-level; `Some(id)`
    /// re-parents it under another task. Cycle prevention (e.g.
    /// picking a descendant as parent) is *not* enforced here —
    /// caller is expected to exclude invalid candidates from the
    /// picker. A cycle won't corrupt the DB, but the recursive
    /// tasklist query will render it oddly.
    pub parent_id: Option<i64>,
    /// Seconds of estimated work. `0` clears the estimate.
    /// Always written (no `Option`) because the edit dialog always
    /// has a value to persist.
    pub estimated_seconds: i32,
    /// Seconds of elapsed work. `0` clears the counter. Same
    /// always-written shape as `estimated_seconds`.
    pub elapsed_seconds: i32,
    /// New value for `tasks.recurrence` — an RFC 5545 RRULE string
    /// like "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR" or empty for
    /// no recurrence. Always written, since the edit dialog always
    /// has state for it. COUNT/UNTIL bits that were in the rule
    /// before the edit are dropped unless the UI chooses to
    /// reconstruct them; the desktop dialog does not edit those in
    /// M2 Phase C8.
    pub recurrence: &'a str,
    /// New value for `tasks.repeat_from` (0 = from due date,
    /// 1 = from completion). Always written.
    pub repeat_from: i32,
}

/// Bundle carrying the three geofence-edit fields together so
/// `TaskEdit` stays readable even as more optional writes land.
#[derive(Debug, Clone, Copy)]
pub struct GeofenceEdit<'a> {
    pub place_uid: &'a str,
    pub arrival: bool,
    pub departure: bool,
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
    // Look up the task's remoteId for the join rows' `task_uid`
    // column. A local-only task correctly has no remoteId
    // (`Option::None` passes through to a NULL column); we only
    // swallow the `NoRows` case here because the caller has
    // already verified the task exists. Any other failure (I/O,
    // corruption) surfaces to the transaction rollback.
    let task_remote_id: Option<String> = match tx.query_row(
        "SELECT remoteId FROM tasks WHERE _id = ?1",
        params![task_id],
        |r| r.get::<_, Option<String>>(0),
    ) {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };

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

fn replace_task_geofence(tx: &Transaction<'_>, task_id: i64, edit: GeofenceEdit<'_>) -> Result<()> {
    tx.execute("DELETE FROM geofences WHERE task = ?1", params![task_id])?;
    if edit.place_uid.is_empty() {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO geofences (task, place, arrival, departure) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            task_id,
            edit.place_uid,
            edit.arrival as i32,
            edit.departure as i32,
        ],
    )?;
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
    // Empty recurrence string → NULL in the DB to match Android's
    // convention (Task.recurrence: String?).
    let recurrence_arg: Option<&str> = if edit.recurrence.is_empty() {
        None
    } else {
        Some(edit.recurrence)
    };
    let rows = tx.execute(
        "UPDATE tasks SET \
             title = ?1, \
             notes = ?2, \
             dueDate = ?3, \
             hideUntil = ?4, \
             importance = ?5, \
             estimatedSeconds = ?6, \
             elapsedSeconds = ?7, \
             recurrence = ?8, \
             repeat_from = ?9, \
             modified = ?10 \
         WHERE _id = ?11",
        params![
            title_arg,
            notes_arg,
            edit.due_ms,
            edit.hide_until_ms,
            edit.priority,
            edit.estimated_seconds,
            edit.elapsed_seconds,
            recurrence_arg,
            edit.repeat_from,
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

    if let Some(geofence) = edit.geofence {
        replace_task_geofence(&tx, task_id, geofence)?;
    }

    if let Some(parent_id) = edit.parent_id {
        // Guard against an obvious cycle: a task can't be its own
        // parent. Other cycles (A → B → A) are the caller's
        // problem; see the TaskEdit docs.
        let sanitized = if parent_id == task_id { 0 } else { parent_id };
        tx.execute(
            "UPDATE tasks SET parent = ?1 WHERE _id = ?2",
            params![sanitized, task_id],
        )?;
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
