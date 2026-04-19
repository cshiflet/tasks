//! Rich-fixture integration tests for the query pipeline.
//!
//! The other integration tests build the minimum table shapes their
//! assertions need. This file fabricates a more realistic database —
//! multiple CalDAV accounts, tasks with subtasks three levels deep,
//! tagged tasks, place-backed geofences, completed/deleted/hidden rows —
//! and exercises `build_recursive_query` + `run_by_filter_id` against
//! it. The intent is to catch integration-level bugs the minimal
//! fixtures miss: join cardinality, indent-level arithmetic,
//! CalDAV-scope isolation with overlapping subtask trees, and the
//! completed-at-bottom sort prelude.
//!
//! When a real Android-captured fixture arrives, this file keeps its
//! assertions and just swaps the `build_fixture` body for a copy of the
//! captured file.

use rusqlite::{params, Connection};

use tasks_core::query::{
    build_recursive_query,
    filter::QueryFilter,
    preferences::QueryPreferences,
    sort::{SORT_COMPLETED, SORT_DUE},
};

/// Full Room-92 subset the query pipeline touches. Kept here inline so
/// the fixture script is self-contained and can be dropped wholesale
/// when a captured Android DB lands.
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
        CREATE TABLE tagdata (
            _id INTEGER PRIMARY KEY AUTOINCREMENT,
            remoteId TEXT,
            name TEXT,
            color INTEGER,
            tagOrdering TEXT,
            td_icon TEXT,
            td_order INTEGER NOT NULL DEFAULT 0
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
        CREATE TABLE filters (
            _id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            title TEXT,
            sql TEXT,
            "values" TEXT,
            criterion TEXT,
            f_color INTEGER,
            f_icon TEXT,
            f_order INTEGER NOT NULL
        );
        "#,
    )
    .unwrap();
}

/// Base wall clock the fixture pins its relative timestamps to:
/// 2024-01-15 12:00:00 UTC. Note: the recursive query's
/// `activeAndVisible` predicate reads `strftime('%s','now')` at query
/// time from SQLite — not `BASE_MS`. We therefore use a
/// `HIDDEN_FUTURE_MS` constant that's far enough in the future that
/// the test still passes years after this file was written.
const BASE_MS: i64 = 1_705_320_000_000;
const DAY: i64 = 86_400_000;
const HIDDEN_FUTURE_MS: i64 = 4_000_000_000_000; // 2096-ish

