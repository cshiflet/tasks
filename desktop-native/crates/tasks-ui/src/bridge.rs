//! cxx-qt bridge: `TaskListViewModel`.
//!
//! Exposes the read-only task list, the currently-selected task's detail
//! fields, and a sidebar of filters/CalDAV calendars to QML. The view
//! model opens the SQLite file the Android client writes (schema-hash
//! pinned), composes the recursive query via `tasks-core::query`, and
//! materialises the rows into parallel Q_PROPERTYs the QML layer indexes
//! by row position.
//!
//! Design note (see desktop-native/DECISIONS.md): we deliberately avoid
//! the QVariantMap/QVariantList round trip because cxx-qt-lib 0.7 does
//! not ship a `QVariant::from(&QVariantMap)` conversion. Parallel
//! QStringList / QList<i64> properties cover every field a ListView
//! delegate needs and compile cleanly against the stock cxx-qt types.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qlist.h");
        #[cxx_name = "QList_i64"]
        type QList_i64 = cxx_qt_lib::QList<i64>;
        #[cxx_name = "QList_i32"]
        type QList_i32 = cxx_qt_lib::QList<i32>;
        #[cxx_name = "QList_bool"]
        type QList_bool = cxx_qt_lib::QList<bool>;
    }

    // `auto_cxx_name` tells cxx-qt to snake_case → camelCase every
    // C++-side name it generates from this block (Q_PROPERTY names,
    // setters, change signals, Q_INVOKABLE methods). Without it,
    // cxx-qt 0.7 emits the raw Rust identifier, so QML bindings
    // written as `viewModel.dbPathDisplay` / `viewModel.sidebarLabels`
    // would silently resolve to `undefined` and the UI would render
    // with no data even though the bridge was otherwise healthy.
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        // List-pane data (parallel arrays indexed by row).
        #[qproperty(i32, count)]
        #[qproperty(QStringList, titles)]
        #[qproperty(QList_i64, task_ids)]
        #[qproperty(QList_i32, indents)]
        #[qproperty(QList_bool, completed_flags)]
        #[qproperty(QStringList, due_labels)]
        #[qproperty(QList_i32, priorities)]
        // Detail-pane data for the currently-selected task.
        #[qproperty(i64, selected_id)]
        #[qproperty(QString, selected_title)]
        #[qproperty(QString, selected_notes)]
        #[qproperty(QString, selected_due_label)]
        // Hide-until as the same round-trippable "YYYY-MM-DD [HH:MM]"
        // string the due field uses. Seeds the edit dialog; empty
        // when `tasks.hideUntil == 0`.
        #[qproperty(QString, selected_hide_until_label)]
        #[qproperty(i32, selected_priority)]
        #[qproperty(bool, selected_completed)]
        // Humanised RRULE ("Every week on Mon, Wed, Fri"). The raw
        // form lives in `tasks.recurrence`; this property is the
        // output of `tasks_core::recurrence::humanize_rrule` with
        // the from-completion suffix pre-applied, so the detail
        // pane can render it verbatim.
        #[qproperty(QString, selected_recurrence)]
        // CalDAV calendars available in the database, for the edit
        // dialog's "list" picker. Built from the same query the
        // sidebar uses; both lists stay in sync because they're
        // populated in `open_at_path`.
        #[qproperty(QStringList, caldav_calendar_labels)]
        #[qproperty(QStringList, caldav_calendar_uuids)]
        // UUID of the selected task's current CalDAV list, or empty
        // when the task has no caldav_tasks row (local-only).
        #[qproperty(QString, selected_caldav_calendar_uuid)]
        // All tag definitions from `tagdata`, parallel arrays for
        // the edit dialog's multi-select picker. `tag_uids` drives
        // the semantic (writes back via the update invokable);
        // `tag_labels` is the human-readable name.
        #[qproperty(QStringList, tag_labels)]
        #[qproperty(QStringList, tag_uids)]
        // Tag UIDs currently attached to the selected task. Used
        // to pre-check the dialog's checkboxes on open.
        #[qproperty(QStringList, selected_tag_uids)]
        // Alarms attached to the selected task. Parallel arrays —
        // labels is the humanised description for display, times
        // and types are the raw columns the edit dialog passes
        // back into `updateSelectedTask`.
        #[qproperty(QStringList, selected_alarm_labels)]
        #[qproperty(QList_i64, selected_alarm_times)]
        #[qproperty(QList_i32, selected_alarm_types)]
        // Places known to the DB, parallel arrays for the edit
        // dialog's location picker.
        #[qproperty(QStringList, place_labels)]
        #[qproperty(QStringList, place_uids)]
        // Geofence currently attached to the selected task; empty
        // `selected_place_uid` means no geofence.
        #[qproperty(QString, selected_place_uid)]
        #[qproperty(bool, selected_place_arrival)]
        #[qproperty(bool, selected_place_departure)]
        // Candidate parents for the subtask picker. Labels are
        // task titles, IDs are tasks._id. Populated on open and
        // refreshed on every select_task so newly-added tasks
        // show up in the dropdown.
        #[qproperty(QStringList, parent_candidate_labels)]
        #[qproperty(QList_i64, parent_candidate_ids)]
        // Current parent task id for the selected row (0 = top-level).
        #[qproperty(i64, selected_parent_id)]
        // Timer fields rendered as H:MM strings for the edit
        // dialog. Empty string means zero.
        #[qproperty(QString, selected_estimated_text)]
        #[qproperty(QString, selected_elapsed_text)]
        // Raw `tasks.recurrence` (RRULE) + `tasks.repeat_from` for
        // the inline recurrence editor. Humanised summary is in
        // `selected_recurrence` for the detail pane's display;
        // these are the edit-dialog round-trip values.
        #[qproperty(QString, selected_recurrence_raw)]
        #[qproperty(i32, selected_repeat_from)]
        // Live values of the query preferences. Seeded from the
        // view model's `preferences` field; the preferences
        // dialog reads these to pre-fill its controls and calls
        // `updatePreferences` on save.
        #[qproperty(i32, pref_sort_mode)]
        #[qproperty(bool, pref_sort_ascending)]
        #[qproperty(bool, pref_show_completed)]
        #[qproperty(bool, pref_show_hidden)]
        #[qproperty(bool, pref_completed_at_bottom)]
        // Sidebar: parallel label / identifier arrays. Identifier format:
        //   "__all__" | "__today__" | "__recent__"  (built-in filters)
        //   "caldav:<uuid>"                          (CalDAV calendar)
        //   "filter:<id>"                            (custom saved filter)
        #[qproperty(QStringList, sidebar_labels)]
        #[qproperty(QStringList, sidebar_ids)]
        #[qproperty(QString, active_filter_id)]
        // Status bar text.
        #[qproperty(QString, status)]
        // Absolute path of the currently-open database, surfaced in
        // the window title + Browse path field so users know which
        // file they're looking at.
        #[qproperty(QString, db_path_display)]
        type TaskListViewModel = super::TaskListViewModelRust;

        #[qinvokable]
        fn open_database(self: Pin<&mut TaskListViewModel>, path: QString);

        #[qinvokable]
        fn open_default_database(self: Pin<&mut TaskListViewModel>);

        #[qinvokable]
        fn import_json_backup(self: Pin<&mut TaskListViewModel>, path: QString);

        #[qinvokable]
        fn select_filter(self: Pin<&mut TaskListViewModel>, id: QString);

        #[qinvokable]
        fn select_task(self: Pin<&mut TaskListViewModel>, id: i64);

        /// Create a brand-new task with the given title. If the
        /// active filter is a CalDAV list, the new task is
        /// assigned to that list automatically.
        #[qinvokable]
        fn add_new_task(self: Pin<&mut TaskListViewModel>, title: QString);

        /// Apply new query preferences and reload the active
        /// filter. Session-local only for now; QSettings
        /// persistence is a follow-up.
        #[qinvokable]
        fn update_preferences(
            self: Pin<&mut TaskListViewModel>,
            sort_mode: i32,
            sort_ascending: bool,
            show_completed: bool,
            show_hidden: bool,
            completed_at_bottom: bool,
        );

        #[qinvokable]
        fn toggle_task_completion(self: Pin<&mut TaskListViewModel>, id: i64, completed: bool);

        #[qinvokable]
        fn delete_selected_task(self: Pin<&mut TaskListViewModel>);

        /// Apply the edit-dialog's form state to the currently-
        /// selected task. `due_text` / `hide_until_text` are parsed
        /// via `tasks_core::datetime::parse_due_input`; an empty
        /// string means "no date". `caldav_uuid` reassigns the
        /// task's CalDAV calendar when non-empty (no-op for local
        /// tasks that don't have a caldav_tasks row). A parse
        /// failure surfaces on the status line and leaves the DB
        /// untouched.
        #[qinvokable]
        fn update_selected_task(
            self: Pin<&mut TaskListViewModel>,
            title: QString,
            notes: QString,
            due_text: QString,
            hide_until_text: QString,
            priority: i32,
            caldav_uuid: QString,
            tag_uids_list: QStringList,
            alarm_times: QList_i64,
            alarm_types: QList_i32,
            place_uid: QString,
            place_arrival: bool,
            place_departure: bool,
            parent_id: i64,
            estimate_text: QString,
            elapsed_text: QString,
            recurrence: QString,
            repeat_from: i32,
        );
    }

    // Opt the view model into cxx-qt's Threading surface so the
    // filesystem-watcher thread can queue reloads back on the Qt
    // event loop thread.
    impl cxx_qt::Threading for TaskListViewModel {}
}

