//! OS-level reminder notifications for task alarms.
//!
//! The Android client fires a notification when a row in the
//! `alarms` table reaches its `time`; the desktop client used to
//! read the same table for its detail-pane chip strip but never
//! fired anything. This module fills the gap.
//!
//! Architecture:
//!
//! * [`AlarmScheduler`] owns a map of pending [`AbortHandle`]s
//!   keyed by `alarms._id`. Each entry corresponds to a single
//!   `tokio::time::sleep_until` task spawned on the bridge's
//!   shared multi-thread runtime — no new long-running OS thread.
//!
//! * [`AlarmScheduler::reschedule_all`] re-reads the alarms table
//!   end-to-end and reconciles: anything no longer present (or
//!   whose computed fire-time has shifted) gets aborted and any
//!   newly-arrived alarm picks up a fresh handle.
//!
//! * Each spawned task wakes at the scheduled instant, opens a
//!   fresh read-only DB handle to look up the task's title +
//!   notes, and dispatches via a [`Notifier`] (defaults to the
//!   `notify-rust` backend; the test suite swaps in a recorder).
//!   Notification failures are logged at warn and swallowed —
//!   a flaky `org.freedesktop.Notifications` daemon shouldn't
//!   bring the app down.
//!
//! Supported alarm types today:
//!
//! * `DATE_TIME` (0) — absolute epoch ms, fired as-is.
//! * `REL_START` (1) — offset relative to `tasks.hideUntil`.
//! * `REL_END`   (2) — offset relative to `tasks.dueDate`.
//! * `SNOOZE`    (4) — absolute epoch ms (a Snooze action on the
//!   Android client lands here).
//!
//! Repeating alarms (`repeat > 0`) reschedule themselves
//! `interval` ms after each fire, decrementing `repeat` until
//! exhausted; the new schedule lives in memory only — we don't
//! mutate the DB row.
//!
//! Skipped (logged at trace, flagged as a follow-up):
//!
//! * `RANDOM`     (3) — Android picks a random offset within the
//!   interval; replicating that scheduling on desktop is doable
//!   but out of scope for the first cut.
//! * `GEO_ENTER`  (5) / `GEO_EXIT` (6) — geofences need a
//!   location service we don't ship.
//!
//! Edge cases:
//!
//! * **Past-due alarms.** When the app reopens after being
//!   closed, an alarm whose `time` is already in the past fires
//!   *once* immediately (so a missed reminder still surfaces
//!   when the user reopens the laptop). Anything more than
//!   [`STALE_WINDOW_MS`] overdue is silently dropped — surfacing
//!   a backlog of 30 reminders after an 8-hour sleep is worse
//!   than missing them.
//!
//! * **Tokio runtime not available.** [`reschedule_all`] takes
//!   a borrowed [`tokio::runtime::Handle`]; the bridge only
//!   constructs a runtime lazily on first sync. Until then, the
//!   bridge skips the call entirely (logged once at warn).
//!
//! * **macOS.** `notify-rust` requires the host process be a
//!   `.app` bundle to actually post a notification. Running from
//!   `cargo run` produces a "no NSBundle" warning the user can
//!   safely ignore; the packaging story uses a bundle.
//!
//! * **Windows.** `notify-rust` 4.x dispatches through the
//!   Toast Notification Manager, which expects the app to be
//!   registered in the Start Menu / via an AppUserModelID. Our
//!   packaging story registers it; running unregistered will
//!   either show a system-default toast or fail silently. If
//!   the registration story turns out to be too painful, the
//!   `winrt-toast` crate is a more direct alternative — flagged
//!   as a follow-up in the report.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tasks_core::models::AlarmType;
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

/// Anything more than this far past now is silently dropped on
/// reopen. The user shouldn't be ambushed with a backlog of stale
/// reminders the moment they unlock their laptop after a night's
/// sleep — but a reminder that fired five minutes ago is still
/// meaningful enough to surface on resume.
pub const STALE_WINDOW_MS: i64 = 5 * 60 * 1000;

/// Trait the scheduler fires through. The `notify-rust` impl is
/// the production path; the smoke test in this module's `tests`
/// section swaps in a recorder so it can observe a "notification
/// would have been shown" event without depending on a desktop
/// notification daemon (which CI doesn't have).
pub trait Notifier: Send + Sync + 'static {
    fn notify(&self, summary: &str, body: &str);
}