/// Populate the fixture. Returns the number of active (non-completed,
/// non-deleted, not-hidden-in-future) top-level tasks, which callers
/// cross-check against the recursive-query row count.
fn populate(conn: &Connection) -> usize {
    create_schema(conn);

    // Two CalDAV accounts (Nextcloud + Google Tasks), three lists.
    for (i, (name, account_type)) in [("Nextcloud", 0_i32), ("Google Tasks", 7_i32)]
        .iter()
        .enumerate()
    {
        conn.execute(
            "INSERT INTO caldav_accounts (cda_uuid, cda_name, cda_account_type, \
             cda_collapsed, cda_server_type, cda_last_sync) VALUES (?1, ?2, ?3, 0, -1, 0)",
            params![format!("acct-{i}"), name, account_type],
        )
        .unwrap();
    }
    let lists = [
        ("list-work", "acct-0", "Work"),
        ("list-home", "acct-0", "Home"),
        ("list-shopping", "acct-1", "Shopping"),
    ];
    for (uuid, account, name) in lists.iter() {
        conn.execute(
            "INSERT INTO caldav_lists (cdl_uuid, cdl_account, cdl_name, cdl_color, \
             cdl_order, cdl_access, cdl_last_sync) VALUES (?1, ?2, ?3, 0, -1, 0, 0)",
            params![uuid, account, name],
        )
        .unwrap();
    }

    // Tag definitions.
    for (i, name) in ["urgent", "waiting", "deep-work"].iter().enumerate() {
        conn.execute(
            "INSERT INTO tagdata (remoteId, name, color, tagOrdering, td_order) \
             VALUES (?1, ?2, 0, '[]', ?3)",
            params![format!("tag-{i}"), name, i as i32],
        )
        .unwrap();
    }

    // A place + geofence on one task so the join is non-empty.
    conn.execute(
        "INSERT INTO places (uid, name, address, latitude, longitude, place_color, \
         place_order) VALUES (?1, ?2, ?3, 37.77, -122.42, 0, -1)",
        params!["place-home", "Home", "1 Main St"],
    )
    .unwrap();

    // Work list: 1 root + 2 children + 1 grandchild + 1 sibling root.
    let work_root = insert_task(conn, "Ship Q1 plan", 0, BASE_MS + DAY, BASE_MS - DAY);
    link_caldav(conn, work_root, "list-work", "w-root");
    tag_task(conn, work_root, "tag-0", "urgent");

    let sub_1 = insert_task(conn, "Draft outline", work_root, BASE_MS, BASE_MS);
    link_caldav(conn, sub_1, "list-work", "w-outline");

    let sub_2 = insert_task(
        conn,
        "Review with lead",
        work_root,
        BASE_MS + 2 * DAY,
        BASE_MS,
    );
    link_caldav(conn, sub_2, "list-work", "w-review");
    tag_task(conn, sub_2, "tag-1", "waiting");

    let grandchild = insert_task(conn, "Book the room", sub_2, BASE_MS + 2 * DAY, BASE_MS);
    link_caldav(conn, grandchild, "list-work", "w-room");

    let work_other = insert_task(conn, "Weekly status", 0, 0, BASE_MS - 3 * DAY);
    link_caldav(conn, work_other, "list-work", "w-status");

    // Home list: 1 completed task + 1 deleted task + 1 hidden-future + 2 active.
    let h_1 = insert_task(conn, "Fix sink", 0, BASE_MS - 2 * DAY, BASE_MS - 5 * DAY);
    link_caldav(conn, h_1, "list-home", "h-sink");
    conn.execute(
        "UPDATE tasks SET completed = ?1 WHERE _id = ?2",
        params![BASE_MS - DAY, h_1],
    )
    .unwrap();

    let h_2 = insert_task(conn, "Cancel cable", 0, 0, BASE_MS - 10 * DAY);
    link_caldav(conn, h_2, "list-home", "h-cable");
    conn.execute(
        "UPDATE tasks SET deleted = ?1 WHERE _id = ?2",
        params![BASE_MS - 5 * DAY, h_2],
    )
    .unwrap();

    let h_3 = insert_task(conn, "Plan trip (next month)", 0, 0, BASE_MS - DAY);
    link_caldav(conn, h_3, "list-home", "h-trip");
    conn.execute(
        "UPDATE tasks SET hideUntil = ?1 WHERE _id = ?2",
        params![HIDDEN_FUTURE_MS, h_3],
    )
    .unwrap();

    let h_4 = insert_task(conn, "Buy houseplant", 0, BASE_MS + 3 * DAY, BASE_MS);
    link_caldav(conn, h_4, "list-home", "h-plant");
    // Attach a geofence so the LEFT JOIN places path gets exercised.
    conn.execute(
        "INSERT INTO geofences (task, place, arrival, departure) VALUES (?1, 'place-home', 1, 0)",
        params![h_4],
    )
    .unwrap();

    let h_5 = insert_task(conn, "Change air filter", 0, 0, BASE_MS);
    link_caldav(conn, h_5, "list-home", "h-filter");
    tag_task(conn, h_5, "tag-2", "deep-work");

    // Shopping list: 3 roots, flat.
    for name in ["Milk", "Eggs", "Coffee"].iter() {
        let id = insert_task(conn, name, 0, 0, BASE_MS);
        link_caldav(
            conn,
            id,
            "list-shopping",
            &format!("sh-{}", name.to_lowercase()),
        );
    }

    // Also a saved filter row so openDatabase's sidebar query doesn't
    // degenerate (not used by recursive-query tests but keeps the
    // fixture representative of a real install).
    conn.execute(
        "INSERT INTO filters (title, sql, f_color, f_icon, f_order) \
         VALUES ('High priority', 'WHERE tasks.importance = 0', 0, 'flag', 1)",
        [],
    )
    .unwrap();

    // Active top-level count by hand: work_root + work_other +
    //   h_4 + h_5 + 3 shopping = 7.
    7
}

