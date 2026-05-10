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

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};

use crate::provider::{Provider, RemoteCalendar, RemoteTask, SyncError, SyncOutcome, SyncResult};

/// Drives one or more sync cycles against `provider`, persisting
/// pulled state into the local SQLite at `db_path`.
pub struct SyncEngine<'a> {
    db_path: &'a Path,
    provider: Box<dyn Provider + Send>,
    /// `caldav_accounts.cda_uuid` to scope writes to. When set,
    /// `push_dirty` only considers rows whose `cd_calendar` joins
    /// to a `caldav_lists` whose `cdl_account` equals this uuid —
    /// otherwise pushing the user's CalDAV account would try to
    /// upload tasks that belong to a different (Google /
    /// Microsoft / Etebase) account and fail with garbled URL
    /// errors. `None` means "every dirty row" — kept for the
    /// engine's own integration tests.
    account_filter: Option<String>,
    /// True after the first successful `provider.connect()` of
    /// this engine's lifetime. Subsequent calls to `pull_all` /
    /// `push_dirty` skip the connect, so `sync_now` doesn't pay
    /// for two `connect()`s per cycle (CalDAV PROPFIND ×2,
    /// EteSync re-login, etc.). Reset is intentionally absent:
    /// re-connecting requires a fresh engine.
    connected: bool,
}

impl<'a> SyncEngine<'a> {
    pub fn new(db_path: &'a Path, provider: Box<dyn Provider + Send>) -> Self {
        Self {
            db_path,
            provider,
            account_filter: None,
            connected: false,
        }
    }

    /// Per-account constructor. Identical to [`new`] but every
    /// later push cycle filters by `cdl_account = account_uuid`.
    pub fn new_for_account(
        db_path: &'a Path,
        provider: Box<dyn Provider + Send>,
        account_uuid: impl Into<String>,
    ) -> Self {
        Self {
            db_path,
            provider,
            account_filter: Some(account_uuid.into()),
            connected: false,
        }
    }

    /// Idempotent provider connect. Lazy: first call dispatches
    /// `provider.connect()` and flips `self.connected` on success;
    /// subsequent calls are a no-op. Used by both `pull_all` and
    /// `push_dirty` so a `sync_now` (which calls both) only pays
    /// for one round-trip.
    async fn ensure_connected(&mut self) -> SyncResult<()> {
        if self.connected {
            return Ok(());
        }
        self.provider.connect().await?;
        self.connected = true;
        Ok(())
    }

    /// Connect + pull every calendar's tasks. Returns the count of
    /// rows pulled. Does not push.
    ///
    /// **Concurrency contract:** every HTTP round trip happens
    /// *before* the write transaction opens, and the transaction
    /// itself uses `BEGIN IMMEDIATE` so the write lock is acquired
    /// at BEGIN time (where rusqlite's busy_timeout retry is
    /// reliable) rather than on the first write inside a `BEGIN
    /// DEFERRED` (where contention with another writer can surface
    /// as `SQLITE_BUSY` even in WAL mode). Parallel-account sync
    /// previously held the write lock for the duration of HTTP I/O,
    /// which deadlocked the second account on `database is locked`
    /// once HTTP exceeded the 5 s busy_timeout — staging all reads
    /// in memory first cuts the lock-held window down to the actual
    /// write batch.
    pub async fn pull_all(&mut self) -> SyncResult<SyncOutcome> {
        self.ensure_connected().await?;
        let calendars = self.provider.list_calendars().await?;

        // Stage every remote calendar's task list in memory before
        // opening the write tx. Order preserved so `tombstone_missing_tasks`
        // and `relink_parents` see exactly the same data the loops
        // below write.
        let mut staged: Vec<(RemoteCalendar, Vec<RemoteTask>)> =
            Vec::with_capacity(calendars.len());
        for cal in calendars {
            let tasks = self.provider.list_tasks(&cal.remote_id).await?;
            staged.push((cal, tasks));
        }

        let mut conn =
            open_rw(self.db_path).map_err(|e| SyncError::Local(format!("open db: {e}")))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| SyncError::Local(format!("begin tx: {e}")))?;

        for (cal, _) in &staged {
            upsert_calendar(&tx, cal, self.account_filter.as_deref())
                .map_err(|e| SyncError::Local(format!("calendar: {e}")))?;
        }

        let mut tasks_pulled = 0usize;
        let mut tasks_deleted = 0usize;
        // Two-pass to avoid parent ordering hazards: insert/update
        // every task first (parent stays 0), then a second pass
        // backfills `tasks.parent` once every remoteId is in place.
        let mut all_tasks: Vec<RemoteTask> = Vec::new();
        for (cal, tasks) in &staged {
            let mut seen_remote_ids: Vec<String> = Vec::with_capacity(tasks.len());
            for t in tasks {
                let outcome = upsert_task(&tx, t)
                    .map_err(|e| SyncError::Local(format!("task {}: {e}", t.remote_id)))?;
                // upsert_caldav_task always runs even on a no-op so
                // the etag + last_sync stamp track what we just saw
                // from the server (etags can re-mint without the
                // task body changing). Only the count gates on the
                // actual data change.
                upsert_caldav_task(&tx, outcome.id, t, outcome.modified)
                    .map_err(|e| SyncError::Local(format!("caldav_task {}: {e}", t.remote_id)))?;
                if outcome.changed {
                    tasks_pulled += 1;
                }
                seen_remote_ids.push(t.remote_id.clone());
            }
            // Anything in this calendar we had before but the
            // server didn't list this time: tombstone locally.
            // Matches what the Android client does when the
            // remote deletes a task.
            let now = tasks_core::now_ms();
            let removed = tombstone_missing_tasks(&tx, &cal.remote_id, &seen_remote_ids, now)
                .map_err(|e| SyncError::Local(format!("tombstone: {e}")))?;
            tasks_deleted += removed;
            all_tasks.extend(tasks.iter().cloned());
        }

        let parent_links = relink_parents(&tx, &all_tasks)
            .map_err(|e| SyncError::Local(format!("relink: {e}")))?;

        tx.commit()
            .map_err(|e| SyncError::Local(format!("commit: {e}")))?;