/// `notify-rust`-backed implementation. Notification failures are
/// logged at warn and swallowed; the user gets nothing, but the
/// app keeps running. We deliberately don't surface the error
/// any further — a desktop notification is a "fire and forget"
/// reminder, not a transactional acknowledgement.
#[derive(Default)]
pub struct OsNotifier;

impl Notifier for OsNotifier {
    fn notify(&self, summary: &str, body: &str) {
        let mut n = notify_rust::Notification::new();
        n.summary(summary);
        if !body.is_empty() {
            n.body(body);
        }
        // App-name shows in the Linux notification daemon's
        // grouping and on the macOS / Windows banners.
        n.appname("Tasks");
        if let Err(e) = n.show() {
            tracing::warn!("notify-rust: couldn't post notification: {e}");
        }
    }
}

/// One scheduled alarm's bookkeeping. Held in the scheduler's
/// `handles` map so a re-read can detect a stale entry by
/// comparing `(task, fire_at, repeat_remaining)` against what's
/// currently in the DB.
#[derive(Debug)]
struct PendingAlarm {
    /// Computed wall-clock fire time in epoch ms. Used by
    /// `reschedule_all`'s reconciliation step to decide whether
    /// the existing handle is still up to date or needs to be
    /// cancelled + respawned.
    fire_at_ms: i64,
    /// Tokio task handle. Drop semantics: `AbortHandle::abort()`
    /// signals the task to wake and exit; the task itself must
    /// still poll once for the abort to take effect.
    abort: AbortHandle,
}

/// Scheduler shared across [`reschedule_all`] / [`cancel_all`]
/// callers. The mutex is uncontended in practice — `reschedule_all`
/// runs on the QML thread, the spawned tasks only touch the map
/// from inside their own fire path on shutdown.
///
/// Wrap in [`Arc`] before sharing with spawned tokio tasks so each
/// fire can self-clean its entry without needing a separate
/// channel.
pub struct AlarmScheduler {
    handles: Mutex<HashMap<i64, PendingAlarm>>,
    notifier: Arc<dyn Notifier>,
}

impl Default for AlarmScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl AlarmScheduler {
    pub fn new() -> Self {
        Self::with_notifier(Arc::new(OsNotifier))
    }

    /// Test seam — lets the smoke test inject a recorder.
    pub fn with_notifier(notifier: Arc<dyn Notifier>) -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            notifier,
        }
    }

    /// Re-read every row from `alarms` and reconcile in-memory
    /// state. Called from the four hook sites listed in
    /// `bridge.rs`: post-open, post-watcher-debounce, post-sync,
    /// and post-task-edit.
    ///
    /// Takes `self: &Arc<Self>` so each spawned task can hold a
    /// weak reference back into the scheduler and remove its own
    /// entry from `handles` after firing — that keeps the smoke
    /// test's "pending_count drained itself" assertion honest
    /// without forcing the caller to re-run `reschedule_all`.
    pub fn reschedule_all(self: &Arc<Self>, runtime: &Handle, db_path: &Path) {
        let now_ms = now_ms();
        let rows = match read_alarms_with_anchors(db_path) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("notifier: couldn't read alarms from {db_path:?}: {e}");
                return;
            }
        };

        // Build a map of alarm_id -> desired schedule for this
        // refresh. Skip unsupported types up front so we don't
        // count them as "still wanted".
        let mut wanted: HashMap<i64, Schedule> = HashMap::new();
        for row in rows {
            let Some(schedule) = compute_schedule(&row, now_ms) else {
                continue;
            };
            wanted.insert(row.id, schedule);
        }

        // Reconcile against the existing map: cancel anything no
        // longer wanted or whose fire-time changed; spawn anything
        // newly wanted or whose timing shifted.
        let mut handles = self.handles.lock().expect("scheduler mutex poisoned");
        let stale: Vec<i64> = handles
            .iter()
            .filter_map(|(id, pending)| match wanted.get(id) {
                Some(sched) if sched.fire_at_ms == pending.fire_at_ms => None,
                _ => Some(*id),
            })
            .collect();
        for id in stale {
            if let Some(prev) = handles.remove(&id) {
                prev.abort.abort();
            }
        }

        for (id, schedule) in wanted {
            if handles.contains_key(&id) {
                continue;
            }
            let abort = spawn_alarm_task(
                runtime,
                Arc::clone(self),
                db_path.to_path_buf(),
                id,
                schedule,
            );
            handles.insert(
                id,
                PendingAlarm {
                    fire_at_ms: schedule.fire_at_ms,
                    abort,
                },
            );
        }
    }

    /// Internal: drop the entry for `alarm_id` from the handles
    /// map. Called by the spawned task once it's done firing
    /// (one-shot alarms or the final iteration of a repeater).
    fn forget(&self, alarm_id: i64) {
        if let Ok(mut handles) = self.handles.lock() {
            handles.remove(&alarm_id);
        }
    }

    /// Abort every pending handle. Called on app shutdown via
    /// `Drop` and when the user disables notifications in
    /// preferences. Idempotent.
    pub fn cancel_all(&self) {
        let mut handles = self.handles.lock().expect("scheduler mutex poisoned");
        for (_id, pending) in handles.drain() {
            pending.abort.abort();
        }
    }

    /// Number of currently-scheduled alarms. Used by the smoke
    /// test to assert "the alarm fired and removed itself";
    /// otherwise not exposed.
    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.handles.lock().map(|h| h.len()).unwrap_or(0)
    }
}

