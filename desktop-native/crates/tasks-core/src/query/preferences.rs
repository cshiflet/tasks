//! Port of `QueryPreferences` — the flags the Android settings screen feeds
//! into the task-list query builder.
//!
//! Upstream this is an interface whose implementation reads SharedPreferences
//! on Android. The desktop client maps the same knobs onto a plain struct;
//! the UI layer populates it from `QSettings` (or equivalent) and passes it
//! verbatim into the query builders.

use crate::query::sort;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPreferences {
    pub sort_mode: i32,
    pub group_mode: i32,
    pub subtask_mode: i32,
    pub completed_mode: i32,
    pub is_manual_sort: bool,
    pub is_astrid_sort: bool,
    pub sort_ascending: bool,
    pub group_ascending: bool,
    pub subtask_ascending: bool,
    pub completed_ascending: bool,
    pub completed_tasks_at_bottom: bool,
    pub show_completed: bool,
    pub show_hidden: bool,
}

impl Default for QueryPreferences {
    /// Mirrors the Android defaults: sort by auto (due date + importance),
    /// no grouping, completed/hidden tasks hidden, manual sort off.
    fn default() -> Self {
        QueryPreferences {
            sort_mode: sort::SORT_AUTO,
            group_mode: sort::GROUP_NONE,
            subtask_mode: sort::SORT_AUTO,
            completed_mode: sort::SORT_COMPLETED,
            is_manual_sort: false,
            is_astrid_sort: false,
            sort_ascending: true,
            group_ascending: true,
            subtask_ascending: true,
            completed_ascending: false,
            completed_tasks_at_bottom: false,
            show_completed: false,
            show_hidden: false,
        }
    }
}
