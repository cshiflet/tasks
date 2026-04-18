//! Integration tests for the recursive TaskListQuery port.
//!
//! These build a small fixture database containing every table the recursive
//! query joins against (tasks, caldav_tasks, caldav_lists, caldav_accounts,
//! tags, geofences, places), prepare the generated SQL, and assert that it
//! both parses and returns rows in the expected order. Snapshot-style
//! substring assertions protect the overall shape of the CTE so obvious
//! drift (missing clause, reordered ORDER BY) trips the test suite.

use rusqlite::{params, Connection};

use tasks_core::query::{
    build_recursive_query,
    preferences::QueryPreferences,
    sort::{SORT_CREATED, SORT_DUE},
    QueryFilter,
};

fn create_schema(conn: &Connection) {
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
        CREATE TABLE caldav_tasks (
            cd_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            cd_task INTEGER NOT NULL,
            cd_calendar TEXT,
            cd_remote_id TEXT,
            cd_object TEXT,
            cd_etag TEXT,
            cd_last_sync INTEGER NOT NULL DEFAULT 0,
            cd_deleted INTEGER NOT NULL DEFAULT 0,
            cd_remote_parent TEXT,
            gt_moved INTEGER NOT NULL DEFAULT 0,
            gt_remote_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE caldav_lists (
            cdl_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            cdl_account TEXT,
            cdl_uuid TEXT,
            cdl_name TEXT,
            cdl_color INTEGER NOT NULL DEFAULT 0,
            cdl_ctag TEXT,
            cdl_url TEXT,
            cdl_icon TEXT,
            cdl_order INTEGER NOT NULL DEFAULT -1,
            cdl_access INTEGER NOT NULL DEFAULT 0,
            cdl_last_sync INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE caldav_accounts (
            cda_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            cda_uuid TEXT,
            cda_name TEXT,
            cda_url TEXT,
            cda_username TEXT,
            cda_password TEXT,
            cda_error TEXT,
            cda_account_type INTEGER NOT NULL DEFAULT 0,
            cda_collapsed INTEGER NOT NULL DEFAULT 0,
            cda_server_type INTEGER NOT NULL DEFAULT -1,
            cda_last_sync INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE tags (
            _id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            task INTEGER NOT NULL,
            name TEXT,
            tag_uid TEXT,
            task_uid TEXT
        );
        CREATE TABLE geofences (
            geofence_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            task INTEGER NOT NULL,
            place TEXT,
            arrival INTEGER NOT NULL DEFAULT 0,
            departure INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE places (
            place_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            uid TEXT,
            name TEXT,
            address TEXT,
            phone TEXT,
            url TEXT,
            latitude REAL NOT NULL DEFAULT 0,
            longitude REAL NOT NULL DEFAULT 0,
            place_color INTEGER NOT NULL DEFAULT 0,
            place_icon TEXT,
            place_order INTEGER NOT NULL DEFAULT -1,
            radius INTEGER NOT NULL DEFAULT 250
        );
        "#,
    )
    .expect("create schema");
}

fn insert_caldav_list(conn: &Connection, uuid: &str, account: &str) {
    conn.execute(
        "INSERT INTO caldav_lists (cdl_uuid, cdl_account, cdl_name) VALUES (?1, ?2, ?3)",
        params![uuid, account, "Work"],
    )
    .unwrap();
}

fn insert_task(conn: &Connection, title: &str, parent: i64, due: i64, created: i64) -> i64 {
    conn.execute(
        "INSERT INTO tasks (title, parent, dueDate, created, modified) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![title, parent, due, created],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn link_caldav(conn: &Connection, task: i64, calendar: &str, remote_id: &str) {
    conn.execute(
        "INSERT INTO caldav_tasks (cd_task, cd_calendar, cd_remote_id, cd_object) \
         VALUES (?1, ?2, ?3, ?3)",
        params![task, calendar, remote_id],
    )
    .unwrap();
}

#[test]
fn custom_filter_query_prepares_and_runs() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    let created_base = 1_700_000_000_000;
    insert_task(&conn, "Top-level A", 0, 0, created_base);
    insert_task(&conn, "Top-level B", 0, 0, created_base + 1);

    // Custom filter: plain WHERE clause with no placeholders — the most
    // common saved-filter form.
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        sort_mode: SORT_CREATED,
        ..QueryPreferences::default()
    };

    let sql = build_recursive_query(&filter, &prefs, 0, None);
    assert!(sql.contains("WITH RECURSIVE recursive_tasks"));
    assert!(sql.contains("max_indent AS"));
    assert!(sql.contains("descendants_recursive AS"));
    assert!(sql.contains("GROUP BY tasks._id"));
    assert!(sql.contains("ORDER BY sequence"));

    let mut stmt = conn
        .prepare(&sql)
        .unwrap_or_else(|e| panic!("prepare recursive query: {e}\n\nSQL:\n{sql}"));
    let count: i64 = stmt
        .query_map([], |row| row.get::<_, i64>("_id"))
        .unwrap()
        .count() as i64;
    assert_eq!(count, 2);
}

#[test]
fn caldav_filter_scopes_to_calendar_and_expands_subtasks() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);
    insert_caldav_list(&conn, "list-A", "acct-1");
    insert_caldav_list(&conn, "list-B", "acct-1");

    // list-A: one parent with two children.
    let base = 1_700_000_000_000;
    let parent_a = insert_task(&conn, "Root A", 0, 0, base);
    link_caldav(&conn, parent_a, "list-A", "uid-root-a");
    let child_a1 = insert_task(&conn, "A.1", parent_a, 0, base + 1);
    link_caldav(&conn, child_a1, "list-A", "uid-a1");
    let child_a2 = insert_task(&conn, "A.2", parent_a, 0, base + 2);
    link_caldav(&conn, child_a2, "list-A", "uid-a2");

    // list-B: a single task that must NOT leak into the list-A result.
    let b = insert_task(&conn, "B", 0, 0, base + 3);
    link_caldav(&conn, b, "list-B", "uid-b");

    let filter = QueryFilter::caldav("list-A");
    let prefs = QueryPreferences::default();

    let sql = build_recursive_query(&filter, &prefs, 0, None);
    let mut stmt = conn
        .prepare(&sql)
        .unwrap_or_else(|e| panic!("prepare recursive query: {e}\n\nSQL:\n{sql}"));
    let titles: Vec<String> = stmt
        .query_map([], |row| row.get::<_, Option<String>>("title"))
        .unwrap()
        .filter_map(|r| r.ok().flatten())
        .collect();

    assert_eq!(titles.len(), 3, "list-A has 3 tasks; got: {titles:?}");
    assert!(
        !titles.iter().any(|t| t == "B"),
        "list-B should be filtered out"
    );
    assert!(titles.contains(&"Root A".to_string()));
    assert!(titles.contains(&"A.1".to_string()));
    assert!(titles.contains(&"A.2".to_string()));
}