/// One row from [`read_alarms_with_anchors`]. Carries the
/// owning task's `dueDate` / `hideUntil` so [`compute_schedule`]
/// can resolve relative offsets without a second query.
#[derive(Debug, Clone, Copy)]
struct AlarmRow {
    id: i64,
    task: i64,
    time: i64,
    alarm_type: i32,
    repeat: i32,
    interval: i64,
    due_date: i64,
    hide_until: i64,
}

/// What [`compute_schedule`] hands back. `Copy` so the reconcile
/// loop can compare two by value cheaply.
#[derive(Debug, Clone, Copy)]
struct Schedule {
    fire_at_ms: i64,
    /// Repeats remaining *after* the upcoming fire. 0 = drop on
    /// fire; >0 = re-schedule for `fire_at_ms + interval` with
    /// `repeat - 1`.
    repeat: i32,
    interval: i64,
}

/// Resolve an alarm row to a concrete `(fire_at_ms, repeat,
/// interval)` triple, or `None` if the alarm should be skipped
/// entirely (unsupported type, missing anchor, or so far past
/// due that we drop it).
///
/// Pulled out into a free function so the unit tests can drive
/// the predicate directly without standing up a tokio runtime.
fn compute_schedule(row: &AlarmRow, now_ms: i64) -> Option<Schedule> {
    let raw_fire = match row.alarm_type {
        AlarmType::DATE_TIME | AlarmType::SNOOZE => row.time,
        AlarmType::REL_START => {
            if row.hide_until <= 0 {
                tracing::trace!(
                    "notifier: skipping REL_START alarm {} on task {} (no hideUntil)",
                    row.id,
                    row.task
                );
                return None;
            }
            row.hide_until.saturating_add(row.time)
        }
        AlarmType::REL_END => {
            if row.due_date <= 0 {
                tracing::trace!(
                    "notifier: skipping REL_END alarm {} on task {} (no dueDate)",
                    row.id,
                    row.task
                );
                return None;
            }
            row.due_date.saturating_add(row.time)
        }
        AlarmType::RANDOM | AlarmType::GEO_ENTER | AlarmType::GEO_EXIT => {
            tracing::trace!(
                "notifier: alarm type {} (id {}) is a follow-up — skipping",
                row.alarm_type,
                row.id
            );
            return None;
        }
        other => {
            tracing::trace!(
                "notifier: unknown alarm type {other} on id {} — skipping",
                row.id
            );
            return None;
        }
    };

    // Past-due handling.
    let fire_at_ms = if raw_fire <= now_ms {
        let overdue = now_ms.saturating_sub(raw_fire);
        if overdue > STALE_WINDOW_MS {
            // For repeating alarms we still want to advance to
            // the next future iteration rather than firing a
            // stale one — the user signed up for a recurring
            // ping, and skipping today's beat is fine.
            if row.repeat > 0 && row.interval > 0 {
                let mut next = raw_fire;
                let mut remaining = row.repeat;
                while next <= now_ms && remaining > 0 {
                    next = next.saturating_add(row.interval);
                    remaining -= 1;
                }
                if next > now_ms && remaining >= 0 {
                    return Some(Schedule {
                        fire_at_ms: next,
                        repeat: remaining,
                        interval: row.interval,
                    });
                }
            }
            tracing::trace!(
                "notifier: alarm {} on task {} is {} ms overdue (>{}) — dropping",
                row.id,
                row.task,
                overdue,
                STALE_WINDOW_MS
            );
            return None;
        }
        // Within the stale window: fire immediately.
        now_ms
    } else {
        raw_fire
    };

    Some(Schedule {
        fire_at_ms,
        repeat: row.repeat,
        interval: row.interval,
    })
}

