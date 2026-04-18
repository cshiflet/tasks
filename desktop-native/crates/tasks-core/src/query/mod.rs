//! SQL builders for task list views.
//!
//! Two paths live here:
//!
//! * `TaskFilter` + `run` — a tiny, concrete filter set used by the
//!   read-only desktop client's initial screens. Each filter emits its own
//!   hand-rolled `SELECT` and binds through the prepared-statement path, so
//!   there's no string interpolation of user data.
//! * `build_recursive_query` — a faithful port of the Android
//!   `TaskListQueryRecursive` builder. Produces the full CTE-based query
//!   the main task list uses; the UI layer pairs it with a user's
//!   `QueryPreferences` and the active `QueryFilter`.

pub mod filter;
pub mod non_recursive;
pub mod permasql;
pub mod preferences;
pub mod recursive;
pub mod sort;

pub use filter::QueryFilter;
pub use non_recursive::build_non_recursive_query;
pub use preferences::QueryPreferences;
pub use recursive::build_recursive_query;

use rusqlite::params;

use crate::db::Database;
use crate::error::Result;
use crate::models::Task;

/// Mirrors `TaskListQuery.getQuery`: pick the recursive or non-recursive
/// builder depending on filter capabilities and user preferences.
///
/// The cascade matches Kotlin's `when` ordering:
///
///   1. `supportsManualSort() && preferences.isManualSort` → recursive
///   2. `filter is AstridOrderingFilter && preferences.isAstridSort` →
///      non-recursive
///   3. `filter.supportsSorting()` → recursive
///   4. else → non-recursive
///
/// `supportsSorting()` is `false` only for `RecentlyModifiedFilter`; the
/// other `Filter` subclasses inherit the default `true`. We therefore use
/// `!is_recently_modified` as the stand-in for case 3.
pub fn build_query(
    filter: &QueryFilter,
    prefs: &QueryPreferences,
    now_ms: i64,
    limit: Option<usize>,
) -> String {
    let supports_manual = filter.supports_manual_sort();
    let is_astrid = matches!(
        filter,
        QueryFilter::Custom {
            supports_astrid_ordering: true,
            ..
        }
    );
    let is_recently_modified = matches!(
        filter,
        QueryFilter::Custom {
            is_recently_modified: true,
            ..
        }
    );

    let recursive = if supports_manual && prefs.is_manual_sort {
        true
    } else if is_astrid && prefs.is_astrid_sort {
        false
    } else {
        !is_recently_modified
    };

    if recursive {
        build_recursive_query(filter, prefs, now_ms, limit)
    } else {
        build_non_recursive_query(filter, prefs, now_ms, limit)
    }
}

/// Selectors for task list views. Mirrors a subset of the built-in filters
/// from `app/src/main/java/org/tasks/filters/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    /// All active (not completed, not deleted, past hide-until) tasks.
    Active,
    /// Tasks due today (local day in the caller-supplied timezone offset).
    Today {
        day_start_utc_ms: i64,
        day_end_utc_ms: i64,
    },
}

/// Active-tasks predicate shared across most filters. Mirrors the Android
/// `activeAndVisible` index intent: incomplete, not deleted, and past the
/// hide-until threshold supplied by the caller (typically "now").
fn active_clause() -> &'static str {
    "tasks.completed = 0 AND tasks.deleted = 0 AND tasks.hideUntil <= ?1"
}

fn select_columns() -> &'static str {
    // Column list matches `Task::from_row`. Kept explicit (not `*`) so a
    // schema change that adds/removes columns surfaces as a compile-time
    // failure when `Task::from_row` is updated.
    "tasks._id, tasks.title, tasks.importance, tasks.dueDate, tasks.hideUntil, \
     tasks.created, tasks.modified, tasks.completed, tasks.deleted, tasks.notes, \
     tasks.estimatedSeconds, tasks.elapsedSeconds, tasks.timerStart, \
     tasks.notificationFlags, tasks.lastNotified, tasks.recurrence, \
     tasks.repeat_from, tasks.calendarUri, tasks.remoteId, tasks.collapsed, \
     tasks.parent, tasks.\"order\", tasks.read_only"
}

/// Built-in sidebar filter identifier for "all active tasks".
pub const FILTER_ALL: &str = "__all__";
/// Built-in sidebar filter identifier for "due today".
pub const FILTER_TODAY: &str = "__today__";
/// Built-in sidebar filter identifier for "recently modified".
pub const FILTER_RECENT: &str = "__recent__";