#[test]
fn permasql_placeholders_are_expanded_in_custom_sql() {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn);

    let now = 1_700_000_000_000;
    // A filter fragment that uses NOW() — this is exactly how saved filters
    // reference "tasks due after the current instant".
    let filter = QueryFilter::custom("WHERE tasks.dueDate > NOW()");
    let prefs = QueryPreferences::default();

    let sql = build_recursive_query(&filter, &prefs, now, None);
    assert!(!sql.contains("NOW()"), "NOW() was not substituted in {sql}");
    assert!(sql.contains("1700000000000"));
    // Still has to prepare cleanly:
    conn.prepare(&sql)
        .expect("prepare after placeholder expansion");
}

#[test]
fn limit_clause_is_emitted_when_requested() {
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences::default();
    let sql = build_recursive_query(&filter, &prefs, 0, Some(25));
    assert!(sql.contains("LIMIT 25"));
}

#[test]
fn show_completed_rewrites_active_predicate() {
    // When showCompleted is on, the active predicate `tasks.completed<=0`
    // should be replaced by the always-true `1` so completed tasks join the
    // recursive tree.
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        show_completed: true,
        ..QueryPreferences::default()
    };
    let sql = build_recursive_query(&filter, &prefs, 0, None);
    assert!(
        !sql.contains("tasks.completed<=0"),
        "active predicate should have been rewritten:\n{sql}"
    );
}

#[test]
fn caldav_uuid_is_escaped_for_single_quotes() {
    let filter = QueryFilter::caldav("o'brien");
    let prefs = QueryPreferences::default();
    let sql = build_recursive_query(&filter, &prefs, 0, None);
    // Single quote inside the uuid must be doubled to keep the SQL valid.
    assert!(sql.contains("'o''brien'"), "quote not escaped in:\n{sql}");
}

#[test]
fn sort_mode_due_uses_adjusted_due_date_expression() {
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        sort_mode: SORT_DUE,
        ..QueryPreferences::default()
    };
    let sql = build_recursive_query(&filter, &prefs, 0, None);
    assert!(
        sql.contains("tasks.dueDate / 1000") || sql.contains("tasks.dueDate/1000"),
        "SORT_DUE should reference ADJUSTED_DUE_DATE:\n{sql}"
    );
}
