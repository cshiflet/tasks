//! Entry point for the Qt 6 front-end.
//!
//! By default launches the QML shell. When the first argument is `--cli
//! <path-to-tasks.db>` the binary instead dumps the Active filter to stdout
//! — useful for smoke-testing the data layer without a display.

// cxx-qt's bridge macro generates support code whose arity mirrors the
// Q_INVOKABLE signatures we declare. `update_selected_task` is wide on
// purpose (every editable task field is passed in one call so the UPDATE
// stays atomic); bundling into a Rust struct would force a C++ shim to
// cross the FFI boundary. Since cxx_qt::bridge rejects inner `#![allow]`s
// inside the generated module, we opt out of the lint at the crate root.
#![allow(clippy::too_many_arguments)]

use std::env;
use std::process::ExitCode;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

mod bridge;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--cli") {
        return run_cli(args.get(2).map(String::as_str));
    }

    // Pick the Material style so `Material.theme: Material.System` in
    // QML follows the OS dark-mode toggle. Set before QGuiApplication
    // constructs so QQuickStyle picks it up.
    //
    // Respect an operator-set override (e.g. QT_QUICK_CONTROLS_STYLE=
    // Fusion) — useful for debugging and for users who prefer a
    // platform-native look.
    if env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        env::set_var("QT_QUICK_CONTROLS_STYLE", "Material");
    }

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/tasks/desktop/qml/Main.qml"));
    }

    match app.as_mut().map(|app| app.exec()) {
        Some(code) => ExitCode::from(code as u8),
        None => {
            eprintln!("failed to construct QGuiApplication");
            ExitCode::from(1)
        }
    }
}

fn run_cli(db_path: Option<&str>) -> ExitCode {
    use tasks_core::db::Database;
    use tasks_core::query::{self, TaskFilter};

    let Some(db_path) = db_path else {
        eprintln!("usage: tasks-desktop --cli <path-to-tasks.db>");
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