/// Spawn a single tokio task that sleeps until `schedule.fire_at_ms`,
/// fires the notification, and (for repeating alarms) reschedules.
fn spawn_alarm_task(
    runtime: &Handle,
    scheduler: Arc<AlarmScheduler>,
    db_path: PathBuf,
    alarm_id: i64,
    schedule: Schedule,
) -> AbortHandle {
    runtime
        .spawn(async move {
            let notifier = Arc::clone(&scheduler.notifier);
            let mut current = schedule;
            loop {
                let wait_ms = (current.fire_at_ms - now_ms()).max(0) as u64;
                tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                // Look up the task's title + notes on a fresh
                // read-only handle. This is intentionally a fresh
                // open per fire — the runtime task never holds a
                // long-lived rusqlite::Connection (Connection is
                // !Sync; holding one across .await would force a
                // spawn_local).
                match read_task_for_alarm(&db_path, alarm_id) {
                    Ok(Some((title, notes))) => {
                        let summary = if title.is_empty() {
                            "Reminder".to_string()
                        } else {
                            title
                        };
                        notifier.notify(&summary, &notes);
                    }
                    Ok(None) => {
                        tracing::trace!(
                            "notifier: alarm {alarm_id} fired but task row was missing; \
                             swallowed"
                        );
                    }
                    Err(e) => {
                        tracing::warn!("notifier: couldn't read task for alarm {alarm_id}: {e}");
                    }
                }

                if current.repeat > 0 && current.interval > 0 {
                    current = Schedule {
                        fire_at_ms: current.fire_at_ms.saturating_add(current.interval),
                        repeat: current.repeat - 1,
                        interval: current.interval,
                    };
                    continue;
                }
                break;
            }
            // One-shot or last iteration of a repeater: drop
            // ourselves from the scheduler's handle map so
            // `pending_count()` reflects reality and a subsequent
            // `reschedule_all` doesn't see a phantom entry.
            scheduler.forget(alarm_id);
        })
        .abort_handle()
}

