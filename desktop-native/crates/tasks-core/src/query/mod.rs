//! SQL builders for task list views.
//!
//! The Android client composes list queries from many small fragments in
//! `kmp/src/commonMain/kotlin/org/tasks/data/TaskListQuery*.kt`, wrapped
//! around a Kotlin `Query` DSL. Porting that DSL is tracked as a later
//! milestone — for the read-only companion we only need enough to render
//! the most common filters, so we emit the SQL directly here and extend as
//! more filters come online.

use rusqlite::params;

use crate::db::Database;
use crate::error::Result;
use crate::models::Task;

/// Selectors for task list views. Mirrors a subset of the built-in filters
/// from `app/src/main/java/org/tasks/filters/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    /// All active (not completed, not deleted, past hide-until) tasks.
    Active,
    /// Tasks due today (local day in the caller-supplied timezone offset).
    Today { day_start_utc_ms: i64, day_end_utc_ms: i64 },
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
