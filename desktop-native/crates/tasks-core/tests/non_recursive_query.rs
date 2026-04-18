//! Tests for the non-recursive query builder and `build_query` dispatcher.

use rusqlite::{params, Connection};

use tasks_core::query::{
    build_non_recursive_query, build_query,
    preferences::QueryPreferences,
    sort::{SORT_CREATED, SORT_MODIFIED},
    QueryFilter,
};

fn create_minimal_schema(conn: &Connection) {
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
        CREATE TABLE caldav_tasks (cd_id INTEGER PRIMARY KEY AUTOINCREMENT, cd_task INTEGER, cd_calendar TEXT, cd_remote_id TEXT, cd_object TEXT, cd_etag TEXT, cd_last_sync INTEGER DEFAULT 0, cd_deleted INTEGER DEFAULT 0, cd_remote_parent TEXT, gt_moved INTEGER DEFAULT 0, gt_remote_order INTEGER DEFAULT 0);
        CREATE TABLE caldav_lists (cdl_id INTEGER PRIMARY KEY AUTOINCREMENT, cdl_account TEXT, cdl_uuid TEXT, cdl_name TEXT, cdl_color INTEGER DEFAULT 0, cdl_ctag TEXT, cdl_url TEXT, cdl_icon TEXT, cdl_order INTEGER DEFAULT -1, cdl_access INTEGER DEFAULT 0, cdl_last_sync INTEGER DEFAULT 0);
        CREATE TABLE caldav_accounts (cda_id INTEGER PRIMARY KEY AUTOINCREMENT, cda_uuid TEXT, cda_name TEXT, cda_url TEXT, cda_username TEXT, cda_password TEXT, cda_error TEXT, cda_account_type INTEGER DEFAULT 0, cda_collapsed INTEGER DEFAULT 0, cda_server_type INTEGER DEFAULT -1, cda_last_sync INTEGER DEFAULT 0);
        CREATE TABLE tags (_id INTEGER PRIMARY KEY AUTOINCREMENT, task INTEGER, name TEXT, tag_uid TEXT, task_uid TEXT);
        CREATE TABLE geofences (geofence_id INTEGER PRIMARY KEY AUTOINCREMENT, task INTEGER, place TEXT, arrival INTEGER DEFAULT 0, departure INTEGER DEFAULT 0);
        CREATE TABLE places (place_id INTEGER PRIMARY KEY AUTOINCREMENT, uid TEXT, name TEXT, address TEXT, phone TEXT, url TEXT, latitude REAL DEFAULT 0, longitude REAL DEFAULT 0, place_color INTEGER DEFAULT 0, place_icon TEXT, place_order INTEGER DEFAULT -1, radius INTEGER DEFAULT 250);
        "#,
    )
    .expect("create schema");
}

fn insert_task(conn: &Connection, title: &str, modified: i64) -> i64 {
    conn.execute(
        "INSERT INTO tasks (title, created, modified) VALUES (?1, ?2, ?2)",
        params![title, modified],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[test]
fn non_recursive_query_prepares_and_groups_by_task_id() {
    let conn = Connection::open_in_memory().unwrap();
    create_minimal_schema(&conn);
    insert_task(&conn, "A", 1);
    insert_task(&conn, "B", 2);

    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        sort_mode: SORT_CREATED,
        ..QueryPreferences::default()
    };

    let sql = build_non_recursive_query(&filter, &prefs, 0, None);
    assert!(sql.contains("GROUP BY tasks._id"));
    assert!(sql.to_uppercase().contains("ORDER BY"));
    assert!(sql.contains("FROM tasks"));

    let mut stmt = conn
        .prepare(&sql)
        .unwrap_or_else(|e| panic!("prepare: {e}\nSQL:\n{sql}"));
    let rows = stmt.query_map([], |r| r.get::<_, i64>("_id")).unwrap();
    assert_eq!(rows.count(), 2);
}

#[test]
fn recently_modified_preserves_filter_order_by() {
    let filter = QueryFilter::recently_modified(
        "WHERE tasks.deleted = 0 AND tasks.modified > 100 ORDER BY tasks.modified DESC",
    );
    let prefs = QueryPreferences::default();

    let sql = build_non_recursive_query(&filter, &prefs, 0, None);
    // The filter's own ORDER BY is retained; GROUP BY is inserted just
    // before it rather than prepending a completed-at-bottom sort.
    assert!(sql.contains("GROUP BY tasks._id ORDER BY tasks.modified DESC"));
    assert!(
        !sql.contains("parentComplete ASC"),
        "recently-modified should skip completed-at-bottom prelude"
    );
}

#[test]
fn completed_at_bottom_prepends_sort_prelude() {
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        sort_mode: SORT_MODIFIED,
        completed_tasks_at_bottom: true,
        ..QueryPreferences::default()
    };

    let sql = build_non_recursive_query(&filter, &prefs, 0, None);
    assert!(
        sql.contains("GROUP BY tasks._id ORDER BY parentComplete ASC, tasks.completed DESC,"),
        "expected completed-at-bottom prelude before ORDER BY in:\n{sql}"
    );
}