use core::pin::Pin;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QDateTime, QList, QString, QStringList};

use tasks_core::datetime::{
    describe_alarm, format_due_label, format_duration_hhmm, parse_due_input, parse_duration_input,
};
use tasks_core::db::{default_db_path, Database};
use tasks_core::models::{CaldavCalendar, Filter as CustomFilter, Priority, RepeatFrom, Task};
use tasks_core::query::{
    run_by_filter_id, QueryPreferences, FILTER_ALL, FILTER_RECENT, FILTER_TODAY,
};
use tasks_core::recurrence::humanize_rrule;
use tasks_core::watch::DatabaseWatcher;

pub struct TaskListViewModelRust {
    // List state.
    count: i32,
    titles: QStringList,
    task_ids: QList<i64>,
    indents: QList<i32>,
    completed_flags: QList<bool>,
    due_labels: QStringList,
    priorities: QList<i32>,
    // Detail state.
    selected_id: i64,
    selected_title: QString,
    selected_notes: QString,
    selected_due_label: QString,
    selected_hide_until_label: QString,
    selected_priority: i32,
    selected_completed: bool,
    selected_recurrence: QString,
    caldav_calendar_labels: QStringList,
    caldav_calendar_uuids: QStringList,
    selected_caldav_calendar_uuid: QString,
    tag_labels: QStringList,
    tag_uids: QStringList,
    selected_tag_uids: QStringList,
    selected_alarm_labels: QStringList,
    selected_alarm_times: QList<i64>,
    selected_alarm_types: QList<i32>,
    place_labels: QStringList,
    place_uids: QStringList,
    selected_place_uid: QString,
    selected_place_arrival: bool,
    selected_place_departure: bool,
    parent_candidate_labels: QStringList,
    parent_candidate_ids: QList<i64>,
    selected_parent_id: i64,
    selected_estimated_text: QString,
    selected_elapsed_text: QString,
    selected_recurrence_raw: QString,
    selected_repeat_from: i32,
    pref_sort_mode: i32,
    pref_sort_ascending: bool,
    pref_show_completed: bool,
    pref_show_hidden: bool,
    pref_completed_at_bottom: bool,
    // Sidebar state.
    sidebar_labels: QStringList,
    sidebar_ids: QStringList,
    active_filter_id: QString,
    // Status.
    status: QString,
    db_path_display: QString,
    // Non-Qt bookkeeping. Held on the Rust side only; not exposed to QML.
    db_path: Option<PathBuf>,
    db: Option<Database>,
    task_cache: Vec<Task>,
    /// User preferences fed to `run_by_filter_id`. The UI panel for
    /// editing these isn't wired yet (Milestone 1 scope); for now it
    /// stays at the Android defaults.
    preferences: QueryPreferences,
    /// Flag the filesystem-watcher thread checks periodically to exit
    /// when a new DB is opened (or the view model is dropped). Shared
    /// with the spawned thread via `Arc`. `None` when no watcher is
    /// currently active.
    watcher_stop: Option<Arc<AtomicBool>>,
}

