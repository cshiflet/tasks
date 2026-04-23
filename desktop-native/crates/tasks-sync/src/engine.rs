//! Sync engine: orchestrates pull / reconcile / push between a
//! [`crate::Provider`] and the desktop's local SQLite database.
//!
//! **Status:** pull cycle + push_dirty cycle. `sync_once()`
//! composes both. Actual network / HTTP lives behind the
//! [`Provider`] trait — the provider stubs return
//! `NotYetImplemented` today; [`MockProvider`] exercises the
//! whole orchestration end-to-end from in-memory state.
//!
//! Merge policy for pull: **remote wins** for the columns the
//! provider is authoritative on (title, notes, dates, recurrence,
//! status, parent). Local-only state — tags, alarms, geofence —
//! is preserved unchanged across pulls because the provider
//! doesn't speak it (or, when it eventually does, the engine
//! grows a separate replace_* helper to cover it). Sync conflict
//! detection (etag mismatch) is the next step; this engine
//! refuses no writes today.
//!
//! The sync runs inside a single transaction per pull cycle so a
//! mid-pull failure (network drop, parse error) leaves the DB
//! exactly where it was.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::provider::{Provider, RemoteCalendar, RemoteTask, SyncError, SyncOutcome, SyncResult};

/// Drives one or more sync cycles against `provider`, persisting
/// pulled state into the local SQLite at `db_path`.
pub struct SyncEngine<'a> {
    db_path: &'a Path,
    provider: Box<dyn Provider>,
}

impl<'a> SyncEngine<'a> {
    pub fn new(db_path: &'a Path, provider: Box<dyn Provider>) -> Self {
        Self { db_path, provider }
    }

    /// Connect + pull every calendar's tasks. Returns the count of
    /// rows pulled. Does not push.
    pub async fn pull_all(&mut self) -> SyncResult<SyncOutcome> {
        self.provider.connect().await?;
        let calendars = self.provider.list_calendars().await?;

        let mut conn =
            open_rw(self.db_path).map_err(|e| SyncError::Local(format!("open db: {e}")))?;
        let tx = conn
            .transaction()
            .map_err(|e| SyncError::Local(format!("begin tx: {e}")))?;

        for cal in &calendars {
            upsert_calendar(&tx, cal).map_err(|e| SyncError::Local(format!("calendar: {e}")))?;
        }

        let mut tasks_pulled = 0usize;
        let mut tasks_deleted = 0usize;
        // Two-pass to avoid parent ordering hazards: insert/update
        // every task first (parent stays 0), then a second pass
        // backfills `tasks.parent` once every remoteId is in place.
        let mut all_tasks: Vec<RemoteTask> = Vec::new();
        for cal in &calendars {
            let tasks = self.provider.list_tasks(&cal.remote_id).await?;
            let mut seen_remote_ids: Vec<String> = Vec::with_capacity(tasks.len());
            for t in &tasks {
                let task_id = upsert_task(&tx, t)
                    .map_err(|e| SyncError::Local(format!("task {}: {e}", t.remote_id)))?;
                upsert_caldav_task(&tx, task_id, t)
                    .map_err(|e| SyncError::Local(format!("caldav_task {}: {e}", t.remote_id)))?;
                tasks_pulled += 1;
                seen_remote_ids.push(t.remote_id.clone());
            }
            // Anything in this calendar we had before but the
            // server didn't list this time: tombstone locally.
            // Matches what the Android client does when the
            // remote deletes a task.
            let now = now_ms();
            let removed = tombstone_missing_tasks(&tx, &cal.remote_id, &seen_remote_ids, now)
                .map_err(|e| SyncError::Local(format!("tombstone: {e}")))?;
            tasks_deleted += removed;
            all_tasks.extend(tasks);
        }

        let parent_links = relink_parents(&tx, &all_tasks)
            .map_err(|e| SyncError::Local(format!("relink: {e}")))?;

        tx.commit()
            .map_err(|e| SyncError::Local(format!("commit: {e}")))?;

        tracing::info!(
            "sync pull: {} calendars, {} tasks, {} parent links, {} tombstoned",
            calendars.len(),
            tasks_pulled,
            parent_links,
            tasks_deleted,
        );
        let _ = tasks_deleted; // (exposed through tracing; SyncOutcome
                               // doesn't carry a deletions counter yet)
        Ok(SyncOutcome {
            calendars_pulled: calendars.len(),
            tasks_pulled,
            tasks_pushed: 0,
            conflicts: 0,
        })
    }