fn insert_task(conn: &Connection, title: &str, parent: i64, due: i64, created: i64) -> i64 {
    conn.execute(
        "INSERT INTO tasks (title, parent, dueDate, created, modified, hideUntil) \
         VALUES (?1, ?2, ?3, ?4, ?4, 0)",
        params![title, parent, due, created],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn link_caldav(conn: &Connection, task: i64, calendar: &str, remote_id: &str) {
    conn.execute(
        "INSERT INTO caldav_tasks (cd_task, cd_calendar, cd_remote_id, cd_object, \
         cd_last_sync, cd_deleted, gt_moved, gt_remote_order) \
         VALUES (?1, ?2, ?3, ?3, 0, 0, 0, 0)",
        params![task, calendar, remote_id],
    )
    .unwrap();
}

fn tag_task(conn: &Connection, task: i64, tag_uid: &str, name: &str) {
    conn.execute(
        "INSERT INTO tags (task, name, tag_uid, task_uid) VALUES (?1, ?2, ?3, NULL)",
        params![task, name, tag_uid],
    )
    .unwrap();
}

#[test]
fn caldav_work_list_yields_full_subtree_including_grandchild() {
    let conn = Connection::open_in_memory().unwrap();
    populate(&conn);

    let filter = QueryFilter::caldav("list-work");
    let prefs = QueryPreferences::default();
    let sql = build_recursive_query(&filter, &prefs, BASE_MS, None);
    let mut stmt = conn
        .prepare(&sql)
        .unwrap_or_else(|e| panic!("prepare: {e}\nSQL:\n{sql}"));
    let titles: Vec<String> = stmt
        .query_map([], |row| row.get::<_, Option<String>>("title"))
        .unwrap()
        .filter_map(|r| r.ok().flatten())
        .collect();

    // list-work has 5 tasks (root, outline, review, room, status) and
    // none are completed / deleted / hidden.
    assert_eq!(titles.len(), 5, "got {titles:?}");
    assert!(titles.contains(&"Ship Q1 plan".to_string()));
    assert!(titles.contains(&"Book the room".to_string()));
    assert!(!titles.contains(&"Cancel cable".to_string())); // list-home
    assert!(!titles.contains(&"Milk".to_string())); // list-shopping
}

#[test]
fn caldav_home_list_excludes_completed_deleted_and_hidden_in_future() {
    let conn = Connection::open_in_memory().unwrap();
    populate(&conn);

    let filter = QueryFilter::caldav("list-home");
    let prefs = QueryPreferences::default();
    let sql = build_recursive_query(&filter, &prefs, BASE_MS, None);
    let mut stmt = conn.prepare(&sql).unwrap();
    let titles: Vec<String> = stmt
        .query_map([], |row| row.get::<_, Option<String>>("title"))
        .unwrap()
        .filter_map(|r| r.ok().flatten())
        .collect();

    // Fix sink (completed), Cancel cable (deleted), Plan trip (hidden
    // until next month) all excluded; Buy houseplant + Change air
    // filter remain.
    assert_eq!(titles.len(), 2, "got {titles:?}");
    assert!(titles.contains(&"Buy houseplant".to_string()));
    assert!(titles.contains(&"Change air filter".to_string()));
}

#[test]
fn subtree_indent_increases_one_per_level() {
    let conn = Connection::open_in_memory().unwrap();
    populate(&conn);

    let filter = QueryFilter::caldav("list-work");
    let prefs = QueryPreferences::default();
    let sql = build_recursive_query(&filter, &prefs, BASE_MS, None);
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows: Vec<(String, i32)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>("title")?.unwrap_or_default(),
                row.get::<_, i32>("indent")?,
            ))
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    // Deepest grandchild is indent 2 ("Ship Q1 plan" 0 → "Review"
    // 1 → "Book the room" 2). `Weekly status` is a sibling root at
    // indent 0.
    let max_indent = rows.iter().map(|(_, d)| *d).max().unwrap_or(-1);
    assert_eq!(max_indent, 2, "expected 3-level tree; rows: {rows:?}");

    let indent_of =
        |title: &str| -> Option<i32> { rows.iter().find(|(t, _)| t == title).map(|(_, d)| *d) };
    assert_eq!(indent_of("Ship Q1 plan"), Some(0));
    assert_eq!(indent_of("Draft outline"), Some(1));
    assert_eq!(indent_of("Review with lead"), Some(1));
    assert_eq!(indent_of("Book the room"), Some(2));
    assert_eq!(indent_of("Weekly status"), Some(0));
}

