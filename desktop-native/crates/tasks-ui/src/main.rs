//! Placeholder entry point for the Qt front-end.
//!
//! The UI is intentionally a stub in this commit — wiring cxx-qt + QML
//! requires Qt 6 to be installed at build time and hasn't been verified in
//! the current CI environment yet (see `desktop-native/README.md`). For
//! now the binary exposes the read-only core over the command line so the
//! data layer can be exercised without Qt.

use std::env;
use std::process::ExitCode;

use tasks_core::db::Database;
use tasks_core::query::{self, TaskFilter};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = env::args().collect();
    let Some(db_path) = args.get(1) else {
        eprintln!("usage: tasks-desktop <path-to-tasks.db>");
        return ExitCode::from(2);
    };

    let db = match Database::open_read_only(db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("failed to open {db_path}: {e}");
            return ExitCode::from(1);
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    match query::run(&db, TaskFilter::Active, now) {
        Ok(tasks) => {
            println!("{} active task(s):", tasks.len());
            for t in tasks {
                println!("  [{}] {}", t.id, t.title.unwrap_or_default());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("query failed: {e}");
            ExitCode::from(1)
        }
    }
}