impl Default for TaskListViewModelRust {
    fn default() -> Self {
        TaskListViewModelRust {
            count: 0,
            titles: QStringList::default(),
            task_ids: QList::default(),
            indents: QList::default(),
            completed_flags: QList::default(),
            due_labels: QStringList::default(),
            priorities: QList::default(),
            selected_id: 0,
            selected_title: QString::default(),
            selected_notes: QString::default(),
            selected_due_label: QString::default(),
            selected_hide_until_label: QString::default(),
            selected_priority: Priority::NONE,
            selected_completed: false,
            selected_recurrence: QString::default(),
            caldav_calendar_labels: QStringList::default(),
            caldav_calendar_uuids: QStringList::default(),
            selected_caldav_calendar_uuid: QString::default(),
            tag_labels: QStringList::default(),
            tag_uids: QStringList::default(),
            selected_tag_uids: QStringList::default(),
            selected_alarm_labels: QStringList::default(),
            selected_alarm_times: QList::default(),
            selected_alarm_types: QList::default(),
            place_labels: QStringList::default(),
            place_uids: QStringList::default(),
            selected_place_uid: QString::default(),
            selected_place_arrival: false,
            selected_place_departure: false,
            parent_candidate_labels: QStringList::default(),
            parent_candidate_ids: QList::default(),
            selected_parent_id: 0,
            selected_estimated_text: QString::default(),
            selected_elapsed_text: QString::default(),
            selected_recurrence_raw: QString::default(),
            selected_repeat_from: 0,
            // Seed from Android defaults — sort_auto, ascending,
            // completed+hidden hidden. The Default impl of
            // QueryPreferences carries the same values; we keep
            // them in sync here so the Q_PROPERTYs read correctly
            // on first open before the preferences dialog is ever
            // invoked.
            pref_sort_mode: 0, // SORT_AUTO
            pref_sort_ascending: true,
            pref_show_completed: false,
            pref_show_hidden: false,
            pref_completed_at_bottom: false,
            sidebar_labels: QStringList::default(),
            sidebar_ids: QStringList::default(),
            active_filter_id: QString::from(FILTER_ALL),
            status: QString::default(),
            db_path_display: QString::default(),
            db_path: None,
            db: None,
            task_cache: Vec::new(),
            preferences: QueryPreferences::default(),
            watcher_stop: None,
        }
    }
}