/// UI-level filter dispatcher. Accepts a filter identifier from the
/// sidebar (`__all__`, `__today__`, `__recent__`, `caldav:<uuid>`, or
/// `filter:<id>`) and returns the matching Task rows.
///
/// `local_offset_secs` is the caller's current UTC offset (positive east
/// of UTC) and is only consulted by `FILTER_TODAY` so that the day
/// window is anchored to local midnight, not UTC midnight.
///
/// `prefs` is the user's stored `QueryPreferences`. The UI populates it
/// from QSettings; the CLI / smoke path passes
/// `&QueryPreferences::default()`.
pub fn run_by_filter_id(
    db: &Database,
    id: &str,
    now_ms: i64,
    local_offset_secs: i32,
    prefs: &QueryPreferences,
) -> Result<Vec<Task>> {
    match id {
        FILTER_ALL => run(db, TaskFilter::Active, now_ms),
        FILTER_TODAY => {
            let (start, end) = today_window_ms(now_ms, local_offset_secs);
            run(
                db,
                TaskFilter::Today {
                    day_start_utc_ms: start,
                    day_end_utc_ms: end,
                },
                now_ms,
            )
        }
        FILTER_RECENT => {
            let filter = QueryFilter::recently_modified(
                "WHERE tasks.deleted = 0 AND tasks.modified > 0 ORDER BY tasks.modified DESC",
            );
            let sql = build_query(&filter, prefs, now_ms, Some(500));
            run_sql(db, &sql)
        }
        id if id.starts_with("caldav:") => {
            let uuid = &id[7..];
            let filter = QueryFilter::caldav(uuid);
            let sql = build_query(&filter, prefs, now_ms, Some(1000));
            run_sql(db, &sql)
        }
        id if id.starts_with("filter:") => {
            let row_id: i64 = match id[7..].parse() {
                Ok(n) => n,
                Err(_) => {
                    tracing::warn!("invalid filter id `{id}` (expected filter:<i64>)");
                    return Ok(Vec::new());
                }
            };
            let conn = db.connection();
            let mut stmt = conn.prepare("SELECT sql FROM filters WHERE _id = ?1")?;
            let sql_text: Option<String> = match stmt.query_row(params![row_id], |r| r.get(0)) {
                Ok(v) => Some(v),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => return Err(e.into()),
            };
            let Some(sql_text) = sql_text else {
                return Ok(Vec::new());
            };
            let filter = QueryFilter::custom(sql_text);
            let sql = build_query(&filter, prefs, now_ms, Some(1000));
            run_sql(db, &sql)
        }
        _ => Ok(Vec::new()),
    }
}

fn run_sql(db: &Database, sql: &str) -> Result<Vec<Task>> {
    let mut stmt = db.connection().prepare(sql)?;
    let rows = stmt.query_map([], Task::from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Compute `(start_utc_ms, end_utc_ms)` anchored at local midnight on the
/// caller's current day, expressed in UTC milliseconds. `local_offset_secs`
/// is the POSIX-style "east of UTC is positive" offset that Qt's
/// `QDateTime::offsetFromUtc()` and libc's `tm_gmtoff` both return.
///
/// For a caller in UTC-8, `local_offset_secs = -8 * 3600`; local midnight
/// on their day is 08:00:00 UTC, so the window returned spans
/// 08:00:00-today UTC through 07:59:59.999-tomorrow UTC. Contrast with
/// the previous implementation which returned a UTC-midnight window —
/// an up-to-23-hour drift from the user's "today".
fn today_window_ms(now_ms: i64, local_offset_secs: i32) -> (i64, i64) {
    const DAY: i64 = 86_400_000;
    let offset_ms = local_offset_secs as i64 * 1000;
    let local_now_ms = now_ms + offset_ms;
    let local_day_start_ms = (local_now_ms.div_euclid(DAY)) * DAY;
    let utc_day_start_ms = local_day_start_ms - offset_ms;
    (utc_day_start_ms, utc_day_start_ms + DAY - 1)
}

pub fn run(db: &Database, filter: TaskFilter, now_ms: i64) -> Result<Vec<Task>> {
    let conn = db.connection();
    match filter {
        TaskFilter::Active => {
            let sql = format!(
                "SELECT {cols} FROM tasks WHERE {active} \
                 ORDER BY CASE WHEN tasks.dueDate = 0 THEN 1 ELSE 0 END, \
                          tasks.dueDate, tasks.importance, tasks.created",
                cols = select_columns(),
                active = active_clause(),
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![now_ms], Task::from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        }
        TaskFilter::Today {
            day_start_utc_ms,
            day_end_utc_ms,
        } => {
            let sql = format!(
                "SELECT {cols} FROM tasks \
                 WHERE {active} AND tasks.dueDate BETWEEN ?2 AND ?3 \
                 ORDER BY tasks.dueDate, tasks.importance, tasks.created",
                cols = select_columns(),
                active = active_clause(),
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(
                params![now_ms, day_start_utc_ms, day_end_utc_ms],
                Task::from_row,
            )?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        }
    }
}

#[cfg(test)]
mod today_window_tests {
    use super::today_window_ms;

    #[test]
    fn utc_offset_returns_utc_midnight() {
        // 2023-11-14 22:13:20 UTC.
        let now = 1_700_000_000_000;
        let (start, end) = today_window_ms(now, 0);
        assert_eq!(start, 1_699_920_000_000); // 2023-11-14 00:00:00 UTC
        assert_eq!(end, start + 86_400_000 - 1);
    }

    #[test]
    fn negative_offset_anchors_to_local_midnight() {
        // 2023-11-14 22:13:20 UTC = 2023-11-14 14:13:20 PST (UTC-8).
        // Local midnight is 2023-11-14 00:00:00 PST = 2023-11-14 08:00:00 UTC.
        let now = 1_700_000_000_000;
        let (start, _end) = today_window_ms(now, -8 * 3600);
        assert_eq!(start, 1_699_948_800_000);
    }

    #[test]
    fn positive_offset_rolls_day_forward() {
        // 2023-11-14 22:13:20 UTC = 2023-11-15 09:13:20 (UTC+11).
        // Local midnight is 2023-11-15 00:00:00 (+11) = 2023-11-14 13:00:00 UTC.
        let now = 1_700_000_000_000;
        let (start, _end) = today_window_ms(now, 11 * 3600);
        assert_eq!(start, 1_699_966_800_000);
    }
}
