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
        #[qproperty(i32, selected_priority)]
        #[qproperty(bool, selected_completed)]
        // Raw RRULE from `tasks.recurrence` (e.g. "FREQ=DAILY;INTERVAL=1")
        // or empty when the task doesn't repeat. Humanising this to
        // prose ("Every day", "Every other Tuesday") is a later milestone;
        // for now showing the literal rule is better than hiding it.
        #[qproperty(QString, selected_recurrence)]
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

        #[qinvokable]
        fn toggle_task_completion(self: Pin<&mut TaskListViewModel>, id: i64, completed: bool);

        #[qinvokable]
        fn delete_selected_task(self: Pin<&mut TaskListViewModel>);
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
    selected_priority: i32,
    selected_completed: bool,
    selected_recurrence: QString,
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
            selected_priority: Priority::NONE,
            selected_completed: false,
            selected_recurrence: QString::default(),
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
                self.as_mut().set_selected_id(0);
                self.as_mut().set_selected_title(QString::default());
                self.as_mut().set_selected_notes(QString::default());
                self.as_mut().set_selected_due_label(QString::default());
                self.as_mut().set_selected_priority(Priority::NONE);
                self.as_mut().set_selected_completed(false);
                self.as_mut().set_selected_recurrence(QString::default());
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
            // Unknown id — reset detail pane.
            self.as_mut().set_selected_id(0);
            self.as_mut().set_selected_title(QString::default());
            self.as_mut().set_selected_notes(QString::default());
            self.as_mut().set_selected_due_label(QString::default());
            self.as_mut().set_selected_priority(Priority::NONE);
            self.as_mut().set_selected_completed(false);
            self.as_mut().set_selected_recurrence(QString::default());
            return;
        };

        self.as_mut().set_selected_id(task.id);
        self.as_mut()
            .set_selected_title(QString::from(task.title.as_deref().unwrap_or("")));
        self.as_mut()
            .set_selected_notes(QString::from(task.notes.as_deref().unwrap_or("")));
        self.as_mut()
            .set_selected_due_label(QString::from(&format_due_label(task.due_date)));
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
            let mut inner = vm.as_mut().rust_mut();
            inner.db_path = None;
            inner.db = None;
        }
    }
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

/// Format a millisecond-epoch due date as a compact local-time string.
/// Falls back to an empty label when `due_ms` is 0 (no date set).
///
/// The Android client encodes "date-only" vs "date + time" tasks by
/// seeding timed tasks with a non-zero seconds component, so
/// `due_ms / 1000 % 60 > 0` picks out timed tasks the same way
/// `Task.hasDueTime` does in Kotlin.
fn format_due_label(due_ms: i64) -> String {
    if due_ms <= 0 {
        return String::new();
    }
    let secs = due_ms / 1000;
    let days_from_epoch = secs.div_euclid(86_400);
    let (y, m, d) = days_to_ymd(days_from_epoch);
    let has_time = secs % 60 > 0;
    if has_time {
        let secs_of_day = secs - days_from_epoch * 86_400;
        let h = secs_of_day / 3600;
        let min = (secs_of_day % 3600) / 60;
        format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// Convert Unix-epoch day count to `(year, month, day)` in the proleptic
/// Gregorian calendar. Implements Howard Hinnant's `civil_from_days`
/// algorithm with era-offset = 719468 days (from 0000-03-01 to 1970-01-01).
/// Accurate from -1 000 000 to 1 000 000 AD; sufficient for any task due
/// date the Android client writes.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::{days_to_ymd, format_due_label};

    #[test]
    fn days_to_ymd_round_trip_known_values() {
        // 1970-01-01 is day 0.
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        // 2000-01-01 is day 10957.
        assert_eq!(days_to_ymd(10957), (2000, 1, 1));
        // 2020-02-29 (leap day) is day 18321.
        assert_eq!(days_to_ymd(18321), (2020, 2, 29));
    }

    #[test]
    fn format_due_label_date_only_vs_datetime() {
        // Midnight UTC on 2020-02-29 → date-only (seconds == 0).
        assert_eq!(format_due_label(1_582_934_400_000), "2020-02-29");
        // 2020-02-29 12:34:01 UTC → timed: Android convention stores a
        // non-zero seconds component to flag "has time" on write.
        assert_eq!(format_due_label(1_582_979_641_000), "2020-02-29 12:34");
        // No date → empty.
        assert_eq!(format_due_label(0), "");
    }
}
