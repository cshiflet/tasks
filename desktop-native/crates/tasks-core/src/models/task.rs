use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Mirrors `org.tasks.data.entity.Task`.
///
/// Column names follow the Room `@ColumnInfo(name = ...)` attribute, not the
/// Kotlin field name. The `_id`/`title`/etc. naming is load-bearing — it must
/// match the physical SQLite schema, not the Kotlin property names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub title: Option<String>,
    pub priority: i32,
    pub due_date: i64,
    pub hide_until: i64,
    pub creation_date: i64,
    pub modification_date: i64,
    pub completion_date: i64,
    pub deletion_date: i64,
    pub notes: Option<String>,
    pub estimated_seconds: i32,
    pub elapsed_seconds: i32,
    pub timer_start: i64,
    pub ring_flags: i32,
    pub reminder_last: i64,
    pub recurrence: Option<String>,
    pub repeat_from: i32,
    pub calendar_uri: Option<String>,
    pub remote_id: Option<String>,
    pub is_collapsed: bool,
    pub parent: i64,
    pub order: Option<i64>,
    pub read_only: bool,
}

impl Task {
    pub fn is_completed(&self) -> bool {
        self.completion_date > 0
    }

    pub fn is_deleted(&self) -> bool {
        self.deletion_date > 0
    }

    pub fn has_due_date(&self) -> bool {
        self.due_date > 0
    }

    /// Mirrors `Task.hasDueTime(dueDate)` — if the date has a sub-minute
    /// component, it's a date+time; otherwise date-only.
    pub fn has_due_time(&self) -> bool {
        self.due_date > 0 && self.due_date % 60_000 > 0
    }

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Task {
            id: row.get("_id")?,
            title: row.get("title")?,
            priority: row.get("importance")?,
            due_date: row.get("dueDate")?,
            hide_until: row.get("hideUntil")?,
            creation_date: row.get("created")?,
            modification_date: row.get("modified")?,
            completion_date: row.get("completed")?,
            deletion_date: row.get("deleted")?,
            notes: row.get("notes")?,
            estimated_seconds: row.get("estimatedSeconds")?,
            elapsed_seconds: row.get("elapsedSeconds")?,
            timer_start: row.get("timerStart")?,
            ring_flags: row.get("notificationFlags")?,
            reminder_last: row.get("lastNotified")?,
            recurrence: row.get("recurrence")?,
            repeat_from: row.get("repeat_from")?,
            calendar_uri: row.get("calendarUri")?,
            remote_id: row.get("remoteId")?,
            is_collapsed: row.get::<_, i32>("collapsed")? != 0,
            parent: row.get("parent")?,
            order: row.get("order")?,
            read_only: row.get::<_, i32>("read_only")? != 0,
        })
    }
}

pub struct Priority;

impl Priority {
    pub const HIGH: i32 = 0;
    pub const MEDIUM: i32 = 1;
    pub const LOW: i32 = 2;
    pub const NONE: i32 = 3;
}

pub struct RepeatFrom;

impl RepeatFrom {
    pub const DUE_DATE: i32 = 0;
    pub const COMPLETION_DATE: i32 = 1;
}