impl Drop for TaskListViewModelRust {
    fn drop(&mut self) {
        // Ensure the watcher thread exits when the view model does.
        if let Some(stop) = self.watcher_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl qobject::TaskListViewModel {
    /// Open a user-specified database file read-only. Used by the
    /// Browse… button in the QML toolbar.
    pub fn open_database(self: Pin<&mut Self>, path: QString) {
        let path_buf = PathBuf::from(path.to_string());
        open_at_path(self, path_buf, OpenMode::ReadOnlyOnly);
    }

    /// Open the desktop client's managed database at the default
    /// per-OS path, creating an empty schema if the file doesn't
    /// exist yet. Called from `Main.qml`'s `Component.onCompleted`
    /// so users don't have to pick a file on first launch.
    pub fn open_default_database(self: Pin<&mut Self>) {
        match default_db_path() {
            Some(path) => open_at_path(self, path, OpenMode::CreateIfMissing),
            None => {
                // No resolvable data directory — very rare; surface
                // to the status bar and let the user pick via Browse.
                self.set_status(QString::from(
                    "Couldn't resolve a default data directory; use Browse\u{2026}",
                ));
            }
        }
    }

    /// Import a Tasks.org JSON backup (the file produced by the
    /// Android app's Settings → Backups → Export JSON flow) into
    /// the currently-open database. Tears down the watcher while
    /// the write happens, then reopens the DB read-only and reloads
    /// the active filter so the new rows show up immediately.
    pub fn import_json_backup(mut self: Pin<&mut Self>, path: QString) {
        let source = PathBuf::from(path.to_string());
        // Import targets the currently-open DB. If none is open yet
        // (first launch, pre-openDefault), surface a clear error
        // rather than silently targeting the default.
        let target = match self.db_path.clone() {
            Some(p) => p,
            None => {
                self.as_mut().set_status(QString::from(
                    "Open or create a database first, then import.",
                ));
                return;
            }
        };

        // Close the read-only handle + stop the watcher so the
        // importer's writable open has exclusive access. The target
        // is our own file; no other process touches it.
        stop_prior_watcher(self.as_mut());
        self.as_mut().rust_mut().db = None;

        let outcome = tasks_core::import::import_json_backup(&target, &source);
        match outcome {
            Ok(stats) => {
                let msg = format!(
                    "Imported {} tasks, {} places, {} tags, {} filters from {}",
                    stats.tasks,
                    stats.places,
                    stats.tag_data,
                    stats.filters,
                    source.display()
                );
                tracing::info!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                // Reopen the DB read-only and refresh the views.
                open_at_path(self.as_mut(), target, OpenMode::ReadOnlyOnly);
            }
            Err(e) => {
                let msg = format!("Import failed: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                // Reopen the target so the UI isn't stuck in a closed
                // state.
                open_at_path(self.as_mut(), target, OpenMode::ReadOnlyOnly);
            }
        }
    }

    pub fn select_filter(mut self: Pin<&mut Self>, id: QString) {
        self.as_mut().set_active_filter_id(id);
        self.as_mut().reload_active_filter();
    }

    /// Apply new query preferences and reload. Session-local.
    pub fn update_preferences(
        mut self: Pin<&mut Self>,
        sort_mode: i32,
        sort_ascending: bool,
        show_completed: bool,
        show_hidden: bool,
        completed_at_bottom: bool,
    ) {
        {
            let mut inner = self.as_mut().rust_mut();
            inner.preferences.sort_mode = sort_mode;
            inner.preferences.sort_ascending = sort_ascending;
            inner.preferences.show_completed = show_completed;
            inner.preferences.show_hidden = show_hidden;
            inner.preferences.completed_tasks_at_bottom = completed_at_bottom;
        }
        self.as_mut().set_pref_sort_mode(sort_mode);
        self.as_mut().set_pref_sort_ascending(sort_ascending);
        self.as_mut().set_pref_show_completed(show_completed);
        self.as_mut().set_pref_show_hidden(show_hidden);
        self.as_mut()
            .set_pref_completed_at_bottom(completed_at_bottom);
        self.as_mut().reload_active_filter();
    }

    /// Create a new task in the open DB with `title`. If the user
    /// is currently viewing a CalDAV-scoped filter
    /// (`caldav:<uuid>`), the new task is stamped into that list;
    /// otherwise it lands as a local task. After creation we
    /// reload the active filter and select the new row so the
    /// user sees it land and can immediately flesh it out via
    /// Edit….
    pub fn add_new_task(mut self: Pin<&mut Self>, title: QString) {
        let title_str = title.to_string();
        let title_trim = title_str.trim();
        if title_trim.is_empty() {
            self.as_mut()
                .set_status(QString::from("Empty task title — nothing created."));
            return;
        }
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't create."));
            return;
        };
        let active = self.active_filter_id.to_string();
        let caldav_uuid = active.strip_prefix("caldav:");
        match tasks_core::create_task(&path, title_trim, now_ms(), caldav_uuid) {
            Ok(new_id) => {
                self.as_mut().reload_active_filter();
                self.as_mut().select_task(new_id);
                self.as_mut()
                    .set_status(QString::from(&format!("Created \"{title_trim}\"")));
            }
            Err(e) => {
                let msg = format!("Couldn't create task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Mark task `id` as completed (or restore it to active when
    /// `completed = false`). Delegates to `tasks_core::write` and
    /// reloads the active filter so the UI reflects the change
    /// immediately — the filesystem watcher would pick it up on its
    /// next tick anyway, but clicking a checkbox should feel
    /// instantaneous.
    pub fn toggle_task_completion(mut self: Pin<&mut Self>, id: i64, completed: bool) {
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't mark task."));
            return;
        };
        match tasks_core::set_task_completion(&path, id, completed, now_ms()) {
            Ok(true) => {
                self.as_mut().reload_active_filter();
                // reload_active_filter rewrites the status line with a
                // task count; the write feedback is implicit in that.
                // If the selected task was the one we toggled, refresh
                // its detail pane to match.
                if self.selected_id == id {
                    self.as_mut().set_selected_completed(completed);
                }
            }
            Ok(false) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Task {id} not found")));
            }
            Err(e) => {
                let msg = format!("Couldn't update task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Soft-delete the task currently shown in the detail pane. No-op
    /// when nothing is selected. After a successful delete the detail
    /// pane clears itself, matching the Android "swipe to delete"
    /// behaviour minus the animation.
    pub fn delete_selected_task(mut self: Pin<&mut Self>) {
        let id = self.selected_id;
        if id <= 0 {
            return;
        }
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't delete."));
            return;
        };
        match tasks_core::set_task_deleted(&path, id, now_ms()) {
            Ok(true) => {
                clear_detail_pane(self.as_mut());
                self.as_mut().reload_active_filter();
            }
            Ok(false) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Task {id} not found")));
            }
            Err(e) => {
                let msg = format!("Couldn't delete task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    pub fn select_task(mut self: Pin<&mut Self>, id: i64) {
        let Some(task) = self.task_cache.iter().find(|t| t.id == id).cloned() else {
            clear_detail_pane(self.as_mut());
            return;
        };

        self.as_mut().set_selected_id(task.id);
        self.as_mut()
            .set_selected_title(QString::from(task.title.as_deref().unwrap_or("")));
        self.as_mut()
            .set_selected_notes(QString::from(task.notes.as_deref().unwrap_or("")));
        self.as_mut()
            .set_selected_due_label(QString::from(&format_due_label(task.due_date)));
        self.as_mut()
            .set_selected_hide_until_label(QString::from(&format_due_label(task.hide_until)));
        self.as_mut().set_selected_priority(task.priority);
        self.as_mut().set_selected_completed(task.is_completed());
        // Humanise the RRULE (FREQ/INTERVAL/BYDAY/UNTIL/COUNT) and
        // mark repeat-from-completion so the user sees the semantic
        // difference from repeat-from-due-date without needing to
        // decode RRULE text.
        let humanized = humanize_rrule(
            task.recurrence.as_deref().unwrap_or(""),
            task.repeat_from == RepeatFrom::COMPLETION_DATE,
        );
        self.as_mut()
            .set_selected_recurrence(QString::from(&humanized));

        // Current CalDAV list assignment (empty for local tasks).
        let uuid = match &self.db {
            Some(db) => current_caldav_uuid_for(db, task.id),
            None => String::new(),
        };
        self.as_mut()
            .set_selected_caldav_calendar_uuid(QString::from(&uuid));

        // Current tag set.
        let task_tag_uids = match &self.db {
            Some(db) => current_tag_uids_for(db, task.id),
            None => Vec::new(),
        };
        self.as_mut().set_selected_tag_uids(string_list_from_iter(
            task_tag_uids.iter().map(String::as_str),
        ));

        // Current alarms (parallel labels/times/types).
        let (alarm_labels, alarm_times, alarm_types) = match &self.db {
            Some(db) => current_alarms_for(db, task.id),
            None => (Vec::new(), Vec::new(), Vec::new()),
        };
        self.as_mut()
            .set_selected_alarm_labels(string_list_from_iter(
                alarm_labels.iter().map(String::as_str),
            ));
        let mut ql_times: QList<i64> = QList::default();
        for t in &alarm_times {
            ql_times.append(*t);
        }
        self.as_mut().set_selected_alarm_times(ql_times);
        let mut ql_types: QList<i32> = QList::default();
        for t in &alarm_types {
            ql_types.append(*t);
        }
        self.as_mut().set_selected_alarm_types(ql_types);

        // Current geofence.
        let (place_uid, arrival, departure) = match &self.db {
            Some(db) => current_geofence_for(db, task.id),
            None => (String::new(), false, false),
        };
        self.as_mut()
            .set_selected_place_uid(QString::from(&place_uid));
        self.as_mut().set_selected_place_arrival(arrival);
        self.as_mut().set_selected_place_departure(departure);

        // Parent picker candidates + current parent.
        let (parent_labels, parent_ids) = match &self.db {
            Some(db) => list_parent_candidates(db, task.id),
            None => (Vec::new(), Vec::new()),
        };
        self.as_mut()
            .set_parent_candidate_labels(string_list_from_iter(
                parent_labels.iter().map(String::as_str),
            ));
        let mut ql_parent_ids: QList<i64> = QList::default();
        for pid in &parent_ids {
            ql_parent_ids.append(*pid);
        }
        self.as_mut().set_parent_candidate_ids(ql_parent_ids);
        self.as_mut().set_selected_parent_id(task.parent);

        // Timer columns → H:MM text for the edit dialog.
        self.as_mut()
            .set_selected_estimated_text(QString::from(&format_duration_hhmm(
                task.estimated_seconds,
            )));
        self.as_mut()
            .set_selected_elapsed_text(QString::from(&format_duration_hhmm(task.elapsed_seconds)));

        // Recurrence raw + repeat_from for the inline RRULE editor.
        self.as_mut()
            .set_selected_recurrence_raw(QString::from(task.recurrence.as_deref().unwrap_or("")));
        self.as_mut().set_selected_repeat_from(task.repeat_from);
    }

    /// Apply edits from the task edit dialog and refresh the view.
    ///
    /// Parses the two date text fields via
    /// `tasks_core::datetime::parse_due_input`; on error we leave
    /// the DB untouched and surface the failure on the status line
    /// so the user can correct the input without losing work.
    pub fn update_selected_task(
        mut self: Pin<&mut Self>,
        title: QString,
        notes: QString,
        due_text: QString,
        hide_until_text: QString,
        priority: i32,
        caldav_uuid: QString,
        tag_uids_list: QStringList,
        alarm_times: QList<i64>,
        alarm_types: QList<i32>,
        place_uid: QString,
        place_arrival: bool,
        place_departure: bool,
        parent_id: i64,
        estimate_text: QString,
        elapsed_text: QString,
        recurrence: QString,
        repeat_from: i32,
    ) {
        let id = self.selected_id;
        if id <= 0 {
            return;
        }
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't save edits."));
            return;
        };

        let title_str = title.to_string();
        let notes_str = notes.to_string();
        let caldav_str = caldav_uuid.to_string();
        // QStringList → Vec<String>. cxx-qt-lib's QStringList has
        // no direct iterator, but it converts into QList<QString>
        // cheaply, which does.
        let tag_uids_owned: Vec<String> = {
            let list: QList<QString> = QList::from(&tag_uids_list);
            list.iter().map(|s| s.to_string()).collect()
        };

        // Zip parallel time/type QLists into Vec<(time, type)> for
        // the write helper. If the two arrays disagree in length we
        // trim to the shorter, matching the QML side's guarantee
        // that both are built from the same source.
        let alarm_pairs: Vec<(i64, i32)> = alarm_times
            .iter()
            .zip(alarm_types.iter())
            .map(|(t, ty)| (*t, *ty))
            .collect();
        let place_uid_str = place_uid.to_string();

        let estimated = match parse_duration_input(&estimate_text.to_string()) {
            Ok(s) => s,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Estimate: {msg}")));
                return;
            }
        };
        let recurrence_str = recurrence.to_string();
        let elapsed = match parse_duration_input(&elapsed_text.to_string()) {
            Ok(s) => s,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Elapsed: {msg}")));
                return;
            }
        };
        let due_ms = match parse_due_input(&due_text.to_string()) {
            Ok(ms) => ms,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Due: {msg}")));
                return;
            }
        };
        let hide_ms = match parse_due_input(&hide_until_text.to_string()) {
            Ok(ms) => ms,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Hide-until: {msg}")));
                return;
            }
        };

        let edit = tasks_core::TaskEdit {
            title: &title_str,
            notes: &notes_str,
            due_ms,
            hide_until_ms: hide_ms,
            priority,
            // Empty string = "don't touch caldav_tasks" (QML passes
            // the ComboBox's current UUID, which equals the task's
            // existing assignment when unchanged — the helper's
            // UPDATE is idempotent in that case).
            caldav_calendar_uuid: if caldav_str.is_empty() {
                None
            } else {
                Some(caldav_str.as_str())
            },
            tag_uids: Some(&tag_uids_owned),
            alarms: Some(&alarm_pairs),
            geofence: Some(tasks_core::GeofenceEdit {
                place_uid: &place_uid_str,
                arrival: place_arrival,
                departure: place_departure,
            }),
            parent_id: Some(parent_id),
            estimated_seconds: estimated,
            elapsed_seconds: elapsed,
            recurrence: &recurrence_str,
            repeat_from,
        };
        match tasks_core::update_task_fields(&path, id, &edit, now_ms()) {
            Ok(true) => {
                self.as_mut().reload_active_filter();
                // Refresh the detail pane from the cache that
                // `reload_active_filter` just rebuilt, so the user
                // sees their edits reflected without having to
                // re-click the row.
                self.as_mut().select_task(id);
            }
            Ok(false) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Task {id} not found")));
            }
            Err(e) => {
                let msg = format!("Couldn't save task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Re-query the DB using `self.active_filter_id` and publish the
    /// parallel list arrays.
    fn reload_active_filter(mut self: Pin<&mut Self>) {
        if self.db.is_none() {
            self.as_mut().clear_list();
            return;
        }

        let now_ms = now_ms();
        let offset = current_local_offset_secs();
        let active_id = self.active_filter_id.to_string();
        let prefs = self.preferences.clone();
        // Borrow `db` immutably for the duration of the query, then drop
        // the borrow before any `rust_mut()` call below. `db` lives on
        // `self` (no extra open), so repeated filter navigations reuse
        // the same verified-hash handle.
        let query_result = {
            let Some(ref db) = self.db else {
                unreachable!("db presence checked above");
            };
            run_by_filter_id(db, &active_id, now_ms, offset, &prefs)
        };
        let tasks = match query_result {
            Ok(t) => t,
            Err(e) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Query failed: {e}")));
                self.as_mut().clear_list();
                return;
            }
        };

        publish_tasks(self.as_mut(), tasks);
    }

    fn clear_list(mut self: Pin<&mut Self>) {
        self.as_mut().set_count(0);
        self.as_mut().set_titles(QStringList::default());
        self.as_mut().set_task_ids(QList::default());
        self.as_mut().set_indents(QList::default());
        self.as_mut().set_completed_flags(QList::default());
        self.as_mut().set_due_labels(QStringList::default());
        self.as_mut().set_priorities(QList::default());
        self.as_mut().rust_mut().task_cache.clear();
    }
}

fn publish_tasks(mut vm: Pin<&mut qobject::TaskListViewModel>, tasks: Vec<Task>) {
    let mut task_ids: QList<i64> = QList::default();
    let mut indents: QList<i32> = QList::default();
    let mut completed_flags: QList<bool> = QList::default();
    let mut priorities: QList<i32> = QList::default();

    // `append(&QString)` on QStringList isn't exposed as a public helper;
    // build QList<QString>s alongside and convert at the end.
    let mut title_list: QList<QString> = QList::default();
    let mut due_list: QList<QString> = QList::default();

    // Parent id -> indent depth, cached while iterating so subtasks pick up
    // their parent's indent + 1. The recursive query already sorts with
    // parents before children, but when the prepared-statement fallbacks
    // are used we compute indent from `tasks.parent` on the fly.
    let mut indent_by_id: std::collections::HashMap<i64, i32> = Default::default();

    for t in &tasks {
        title_list.append(QString::from(t.title.as_deref().unwrap_or("")));
        task_ids.append(t.id);
        let indent = if t.parent == 0 {
            0
        } else {
            indent_by_id.get(&t.parent).copied().unwrap_or(0) + 1
        };
        indent_by_id.insert(t.id, indent);
        indents.append(indent);
        completed_flags.append(t.is_completed());
        due_list.append(QString::from(&format_due_label(t.due_date)));
        priorities.append(t.priority);
    }
    let titles = QStringList::from(&title_list);
    let due_labels = QStringList::from(&due_list);

    // Two-phase publish to keep QML delegate bindings out of the
    // stale-array race:
    //
    //   1. `set_count(0)` tears down every existing delegate. Each
    //      tear-down reads the old (still consistent) arrays one
    //      last time.
    //   2. Refill the parallel arrays with the new data.
    //   3. `set_count(new_count)` creates fresh delegates which
    //      index into the already-updated arrays.
    //
    // Without this, a filter change from an N-row list to an
    // M-row one (M < N) would leave M+1..N delegates briefly bound
    // to `titles[k>=M]` etc., which resolves to `undefined` and
    // QML emits "Unable to assign [undefined] to QString" warnings.
    // The extra set_count(0) is the price of a clean transition.
    let count = tasks.len() as i32;
    vm.as_mut().set_count(0);
    vm.as_mut().set_titles(titles);
    vm.as_mut().set_task_ids(task_ids);
    vm.as_mut().set_indents(indents);
    vm.as_mut().set_completed_flags(completed_flags);
    vm.as_mut().set_due_labels(due_labels);
    vm.as_mut().set_priorities(priorities);
    vm.as_mut().set_count(count);
    vm.as_mut()
        .set_status(QString::from(&format!("{count} task(s) in view")));
    vm.as_mut().rust_mut().task_cache = tasks;
}

/// Enumerate the sidebar entries we surface: built-in filters, every CalDAV
/// calendar, then every saved custom filter. Order matches the Android
/// nav drawer's default ordering.
///
/// Errors from reading the `caldav_lists` / `filters` tables are logged
/// (not fatal) so a broken/missing table shows as an incomplete sidebar
/// rather than aborting the whole `openDatabase` flow.
/// Whether an open operation is allowed to bootstrap a missing
/// database file. `openDatabase(path)` from the Browse button only
/// opens what already exists; `openDefaultDatabase` creates-on-miss.
enum OpenMode {
    ReadOnlyOnly,
    CreateIfMissing,
}

/// Shared implementation behind `open_database` and
/// `open_default_database`. Tears down the prior watcher, opens (or
/// initialises) the file, rebuilds the sidebar, and kicks off the
/// initial query. Status-line text and error branches stay the same
/// regardless of which entry point called us.
fn open_at_path(mut vm: Pin<&mut qobject::TaskListViewModel>, path: PathBuf, mode: OpenMode) {
    stop_prior_watcher(vm.as_mut());

    let path_display = path.display().to_string();
    let result = match mode {
        OpenMode::ReadOnlyOnly => Database::open_read_only(&path),
        OpenMode::CreateIfMissing => Database::open_or_create_read_only(&path),
    };

    match result {
        Ok(db) => {
            let (labels, ids) = build_sidebar(&db);
            vm.as_mut()
                .set_sidebar_labels(string_list_from_iter(labels.iter().map(String::as_str)));
            vm.as_mut()
                .set_sidebar_ids(string_list_from_iter(ids.iter().map(String::as_str)));
            // Edit dialog's CalDAV list picker uses the calendars
            // directly (no built-in filters prepended, no
            // "caldav:" prefix on the UUID).
            let (cal_labels, cal_uuids) = list_caldav_calendars(&db);
            vm.as_mut()
                .set_caldav_calendar_labels(string_list_from_iter(
                    cal_labels.iter().map(String::as_str),
                ));
            vm.as_mut().set_caldav_calendar_uuids(string_list_from_iter(
                cal_uuids.iter().map(String::as_str),
            ));
            let (tag_names, tag_uid_list) = list_all_tags(&db);
            vm.as_mut()
                .set_tag_labels(string_list_from_iter(tag_names.iter().map(String::as_str)));
            vm.as_mut().set_tag_uids(string_list_from_iter(
                tag_uid_list.iter().map(String::as_str),
            ));
            let (place_names, place_uids) = list_all_places(&db);
            vm.as_mut().set_place_labels(string_list_from_iter(
                place_names.iter().map(String::as_str),
            ));
            vm.as_mut()
                .set_place_uids(string_list_from_iter(place_uids.iter().map(String::as_str)));
            vm.as_mut().set_status(QString::from(&format!(
                "Opened {path_display} ({} sidebar entries)",
                labels.len()
            )));
            vm.as_mut()
                .set_db_path_display(QString::from(&path_display));
            {
                let mut inner = vm.as_mut().rust_mut();
                inner.db_path = Some(path.clone());
                inner.db = Some(db);
            }
            vm.as_mut().reload_active_filter();
            start_watcher(vm.as_mut(), path);
        }
        Err(e) => {
            let msg = format!("Couldn't open {path_display}: {e}");
            tracing::warn!("{msg}");
            vm.as_mut().set_status(QString::from(&msg));
            vm.as_mut().set_db_path_display(QString::default());
            vm.as_mut().clear_list();
            // Also blank every per-task detail field + the edit
            // dialog's catalog arrays (tags/places/caldav lists).
            // Otherwise a failed open after an earlier successful
            // open leaves stale values visible in the UI.
            clear_detail_pane(vm.as_mut());
            vm.as_mut().set_tag_labels(QStringList::default());
            vm.as_mut().set_tag_uids(QStringList::default());
            vm.as_mut().set_place_labels(QStringList::default());
            vm.as_mut().set_place_uids(QStringList::default());
            vm.as_mut()
                .set_caldav_calendar_labels(QStringList::default());
            vm.as_mut()
                .set_caldav_calendar_uuids(QStringList::default());
            vm.as_mut()
                .set_parent_candidate_labels(QStringList::default());
            vm.as_mut().set_parent_candidate_ids(QList::default());
            let mut inner = vm.as_mut().rust_mut();
            inner.db_path = None;
            inner.db = None;
        }
    }
}

/// Return parallel `(labels, uids)` for every tagdata row. Used by
/// the edit dialog's multi-select tag picker.
fn list_all_tags(db: &Database) -> (Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut uids = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT remoteId, name FROM tagdata \
         WHERE remoteId IS NOT NULL AND name IS NOT NULL \
         ORDER BY td_order, name",
    ) else {
        return (labels, uids);
    };
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
    if let Ok(rows) = rows {
        for (uid, name) in rows.flatten() {
            uids.push(uid);
            labels.push(name);
        }
    }
    (labels, uids)
}

/// Return parallel `(labels, ids)` for every non-deleted task,
/// excluding `exclude_id` (the task currently being edited, so the
/// picker never offers the task itself as its own parent). Sorted
/// by title for a predictable dropdown.
fn list_parent_candidates(db: &Database, exclude_id: i64) -> (Vec<String>, Vec<i64>) {
    let mut labels = Vec::new();
    let mut ids = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT _id, title FROM tasks \
         WHERE deleted = 0 AND _id != ?1 \
         ORDER BY COALESCE(UPPER(title), ''), _id",
    ) else {
        return (labels, ids);
    };
    let rows = stmt.query_map([exclude_id], |r| {
        let id: i64 = r.get(0)?;
        let title: Option<String> = r.get(1)?;
        Ok((id, title.unwrap_or_default()))
    });
    if let Ok(rows) = rows {
        for (id, title) in rows.flatten() {
            ids.push(id);
            labels.push(title);
        }
    }
    (labels, ids)
}

/// Return parallel `(labels, uids)` for every row in `places`.
fn list_all_places(db: &Database) -> (Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut uids = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT uid, name FROM places \
         WHERE uid IS NOT NULL AND name IS NOT NULL \
         ORDER BY place_order, name",
    ) else {
        return (labels, uids);
    };
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
    if let Ok(rows) = rows {
        for (uid, name) in rows.flatten() {
            uids.push(uid);
            labels.push(name);
        }
    }
    (labels, uids)
}

/// Fetch the geofence row for `task_id`, returning
/// `(place_uid, arrival, departure)`. Empty `place_uid` = no row.
/// If a task has multiple geofences (rare; schema allows it) we
/// pick the first by rowid.
fn current_geofence_for(db: &Database, task_id: i64) -> (String, bool, bool) {
    db.connection()
        .query_row(
            "SELECT place, arrival, departure FROM geofences \
             WHERE task = ?1 ORDER BY geofence_id LIMIT 1",
            [task_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, i32>(1)? != 0,
                    r.get::<_, i32>(2)? != 0,
                ))
            },
        )
        .unwrap_or_default()
}

/// Read the alarms attached to `task_id`. Returns three parallel
/// vectors suitable for the bridge's QStringList/QList Q_PROPERTYs.
fn current_alarms_for(db: &Database, task_id: i64) -> (Vec<String>, Vec<i64>, Vec<i32>) {
    let mut labels = Vec::new();
    let mut times = Vec::new();
    let mut types = Vec::new();
    let Ok(mut stmt) = db
        .connection()
        .prepare("SELECT time, type FROM alarms WHERE task = ?1 ORDER BY time")
    else {
        return (labels, times, types);
    };
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i32>(1)?))
    });
    if let Ok(rows) = rows {
        for (time, alarm_type) in rows.flatten() {
            labels.push(describe_alarm(alarm_type, time));
            times.push(time);
            types.push(alarm_type);
        }
    }
    (labels, times, types)
}