    /// Push local tasks whose `tasks.modified` is newer than the
    /// last sync stamp of their `caldav_tasks` row (or which have
    /// no `caldav_tasks` row at all — that's a brand-new local
    /// task belonging to a CalDAV list).
    ///
    /// On success, the returned etag is stamped back onto
    /// `cd_etag` + `cd_last_sync`. A
    /// [`SyncError::Conflict`] from the provider is caught,
    /// recorded against the task, and the loop continues with the
    /// remaining rows — one conflict shouldn't block unrelated
    /// pushes. Callers get a total count of conflicts via
    /// [`SyncOutcome::conflicts`].
    pub async fn push_dirty(&mut self) -> SyncResult<SyncOutcome> {
        self.provider.connect().await?;
        let conn = open_rw(self.db_path).map_err(|e| SyncError::Local(format!("open db: {e}")))?;
        let dirty =
            load_dirty_tasks(&conn).map_err(|e| SyncError::Local(format!("load dirty: {e}")))?;
        drop(conn); // Release the write handle before the async round trips.

        let mut pushed = 0usize;
        let mut conflicts = 0usize;
        for task in &dirty {
            match self.provider.push_task(task).await {
                Ok(new_etag) => {
                    let conn = open_rw(self.db_path)
                        .map_err(|e| SyncError::Local(format!("reopen db: {e}")))?;
                    record_push_success(&conn, &task.remote_id, new_etag.as_deref(), now_ms())
                        .map_err(|e| SyncError::Local(format!("stamp etag: {e}")))?;
                    pushed += 1;
                }
                Err(SyncError::Conflict { remote_id, .. }) => {
                    tracing::warn!("push conflict on {remote_id}; keeping local");
                    conflicts += 1;
                }
                Err(other) => return Err(other),
            }
        }
        Ok(SyncOutcome {
            calendars_pulled: 0,
            tasks_pulled: 0,
            tasks_pushed: pushed,
            conflicts,
        })
    }

