//! Round-trip tests for each entity added to `tasks-core::models`.
//!
//! Each test creates the minimum subset of the Room schema 92 needed for the
//! entity under test, inserts a representative row via raw SQL, then reads
//! it back through the corresponding `from_row` helper and asserts the
//! fields come back unchanged. Column names, types, and defaults here must
//! match `data/schemas/org.tasks.data.db.Database/92.json`.

use rusqlite::Connection;

use tasks_core::models::{
    AccountType, Alarm, AlarmType, CaldavAccount, CaldavCalendar, CaldavTask, CalendarAccess,
    Filter, Geofence, Place, Tag, TagData,
};

fn open_mem() -> Connection {
    Connection::open_in_memory().expect("open in-memory sqlite")
}

#[test]
fn tag_and_tagdata_round_trip() {
    let conn = open_mem();
    conn.execute_batch(
        r#"
        CREATE TABLE tagdata (
            _id INTEGER PRIMARY KEY AUTOINCREMENT,
            remoteId TEXT,
            name TEXT,
            color INTEGER,
            tagOrdering TEXT,
            td_icon TEXT,
            td_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE tags (
            _id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            task INTEGER NOT NULL,
            name TEXT,
            tag_uid TEXT,
            task_uid TEXT
        );
        INSERT INTO tagdata (remoteId, name, color, tagOrdering, td_icon, td_order)
            VALUES ('uid-1', 'Errands', 42, '[]', 'shopping_cart', 3);
        INSERT INTO tags (task, name, tag_uid, task_uid)
            VALUES (100, 'Errands', 'uid-1', 'task-uid-1');
        "#,
    )
    .unwrap();

    let td: TagData = conn
        .query_row("SELECT * FROM tagdata", [], TagData::from_row)
        .unwrap();
    assert_eq!(td.name.as_deref(), Some("Errands"));
    assert_eq!(td.remote_id.as_deref(), Some("uid-1"));
    assert_eq!(td.color, Some(42));
    assert_eq!(td.icon.as_deref(), Some("shopping_cart"));
    assert_eq!(td.order, 3);

    let tag: Tag = conn
        .query_row("SELECT * FROM tags", [], Tag::from_row)
        .unwrap();
    assert_eq!(tag.task, 100);
    assert_eq!(tag.tag_uid.as_deref(), Some("uid-1"));
    assert_eq!(tag.task_uid.as_deref(), Some("task-uid-1"));
}

#[test]
fn alarm_round_trip_preserves_type_constants() {
    let conn = open_mem();
    conn.execute_batch(
        r#"
        CREATE TABLE alarms (
            _id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            task INTEGER NOT NULL,
            time INTEGER NOT NULL,
            type INTEGER NOT NULL DEFAULT 0,
            repeat INTEGER NOT NULL DEFAULT 0,
            interval INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO alarms (task, time, type, repeat, interval)
            VALUES (7, 1700000000000, 2, 3, 86400000);
        "#,
    )
    .unwrap();

    let alarm: Alarm = conn
        .query_row("SELECT * FROM alarms", [], Alarm::from_row)
        .unwrap();
    assert_eq!(alarm.task, 7);
    assert_eq!(alarm.time, 1_700_000_000_000);
    assert_eq!(alarm.alarm_type, AlarmType::REL_END);
    assert_eq!(alarm.repeat, 3);
    assert_eq!(alarm.interval, 86_400_000);
}

#[test]
fn place_and_geofence_round_trip() {
    let conn = open_mem();
    conn.execute_batch(
        r#"
        CREATE TABLE places (
            place_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            uid TEXT,
            name TEXT,
            address TEXT,
            phone TEXT,
            url TEXT,
            latitude REAL NOT NULL,
            longitude REAL NOT NULL,
            place_color INTEGER NOT NULL,
            place_icon TEXT,
            place_order INTEGER NOT NULL,
            radius INTEGER NOT NULL DEFAULT 250
        );
        CREATE TABLE geofences (
            geofence_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            task INTEGER NOT NULL,
            place TEXT,
            arrival INTEGER NOT NULL,
            departure INTEGER NOT NULL
        );
        INSERT INTO places (uid, name, address, latitude, longitude, place_color, place_order)
            VALUES ('place-1', 'Home', '123 Main St', 37.77, -122.42, 0, -1);
        INSERT INTO places (uid, name, address, latitude, longitude, place_color, place_order)
            VALUES ('place-2', NULL, NULL, 0.0, 0.0, 0, -1);
        INSERT INTO geofences (task, place, arrival, departure) VALUES (42, 'place-1', 1, 0);
        "#,
    )
    .unwrap();

    let place: Place = conn
        .query_row(
            "SELECT * FROM places WHERE uid = 'place-1'",
            [],
            Place::from_row,
        )
        .unwrap();
    assert_eq!(place.name.as_deref(), Some("Home"));
    assert_eq!(place.latitude, 37.77);
    assert_eq!(place.radius, 250);
    assert_eq!(place.display_name(), "Home");

    let coord_place: Place = conn
        .query_row(
            "SELECT * FROM places WHERE uid = 'place-2'",
            [],
            Place::from_row,
        )
        .unwrap();
    // No name, no address — display falls back to the lat/long.
    assert!(coord_place.display_name().contains("0.000000"));

    let fence: Geofence = conn
        .query_row("SELECT * FROM geofences", [], Geofence::from_row)
        .unwrap();
    assert_eq!(fence.task, 42);
    assert_eq!(fence.place.as_deref(), Some("place-1"));
    assert!(fence.arrival);
    assert!(!fence.departure);
}

#[test]
fn filter_round_trip() {
    let conn = open_mem();
    conn.execute_batch(
        r#"
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
        INSERT INTO filters (title, sql, criterion, f_color, f_icon, f_order)
            VALUES ('High priority', 'WHERE tasks.importance = 0', 'urgent', 0xFF0000, 'flag', 1);
        "#,
    )
    .unwrap();

    let filter: Filter = conn
        .query_row("SELECT * FROM filters", [], Filter::from_row)
        .unwrap();
    assert_eq!(filter.title.as_deref(), Some("High priority"));
    assert_eq!(filter.sql.as_deref(), Some("WHERE tasks.importance = 0"));
    assert_eq!(filter.icon.as_deref(), Some("flag"));
    assert_eq!(filter.order, 1);
}

#[test]
fn caldav_entities_round_trip() {
    let conn = open_mem();
    conn.execute_batch(
        r#"
        CREATE TABLE caldav_accounts (
            cda_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            cda_uuid TEXT,
            cda_name TEXT,
            cda_url TEXT,
            cda_username TEXT,
            cda_password TEXT,
            cda_error TEXT,
            cda_account_type INTEGER NOT NULL,
            cda_collapsed INTEGER NOT NULL,
            cda_server_type INTEGER NOT NULL,
            cda_last_sync INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE caldav_lists (
            cdl_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            cdl_account TEXT,
            cdl_uuid TEXT,
            cdl_name TEXT,
            cdl_color INTEGER NOT NULL,
            cdl_ctag TEXT,
            cdl_url TEXT,
            cdl_icon TEXT,
            cdl_order INTEGER NOT NULL,
            cdl_access INTEGER NOT NULL,
            cdl_last_sync INTEGER NOT NULL
        );
        CREATE TABLE caldav_tasks (
            cd_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
            cd_task INTEGER NOT NULL,
            cd_calendar TEXT,
            cd_remote_id TEXT,
            cd_object TEXT,
            cd_etag TEXT,
            cd_last_sync INTEGER NOT NULL,
            cd_deleted INTEGER NOT NULL,
            cd_remote_parent TEXT,
            gt_moved INTEGER NOT NULL,
            gt_remote_order INTEGER NOT NULL
        );
        INSERT INTO caldav_accounts
            (cda_uuid, cda_name, cda_url, cda_username, cda_account_type,
             cda_collapsed, cda_server_type, cda_last_sync)
            VALUES ('acct-1', 'Nextcloud', 'https://cloud.example/caldav', 'alice', 0, 0, 4, 0);
        INSERT INTO caldav_lists
            (cdl_account, cdl_uuid, cdl_name, cdl_color, cdl_url, cdl_order, cdl_access, cdl_last_sync)
            VALUES ('acct-1', 'list-1', 'Work', 255, 'https://cloud.example/caldav/work/', 0, 2, 0);
        INSERT INTO caldav_tasks
            (cd_task, cd_calendar, cd_remote_id, cd_etag, cd_last_sync, cd_deleted, gt_moved, gt_remote_order)
            VALUES (5, 'list-1', 'vtodo-1', '"etag"', 0, 0, 0, 17);
        "#,
    )
    .unwrap();

    let acct: CaldavAccount = conn
        .query_row("SELECT * FROM caldav_accounts", [], CaldavAccount::from_row)
        .unwrap();
    assert!(acct.is_caldav());
    assert_eq!(acct.account_type, AccountType::CALDAV);
    assert_eq!(acct.username.as_deref(), Some("alice"));

    let cal: CaldavCalendar = conn
        .query_row("SELECT * FROM caldav_lists", [], CaldavCalendar::from_row)
        .unwrap();
    assert_eq!(cal.name.as_deref(), Some("Work"));
    assert_eq!(cal.access, CalendarAccess::READ_ONLY);
    assert!(cal.is_read_only());

    let ct: CaldavTask = conn
        .query_row("SELECT * FROM caldav_tasks", [], CaldavTask::from_row)
        .unwrap();
    assert_eq!(ct.task, 5);
    assert_eq!(ct.remote_id.as_deref(), Some("vtodo-1"));
    assert_eq!(ct.remote_order, 17);
    assert!(!ct.is_deleted());
}
