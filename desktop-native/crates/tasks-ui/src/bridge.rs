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
        // Sidebar: parallel label / identifier arrays. Identifier format:
        //   "__all__" | "__today__" | "__recent__"  (built-in filters)
        //   "caldav:<uuid>"                          (CalDAV calendar)
        //   "filter:<id>"                            (custom saved filter)
        #[qproperty(QStringList, sidebar_labels)]
        #[qproperty(QStringList, sidebar_ids)]
        #[qproperty(QString, active_filter_id)]
        // Status bar text.
        #[qproperty(QString, status)]
        type TaskListViewModel = super::TaskListViewModelRust;

        #[qinvokable]
        fn open_database(self: Pin<&mut TaskListViewModel>, path: QString);

        #[qinvokable]
        fn select_filter(self: Pin<&mut TaskListViewModel>, id: QString);

        #[qinvokable]
        fn select_task(self: Pin<&mut TaskListViewModel>, id: i64);
    }
}

use core::pin::Pin;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QDateTime, QList, QString, QStringList};

use tasks_core::db::Database;
use tasks_core::models::{CaldavCalendar, Filter as CustomFilter, Priority, Task};
use tasks_core::query::{
    run_by_filter_id, QueryPreferences, FILTER_ALL, FILTER_RECENT, FILTER_TODAY,
};

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
    // Sidebar state.
    sidebar_labels: QStringList,
    sidebar_ids: QStringList,
    active_filter_id: QString,
    // Status.
    status: QString,
    // Non-Qt bookkeeping. Held on the Rust side only; not exposed to QML.
    db_path: Option<PathBuf>,
    db: Option<Database>,
    task_cache: Vec<Task>,
    /// User preferences fed to `run_by_filter_id`. The UI panel for
    /// editing these isn't wired yet (Milestone 1 scope); for now it
    /// stays at the Android defaults.
    preferences: QueryPreferences,
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
            sidebar_labels: QStringList::default(),
            sidebar_ids: QStringList::default(),
            active_filter_id: QString::from(FILTER_ALL),
            status: QString::default(),
            db_path: None,
            db: None,
            task_cache: Vec::new(),
            preferences: QueryPreferences::default(),
        }
    }
}

impl qobject::TaskListViewModel {
    pub fn open_database(mut self: Pin<&mut Self>, path: QString) {
        let path_string = path.to_string();
        let path_buf = PathBuf::from(&path_string);

        // Read sidebar (filters + calendars) eagerly, independently of the
        // list query — the sidebar is stable across filter selection.
        // The Database handle is cached on the view model so selectFilter
        // / selectTask don't have to re-verify the identity hash on every
        // navigation.
        match Database::open_read_only(&path_buf) {
            Ok(db) => {
                let (labels, ids) = build_sidebar(&db);
                self.as_mut()
                    .set_sidebar_labels(string_list_from_iter(labels.iter().map(String::as_str)));
                self.as_mut()
                    .set_sidebar_ids(string_list_from_iter(ids.iter().map(String::as_str)));
                self.as_mut().set_status(QString::from(&format!(
                    "Opened {path_string}. {} sidebar entries.",
                    labels.len()
                )));
                {
                    let mut inner = self.as_mut().rust_mut();
                    inner.db_path = Some(path_buf);
                    inner.db = Some(db);
                    // `inner` drops here so subsequent as_mut() calls can
                    // re-borrow without an overlap.
                }
                self.as_mut().reload_active_filter();
            }
            Err(e) => {
                let msg = format!("Couldn't open {path_string}: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                self.as_mut().clear_list();
                let mut inner = self.as_mut().rust_mut();
                inner.db_path = None;
                inner.db = None;
            }
        }
    }

    pub fn select_filter(mut self: Pin<&mut Self>, id: QString) {
        self.as_mut().set_active_filter_id(id);
        self.as_mut().reload_active_filter();
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

    // Populate the parallel arrays before count. QML delegates index into
    // `titles[i]` etc. up to `count`, so if a re-render happened between
    // setting a larger count and the corresponding array, it could pick up
    // a stale/empty value for that row. In practice cxx-qt runs on the Qt
    // event loop thread and deferred updates batch per-frame, but the
    // defensive ordering is free.
    let count = tasks.len() as i32;
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
