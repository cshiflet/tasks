//! cxx-qt bridge exposing a minimal read-only task-list view model to QML.
//!
//! This is the first seam between Rust (tasks-core) and Qt. The QML layer
//! calls `openDatabase(path)` to point the model at an Android-produced
//! SQLite file; the model opens it read-only, runs the Active filter, and
//! publishes the result as a QStringList the view can bind directly to.
//!
//! The surface is intentionally thin — a proper `QAbstractListModel` with
//! per-row roles (title, due-date, subtask indent, priority colour, …) is
//! the next step, once the end-to-end wiring is proven.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, count)]
        #[qproperty(QString, status)]
        #[qproperty(QStringList, titles)]
        type TaskListViewModel = super::TaskListViewModelRust;

        #[qinvokable]
        fn open_database(self: Pin<&mut TaskListViewModel>, path: QString);
    }
}

use core::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use cxx_qt_lib::{QList, QString, QStringList};

use tasks_core::db::Database;
use tasks_core::query::{self, TaskFilter};

#[derive(Default)]
pub struct TaskListViewModelRust {
    count: i32,
    status: QString,
    titles: QStringList,
}

impl qobject::TaskListViewModel {
    pub fn open_database(mut self: Pin<&mut Self>, path: QString) {
        let path_string = path.to_string();

        let db = match Database::open_read_only(&path_string) {
            Ok(db) => db,
            Err(e) => {
                let msg = format!("Couldn't open {path_string}: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                self.as_mut().set_count(0);
                self.as_mut().set_titles(QStringList::default());
                return;
            }
        };

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        match query::run(&db, TaskFilter::Active, now_ms) {
            Ok(tasks) => {
                let count = tasks.len() as i32;
                let mut list: QList<QString> = QList::default();
                for t in &tasks {
                    let title = t.title.as_deref().unwrap_or("(no title)");
                    list.append(QString::from(title));
                }
                let status = format!("Loaded {count} active task(s) from {path_string}");
                self.as_mut().set_count(count);
                self.as_mut().set_status(QString::from(&status));
                self.as_mut().set_titles(QStringList::from(&list));
            }
            Err(e) => {
                let msg = format!("Query failed: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                self.as_mut().set_count(0);
                self.as_mut().set_titles(QStringList::default());
            }
        }
    }
}