/// Fetch the tag UIDs attached to `task_id` via the `tags` join.
fn current_tag_uids_for(db: &Database, task_id: i64) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(mut stmt) = db
        .connection()
        .prepare("SELECT tag_uid FROM tags WHERE task = ?1 AND tag_uid IS NOT NULL")
    else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([task_id], |r| r.get::<_, String>(0)) {
        for uid in rows.flatten() {
            out.push(uid);
        }
    }
    out
}

/// Return parallel `(labels, uuids)` for every CalDAV calendar. Used
/// by the edit dialog's list picker; `build_sidebar` has a richer
/// shape because its output also includes the built-in filter IDs.
fn list_caldav_calendars(db: &Database) -> (Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut uuids = Vec::new();
    let Ok(mut stmt) = db
        .connection()
        .prepare("SELECT * FROM caldav_lists ORDER BY cdl_order, cdl_name")
    else {
        return (labels, uuids);
    };
    let Ok(rows) = stmt.query_map([], CaldavCalendar::from_row) else {
        return (labels, uuids);
    };
    for row in rows.flatten() {
        if let (Some(name), Some(uuid)) = (row.name, row.uuid) {
            labels.push(name);
            uuids.push(uuid);
        }
    }
    (labels, uuids)
}

/// Look up the CalDAV calendar UUID assigned to `task_id`. Returns
/// empty when the task has no `caldav_tasks` row (local-only).
fn current_caldav_uuid_for(db: &Database, task_id: i64) -> String {
    db.connection()
        .query_row(
            "SELECT cd_calendar FROM caldav_tasks WHERE cd_task = ?1 LIMIT 1",
            [task_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn build_sidebar(db: &Database) -> (Vec<String>, Vec<String>) {
    let mut labels = vec![
        "All active".to_string(),
        "Today".to_string(),
        "Recently modified".to_string(),
    ];
    let mut ids = vec![
        FILTER_ALL.to_string(),
        FILTER_TODAY.to_string(),
        FILTER_RECENT.to_string(),
    ];

    match db
        .connection()
        .prepare("SELECT * FROM caldav_lists ORDER BY cdl_order, cdl_name")
    {
        Ok(mut stmt) => match stmt.query_map([], CaldavCalendar::from_row) {
            Ok(rows) => {
                for row in rows {
                    match row {
                        Ok(cal) => {
                            if let (Some(name), Some(uuid)) = (cal.name, cal.uuid) {
                                labels.push(name);
                                ids.push(format!("caldav:{uuid}"));
                            }
                        }
                        Err(e) => tracing::warn!("caldav_lists row decode failed: {e}"),
                    }
                }
            }
            Err(e) => tracing::warn!("caldav_lists query_map failed: {e}"),
        },
        Err(e) => tracing::warn!("caldav_lists prepare failed: {e}"),
    }

    match db
        .connection()
        .prepare("SELECT * FROM filters ORDER BY f_order, title")
    {
        Ok(mut stmt) => match stmt.query_map([], CustomFilter::from_row) {
            Ok(rows) => {
                for row in rows {
                    match row {
                        Ok(f) => {
                            if let Some(title) = f.title {
                                labels.push(title);
                                ids.push(format!("filter:{}", f.id));
                            }
                        }
                        Err(e) => tracing::warn!("filters row decode failed: {e}"),
                    }
                }
            }
            Err(e) => tracing::warn!("filters query_map failed: {e}"),
        },
        Err(e) => tracing::warn!("filters prepare failed: {e}"),
    }

    (labels, ids)
}

fn string_list_from_iter<'a>(iter: impl Iterator<Item = &'a str>) -> QStringList {
    let mut list: QList<QString> = QList::default();
    for s in iter {
        list.append(QString::from(s));
    }
    QStringList::from(&list)
}

/// Tell any previously-running watcher thread to exit. The thread
/// observes the shared atomic on its next 500 ms tick and returns.
fn stop_prior_watcher(mut vm: Pin<&mut qobject::TaskListViewModel>) {
    if let Some(stop) = vm.as_mut().rust_mut().watcher_stop.take() {
        stop.store(true, Ordering::Relaxed);
    }
}

/// Spawn a background thread that watches the directory containing
/// `path` and queues a `reload_active_filter` on the Qt thread when
/// the debouncer fires.
///
/// The thread takes its own `DatabaseWatcher` so the `Receiver`
/// stays thread-local and the Debouncer is kept alive for the watch
/// duration. A shared `AtomicBool` lets `open_database` terminate
/// the prior watcher when a new file is selected.
fn start_watcher(mut vm: Pin<&mut qobject::TaskListViewModel>, path: PathBuf) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    vm.as_mut().rust_mut().watcher_stop = Some(stop);

    let qt_thread = vm.as_ref().qt_thread();

    std::thread::spawn(move || {
        let watcher = match DatabaseWatcher::start(&path) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("DatabaseWatcher::start failed for {:?}: {e}", path);
                return;
            }
        };
        tracing::info!("watching {} for changes", path.display());
        loop {
            if stop_thread.load(Ordering::Relaxed) {
                tracing::debug!("watcher thread stopping for {:?}", path);
                return;
            }
            match watcher.events.recv_timeout(Duration::from_millis(500)) {
                Ok(_event) => {
                    if let Err(e) = qt_thread.queue(|pinned| {
                        pinned.reload_active_filter();
                    }) {
                        tracing::warn!("couldn't queue reload on Qt thread: {e}");
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    tracing::debug!("watcher channel closed; thread exiting");
                    return;
                }
            }
        }
    });
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Current process-local UTC offset in seconds, east-positive. Used to
/// anchor `FILTER_TODAY` to local midnight rather than UTC midnight.
/// Delegates to Qt's `QDateTime::offsetFromUtc()` so Qt's own timezone
/// database resolves DST transitions — we avoid pulling in a parallel
/// time library.
fn current_local_offset_secs() -> i32 {
    QDateTime::current_date_time().offset_from_utc()
}

/// Reset every `selected_*` Q_PROPERTY to its empty/default value.
/// Shared between the "unknown id" branch of `select_task` and the
/// post-delete cleanup so a future new field only has to be added
/// in one place.
fn clear_detail_pane(mut vm: Pin<&mut qobject::TaskListViewModel>) {
    vm.as_mut().set_selected_id(0);
    vm.as_mut().set_selected_title(QString::default());
    vm.as_mut().set_selected_notes(QString::default());
    vm.as_mut().set_selected_due_label(QString::default());
    vm.as_mut()
        .set_selected_hide_until_label(QString::default());
    vm.as_mut().set_selected_priority(Priority::NONE);
    vm.as_mut().set_selected_completed(false);
    vm.as_mut().set_selected_recurrence(QString::default());
    vm.as_mut()
        .set_selected_caldav_calendar_uuid(QString::default());
    vm.as_mut().set_selected_tag_uids(QStringList::default());
    vm.as_mut()
        .set_selected_alarm_labels(QStringList::default());
    vm.as_mut().set_selected_alarm_times(QList::default());
    vm.as_mut().set_selected_alarm_types(QList::default());
    vm.as_mut().set_selected_place_uid(QString::default());
    vm.as_mut().set_selected_place_arrival(false);
    vm.as_mut().set_selected_place_departure(false);
    vm.as_mut()
        .set_parent_candidate_labels(QStringList::default());
    vm.as_mut().set_parent_candidate_ids(QList::default());
    vm.as_mut().set_selected_parent_id(0);
    vm.as_mut().set_selected_estimated_text(QString::default());
    vm.as_mut().set_selected_elapsed_text(QString::default());
    vm.as_mut().set_selected_recurrence_raw(QString::default());
    vm.as_mut().set_selected_repeat_from(0);
}
