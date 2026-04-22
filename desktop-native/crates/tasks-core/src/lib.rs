//! Read-only core for the Tasks.org native desktop client.
//!
//! Milestone 1 scope: open the SQLite database that the Android app writes,
//! verify the Room schema identity hash, and run a small set of task list
//! queries. No sync, no writes — see `desktop-native/README.md` for the
//! broader roadmap.

pub mod datetime;
pub mod db;
pub mod error;
pub mod import;
pub mod models;
pub mod query;
pub mod recurrence;
pub mod watch;
pub mod write;

pub use query::{
    build_non_recursive_query, build_query, build_recursive_query, run_by_filter_id, QueryFilter,
    QueryPreferences, FILTER_ALL, FILTER_RECENT, FILTER_TODAY,
};

pub use datetime::{days_to_ymd, format_due_label, parse_due_input, ymd_to_days};
pub use db::{default_db_path, Database};
pub use error::{CoreError, Result};
pub use import::{import_json_backup, ImportStats};
pub use models::{
    AccountType, Alarm, AlarmType, CaldavAccount, CaldavCalendar, CaldavTask, CalendarAccess,
    Filter, Geofence, Place, Priority, RepeatFrom, Tag, TagData, Task,
};
pub use recurrence::humanize_rrule;
pub use write::{
    set_task_completion, set_task_deleted, update_task_fields, GeofenceEdit, TaskEdit,
};