/// Read every row from `alarms` joined to `tasks` so the
/// scheduler has the anchor (`hideUntil` / `dueDate`) it needs to
/// resolve REL_START / REL_END.
fn read_alarms_with_anchors(db_path: &Path) -> rusqlite::Result<Vec<AlarmRow>> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(50))?;
    let mut stmt = conn.prepare(
        "SELECT a._id, a.task, a.time, a.type, a.repeat, a.interval, \
                t.dueDate, t.hideUntil \
         FROM alarms a \
         JOIN tasks t ON t._id = a.task \
         WHERE t.deleted = 0 AND t.completed = 0",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AlarmRow {
            id: r.get(0)?,
            task: r.get(1)?,
            time: r.get(2)?,
            alarm_type: r.get(3)?,
            repeat: r.get(4)?,
            interval: r.get(5)?,
            due_date: r.get(6)?,
            hide_until: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Tiny single-row lookup: title + notes for the task that
/// owns `alarm_id`. Empty strings on missing fields so the
/// notifier always has something to render.
fn read_task_for_alarm(
    db_path: &Path,
    alarm_id: i64,
) -> rusqlite::Result<Option<(String, String)>> {
    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(50))?;
    let row = conn
        .query_row(
            "SELECT COALESCE(t.title, ''), COALESCE(t.notes, '') \
             FROM tasks t JOIN alarms a ON a.task = t._id \
             WHERE a._id = ?1",
            [alarm_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .ok();
    Ok(row)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(alarm_type: i32, time: i64, due: i64, hide: i64) -> AlarmRow {
        AlarmRow {
            id: 1,
            task: 1,
            time,
            alarm_type,
            repeat: 0,
            interval: 0,
            due_date: due,
            hide_until: hide,
        }
    }

    #[test]
    fn date_time_in_future_schedules_at_time() {
        let now = 1_700_000_000_000;
        let r = row(AlarmType::DATE_TIME, now + 60_000, 0, 0);
        let s = compute_schedule(&r, now).expect("future DATE_TIME should schedule");
        assert_eq!(s.fire_at_ms, now + 60_000);
        assert_eq!(s.repeat, 0);
    }

    #[test]
    fn date_time_within_stale_window_fires_now() {
        let now = 1_700_000_000_000;
        // 30 seconds overdue — well inside the 5-minute window.
        let r = row(AlarmType::DATE_TIME, now - 30_000, 0, 0);
        let s = compute_schedule(&r, now).expect("recent past should fire immediately");
        assert_eq!(s.fire_at_ms, now);
    }

    #[test]
    fn date_time_beyond_stale_window_drops() {
        let now = 1_700_000_000_000;
        let r = row(AlarmType::DATE_TIME, now - STALE_WINDOW_MS - 1, 0, 0);
        assert!(compute_schedule(&r, now).is_none());
    }

    #[test]
    fn rel_start_resolves_against_hide_until() {
        let now = 1_700_000_000_000;
        let hide = now + 10 * 60_000;
        // Fire 5 minutes after hideUntil.
        let r = row(AlarmType::REL_START, 5 * 60_000, 0, hide);
        let s = compute_schedule(&r, now).expect("REL_START with hideUntil should schedule");
        assert_eq!(s.fire_at_ms, hide + 5 * 60_000);
    }

    #[test]
    fn rel_start_without_hide_until_skipped() {
        let now = 1_700_000_000_000;
        let r = row(AlarmType::REL_START, -5 * 60_000, 0, 0);
        assert!(compute_schedule(&r, now).is_none());
    }

    #[test]
    fn rel_end_resolves_against_due_date() {
        let now = 1_700_000_000_000;
        let due = now + 60 * 60_000;
        // 30 minutes before due.
        let r = row(AlarmType::REL_END, -30 * 60_000, due, 0);
        let s = compute_schedule(&r, now).expect("REL_END with dueDate should schedule");
        assert_eq!(s.fire_at_ms, due - 30 * 60_000);
    }

    #[test]
    fn rel_end_without_due_skipped() {
        let now = 1_700_000_000_000;
        let r = row(AlarmType::REL_END, -30 * 60_000, 0, 0);
        assert!(compute_schedule(&r, now).is_none());
    }

    #[test]
    fn snooze_schedules_at_absolute_time() {
        let now = 1_700_000_000_000;
        let r = row(AlarmType::SNOOZE, now + 5 * 60_000, 0, 0);
        let s = compute_schedule(&r, now).expect("future SNOOZE should schedule");
        assert_eq!(s.fire_at_ms, now + 5 * 60_000);
    }

    #[test]
    fn random_geo_skipped() {
        let now = 1_700_000_000_000;
        let future = now + 60_000;
        for ty in [AlarmType::RANDOM, AlarmType::GEO_ENTER, AlarmType::GEO_EXIT] {
            let r = row(ty, future, future, future);
            assert!(compute_schedule(&r, now).is_none(), "type {ty} should skip");
        }
    }

    #[test]
    fn repeating_overdue_advances_to_next_future_iteration() {
        let now = 1_700_000_000_000;
        // 90 minutes overdue (well past the stale window) but with
        // enough repeats left at a 1-hour interval to reach a
        // future iteration — should re-anchor instead of dropping.
        let mut r = row(AlarmType::DATE_TIME, now - 90 * 60 * 1000, 0, 0);
        r.repeat = 5;
        r.interval = 60 * 60 * 1000; // 1 hour
        let s = compute_schedule(&r, now).expect("repeating overdue alarm should re-anchor");
        assert!(s.fire_at_ms > now, "next iteration must be in the future");
        // raw fire was 90m ago; advancing 2× hour-interval lands
        // 30m in the future, consuming 2 of the 5 repeats. We don't
        // assert exact remaining since the rule is "first future
        // iteration", just that we burned at least one and the
        // remainder is still sane.
        assert!(s.repeat < 5);
        assert!(s.repeat >= 0);
    }

    #[test]
    fn repeating_overdue_drops_when_nothing_left() {
        let now = 1_700_000_000_000;
        let mut r = row(AlarmType::DATE_TIME, now - 24 * 60 * 60 * 1000, 0, 0);
        r.repeat = 1;
        r.interval = 60 * 60 * 1000; // 1h, but only 1 left → nothing future
        assert!(compute_schedule(&r, now).is_none());
    }

    /// Smoke test: hand the scheduler a fixture DB whose only
    /// alarm is 100 ms in the future, sleep 250 ms, and assert
    /// the recorder saw exactly one fire and the scheduler's
    /// pending map drained itself.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scheduler_fires_due_alarm() {
        use std::sync::Mutex as StdMutex;

        struct Recorder {
            calls: StdMutex<Vec<(String, String)>>,
        }
        impl Notifier for Recorder {
            fn notify(&self, summary: &str, body: &str) {
                self.calls
                    .lock()
                    .unwrap()
                    .push((summary.to_string(), body.to_string()));
            }
        }

        let recorder = Arc::new(Recorder {
            calls: StdMutex::new(Vec::new()),
        });

        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("tasks.db");
        // Stand up the schema via the tasks-core helper, then
        // insert one task + one alarm pointing 100 ms into the
        // future.
        drop(tasks_core::db::Database::open_or_create_read_only(&db_path).unwrap());
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let now = now_ms();
        let fire_at = now + 100;
        conn.execute(
            "INSERT INTO tasks (_id, title, notes, importance, dueDate, hideUntil, \
             created, modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, recurrence, repeat_from, \
             calendarUri, remoteId, collapsed, parent, `order`, read_only) \
             VALUES (1, 'Buy milk', 'Whole milk', 3, 0, 0, ?1, ?1, 0, 0, 0, 0, 0, 0, 0, \
             '', 0, NULL, NULL, 0, 0, NULL, 0)",
            [now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO alarms (_id, task, time, type, repeat, interval) \
             VALUES (1, 1, ?1, 0, 0, 0)",
            [fire_at],
        )
        .unwrap();
        drop(conn);

        let scheduler = Arc::new(AlarmScheduler::with_notifier(Arc::clone(&recorder) as _));
        let handle = Handle::current();
        scheduler.reschedule_all(&handle, &db_path);
        assert_eq!(scheduler.pending_count(), 1, "alarm should be scheduled");

        // Sleep long enough for the fire + a bit of slack for
        // the task to remove itself.
        tokio::time::sleep(Duration::from_millis(400)).await;

        let calls = recorder.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "expected one notification");
        assert_eq!(calls[0].0, "Buy milk");
        assert_eq!(calls[0].1, "Whole milk");

        // Spawned task removes itself from the scheduler's map
        // after firing (one-shot alarm, no repeats), so the
        // pending count should now be zero.
        assert_eq!(
            scheduler.pending_count(),
            0,
            "fired alarm should be cleared from the handle map"
        );

        // Cancel everything as a teardown nicety; the spawned
        // task has already finished naturally by now.
        scheduler.cancel_all();
    }

    /// `cancel_all` aborts pending handles before they fire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_all_prevents_fire() {
        struct Recorder {
            calls: std::sync::Mutex<u32>,
        }
        impl Notifier for Recorder {
            fn notify(&self, _summary: &str, _body: &str) {
                *self.calls.lock().unwrap() += 1;
            }
        }
        let recorder = Arc::new(Recorder {
            calls: std::sync::Mutex::new(0),
        });

        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("tasks.db");
        drop(tasks_core::db::Database::open_or_create_read_only(&db_path).unwrap());
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO tasks (_id, title, notes, importance, dueDate, hideUntil, \
             created, modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
             timerStart, notificationFlags, lastNotified, recurrence, repeat_from, \
             calendarUri, remoteId, collapsed, parent, `order`, read_only) \
             VALUES (1, 'X', '', 3, 0, 0, ?1, ?1, 0, 0, 0, 0, 0, 0, 0, '', 0, NULL, \
             NULL, 0, 0, NULL, 0)",
            [now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO alarms (_id, task, time, type, repeat, interval) \
             VALUES (1, 1, ?1, 0, 0, 0)",
            [now + 5_000],
        )
        .unwrap();
        drop(conn);

        let scheduler = Arc::new(AlarmScheduler::with_notifier(Arc::clone(&recorder) as _));
        scheduler.reschedule_all(&Handle::current(), &db_path);
        assert_eq!(scheduler.pending_count(), 1);
        scheduler.cancel_all();
        assert_eq!(scheduler.pending_count(), 0);

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(*recorder.calls.lock().unwrap(), 0);
    }
}