#[test]
fn limit_is_appended_at_the_tail() {
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences::default();
    let sql = build_non_recursive_query(&filter, &prefs, 0, Some(50));
    assert!(
        sql.trim_end().ends_with("LIMIT 50"),
        "LIMIT should end the query; got ...{}",
        &sql[sql.len().saturating_sub(80)..]
    );
}

#[test]
fn dispatcher_picks_non_recursive_for_recently_modified() {
    let filter =
        QueryFilter::recently_modified("WHERE tasks.deleted = 0 ORDER BY tasks.modified DESC");
    let prefs = QueryPreferences::default();
    let sql = build_query(&filter, &prefs, 0, None);
    assert!(
        !sql.contains("WITH RECURSIVE"),
        "recently-modified should take the non-recursive path"
    );
}

#[test]
fn dispatcher_picks_recursive_for_default_custom() {
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences::default();
    let sql = build_query(&filter, &prefs, 0, None);
    assert!(sql.contains("WITH RECURSIVE"));
}

#[test]
fn dispatcher_astrid_without_astrid_sort_still_picks_recursive() {
    // Regression guard for the self-review fix: when a filter extends
    // AstridOrderingFilter but the user is not in Astrid sort mode, the
    // Kotlin cascade falls through to `supportsSorting()` → recursive.
    let filter = QueryFilter::Custom {
        sql: "WHERE tasks.parent = 0".to_string(),
        supports_astrid_ordering: true,
        is_recently_modified: false,
    };
    let prefs = QueryPreferences {
        is_astrid_sort: false,
        ..QueryPreferences::default()
    };
    let sql = build_query(&filter, &prefs, 0, None);
    assert!(
        sql.contains("WITH RECURSIVE"),
        "astrid filter with astrid_sort off should still be recursive:\n{sql}"
    );
}

#[test]
fn show_hidden_rewrites_hide_until_predicate() {
    // Regression guard for review finding #1: show_hidden was a silent
    // no-op because the literal still carried the regex's `?` character.
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        show_hidden: true,
        ..QueryPreferences::default()
    };
    let sql = build_non_recursive_query(&filter, &prefs, 0, None);
    assert!(
        !sql.contains("tasks.hideUntil<=(strftime('%s','now')*1000)"),
        "show_hidden should rewrite the active-visible predicate:\n{sql}"
    );
}

#[test]
fn sort_modified_ascending_pref_produces_asc_direction() {
    // Regression guard for review finding #2: the Kotlin reverse()
    // dance collapses to "final direction = preferences.sortAscending".
    // Previously we inverted for SORT_MODIFIED / SORT_CREATED.
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs_asc = QueryPreferences {
        sort_mode: SORT_MODIFIED,
        sort_ascending: true,
        ..QueryPreferences::default()
    };
    let sql_asc = build_non_recursive_query(&filter, &prefs_asc, 0, None);
    assert!(
        sql_asc.contains("tasks.modified ASC"),
        "sort_ascending=true with SORT_MODIFIED should emit ASC:\n{sql_asc}"
    );

    let prefs_desc = QueryPreferences {
        sort_mode: SORT_MODIFIED,
        sort_ascending: false,
        ..QueryPreferences::default()
    };
    let sql_desc = build_non_recursive_query(&filter, &prefs_desc, 0, None);
    assert!(
        sql_desc.contains("tasks.modified DESC"),
        "sort_ascending=false with SORT_MODIFIED should emit DESC:\n{sql_desc}"
    );
}

#[test]
fn sort_list_uses_cdl_order_as_primary_with_cdl_name_secondary() {
    // Regression guard for review finding #3: SORT_LIST was previously
    // emitting `UPPER(cdl_name)` as the primary and dropping `cdl_name`
    // as the secondary, diverging from SortHelper.ORDER_LIST.
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        sort_mode: tasks_core::query::sort::SORT_LIST,
        ..QueryPreferences::default()
    };
    let sql = build_non_recursive_query(&filter, &prefs, 0, None);
    assert!(
        sql.contains("UPPER(cdl_order) ASC"),
        "SORT_LIST primary should be UPPER(cdl_order):\n{sql}"
    );
    assert!(
        sql.contains("cdl_name ASC"),
        "SORT_LIST should keep cdl_name as secondary:\n{sql}"
    );
}

#[test]
fn dispatcher_astrid_with_astrid_sort_picks_non_recursive() {
    let filter = QueryFilter::Custom {
        sql: "WHERE tasks.parent = 0".to_string(),
        supports_astrid_ordering: true,
        is_recently_modified: false,
    };
    let prefs = QueryPreferences {
        is_astrid_sort: true,
        ..QueryPreferences::default()
    };
    let sql = build_query(&filter, &prefs, 0, None);
    assert!(
        !sql.contains("WITH RECURSIVE"),
        "astrid filter with astrid_sort on should be non-recursive:\n{sql}"
    );
}