#[test]
fn completed_at_bottom_sorts_them_after_active() {
    let conn = Connection::open_in_memory().unwrap();
    populate(&conn);

    // Query the whole DB (not scoped to a CalDAV list) so the sort
    // prelude has meaningful input. Show completed too so the "Fix
    // sink" row actually appears.
    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        completed_tasks_at_bottom: true,
        show_completed: true,
        sort_mode: SORT_COMPLETED,
        ..QueryPreferences::default()
    };
    let sql = build_recursive_query(&filter, &prefs, BASE_MS, None);

    let mut stmt = conn
        .prepare(&sql)
        .unwrap_or_else(|e| panic!("prepare: {e}\nSQL:\n{sql}"));
    let rows: Vec<(String, bool)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>("title")?.unwrap_or_default(),
                row.get::<_, i64>("parent_complete")? > 0,
            ))
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    // Find "Fix sink" (completed) — should appear only at or after the
    // last active (uncompleted) row.
    let last_active = rows.iter().rposition(|(_, done)| !done);
    let fix_sink = rows.iter().position(|(t, _)| t == "Fix sink");
    if let (Some(l), Some(f)) = (last_active, fix_sink) {
        assert!(
            f > l,
            "`Fix sink` should sort after the last active task; rows: {rows:?}"
        );
    } else {
        panic!("expected both `Fix sink` and at least one active task in {rows:?}");
    }
}

#[test]
fn sort_due_puts_earliest_due_first_among_active_roots() {
    let conn = Connection::open_in_memory().unwrap();
    populate(&conn);

    let filter = QueryFilter::custom("WHERE tasks.parent = 0");
    let prefs = QueryPreferences {
        sort_mode: SORT_DUE,
        ..QueryPreferences::default()
    };
    let sql = build_recursive_query(&filter, &prefs, BASE_MS, None);
    let mut stmt = conn.prepare(&sql).unwrap();
    // Roots only (indent = 0, non-zero dueDate). The recursive query
    // interleaves children with their parents; the primary sort only
    // applies to the top-level rows.
    let roots: Vec<(String, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>("title")?.unwrap_or_default(),
                row.get::<_, i64>("dueDate")?,
                row.get::<_, i32>("indent")?,
            ))
        })
        .unwrap()
        .filter_map(Result::ok)
        .filter(|(_, due, indent)| *due > 0 && *indent == 0)
        .map(|(t, due, _)| (t, due))
        .collect();

    // Ensure the dated-root ordering is monotonic non-decreasing.
    for pair in roots.windows(2) {
        assert!(
            pair[0].1 <= pair[1].1,
            "due-date sort should be monotonic across roots; got {roots:?}"
        );
    }
}
