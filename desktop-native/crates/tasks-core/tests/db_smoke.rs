//! End-to-end smoke test for the read-only database layer.
//!
//! Builds a minimal Room-compatible database at Schema 92, writes a handful
//! of tasks through the raw sqlite API, then opens it via `Database` and
//! runs each `TaskFilter`. The fixture is regenerated per run (no binary
//! blobs in git).

use rusqlite::{params, Connection};
use std::path::Path;

use tasks_core::db::{Database, PINNED_IDENTITY_HASH};
use tasks_core::query::{self, TaskFilter};

/// Minimal subset of the Room schema 92 tasks table — just the columns the
/// desktop read path needs. Production deployments open the Android DB,
/// which obviously has all 17 tables; this fixture keeps the test fast.
fn create_tasks_table(conn: &Connection) {
    conn.execute_batch(
        r#"
        CREATE TABLE tasks (
            _id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            title TEXT,
            importance INTEGER NOT NULL DEFAULT 3,
            dueDate INTEGER NOT NULL DEFAULT 0,
            hideUntil INTEGER NOT NULL DEFAULT 0,
            created INTEGER NOT NULL DEFAULT 0,
            modified INTEGER NOT NULL DEFAULT 0,
            completed INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0,
            notes TEXT,
            estimatedSeconds INTEGER NOT NULL DEFAULT 0,
            elapsedSeconds INTEGER NOT NULL DEFAULT 0,
            timerStart INTEGER NOT NULL DEFAULT 0,
            notificationFlags INTEGER NOT NULL DEFAULT 0,
            lastNotified INTEGER NOT NULL DEFAULT 0,
            recurrence TEXT,
            repeat_from INTEGER NOT NULL DEFAULT 0,
            calendarUri TEXT,
            remoteId TEXT,
            collapsed INTEGER NOT NULL DEFAULT 0,
            parent INTEGER NOT NULL DEFAULT 0,
            "order" INTEGER,
            read_only INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .expect("create tasks");
}

fn create_room_metadata(conn: &Connection, identity_hash: &str) {
    conn.execute_batch(
        r#"
        CREATE TABLE room_master_table (id INTEGER PRIMARY KEY, identity_hash TEXT);
        "#,
    )
    .expect("create room_master_table");
    conn.execute(
        "INSERT INTO room_master_table (id, identity_hash) VALUES (42, ?1)",
        params![identity_hash],
    )
    .expect("insert identity");
}

fn insert_task(
    conn: &Connection,
    title: &str,
    due_date: i64,
    completed: i64,
    deleted: i64,
    hide_until: i64,
) {
    conn.execute(
        "INSERT INTO tasks (title, dueDate, completed, deleted, hideUntil) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![title, due_date, completed, deleted, hide_until],
    )
    .expect("insert task");
}

fn build_fixture_db(path: &Path, identity_hash: &str, day_start: i64, day_end: i64) {
    let conn = Connection::open(path).expect("open writable fixture");
    create_room_metadata(&conn, identity_hash);
    create_tasks_table(&conn);

    // Two active, one due today, one due tomorrow.
    insert_task(&conn, "Active, no due date", 0, 0, 0, 0);
    insert_task(
        &conn,
        "Due today morning",
        day_start + 9 * 3_600_000,
        0,
        0,
        0,
    );
    insert_task(&conn, "Due today evening", day_end - 60_000, 0, 0, 0);
    insert_task(&conn, "Due tomorrow", day_end + 3_600_000, 0, 0, 0);

    // Should be excluded: completed, deleted, hide-until in the future.
    insert_task(
        &conn,
        "Completed today",
        day_start + 8 * 3_600_000,
        day_start,
        0,
        0,
    );
    insert_task(&conn, "Deleted", day_start + 8 * 3_600_000, 0, day_start, 0);
    insert_task(
        &conn,
        "Hidden in future",
        day_start + 8 * 3_600_000,
        0,
        0,
        day_end + 60_000,
    );
}

#[test]
fn schema_mismatch_is_reported_clearly() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("fixture.db");
    build_fixture_db(&db_path, "not-the-right-hash", 0, 0);

    let err = Database::open_read_only(&db_path).expect_err("should reject");
    let msg = err.to_string();
    assert!(msg.contains("identity hash mismatch"), "msg = {msg}");
    assert!(msg.contains(PINNED_IDENTITY_HASH), "msg = {msg}");
}

#[test]
fn active_filter_excludes_completed_deleted_and_hidden() {
    let day_start = 1_700_000_000_000i64;
    let day_end = day_start + 24 * 3_600_000 - 1;

    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("fixture.db");
    build_fixture_db(&db_path, PINNED_IDENTITY_HASH, day_start, day_end);

    let db = Database::open_read_only(&db_path).expect("open");
    // `now` = mid-day so the hide-until-in-future task is excluded.
    let now = day_start + 12 * 3_600_000;
    let rows = query::run(&db, TaskFilter::Active, now).expect("query");

    let titles: Vec<_> = rows.iter().filter_map(|t| t.title.clone()).collect();
    assert_eq!(
        titles,
        vec![
            "Due today morning".to_string(),
            "Due today evening".to_string(),
            "Due tomorrow".to_string(),
            "Active, no due date".to_string(),
        ],
        "active filter should sort dated tasks before undated"
    );
}

#[test]
fn today_filter_returns_only_tasks_due_in_window() {
    let day_start = 1_700_000_000_000i64;
    let day_end = day_start + 24 * 3_600_000 - 1;

    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("fixture.db");
    build_fixture_db(&db_path, PINNED_IDENTITY_HASH, day_start, day_end);

    let db = Database::open_read_only(&db_path).expect("open");
    let now = day_start + 12 * 3_600_000;
    let rows = query::run(
        &db,
        TaskFilter::Today {
            day_start_utc_ms: day_start,
            day_end_utc_ms: day_end,
        },
        now,
    )
    .expect("query");

    let titles: Vec<_> = rows.iter().filter_map(|t| t.title.clone()).collect();
    assert_eq!(
        titles,
        vec![
            "Due today morning".to_string(),
            "Due today evening".to_string()
        ]
    );
}

/// H-4: substring search returns matching tasks regardless of due
/// date / completion / hide-until (we want to find anything by
/// keyword). Empty search returns no rows; single-quote in the
/// query escapes safely.
#[test]
fn search_matches_title_and_handles_quotes() {
    let day_start = 1_700_000_000_000i64;
    let day_end = day_start + 24 * 3_600_000 - 1;
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("fixture.db");
    build_fixture_db(&db_path, PINNED_IDENTITY_HASH, day_start, day_end);

    let db = Database::open_read_only(&db_path).expect("open");
    let prefs = tasks_core::query::QueryPreferences::default();

    // Plain substring match — finds both "Due today morning" and
    // "Due today evening" plus "Due tomorrow".
    let rows = tasks_core::query::run_search(&db, "Due", day_start, 0, &prefs).expect("search");
    let titles: Vec<_> = rows.iter().filter_map(|t| t.title.clone()).collect();
    assert!(
        titles.contains(&"Due today morning".to_string())
            && titles.contains(&"Due today evening".to_string())
            && titles.contains(&"Due tomorrow".to_string()),
        "expected Due-prefixed matches, got {titles:?}"
    );

    // Empty / whitespace-only query returns no rows.
    let rows = tasks_core::query::run_search(&db, "   ", day_start, 0, &prefs).expect("empty");
    assert!(rows.is_empty());

    // Quote in query is safely escaped — no SQL syntax error.
    let _ = tasks_core::query::run_search(&db, "o'brien", day_start, 0, &prefs)
        .expect("quote-bearing query must parse");
}