        tracing::info!(
            "sync pull: {} calendars, {} tasks, {} parent links, {} tombstoned",
            staged.len(),
            tasks_pulled,
            parent_links,
            tasks_deleted,
        );
        // `tasks_deleted` here counts server-side tombstones the
        // engine applied locally (rows the server told us are
        // gone). `SyncOutcome::tasks_deleted` represents the
        // opposite direction — locally soft-deleted rows we
        // successfully pushed up via `delete_task` — and is
        // populated by `push_dirty`. Keep them separate so the
        // status line's "K🗑" badge means "the user's deletes
        // reached the server" rather than "the server has new
        // tombstones for us."
        Ok(SyncOutcome {
            calendars_pulled: staged.len(),
            tasks_pulled,
            tasks_pushed: 0,
            tasks_deleted: 0,
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
    /// [`SyncError::Conflict`] from the provider is logged at
    /// warn, counted into [`SyncOutcome::conflicts`], and the
    /// row is left dirty for the next cycle — one conflict
    /// shouldn't block unrelated pushes, and the user resolves
    /// it by editing locally or accepting the server version on
    /// the next pull. Conflicts on the delete pass roll up into
    /// the same counter.
    pub async fn push_dirty(&mut self) -> SyncResult<SyncOutcome> {
        self.ensure_connected().await?;
        let conn = open_rw(self.db_path).map_err(|e| SyncError::Local(format!("open db: {e}")))?;
        let dirty = load_dirty_tasks(&conn, self.account_filter.as_deref())
            .map_err(|e| SyncError::Local(format!("load dirty: {e}")))?;
        // First pass: propagate locally-soft-deleted rows.
        //
        // Order: deletes BEFORE pushes. Soft-deletes are atomic
        // remote-side (no etag dance, no body) and benefit from
        // running while the rest of the world is still in flight;
        // pushing first would mean a single transient `Auth` /
        // `Network` error on a dirty row aborts the whole cycle
        // and the deletes pile up. A row qualifies when the
        // local `tasks.deleted` column is non-zero AND it carries
        // a server-acknowledged `cd_etag` (otherwise it never
        // made it upstream — nothing to do remotely; the row is
        // just a local-only stub) AND `cd_deleted` is zero.
        // On success: stamp `cd_deleted = now` and clear
        // `cd_etag` so we don't retry. On error: log at warn and
        // leave the row alone — next cycle retries. Per-row
        // tolerance applies to both this pass and the dirty
        // pass below.
        let to_delete = load_locally_deleted_tasks(&conn, self.account_filter.as_deref())
            .map_err(|e| SyncError::Local(format!("load deleted: {e}")))?;
        drop(conn); // Release the write handle before the async round trips.
        let mut deleted = 0usize;
        let mut delete_conflicts = 0usize;
        for (task_id, calendar_remote_id, remote_id, etag) in &to_delete {
            match self
                .provider
                .delete_task(calendar_remote_id, remote_id, etag.as_deref())
                .await
            {
                Ok(()) => {
                    let conn = open_rw(self.db_path)
                        .map_err(|e| SyncError::Local(format!("reopen db: {e}")))?;
                    if let Err(e) = record_delete_success(&conn, *task_id, tasks_core::now_ms()) {
                        tracing::warn!(
                            "delete: stamp cd_deleted failed for {remote_id}: {e}; \
                             will retry next cycle"
                        );
                        continue;
                    }
                    deleted += 1;
                }
                Err(SyncError::Conflict {
                    remote_id: rid,
                    server_message,
                    ..
                }) => {
                    // CalDAV's If-Match returned 412: a third
                    // party edited the row server-side after our
                    // local soft-delete was queued. Don't bulldoze
                    // — leave `cd_etag` and `cd_deleted` alone so
                    // the next pull refreshes the etag and the
                    // user can re-issue the delete (or accept
                    // the server version) consciously. Counted
                    // into `outcome.conflicts` so the status line
                    // reflects the unresolved state.
                    tracing::info!(
                        "delete conflict on {rid}: third-party edit; \
                         leaving local row for next cycle. server: {server_message}"
                    );
                    delete_conflicts += 1;
                }
                Err(e) => {
                    // All providers now map 404 / NotFound to
                    // Ok(()); anything reaching this arm is a
                    // genuine transient (Auth / Network /
                    // Protocol) that the next cycle retries.
                    tracing::warn!("delete failed for {remote_id} ({calendar_remote_id}): {e}");
                }
            }
        }

        // Second pass: dirty-row pushes. Per-row tolerance
        // matches the delete pass — a transient `Auth` /
        // `Network` / `Protocol` / `Local` error on one row no
        // longer aborts the whole cycle, just logs at warn and
        // moves on. The earlier "abort on first error" behaviour
        // (Round-2 review's A3) meant a single expired credential
        // would block every other dirty row from pushing AND
        // would have blocked the soft-delete pass too — solved
        // here by reordering deletes-before-pushes plus this
        // continue-on-error change.
        let mut pushed = 0usize;
        let mut conflicts = 0usize;
        let mut push_failures = 0usize;
        for (task, modified_at_load) in &dirty {
            match self.provider.push_task(task).await {
                Ok(new_etag) => {
                    let conn = open_rw(self.db_path)
                        .map_err(|e| SyncError::Local(format!("reopen db: {e}")))?;
                    // Stamp `cd_last_sync` with the modified value
                    // we read at push-start, not now_ms(). If the
                    // user edited the row mid-push, `tasks.modified`
                    // is now newer than the snapshot — leaving
                    // `tasks.modified > cd_last_sync` and re-flagging
                    // the row as dirty so the next cycle re-pushes
                    // their edit. Stamping now_ms() would silently
                    // "ack" the concurrent edit and lose it.
                    if let Err(e) = record_push_success(
                        &conn,
                        &task.remote_id,
                        new_etag.as_deref(),
                        *modified_at_load,
                    ) {
                        tracing::warn!(
                            "stamp etag failed for {}: {e}; will retry next cycle",
                            task.remote_id
                        );
                        push_failures += 1;
                        continue;
                    }
                    pushed += 1;
                }
                Err(SyncError::Conflict { remote_id, .. }) => {
                    tracing::warn!("push conflict on {remote_id}; keeping local");
                    conflicts += 1;
                }
                Err(other) => {
                    tracing::warn!(
                        "push failed for {}: {other}; will retry next cycle",
                        task.remote_id
                    );
                    push_failures += 1;
                }
            }
        }
        if push_failures > 0 {
            tracing::warn!("push_dirty: {push_failures} push(es) failed and will retry");
        }

        Ok(SyncOutcome {
            calendars_pulled: 0,
            tasks_pulled: 0,
            tasks_pushed: pushed,
            tasks_deleted: deleted,
            // Both push-side and delete-side conflicts roll up
            // into a single counter; the status-line semantics
            // ("there are N rows the user needs to look at") are
            // the same. The log line on each branch carries the
            // per-row context if the user wants more.
            conflicts: conflicts + delete_conflicts,
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
            tasks_deleted: pushed.tasks_deleted,
            conflicts: pushed.conflicts,
        })
    }
}

/// Load every task that needs pushing: a caldav_tasks row whose
/// paired `tasks.modified` exceeds `cd_last_sync`, OR (to cover
/// brand-new local tasks about to sync for the first time) rows
/// whose `cd_last_sync` is 0.
///
/// `account_filter` scopes the query to tasks belonging to a
/// specific `caldav_accounts.cda_uuid` — the bridge passes the
/// uuid of the account being synced so a CalDAV push doesn't
/// touch Google / Microsoft / Etebase rows. `None` returns every
/// dirty row, used by the engine's own integration tests.
/// Returns `(RemoteTask, modified_at_load_ms)` per dirty row. The
/// caller passes the snapshot to `record_push_success` so a
/// concurrent edit during the push round-trip leaves the row
/// flagged dirty for the next cycle (`tasks.modified` will be
/// newer than the snapshotted `cd_last_sync`).
fn load_dirty_tasks(
    conn: &Connection,
    account_filter: Option<&str>,
) -> rusqlite::Result<Vec<(RemoteTask, i64)>> {
    let base = "SELECT t._id, t.title, t.notes, t.dueDate, t.completed, \
                       t.importance, t.recurrence, t.modified, t.remoteId, \
                       ct.cd_calendar, ct.cd_remote_id, ct.cd_etag, \
                       ct.cd_remote_parent, ct.cd_last_sync \
                FROM tasks t \
                JOIN caldav_tasks ct ON ct.cd_task = t._id";
    let (sql, account_uuid) = match account_filter {
        Some(uuid) => (
            format!(
                "{base} \
                 JOIN caldav_lists cl ON cl.cdl_uuid = ct.cd_calendar \
                 WHERE t.deleted = 0 \
                   AND (ct.cd_last_sync = 0 OR t.modified > ct.cd_last_sync) \
                   AND cl.cdl_account = ?1"
            ),
            Some(uuid.to_string()),
        ),
        None => (
            format!(
                "{base} \
                 WHERE t.deleted = 0 \
                   AND (ct.cd_last_sync = 0 OR t.modified > ct.cd_last_sync)"
            ),
            None,
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(RemoteTask, i64)> {
        let due_ms: i64 = r.get(3)?;
        let due_has_time = due_ms != 0 && due_ms % 60_000 != 0;
        let modified_at_load: i64 = r.get(7)?;
        let task = RemoteTask {
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
            // Push path doesn't read the remote stamp — push uses
            // the local `tasks.modified` directly. Leaving this
            // None keeps the round-trip honest: nothing in
            // remote_task_to_vtodo references it.
            last_modified_ms: None,
            raw_vtodo: None,
        };
        Ok((task, modified_at_load))
    };
    let mut out = Vec::new();
    if let Some(ref uuid) = account_uuid {
        let rows = stmt.query_map(params![uuid], map_row)?;
        for row in rows {
            out.push(row?);
        }
    } else {
        let rows = stmt.query_map([], map_row)?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

/// After a successful push, stamp the new etag + the
/// snapshotted `modified_at_load` value so the next push_dirty
/// call doesn't re-send the same row. The caller is responsible
/// for passing the `tasks.modified` value it read at load time —
/// not `tasks_core::now_ms()` — so a concurrent local edit during the push
/// round-trip leaves the row legitimately dirty for next cycle.
/// Load every task the user soft-deleted locally that the
/// server hasn't been told about yet: rows where the local
/// `tasks.deleted` column is non-zero, the row carries a
/// server-acknowledged `cd_etag` (otherwise it never made it
/// upstream and there's nothing to delete remotely), and
/// `cd_deleted` is still zero (we haven't already propagated
/// the delete).
///
/// One row's worth of "locally soft-deleted, server-side
/// delete still pending" state. `cd_etag` is `None` for rows
/// where the server hasn't yet acknowledged a body (those
/// wouldn't pass the `cd_etag IS NOT NULL` filter today, but
/// the column is still nullable on the schema so we surface
/// it as `Option`); CalDAV uses it as the `If-Match` value so
/// a concurrent server-side edit surfaces as Conflict.
type PendingDelete = (i64, String, String, Option<String>);

/// The bridge passes `account_filter` so a CalDAV push doesn't
/// touch Google / Microsoft / Etebase rows.
fn load_locally_deleted_tasks(
    conn: &Connection,
    account_filter: Option<&str>,
) -> rusqlite::Result<Vec<PendingDelete>> {
    let base = "SELECT t._id, ct.cd_calendar, ct.cd_remote_id, ct.cd_etag \
                FROM tasks t \
                JOIN caldav_tasks ct ON ct.cd_task = t._id";
    let (sql, account_uuid) = match account_filter {
        Some(uuid) => (
            format!(
                "{base} \
                 JOIN caldav_lists cl ON cl.cdl_uuid = ct.cd_calendar \
                 WHERE t.deleted > 0 \
                   AND ct.cd_etag IS NOT NULL \
                   AND ct.cd_deleted = 0 \
                   AND cl.cdl_account = ?1"
            ),
            Some(uuid.to_string()),
        ),
        None => (
            format!(
                "{base} \
                 WHERE t.deleted > 0 \
                   AND ct.cd_etag IS NOT NULL \
                   AND ct.cd_deleted = 0"
            ),
            None,
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<PendingDelete> {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    };
    let mut out = Vec::new();
    if let Some(ref uuid) = account_uuid {
        for row in stmt.query_map(params![uuid], map_row)? {
            out.push(row?);
        }
    } else {
        for row in stmt.query_map([], map_row)? {
            out.push(row?);
        }
    }
    Ok(out)
}

/// After a successful `provider.delete_task`, mark the row as
/// fully tombstoned locally so the next push_dirty cycle skips
/// it. `cd_etag = NULL` breaks any future pull-side assumption
/// that the server still has the row; `cd_deleted = now` is the
/// idempotency marker.
fn record_delete_success(conn: &Connection, task_id: i64, now_ms: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE caldav_tasks SET cd_deleted = ?1, cd_etag = NULL \
         WHERE cd_task = ?2",
        params![now_ms, task_id],
    )?;
    Ok(())
}

fn record_push_success(
    conn: &Connection,
    remote_id: &str,
    new_etag: Option<&str>,
    cd_last_sync: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE caldav_tasks SET cd_etag = ?1, cd_last_sync = ?2 \
         WHERE cd_remote_id = ?3",
        params![new_etag, cd_last_sync, remote_id],
    )?;
    Ok(())
}

/// Open a writable handle to the desktop's SQLite. Mirrors the
/// shape `tasks_core::write` uses so the locking semantics —
/// WAL journal + 5 s busy_timeout — are the same; without that
/// alignment, parallel sync workers (one per account when the
/// auto-sync timer fans out) deadlock with `SQLITE_BUSY`.
fn open_rw(path: &Path) -> rusqlite::Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(path, flags)?;
    tasks_core::tune_writeback_connection(&conn)?;
    Ok(conn)
}

fn upsert_calendar(
    tx: &rusqlite::Transaction<'_>,
    cal: &RemoteCalendar,
    account_uuid: Option<&str>,
) -> rusqlite::Result<()> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT cdl_id FROM caldav_lists WHERE cdl_uuid = ?1",
            params![cal.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    let access = if cal.read_only { 2 } else { 1 }; // CalendarAccess::READ_ONLY / READ_WRITE
    if let Some(_id) = existing {
        // Update everything *except* cdl_color from the
        // server-supplied row first; colour is a special case
        // because the user can pick it locally via the sidebar's
        // colour picker, and the provider almost always omits it
        // (Radicale's PROPFIND doesn't return Apple-style
        // calendar-color unless explicitly asked for, and most
        // CalDAV deployments don't set one). Only overwrite
        // cdl_color when the provider actually surfaced one.
        if account_uuid.is_some() {
            tx.execute(
                "UPDATE caldav_lists \
                 SET cdl_name = ?1, cdl_url = ?2, \
                     cdl_access = ?3, cdl_ctag = ?4, cdl_account = ?5 \
                 WHERE cdl_uuid = ?6",
                params![
                    cal.name,
                    cal.url,
                    access,
                    cal.change_tag,
                    account_uuid,
                    cal.remote_id
                ],
            )?;
        } else {
            tx.execute(
                "UPDATE caldav_lists SET cdl_name = ?1, cdl_url = ?2, \
                 cdl_access = ?3, cdl_ctag = ?4 WHERE cdl_uuid = ?5",
                params![cal.name, cal.url, access, cal.change_tag, cal.remote_id],
            )?;
        }
        if let Some(c) = cal.color {
            tx.execute(
                "UPDATE caldav_lists SET cdl_color = ?1 WHERE cdl_uuid = ?2",
                params![c, cal.remote_id],
            )?;
        }
    } else {
        // Fresh insert: 0 means "no colour" (the QML chip /
        // sidebar dot fall back to neutral grey).
        let color = cal.color.unwrap_or(0);
        tx.execute(
            "INSERT INTO caldav_lists \
             (cdl_uuid, cdl_name, cdl_color, cdl_url, cdl_access, cdl_ctag, \
              cdl_order, cdl_last_sync, cdl_account) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7)",
            params![
                cal.remote_id,
                cal.name,
                color,
                cal.url,
                access,
                cal.change_tag,
                account_uuid
            ],
        )?;
    }
    Ok(())
}

/// Insert or update the `tasks` row for a remote task.
///
/// Returns `(local_id, chosen_modified_ms)`. The caller threads
/// `chosen_modified_ms` into `caldav_tasks.cd_last_sync` so a
/// freshly-pulled row doesn't immediately re-qualify as "dirty"
/// in the next push cycle (Android SHA `4928091a7`).
///
/// The chosen stamp prefers the remote `LAST-MODIFIED` when the
/// provider supplied one (`t.last_modified_ms`, Android SHA
/// `0c60d7ee3`); future-dated remote stamps are clamped to the
/// local clock so a misconfigured server can't poison the
/// dirtiness query. When the wire didn't carry a stamp, fall
/// back to the legacy synthesis (`completed_ms.max(due_ms).max(1)`)
/// so completed/scheduled tasks still sort sensibly in the
/// "recently modified" view.
/// Outcome of a single [`upsert_task`] call.
///
/// `changed` distinguishes a row that was actually written (INSERT, or
/// UPDATE that advanced the row's `modified` stamp) from a row the
/// server returned but local already had at the same or newer
/// `modified` — the "no-op" case. The caller uses this to keep the
/// per-sync `tasks_pulled` count meaningful as a delta rather than a
/// total of "tasks the server returned."
struct UpsertOutcome {
    id: i64,
    modified: i64,
    changed: bool,
}

fn upsert_task(tx: &rusqlite::Transaction<'_>, t: &RemoteTask) -> rusqlite::Result<UpsertOutcome> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT _id FROM tasks WHERE remoteId = ?1",
            params![t.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    let now = tasks_core::now_ms();
    let chosen_modified = match t.last_modified_ms {
        Some(stamp) => stamp.min(now), // clamp future-dated remote stamps
        None => t.completed_ms.max(t.due_ms).max(1),
    };
    let due_ms = encode_due_with_has_time(t.due_ms, t.due_has_time);
    let title_arg: Option<&str> = t.title.as_deref();
    let notes_arg: Option<&str> = t.notes.as_deref();
    let recurrence_arg: Option<&str> = t.recurrence.as_deref();

    if let Some(id) = existing {
        // `WHERE modified < ?7` makes the UPDATE a no-op when the
        // row's stored modified stamp already meets-or-beats the
        // chosen value — i.e. the server hasn't advanced the row
        // since our last pull. `affected == 0` then signals "nothing
        // changed locally" up to pull_all, which uses it to count
        // only delta rows toward `tasks_pulled` (the badge).
        let affected = tx.execute(
            "UPDATE tasks SET title = ?1, notes = ?2, dueDate = ?3, \
             completed = ?4, importance = ?5, recurrence = ?6, modified = ?7 \
             WHERE _id = ?8 AND modified < ?7",
            params![
                title_arg,
                notes_arg,
                due_ms,
                t.completed_ms,
                t.priority,
                recurrence_arg,
                chosen_modified,
                id,
            ],
        )?;
        Ok(UpsertOutcome {
            id,
            modified: chosen_modified,
            changed: affected > 0,
        })
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
                chosen_modified,
                t.completed_ms,
                notes_arg,
                recurrence_arg,
                t.remote_id,
            ],
        )?;
        Ok(UpsertOutcome {
            id: tx.last_insert_rowid(),
            modified: chosen_modified,
            changed: true,
        })
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
    chosen_modified: i64,
) -> rusqlite::Result<()> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT cd_id FROM caldav_tasks WHERE cd_remote_id = ?1",
            params![t.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(_id) = existing {
        // Stamp `cd_last_sync` with the same value we just wrote
        // to `tasks.modified` so the next push_dirty cycle doesn't
        // see the freshly-pulled row as a local edit (Android SHA
        // `4928091a7`).
        tx.execute(
            "UPDATE caldav_tasks SET cd_task = ?1, cd_calendar = ?2, \
             cd_etag = ?3, cd_object = ?4, cd_remote_parent = ?5, \
             cd_last_sync = ?6 \
             WHERE cd_remote_id = ?7",
            params![
                task_id,
                t.calendar_remote_id,
                t.etag,
                t.raw_vtodo
                    .as_deref()
                    .map(|_| format!("{}.ics", t.remote_id)),
                t.parent_remote_id,
                chosen_modified,
                t.remote_id,
            ],
        )?;
    } else {
        tx.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_etag, cd_object, \
              cd_last_sync, cd_deleted, cd_remote_parent, gt_moved, \
              gt_remote_order) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, 0, 0)",
            params![
                task_id,
                t.calendar_remote_id,
                t.remote_id,
                t.etag,
                format!("{}.ics", t.remote_id),
                chosen_modified,
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
    // Pull the set of caldav_tasks rows currently in this calendar
    // *that the server has acknowledged*. A row originating from a
    // pull carries a non-null `cd_etag` (the server stamped it on
    // the calendar-query); rows created locally before they've ever
    // been pushed have NULL etag and must not be treated as remote
    // deletions, or the pull-then-push cycle wipes a freshly-
    // created task before it ever leaves the device.
    let mut stmt = tx.prepare(
        "SELECT cd_task, cd_remote_id FROM caldav_tasks \
         WHERE cd_calendar = ?1 AND cd_deleted = 0 AND cd_etag IS NOT NULL",
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
        // Skip rows the user has edited since the last sync. The
        // server claims this row is gone, but we have local
        // changes the server hasn't seen — soft-deleting now
        // would silently drop the user's work. Leaving the row
        // intact lets the next push_dirty cycle export the local
        // edit, after which a subsequent server-side delete
        // could be re-detected legitimately. Same shape as the
        // relink_parents gate (Android SHA 3139b39cb).
        let local_modified: Option<(i64, i64)> = tx
            .query_row(
                "SELECT t.modified, ct.cd_last_sync FROM tasks t \
                 JOIN caldav_tasks ct ON ct.cd_task = t._id \
                 WHERE t._id = ?1",
                [task_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .ok();
        if let Some((modified, cd_last_sync)) = local_modified {
            if modified > cd_last_sync {
                tracing::warn!(
                    "tombstone_missing_tasks: keeping locally-edited {remote_id} \
                     (modified={modified} > cd_last_sync={cd_last_sync}) — \
                     server claims it's gone, but local edit is unsynced",
                );
                continue;
            }
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
                // Only backfill the parent on rows the user hasn't
                // touched since the last sync. Without this guard
                // we'd silently overwrite a locally-edited parent
                // with the server's value, and we wouldn't bump
                // `modified` either, so the user's change would
                // never get pushed back. Mirrors Android's
                // `CaldavDao.updateParents` (SHAs `3139b39cb` +
                // `7b7697842`).
                let updated = tx.execute(
                    "UPDATE tasks SET parent = ?1 \
                     WHERE _id = ?2 \
                       AND modified <= (SELECT cd_last_sync FROM caldav_tasks \
                                        WHERE cd_task = ?2)",
                    params![parent_id, child_id],
                )?;
                if updated > 0 {
                    linked += 1;
                }
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
        async fn delete_task(
            &mut self,
            cal: &str,
            id: &str,
            _etag: Option<&str>,
        ) -> SyncResult<()> {
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
            last_modified_ms: None,
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

        // Mutate the remote title and bump last_modified_ms so the
        // engine's "is this row actually newer?" gate sees a real
        // change. Real CalDAV / EteSync / Google / Microsoft servers
        // always advance modified on edit (LAST-MODIFIED / DTSTAMP
        // in iCalendar, etag bump elsewhere); a server that mutated
        // body without bumping modified would be a bug, and the
        // engine intentionally trusts modified as the merge anchor.
        mock.tasks
            .get_mut("cal-1")
            .unwrap()
            .iter_mut()
            .for_each(|t| {
                t.title = Some("Renamed".into());
                t.last_modified_ms = Some(tasks_core::now_ms() + 1);
            });
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

    /// `pull_all`'s `tasks_pulled` counter — surfaced in the per-
    /// account "Synced (N↓ / M↑)" badge — must report a delta, not a
    /// total. A server that returns the same set of unchanged tasks
    /// on every poll (every CalDAV / Google Tasks / MS To Do server
    /// without a sync-token API does this) would otherwise lock the
    /// badge at the initial-pull count forever, hiding any real
    /// activity.
    #[tokio::test]
    async fn second_pull_with_no_remote_changes_reports_zero_pulled() {
        let (_tmp, db_path) = fresh_db();
        let mut tasks = HashMap::new();
        tasks.insert("cal-1".to_string(), vec![task("u-1", "cal-1", None)]);
        let mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let provider1 = mock.clone();
        let mut engine = SyncEngine::new(&db_path, Box::new(provider1));
        let first = engine.pull_all().await.unwrap();
        assert_eq!(first.tasks_pulled, 1, "first pull writes the row");

        // Re-pull the same data with no remote-side mutation.
        let mut engine2 = SyncEngine::new(&db_path, Box::new(mock));
        let second = engine2.pull_all().await.unwrap();
        assert_eq!(
            second.tasks_pulled, 0,
            "second pull saw no actual change → badge should read 0↓"
        );
    }

    /// MockProvider variant whose push_task hands back a canned
    /// etag and records every call. Used to drive the push_dirty
    /// path without needing a real server.
    #[derive(Default, Clone)]
    struct MockWithPushResult {
        pushes: Arc<Mutex<Vec<RemoteTask>>>,
        deletes: Arc<Mutex<Vec<(String, String)>>>,
        /// Parallel to `deletes`, records the `etag: Option<&str>`
        /// the engine passed to each delete_task call. Round-3
        /// review's D-R3-2 wants the engine's `cd_etag` -> CalDAV
        /// `If-Match` plumbing covered without an HTTP harness.
        delete_etags: Arc<Mutex<Vec<Option<String>>>>,
        conflict_on: Option<String>,
        /// When set, `delete_task` returns this error instead of
        /// recording the call. Lets the soft-delete tests exercise
        /// the "leave the row alone for retry" branch.
        delete_error: Option<String>,
        /// When set, `delete_task` returns `SyncError::Conflict`
        /// targeting this remote_id — the 412 case the engine
        /// must NOT collapse into a transient warning. Round-3
        /// review's B-R3-5 fix.
        delete_conflict_on: Option<String>,
        /// When set, `push_task` returns `SyncError::Auth(_)`
        /// instead of recording the call. Round-2 review's A3
        /// — pin that a transient push failure no longer aborts
        /// the rest of `push_dirty`.
        push_error: Option<String>,
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
            if let Some(msg) = &self.push_error {
                return Err(SyncError::Auth(msg.clone()));
            }
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
        async fn delete_task(&mut self, cal: &str, id: &str, etag: Option<&str>) -> SyncResult<()> {
            // Record the etag the engine threaded down BEFORE
            // any conditional return so the test can assert on
            // it in both Ok and Err branches.
            self.delete_etags
                .lock()
                .unwrap()
                .push(etag.map(str::to_string));
            if self.delete_conflict_on.as_deref() == Some(id) {
                return Err(SyncError::Conflict {
                    remote_id: id.to_string(),
                    local: etag.map(str::to_string),
                    server_message: "third party edited".into(),
                });
            }
            if let Some(msg) = &self.delete_error {
                return Err(SyncError::Network(msg.clone()));
            }
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

    /// Fix 1 (Android SHA `0c60d7ee3`): when the wire carried a
    /// `LAST-MODIFIED` we already parsed into `last_modified_ms`,
    /// the engine has to write *that* value into `tasks.modified`
    /// — not a synthesised stamp. Otherwise a pull-then-push
    /// no-op cycle keeps re-marking the row dirty.
    #[tokio::test]
    async fn pull_uses_remote_last_modified_when_present() {
        let (_tmp, db_path) = fresh_db();
        let remote_stamp = 1_700_000_000_000_i64; // well in the past
        let mut t = task("u-1", "cal-1", None);
        t.last_modified_ms = Some(remote_stamp);
        let mut tasks = HashMap::new();
        tasks.insert("cal-1".to_string(), vec![t]);
        let mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        engine.pull_all().await.unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let modified: i64 = conn
            .query_row(
                "SELECT modified FROM tasks WHERE remoteId = 'u-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            modified, remote_stamp,
            "pull should preserve the remote LAST-MODIFIED on tasks.modified"
        );
        // And cd_last_sync should land on the same value so the
        // next push_dirty cycle doesn't see this row as a local
        // edit (Fix 3, SHA `4928091a7`).
        let last_sync: i64 = conn
            .query_row(
                "SELECT cd_last_sync FROM caldav_tasks WHERE cd_remote_id = 'u-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(last_sync, remote_stamp);
    }

    /// Fix 1 follow-up: a server with a skewed clock (or a buggy
    /// provider that emits a far-future LAST-MODIFIED) must not
    /// poison the local "modified" column. We clamp to the local
    /// `now()` so the dirtiness query still works.
    #[tokio::test]
    async fn pull_clamps_future_last_modified_to_now() {
        let (_tmp, db_path) = fresh_db();
        // Year 2999 stamp — way past any plausible local clock.
        let future_stamp = 32_503_680_000_000_i64;
        let before = tasks_core::now_ms();
        let mut t = task("u-future", "cal-1", None);
        t.last_modified_ms = Some(future_stamp);
        let mut tasks = HashMap::new();
        tasks.insert("cal-1".to_string(), vec![t]);
        let mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        engine.pull_all().await.unwrap();
        let after = tasks_core::now_ms();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let modified: i64 = conn
            .query_row(
                "SELECT modified FROM tasks WHERE remoteId = 'u-future'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            modified < future_stamp,
            "future stamp {future_stamp} should have been clamped, got {modified}"
        );
        assert!(
            modified >= before && modified <= after,
            "clamped stamp {modified} should be within [before={before}, after={after}]"
        );
        // cd_last_sync mirrors tasks.modified.
        let last_sync: i64 = conn
            .query_row(
                "SELECT cd_last_sync FROM caldav_tasks WHERE cd_remote_id = 'u-future'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(last_sync, modified);
    }

    /// Fix 3 (Android SHA `4928091a7`): freshly-pulled rows must
    /// land with `tasks.modified == caldav_tasks.cd_last_sync` so
    /// the dirtiness query (`modified > cd_last_sync OR
    /// cd_last_sync = 0`) doesn't fire for them.
    #[tokio::test]
    async fn pull_writes_matching_modified_and_cd_last_sync_for_inserts() {
        let (_tmp, db_path) = fresh_db();
        // No remote LAST-MODIFIED — exercise the fallback path
        // that synthesises from completed/due so the insert
        // doesn't accidentally use a different stamp than the
        // caldav_tasks row.
        let mut tasks = HashMap::new();
        tasks.insert("cal-1".to_string(), vec![task("fresh", "cal-1", None)]);
        let mock = MockProvider {
            calendars: vec![calendar("cal-1", "Work")],
            tasks,
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        engine.pull_all().await.unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (modified, last_sync): (i64, i64) = conn
            .query_row(
                "SELECT t.modified, ct.cd_last_sync FROM tasks t \
                 JOIN caldav_tasks ct ON ct.cd_task = t._id \
                 WHERE t.remoteId = 'fresh'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(modified > 0);
        assert_eq!(
            modified, last_sync,
            "freshly-pulled row should not look dirty: modified ({modified}) must equal cd_last_sync ({last_sync})"
        );

        // Sanity: load_dirty_tasks should report nothing.
        let dirty = load_dirty_tasks(&conn, None).unwrap();
        assert!(
            dirty.iter().all(|(t, _)| t.remote_id != "fresh"),
            "freshly-pulled row should not be in the dirty set"
        );
    }

    /// Fix 2 (Android SHAs `3139b39cb` + `7b7697842`):
    /// `relink_parents` must not overwrite the parent column on
    /// rows the user has touched locally since the last sync.
    /// Otherwise we silently lose the user's edit (and don't bump
    /// `modified` to re-export it either).
    #[test]
    fn relink_parents_skips_locally_modified_rows() {
        let (_tmp, db_path) = fresh_db();
        let mut conn = rusqlite::Connection::open(&db_path).unwrap();
        // Seed: parent + child task. cd_last_sync = 1000 on the
        // child; tasks.modified = 2000 → user edited the child
        // since the last sync.
        conn.execute(
            "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
             modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, repeat_from, \
             collapsed, parent, read_only, remoteId) \
             VALUES ('parent-old', 0, 0, 0, 1, 1000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 'p-old')",
            [],
        )
        .unwrap();
        let parent_old_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
             modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, repeat_from, \
             collapsed, parent, read_only, remoteId) \
             VALUES ('parent-new', 0, 0, 0, 1, 1000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 'p-new')",
            [],
        )
        .unwrap();
        let parent_new_id = conn.last_insert_rowid();
        // The child currently has parent = p-old locally (rowid).
        // The user has edited the child (modified=2000 > cd_last_sync=1000).
        conn.execute(
            "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
             modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, repeat_from, \
             collapsed, parent, read_only, remoteId) \
             VALUES ('child', 0, 0, 0, 1, 2000, 0, 0, 0, 0, 0, 0, 0, 0, 0, ?1, 0, 'c')",
            [parent_old_id],
        )
        .unwrap();
        let child_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'cal-1', 'c', 1000, 0, 0, 0)",
            [child_id],
        )
        .unwrap();
        // Parents need caldav_tasks rows too so the relink lookup
        // resolves their remote ids.
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'cal-1', 'p-old', 1000, 0, 0, 0)",
            [parent_old_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'cal-1', 'p-new', 1000, 0, 0, 0)",
            [parent_new_id],
        )
        .unwrap();

        // Server says the child's parent is now `p-new`.
        let remote = vec![
            task("p-old", "cal-1", None),
            task("p-new", "cal-1", None),
            task("c", "cal-1", Some("p-new")),
        ];
        let tx = conn.transaction().unwrap();
        let linked = relink_parents(&tx, &remote).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            linked, 0,
            "the only candidate child has been locally edited; relink should skip it"
        );

        // Local parent should still point at p-old.
        let actual_parent: i64 = conn
            .query_row("SELECT parent FROM tasks WHERE _id = ?1", [child_id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            actual_parent, parent_old_id,
            "locally-edited child should keep its local parent"
        );

        // And the modified stamp should not have been bumped — the
        // user's edit is still authoritative.
        let modified: i64 = conn
            .query_row(
                "SELECT modified FROM tasks WHERE _id = ?1",
                [child_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(modified, 2000);
    }

    /// Fix 4: `record_push_success` must stamp `cd_last_sync` with
    /// the `tasks.modified` value snapshotted at push-start, not
    /// `tasks_core::now_ms()`. If the user edits the row mid-push, the snapshot
    /// is older than the post-edit modified, so the row stays
    /// dirty and gets re-pushed on the next cycle. Stamping
    /// `tasks_core::now_ms()` would silently ack the concurrent edit and lose
    /// it.
    #[tokio::test]
    async fn push_dirty_uses_modified_snapshot_not_now_for_cd_last_sync() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_dirty_task(&db_path);
        // The seed_dirty_task helper writes tasks.modified=100.
        // After a successful push, cd_last_sync should be 100, not
        // the current wall-clock tasks_core::now_ms() (which is 6+ orders of
        // magnitude larger).
        let mock = MockWithPushResult::default();
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        engine.push_dirty().await.unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let last_sync: i64 = conn
            .query_row(
                "SELECT cd_last_sync FROM caldav_tasks WHERE cd_remote_id = ?1",
                [&uid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            last_sync, 100,
            "cd_last_sync should match the modified value read at \
             load time, not tasks_core::now_ms()"
        );
    }

    /// Fix 4 (continued): if the user edits the row between
    /// load_dirty_tasks and record_push_success, the post-edit
    /// modified must still satisfy `modified > cd_last_sync` so
    /// the next push_dirty picks the row up again.
    #[tokio::test]
    async fn push_dirty_leaves_concurrently_edited_row_dirty() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_dirty_task(&db_path);
        // Custom mock whose push_task simulates the user editing
        // the row mid-flight by bumping tasks.modified before
        // returning. This is the race window the fix addresses.
        #[derive(Clone)]
        struct EditingMock {
            db_path: std::path::PathBuf,
            uid: String,
        }
        #[async_trait]
        impl Provider for EditingMock {
            fn kind(&self) -> ProviderKind {
                ProviderKind::CalDav
            }
            fn account_label(&self) -> &str {
                "editing-mock"
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
            async fn push_task(&mut self, _t: &RemoteTask) -> SyncResult<Option<String>> {
                let conn = rusqlite::Connection::open(&self.db_path).unwrap();
                conn.execute(
                    "UPDATE tasks SET title = 'Edited mid-push', \
                     modified = 5000 WHERE remoteId = ?1",
                    [&self.uid],
                )
                .unwrap();
                Ok(Some("etag-new".into()))
            }
            async fn delete_task(
                &mut self,
                _c: &str,
                _id: &str,
                _etag: Option<&str>,
            ) -> SyncResult<()> {
                Ok(())
            }
            async fn sync_once(&mut self) -> SyncResult<SyncOutcome> {
                Ok(SyncOutcome::default())
            }
        }
        let mock = EditingMock {
            db_path: db_path.clone(),
            uid: uid.clone(),
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        engine.push_dirty().await.unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (modified, last_sync): (i64, i64) = conn
            .query_row(
                "SELECT t.modified, ct.cd_last_sync FROM tasks t \
                 JOIN caldav_tasks ct ON ct.cd_task = t._id \
                 WHERE t.remoteId = ?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(modified, 5000, "concurrent edit should not be overwritten");
        assert_eq!(
            last_sync, 100,
            "cd_last_sync should remain at the snapshot, not the post-edit value"
        );
        assert!(
            modified > last_sync,
            "row must remain dirty so the next cycle re-pushes the user's edit"
        );

        // Confirm the row is detected as dirty.
        let dirty = load_dirty_tasks(&conn, None).unwrap();
        assert!(
            dirty.iter().any(|(t, _)| t.remote_id == uid),
            "row must be in the dirty set after the concurrent edit"
        );
    }

    /// `tombstone_missing_tasks` must skip rows the user has edited
    /// since the last sync. The server claims the row is gone, but
    /// we have unsynced local changes — soft-deleting now would
    /// silently drop the user's work. Same shape as the
    /// relink_parents gate (Android SHA 3139b39cb).
    #[tokio::test]
    async fn tombstone_skips_locally_edited_rows() {
        // First pull: remote has tasks A + B.
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

        // Simulate the user editing B locally — bump tasks.modified
        // past cd_last_sync so the dirty-check fires.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE tasks SET title = 'B (edited locally)', \
             modified = (SELECT cd_last_sync FROM caldav_tasks \
                         WHERE cd_remote_id = 'b') + 10000 \
             WHERE remoteId = 'b'",
            [],
        )
        .unwrap();
        drop(conn);

        // Second pull: only A remains on the server. The B row
        // should be preserved because it has unsynced local edits.
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
        let b_deleted: i64 = conn
            .query_row("SELECT deleted FROM tasks WHERE remoteId = 'b'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            b_deleted, 0,
            "locally-edited row must not be tombstoned by a server-quiet removal"
        );
        let b_caldav_deleted: i64 = conn
            .query_row(
                "SELECT cd_deleted FROM caldav_tasks WHERE cd_remote_id = 'b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b_caldav_deleted, 0);
        let b_title: String = conn
            .query_row("SELECT title FROM tasks WHERE remoteId = 'b'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(b_title, "B (edited locally)");
    }

    /// Seed a task that's been soft-deleted locally and *was*
    /// previously pushed (so it carries a `cd_etag`). The
    /// soft-delete-propagation path in push_dirty should pick
    /// this up.
    fn seed_locally_deleted_task(db_path: &std::path::Path) -> String {
        let conn = rusqlite::Connection::open(db_path).unwrap();
        // deleted = 100 (any non-zero); mirrors what
        // toggle_task_completion / delete_selected_task write.
        conn.execute(
            "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
             modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, repeat_from, \
             collapsed, parent, read_only, remoteId) \
             VALUES ('Goner', 3, 0, 0, 1, 200, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 'gone-1')",
            [],
        )
        .unwrap();
        let task_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_etag, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'cal-1', 'gone-1', 'old-etag', 100, 0, 0, 0)",
            [task_id],
        )
        .unwrap();
        "gone-1".to_string()
    }

    /// A1: a soft-deleted local row that previously synced should
    /// be pushed up as a `provider.delete_task` call, then have
    /// its `cd_deleted` stamped + `cd_etag` cleared so the next
    /// cycle skips it.
    #[tokio::test]
    async fn push_dirty_propagates_local_soft_deletes() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_locally_deleted_task(&db_path);
        let mock = MockWithPushResult::default();
        let deletes = mock.deletes.clone();

        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_deleted, 1);
        assert_eq!(outcome.tasks_pushed, 0); // no live dirty rows seeded.

        // Provider saw exactly the one delete with the right keys.
        // Snapshot under the lock + drop the guard before any
        // .await below — clippy's await_holding_lock would
        // otherwise flag the next push_dirty across a live
        // MutexGuard.
        {
            let seen = deletes.lock().unwrap();
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0], ("cal-1".to_string(), "gone-1".to_string()));
        }

        // Row stamped: cd_deleted > 0, cd_etag NULL.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (cd_deleted, cd_etag): (i64, Option<String>) = conn
            .query_row(
                "SELECT cd_deleted, cd_etag FROM caldav_tasks WHERE cd_remote_id = ?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(cd_deleted > 0);
        assert!(cd_etag.is_none());

        // Second push_dirty is a no-op for this row.
        let outcome2 = engine.push_dirty().await.unwrap();
        assert_eq!(outcome2.tasks_deleted, 0);
    }

    /// A1: when `delete_task` errors (transient network, 5xx),
    /// the row stays as-is and the next cycle retries. The error
    /// must NOT abort the rest of push_dirty — other rows in the
    /// same batch should still process normally.
    #[tokio::test]
    async fn push_dirty_retries_failed_deletes_next_cycle() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_locally_deleted_task(&db_path);
        let mock = MockWithPushResult {
            delete_error: Some("transient 502 from server".into()),
            ..Default::default()
        };
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        // Engine reports zero deletes — the failure is logged,
        // not surfaced as an Err that aborts push_dirty.
        assert_eq!(outcome.tasks_deleted, 0);

        // Row state unchanged: still soft-deleted locally,
        // cd_deleted = 0, cd_etag still 'old-etag'.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (cd_deleted, cd_etag): (i64, Option<String>) = conn
            .query_row(
                "SELECT cd_deleted, cd_etag FROM caldav_tasks WHERE cd_remote_id = ?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(cd_deleted, 0);
        assert_eq!(cd_etag.as_deref(), Some("old-etag"));
    }

    /// A1 boundary: a soft-deleted row that was *never* pushed
    /// (cd_etag NULL) should NOT be reported to the server —
    /// there's nothing remote to delete. The local row just
    /// stays soft-deleted.
    #[tokio::test]
    async fn push_dirty_skips_local_only_soft_deletes() {
        let (_tmp, db_path) = fresh_db();
        // Seed without cd_etag.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
             modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, repeat_from, \
             collapsed, parent, read_only, remoteId) \
             VALUES ('Local stub', 3, 0, 0, 1, 200, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 'stub-1')",
            [],
        )
        .unwrap();
        let task_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_etag, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'cal-1', 'stub-1', NULL, 0, 0, 0, 0)",
            [task_id],
        )
        .unwrap();
        drop(conn);

        let mock = MockWithPushResult::default();
        let deletes = mock.deletes.clone();
        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_deleted, 0);
        assert!(deletes.lock().unwrap().is_empty());
    }

    /// Round-2 review's A3: a transient `Auth` (or any non-
    /// Conflict) error from `push_task` used to abort
    /// `push_dirty` mid-loop, skipping the soft-delete pass and
    /// any unprocessed dirty rows. Pin the new behaviour: the
    /// delete pass runs first AND completes regardless of any
    /// dirty-row push failure. The cycle returns Ok(SyncOutcome).
    #[tokio::test]
    async fn push_dirty_propagates_deletes_even_when_push_fails() {
        let (_tmp, db_path) = fresh_db();
        // One soft-deleted row + one dirty row (seeded by
        // `seed_dirty_task`).
        let _del_uid = seed_locally_deleted_task(&db_path);
        let _dirty_uid = seed_dirty_task(&db_path);
        let mock = MockWithPushResult {
            push_error: Some("token expired".into()),
            ..Default::default()
        };
        let deletes = mock.deletes.clone();

        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        // Critically: push_dirty returns Ok despite the push
        // failure, and the delete still got through.
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_deleted, 1);
        assert_eq!(outcome.tasks_pushed, 0);
        assert_eq!(deletes.lock().unwrap().len(), 1);
    }

    /// Round-3 review's D-R3-2: pin that the engine threads
    /// `caldav_tasks.cd_etag` into `provider.delete_task` as the
    /// etag arg. Without this, the CalDAV `If-Match` plumbing
    /// (A6) is unreachable.
    #[tokio::test]
    async fn push_dirty_threads_cd_etag_into_delete_task() {
        let (_tmp, db_path) = fresh_db();
        // seed_locally_deleted_task hard-codes cd_etag = 'old-etag'.
        let _uid = seed_locally_deleted_task(&db_path);
        let mock = MockWithPushResult::default();
        let etags = mock.delete_etags.clone();

        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_deleted, 1);
        let seen = etags.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].as_deref(), Some("old-etag"));
    }

    /// Round-3 review's B-R3-5 / D-R3-1: a 412 from
    /// `provider.delete_task` (CalDAV's If-Match precondition
    /// fired because a third party edited the row server-side)
    /// must NOT silently overwrite the remote edit on the next
    /// cycle. The engine counts it into `outcome.conflicts`,
    /// leaves `cd_etag` and `cd_deleted` alone (so the next
    /// pull refreshes the etag and the user gets a chance to
    /// re-issue the delete consciously), and the cycle still
    /// returns Ok.
    #[tokio::test]
    async fn push_dirty_surfaces_delete_conflict_without_clearing_state() {
        let (_tmp, db_path) = fresh_db();
        let uid = seed_locally_deleted_task(&db_path);
        let mock = MockWithPushResult {
            delete_conflict_on: Some(uid.clone()),
            ..Default::default()
        };
        let deletes = mock.deletes.clone();

        let mut engine = SyncEngine::new(&db_path, Box::new(mock));
        let outcome = engine.push_dirty().await.unwrap();
        assert_eq!(outcome.tasks_deleted, 0);
        assert_eq!(outcome.conflicts, 1);
        // The conflict short-circuits before the success-record
        // path, so `deletes` (which records only successful
        // calls in this mock) stays empty.
        assert!(deletes.lock().unwrap().is_empty());

        // Row state is preserved for the next cycle: cd_deleted
        // still 0, cd_etag still 'old-etag'. The next pull will
        // refresh cd_etag and the user can re-issue the delete.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let (cd_deleted, cd_etag): (i64, Option<String>) = conn
            .query_row(
                "SELECT cd_deleted, cd_etag FROM caldav_tasks WHERE cd_remote_id = ?1",
                [&uid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(cd_deleted, 0);
        assert_eq!(cd_etag.as_deref(), Some("old-etag"));
    }
}