    /// Full sync cycle: pull, then push. Convenience for the
    /// "sync now" button.
    pub async fn sync_now(&mut self) -> SyncResult<SyncOutcome> {
        let pulled = self.pull_all().await?;
        let pushed = self.push_dirty().await?;
        Ok(SyncOutcome {
            calendars_pulled: pulled.calendars_pulled,
            tasks_pulled: pulled.tasks_pulled,
            tasks_pushed: pushed.tasks_pushed,
            conflicts: pushed.conflicts,
        })
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Load every task that needs pushing: a caldav_tasks row whose
/// paired `tasks.modified` exceeds `cd_last_sync`, OR (to cover
/// brand-new local tasks about to sync for the first time) rows
/// whose `cd_last_sync` is 0.
fn load_dirty_tasks(conn: &Connection) -> rusqlite::Result<Vec<RemoteTask>> {
    let mut stmt = conn.prepare(
        "SELECT t._id, t.title, t.notes, t.dueDate, t.completed, \
                t.importance, t.recurrence, t.modified, t.remoteId, \
                ct.cd_calendar, ct.cd_remote_id, ct.cd_etag, \
                ct.cd_remote_parent, ct.cd_last_sync \
         FROM tasks t \
         JOIN caldav_tasks ct ON ct.cd_task = t._id \
         WHERE t.deleted = 0 \
           AND (ct.cd_last_sync = 0 OR t.modified > ct.cd_last_sync)",
    )?;
    let rows = stmt.query_map([], |r| {
        let due_ms: i64 = r.get(3)?;
        let due_has_time = due_ms != 0 && due_ms % 60_000 != 0;
        Ok(RemoteTask {
            remote_id: r.get::<_, String>(10)?,
            calendar_remote_id: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
            etag: r.get::<_, Option<String>>(11)?,
            title: r.get::<_, Option<String>>(1)?,
            notes: r.get::<_, Option<String>>(2)?,
            due_ms,
            due_has_time,
            completed_ms: r.get::<_, i64>(4)?,
            priority: r.get::<_, i32>(5)?,
            recurrence: r.get::<_, Option<String>>(6)?,
            parent_remote_id: r.get::<_, Option<String>>(12)?,
            raw_vtodo: None,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// After a successful push, stamp the new etag + last-sync
/// timestamp so the next push_dirty call doesn't re-send the
/// same row.
fn record_push_success(
    conn: &Connection,
    remote_id: &str,
    new_etag: Option<&str>,
    now_ms: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE caldav_tasks SET cd_etag = ?1, cd_last_sync = ?2 \
         WHERE cd_remote_id = ?3",
        params![new_etag, now_ms, remote_id],
    )?;
    Ok(())
}

/// Open a writable handle to the desktop's SQLite. Mirrors the
/// shape `tasks_core::write` uses so the locking semantics are
/// the same.
fn open_rw(path: &Path) -> rusqlite::Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(std::time::Duration::from_millis(1_000))?;
    Ok(conn)
}

fn upsert_calendar(tx: &rusqlite::Transaction<'_>, cal: &RemoteCalendar) -> rusqlite::Result<()> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT cdl_id FROM caldav_lists WHERE cdl_uuid = ?1",
            params![cal.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    let access = if cal.read_only { 2 } else { 1 }; // CalendarAccess::READ_ONLY / READ_WRITE
    let color = cal.color.unwrap_or(0);
    if let Some(_id) = existing {
        tx.execute(
            "UPDATE caldav_lists SET cdl_name = ?1, cdl_color = ?2, cdl_url = ?3, \
             cdl_access = ?4, cdl_ctag = ?5 WHERE cdl_uuid = ?6",
            params![
                cal.name,
                color,
                cal.url,
                access,
                cal.change_tag,
                cal.remote_id
            ],
        )?;
    } else {
        tx.execute(
            "INSERT INTO caldav_lists \
             (cdl_uuid, cdl_name, cdl_color, cdl_url, cdl_access, cdl_ctag, \
              cdl_order, cdl_last_sync) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0)",
            params![
                cal.remote_id,
                cal.name,
                color,
                cal.url,
                access,
                cal.change_tag
            ],
        )?;
    }
    Ok(())
}

fn upsert_task(tx: &rusqlite::Transaction<'_>, t: &RemoteTask) -> rusqlite::Result<i64> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT _id FROM tasks WHERE remoteId = ?1",
            params![t.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    let now_ms = t.completed_ms.max(t.due_ms).max(1);
    let due_ms = encode_due_with_has_time(t.due_ms, t.due_has_time);
    let title_arg: Option<&str> = t.title.as_deref();
    let notes_arg: Option<&str> = t.notes.as_deref();
    let recurrence_arg: Option<&str> = t.recurrence.as_deref();

    if let Some(id) = existing {
        tx.execute(
            "UPDATE tasks SET title = ?1, notes = ?2, dueDate = ?3, \
             completed = ?4, importance = ?5, recurrence = ?6, modified = ?7 \
             WHERE _id = ?8",
            params![
                title_arg,
                notes_arg,
                due_ms,
                t.completed_ms,
                t.priority,
                recurrence_arg,
                now_ms,
                id,
            ],
        )?;
        Ok(id)
    } else {
        tx.execute(
            "INSERT INTO tasks \
             (title, importance, dueDate, hideUntil, created, modified, \
              completed, deleted, notes, estimatedSeconds, elapsedSeconds, \
              timerStart, notificationFlags, lastNotified, recurrence, \
              repeat_from, collapsed, parent, read_only, remoteId) \
             VALUES (?1, ?2, ?3, 0, ?4, ?4, ?5, 0, ?6, 0, 0, 0, 0, 0, ?7, \
                     0, 0, 0, 0, ?8)",
            params![
                title_arg,
                t.priority,
                due_ms,
                now_ms,
                t.completed_ms,
                notes_arg,
                recurrence_arg,
                t.remote_id,
            ],
        )?;
        Ok(tx.last_insert_rowid())
    }
}

/// If `due_has_time` is set, ensure the milliseconds value carries
/// a non-zero seconds component so `tasks.dueDate % 60_000 > 0`
/// (Tasks.org's "has time" flag) reads true. CalDAV-sourced rows
/// often have `HH:00:00` exactly, which would otherwise look
/// date-only to the local query path.
fn encode_due_with_has_time(due_ms: i64, has_time: bool) -> i64 {
    if due_ms == 0 || !has_time {
        return due_ms;
    }
    if due_ms % 60_000 == 0 {
        due_ms + 1_000
    } else {
        due_ms
    }
}

fn upsert_caldav_task(
    tx: &rusqlite::Transaction<'_>,
    task_id: i64,
    t: &RemoteTask,
) -> rusqlite::Result<()> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT cd_id FROM caldav_tasks WHERE cd_remote_id = ?1",
            params![t.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(_id) = existing {
        tx.execute(
            "UPDATE caldav_tasks SET cd_task = ?1, cd_calendar = ?2, \
             cd_etag = ?3, cd_object = ?4, cd_remote_parent = ?5 \
             WHERE cd_remote_id = ?6",
            params![
                task_id,
                t.calendar_remote_id,
                t.etag,
                t.raw_vtodo
                    .as_deref()
                    .map(|_| format!("{}.ics", t.remote_id)),
                t.parent_remote_id,
                t.remote_id,
            ],
        )?;
    } else {
        tx.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_etag, cd_object, \
              cd_last_sync, cd_deleted, cd_remote_parent, gt_moved, \
              gt_remote_order) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, 0, 0)",
            params![
                task_id,
                t.calendar_remote_id,
                t.remote_id,
                t.etag,
                format!("{}.ics", t.remote_id),
                t.parent_remote_id,
            ],
        )?;
    }
    Ok(())
}

/// Soft-delete every task in `calendar_remote_id` whose
/// `caldav_tasks.cd_remote_id` isn't in the `seen` set. Matches
/// Android's behaviour when the server's calendar-query report
/// no longer returns a task the client had seen before: the row
/// was deleted remotely, so stamp `tasks.deleted = now_ms` and
/// `caldav_tasks.cd_deleted = now_ms` locally.
///
/// Rows that were already soft-deleted locally stay as-is.
///
/// Returns the count of newly tombstoned rows.
fn tombstone_missing_tasks(
    tx: &rusqlite::Transaction<'_>,
    calendar_remote_id: &str,
    seen: &[String],
    now_ms: i64,
) -> rusqlite::Result<usize> {
    // Pull the set of caldav_tasks rows currently in this calendar.
    let mut stmt = tx.prepare(
        "SELECT cd_task, cd_remote_id FROM caldav_tasks \
         WHERE cd_calendar = ?1 AND cd_deleted = 0",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([calendar_remote_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    let mut removed = 0;
    for (task_id, remote_id) in rows {
        if seen.iter().any(|s| s == &remote_id) {
            continue;
        }
        // Skip rows whose local task is already soft-deleted so
        // we don't bump `modified` and re-publish over sync.
        let already: Option<i64> = tx
            .query_row("SELECT deleted FROM tasks WHERE _id = ?1", [task_id], |r| {
                r.get(0)
            })
            .ok();
        if already.unwrap_or(0) > 0 {
            continue;
        }
        tx.execute(
            "UPDATE tasks SET deleted = ?1, modified = ?1 WHERE _id = ?2",
            params![now_ms, task_id],
        )?;
        tx.execute(
            "UPDATE caldav_tasks SET cd_deleted = ?1 WHERE cd_remote_id = ?2",
            params![now_ms, remote_id],
        )?;
        removed += 1;
    }
    Ok(removed)
}

/// Backfill `tasks.parent` from each task's `parent_remote_id`. Has
/// to run after every task in the batch is inserted so the parent
/// lookup can find rows that arrived later in the pull. Mirrors
/// `tasks_core::import::relink_subtasks`.
fn relink_parents(tx: &rusqlite::Transaction<'_>, tasks: &[RemoteTask]) -> rusqlite::Result<usize> {
    if tasks.is_empty() {
        return Ok(0);
    }
    // Build remote_id → local _id map for the rows we just touched.
    let mut local_id_by_remote: HashMap<&str, i64> = HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT _id, remoteId FROM tasks WHERE remoteId IS NOT NULL AND remoteId != ''",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let owned: Vec<(i64, String)> = rows.collect::<Result<_, _>>()?;
        // Reborrow against `tasks`'s string slices for the lookup.
        for (id, remote_id) in &owned {
            for t in tasks {
                if t.remote_id == *remote_id {
                    local_id_by_remote.insert(t.remote_id.as_str(), *id);
                }
            }
        }
    }
    let mut linked = 0;
    for t in tasks {
        if let Some(parent_remote) = t.parent_remote_id.as_deref() {
            if let (Some(child_id), Some(parent_id)) = (
                local_id_by_remote.get(t.remote_id.as_str()),
                local_id_by_remote.get(parent_remote),
            ) {
                tx.execute(
                    "UPDATE tasks SET parent = ?1 WHERE _id = ?2",
                    params![parent_id, child_id],
                )?;
                linked += 1;
            }
        }
    }
    Ok(linked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{AccountCredentials, ProviderKind};
    use crate::providers::caldav::CalDavProvider;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use tasks_core::db::Database;

    /// In-memory provider for engine tests. Holds a fixed list of
    /// calendars + per-calendar tasks; records every push call
    /// for assertions.
    #[derive(Clone, Default)]
    struct MockProvider {
        calendars: Vec<RemoteCalendar>,
        tasks: HashMap<String, Vec<RemoteTask>>,
        pushes: Arc<Mutex<Vec<RemoteTask>>>,
        deletes: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::CalDav
        }
        fn account_label(&self) -> &str {
            "mock"
        }
        async fn connect(&mut self) -> SyncResult<()> {
            Ok(())
        }
        async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
            Ok(self.calendars.clone())
        }
        async fn list_tasks(&mut self, cal: &str) -> SyncResult<Vec<RemoteTask>> {
            Ok(self.tasks.get(cal).cloned().unwrap_or_default())
        }
        async fn push_task(&mut self, t: &RemoteTask) -> SyncResult<Option<String>> {
            self.pushes.lock().unwrap().push(t.clone());
            Ok(Some("etag-pushed".to_string()))
        }
        async fn delete_task(&mut self, cal: &str, id: &str) -> SyncResult<()> {
            self.deletes
                .lock()
                .unwrap()
                .push((cal.to_string(), id.to_string()));
            Ok(())
        }
        async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
            Ok(SyncOutcome::default())
        }
    }

    fn calendar(uuid: &str, name: &str) -> RemoteCalendar {
        RemoteCalendar {
            remote_id: uuid.to_string(),
            name: name.to_string(),
            url: Some(format!("https://example/dav/{uuid}/")),
            color: Some(0),
            change_tag: Some("ctag-1".to_string()),
            read_only: false,
        }
    }

    fn task(uid: &str, cal: &str, parent: Option<&str>) -> RemoteTask {
        RemoteTask {
            remote_id: uid.to_string(),
            calendar_remote_id: cal.to_string(),
            etag: Some("etag-1".to_string()),
            title: Some(format!("Task {uid}")),
            notes: None,
            due_ms: 1_705_341_600_000,
            due_has_time: true,
            completed_ms: 0,
            priority: 0,
            recurrence: None,
            parent_remote_id: parent.map(str::to_string),
            raw_vtodo: None,
        }
    }

    fn fresh_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("tasks.db");
        drop(Database::open_or_create_read_only(&db_path).unwrap());
        (tmp, db_path)
    }

    #[tokio::test]
    async fn pull_inserts_calendars_and_tasks() {
        let (_tmp, db_path) = fresh_db();
        let mut tasks = HashMap::new();
        tasks.insert("cal-1".to_string(), vec![task("u-1", "cal-1", None)]);
        let mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.pull_all().await.unwrap();
        assert_eq!(outcome.calendars_pulled, 1);
        assert_eq!(outcome.tasks_pulled, 1);

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM caldav_lists", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let title: String = conn
            .query_row("SELECT title FROM tasks WHERE remoteId = 'u-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(title, "Task u-1");
        let cal: String = conn
            .query_row(
                "SELECT cd_calendar FROM caldav_tasks WHERE cd_remote_id = 'u-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cal, "cal-1");
    }

    #[tokio::test]
    async fn second_pull_tombstones_tasks_missing_from_remote() {
        // First pull: remote has tasks A + B.
        // Second pull: remote has only A — B should get
        // soft-deleted locally without bumping A.
        let (_tmp, db_path) = fresh_db();
        let mut tasks = HashMap::new();
        tasks.insert(
            "cal-1".to_string(),
            vec![task("a", "cal-1", None), task("b", "cal-1", None)],
        );
        let first = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(first));
        engine.pull_all().await.unwrap();

        // Second provider: only a remains.
        let mut tasks2 = HashMap::new();
        tasks2.insert("cal-1".to_string(), vec![task("a", "cal-1", None)]);
        let second = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks: tasks2,
            ..Default::default()
        };
        let mut engine2 = SyncEngine::new(&db_path, Box::new(second));
        engine2.pull_all().await.unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        // A stays active.
        let a_deleted: i64 = conn
            .query_row("SELECT deleted FROM tasks WHERE remoteId = 'a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(a_deleted, 0);
        // B got tombstoned.
        let b_deleted: i64 = conn
            .query_row("SELECT deleted FROM tasks WHERE remoteId = 'b'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(b_deleted > 0, "b should be soft-deleted");
        let b_caldav_deleted: i64 = conn
            .query_row(
                "SELECT cd_deleted FROM caldav_tasks WHERE cd_remote_id = 'b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(b_caldav_deleted > 0);
    }

    #[tokio::test]
    async fn pull_relinks_parent_subtask_relationships() {
        let (_tmp, db_path) = fresh_db();
        // Subtask listed *before* parent in the pull — the relink
        // pass must still resolve it.
        let mut tasks = HashMap::new();
        tasks.insert(
            "cal-1".to_string(),
            vec![
                task("child", "cal-1", Some("parent")),
                task("parent", "cal-1", None),
            ],
        );
        let mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        engine.pull_all().await.unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let parent_id: i64 = conn
            .query_row("SELECT _id FROM tasks WHERE remoteId = 'parent'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let child_parent: i64 = conn
            .query_row(
                "SELECT parent FROM tasks WHERE remoteId = 'child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(child_parent, parent_id);
    }

    #[tokio::test]
    async fn second_pull_is_idempotent_and_updates_in_place() {
        let (_tmp, db_path) = fresh_db();
        let mut tasks = HashMap::new();
        tasks.insert("cal-1".to_string(), vec![task("u-1", "cal-1", None)]);
        let mut mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let provider1 = mock.clone();
        let mut engine = SyncEngine::new(&db_path, Box::new(provider1));
        engine.pull_all().await.unwrap();
        // First pull: capture _id.
        let first_id: i64 = rusqlite::Connection::open(&db_path)
            .unwrap()
            .query_row("SELECT _id FROM tasks WHERE remoteId = 'u-1'", [], |r| {
                r.get(0)
            })
            .unwrap();

        // Mutate the remote title and pull again.
        mock.tasks
            .get_mut("cal-1")
            .unwrap()
            .iter_mut()
            .for_each(|t| t.title = Some("Renamed".into()));
        let mut engine2 = SyncEngine::new(&db_path, Box::new(mock));
        engine2.pull_all().await.unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (id, title): (i64, String) = conn
            .query_row(
                "SELECT _id, title FROM tasks WHERE remoteId = 'u-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        // Same row id (UPDATE, not INSERT), new title.
        assert_eq!(id, first_id);
        assert_eq!(title, "Renamed");
        let task_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(task_count, 1);
    }

    /// MockProvider variant whose push_task hands back a canned
    /// etag and records every call. Used to drive the push_dirty
    /// path without needing a real server.
    #[derive(Default, Clone)]
    struct MockWithPushResult {
        pushes: Arc<Mutex<Vec<RemoteTask>>>,
        conflict_on: Option<String>,
    }

    #[async_trait]
    impl Provider for MockWithPushResult {
        fn kind(&self) -> ProviderKind {
            ProviderKind::CalDav
        }
        fn account_label(&self) -> &str {
            "mock-push"
        }
        async fn connect(&mut self) -> SyncResult<()> {
            Ok(())
        }
        async fn list_calendars(&mut self) -> SyncResult<Vec<RemoteCalendar>> {
            Ok(Vec::new())
        }
        async fn list_tasks(&mut self, _cal: &str) -> SyncResult<Vec<RemoteTask>> {
            Ok(Vec::new())
        }
        async fn push_task(&mut self, t: &RemoteTask) -> SyncResult<Option<String>> {
            self.pushes.lock().unwrap().push(t.clone());
            if self.conflict_on.as_deref() == Some(t.remote_id.as_str()) {
                Err(SyncError::Conflict {
                    remote_id: t.remote_id.clone(),
                    local: t.etag.clone(),
                    server_message: "etag mismatch".into(),
                })
            } else {
                Ok(Some("etag-new".to_string()))
            }
        }
        async fn delete_task(&mut self, _c: &str, _id: &str) -> SyncResult<()> {
            Ok(())
        }
        async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
            Ok(SyncOutcome::default())
        }
    }

    fn seed_dirty_task(db_path: &std::path::Path) -> String {
        // Insert a task + a caldav_tasks row with cd_last_sync = 0
        // (brand-new local) so push_dirty picks it up.
        let conn = rusqlite::Connection::open(db_path).unwrap();
        conn.execute(
            "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
             modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, repeat_from, \
             collapsed, parent, read_only, remoteId) \
             VALUES ('Push me', 3, 0, 0, 1, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 'task-1')",
            [],
        )
        .unwrap();
        let task_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'cal-1', 'task-1', 0, 0, 0, 0)",
            [task_id],
        )
        .unwrap();
        "task-1".to_string()
    }

    #[tokio::test]
    async fn push_dirty_stamps_etag_after_success() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_dirty_task(&db_path);
        let mock = MockWithPushResult::default();
        let pushes = mock.pushes.clone();

        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_pushed, 1);
        assert_eq!(outcome.conflicts, 0);

        // Pushed exactly the one dirty task. Scope the guard so
        // clippy is happy about the subsequent `.await`.
        {
            let seen = pushes.lock().unwrap();
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0].remote_id, uid);
        }

        // Etag + last-sync stamped back onto the row.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (etag, last_sync): (Option<String>, i64) = conn
            .query_row(
                "SELECT cd_etag, cd_last_sync FROM caldav_tasks WHERE cd_remote_id = ?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(etag.as_deref(), Some("etag-new"));
        assert!(last_sync > 0);

        // Second push_dirty finds no dirty rows.
        let outcome2 = engine.push_dirty().await.unwrap();
        assert_eq!(outcome2.tasks_pushed, 0);
    }

    #[tokio::test]
    async fn push_dirty_reports_conflicts_without_aborting() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_dirty_task(&db_path);
        let mock = MockWithPushResult {
            conflict_on: Some(uid.clone()),
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_pushed, 0);
        assert_eq!(outcome.conflicts, 1);

        // Conflict did *not* stamp an etag — local row stays
        // "dirty" for the next push_dirty attempt.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let last_sync: i64 = conn
            .query_row(
                "SELECT cd_last_sync FROM caldav_tasks WHERE cd_remote_id = ?1",
                [&uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(last_sync, 0);
    }

    #[tokio::test]
    async fn caldav_provider_is_acceptable_to_engine_signature() {
        // Compile-time check: a real Provider impl plugs into the
        // engine without trait-bound friction. (CalDavProvider
        // returns NotYetImplemented today, so we'd assert the
        // error rather than running pull_all.)
        let (_tmp, db_path) = fresh_db();
        let p = CalDavProvider::new(AccountCredentials::default(), "test");
        let _engine = SyncEngine::new(&db_path, Box::new(p));
    }
}
