//! Integration tests for `tasks_core::write`.
//!
//! Stand up a fresh schema via `Database::open_or_create_read_only`,
//! insert a couple of task rows directly, then exercise the
//! completion/delete helpers and re-read through a fresh read-only
//! handle to verify the mutations land (and stay scoped to the
//! targeted row).

use rusqlite::params;
use tasks_core::db::Database;
use tasks_core::write::{
    set_task_completion, set_task_deleted, update_task_fields, GeofenceEdit, TaskEdit,
};

const NOW: i64 = 1_700_000_000_000;

/// Minimal TaskEdit for tests that only care about the scalar task
/// columns. Individual tests override the fields they're asserting
/// on. Keeps each test's literal small as the struct grows across
/// M2 Phase C commits.
fn edit_default<'a>(title: &'a str, notes: &'a str) -> TaskEdit<'a> {
    TaskEdit {
        title,
        notes,
        due_ms: 0,
        hide_until_ms: 0,
        priority: 3,
        caldav_calendar_uuid: None,
        tag_uids: None,
        alarms: None,
        geofence: None,
    }
}

fn seed_two_tasks(db_path: &std::path::Path) -> (i64, i64) {
    // The importer does full row INSERTs; here we shortcut with the
    // minimum column set the completion/delete helpers care about.
    // The Room schema has NOT NULL defaults on everything else.
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
         modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
         timerStart, notificationFlags, lastNotified, repeat_from, \
         collapsed, parent, read_only) \
         VALUES ('A', 3, 0, 0, ?1, ?1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)",
        params![NOW - 1000],
    )
    .unwrap();
    let a = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO tasks (title, importance, dueDate, hideUntil, created, \
         modified, completed, deleted, estimatedSeconds, elapsedSeconds, \
         timerStart, notificationFlags, lastNotified, repeat_from, \
         collapsed, parent, read_only) \
         VALUES ('B', 3, 0, 0, ?1, ?1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)",
        params![NOW - 1000],
    )
    .unwrap();
    let b = conn.last_insert_rowid();
    (a, b)
}

#[test]
fn complete_then_uncomplete_updates_only_target_row() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, b) = seed_two_tasks(&db_path);

    assert!(set_task_completion(&db_path, a, true, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let conn = db.connection();
    let (completed_a, modified_a): (i64, i64) = conn
        .query_row(
            "SELECT completed, modified FROM tasks WHERE _id = ?1",
            params![a],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(completed_a, NOW, "A should now be completed at NOW");
    assert_eq!(modified_a, NOW, "A's modified should be bumped");

    let completed_b: i64 = conn
        .query_row(
            "SELECT completed FROM tasks WHERE _id = ?1",
            params![b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(completed_b, 0, "B must not be touched");
    drop(db);

    // Undo: set_task_completion(false) clears the timestamp.
    assert!(set_task_completion(&db_path, a, false, NOW + 1).unwrap());
    let db = Database::open_read_only(&db_path).unwrap();
    let (completed_a, modified_a): (i64, i64) = db
        .connection()
        .query_row(
            "SELECT completed, modified FROM tasks WHERE _id = ?1",
            params![a],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(completed_a, 0);
    assert_eq!(modified_a, NOW + 1);
}

#[test]
fn delete_marks_only_target_row() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, b) = seed_two_tasks(&db_path);

    assert!(set_task_deleted(&db_path, b, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let (deleted_b, modified_b): (i64, i64) = db
        .connection()
        .query_row(
            "SELECT deleted, modified FROM tasks WHERE _id = ?1",
            params![b],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(deleted_b, NOW);
    assert_eq!(modified_b, NOW);

    let deleted_a: i64 = db
        .connection()
        .query_row(
            "SELECT deleted FROM tasks WHERE _id = ?1",
            params![a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted_a, 0);
}

#[test]
fn unknown_id_returns_false_without_error() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());

    assert!(!set_task_completion(&db_path, 9_999_999, true, NOW).unwrap());
    assert!(!set_task_deleted(&db_path, 9_999_999, NOW).unwrap());
    assert!(!update_task_fields(&db_path, 9_999_999, &edit_default("x", "y"), NOW).unwrap());
}

#[test]
fn update_task_fields_writes_every_column() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, b) = seed_two_tasks(&db_path);

    let due = 1_700_000_000_000;
    let hide = 1_699_000_000_000;
    let edit = TaskEdit {
        title: "New title",
        notes: "New notes",
        due_ms: due,
        hide_until_ms: hide,
        priority: 1, // MEDIUM
        caldav_calendar_uuid: None,
        tag_uids: None,
        alarms: None,
        geofence: None,
    };
    let updated = update_task_fields(&db_path, a, &edit, NOW).unwrap();
    assert!(updated);

    let db = Database::open_read_only(&db_path).unwrap();
    let (title, notes, due_read, hide_read, importance, modified): (
        String,
        String,
        i64,
        i64,
        i32,
        i64,
    ) = db
        .connection()
        .query_row(
            "SELECT title, notes, dueDate, hideUntil, importance, modified \
             FROM tasks WHERE _id = ?1",
            params![a],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(title, "New title");
    assert_eq!(notes, "New notes");
    assert_eq!(due_read, due);
    assert_eq!(hide_read, hide);
    assert_eq!(importance, 1);
    assert_eq!(modified, NOW);

    // Task B must not be touched.
    let (title_b, modified_b): (String, i64) = db
        .connection()
        .query_row(
            "SELECT title, modified FROM tasks WHERE _id = ?1",
            params![b],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(title_b, "B");
    assert_eq!(modified_b, NOW - 1000, "B's modified should not change");
}

#[test]
fn update_task_fields_clears_notes_to_null_when_empty() {
    // Empty notes string must persist as NULL so `notes IS NULL`
    // checks in Android-side Kotlin stay correct after sync.
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, _) = seed_two_tasks(&db_path);

    // Seed with non-empty notes so the clear is observable.
    update_task_fields(&db_path, a, &edit_default("A", "some notes"), NOW).unwrap();
    // Now clear.
    update_task_fields(&db_path, a, &edit_default("A", ""), NOW + 1).unwrap();

    let db = Database::open_read_only(&db_path).unwrap();
    let notes_is_null: bool = db
        .connection()
        .query_row(
            "SELECT notes IS NULL FROM tasks WHERE _id = ?1",
            params![a],
            |r| r.get(0),
        )
        .unwrap();
    assert!(notes_is_null, "empty notes must round-trip as NULL");
}

#[test]
fn update_task_fields_reassigns_caldav_calendar() {
    // Seed a task plus a caldav_tasks row pointing at calendar "one".
    // An update carrying caldav_calendar_uuid = "two" should
    // reassign. A task without a caldav_tasks row should be a no-op
    // (local task stays local).
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (caldav_task, local_task) = seed_two_tasks(&db_path);

    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'one', 'uid-1', 0, 0, 0, 0)",
            params![caldav_task],
        )
        .unwrap();
    }

    let edit = TaskEdit {
        title: "A",
        notes: "",
        due_ms: 0,
        hide_until_ms: 0,
        priority: 3,
        caldav_calendar_uuid: Some("two"),
        tag_uids: None,
        alarms: None,
        geofence: None,
    };
    assert!(update_task_fields(&db_path, caldav_task, &edit, NOW).unwrap());
    assert!(update_task_fields(&db_path, local_task, &edit, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let calendar: String = db
        .connection()
        .query_row(
            "SELECT cd_calendar FROM caldav_tasks WHERE cd_task = ?1",
            params![caldav_task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(calendar, "two", "caldav-backed task should move");

    let local_rows: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM caldav_tasks WHERE cd_task = ?1",
            params![local_task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(local_rows, 0, "local task must not sprout a caldav row");
}

#[test]
fn update_task_fields_empty_caldav_uuid_is_noop() {
    // UI passes empty string to mean "no change"; we must not
    // accidentally clear the existing cd_calendar.
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, _) = seed_two_tasks(&db_path);
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO caldav_tasks \
             (cd_task, cd_calendar, cd_remote_id, cd_last_sync, cd_deleted, \
              gt_moved, gt_remote_order) \
             VALUES (?1, 'original', 'uid-a', 0, 0, 0, 0)",
            params![a],
        )
        .unwrap();
    }

    let edit = TaskEdit {
        title: "A",
        notes: "",
        due_ms: 0,
        hide_until_ms: 0,
        priority: 3,
        caldav_calendar_uuid: Some(""),
        tag_uids: None,
        alarms: None,
        geofence: None,
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let calendar: String = db
        .connection()
        .query_row(
            "SELECT cd_calendar FROM caldav_tasks WHERE cd_task = ?1",
            params![a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(calendar, "original", "empty uuid must not overwrite");
}

#[test]
fn update_task_fields_replaces_tag_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, _) = seed_two_tasks(&db_path);

    // Seed tagdata for two tags + a stale tag row pointing at task a.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE tasks SET remoteId = 'task-a-uid' WHERE _id = ?1",
            params![a],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tagdata (remoteId, name, color, td_order) \
             VALUES ('tag-one', 'Work', 0, 0), ('tag-two', 'Home', 0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tags (task, name, tag_uid, task_uid) \
             VALUES (?1, 'Stale', 'tag-stale', 'task-a-uid')",
            params![a],
        )
        .unwrap();
    }

    // Set tags to exactly ['tag-one', 'tag-two']. Stale row should be
    // gone, and the two new rows should carry the looked-up names.
    let wanted = vec!["tag-one".to_string(), "tag-two".to_string()];
    let edit = TaskEdit {
        tag_uids: Some(&wanted),
        ..edit_default("A", "")
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let mut stmt = db
        .connection()
        .prepare("SELECT name, tag_uid, task_uid FROM tags WHERE task = ?1 ORDER BY tag_uid")
        .unwrap();
    let rows: Vec<(String, String, String)> = stmt
        .query_map(params![a], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "Work".to_string(),
                "tag-one".to_string(),
                "task-a-uid".to_string()
            ),
            (
                "Home".to_string(),
                "tag-two".to_string(),
                "task-a-uid".to_string()
            ),
        ]
    );

    // Empty slice clears all tags.
    let empty: Vec<String> = Vec::new();
    let edit = TaskEdit {
        tag_uids: Some(&empty),
        ..edit_default("A", "")
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW + 1).unwrap());
    let db = Database::open_read_only(&db_path).unwrap();
    let count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM tags WHERE task = ?1",
            params![a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn update_task_fields_replaces_alarms() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, _) = seed_two_tasks(&db_path);

    // Seed a stale alarm pointing at task a.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO alarms (task, time, type, repeat, interval) \
             VALUES (?1, 0, 3, 0, 60000)",
            params![a],
        )
        .unwrap();
    }

    let wanted = vec![
        (-30 * 60 * 1000_i64, 2_i32),   // 30 min before due (REL_END)
        (1_700_000_000_000_i64, 0_i32), // absolute DATE_TIME
    ];
    let edit = TaskEdit {
        alarms: Some(&wanted),
        ..edit_default("A", "")
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let mut stmt = db
        .connection()
        .prepare("SELECT time, type FROM alarms WHERE task = ?1 ORDER BY time")
        .unwrap();
    let rows: Vec<(i64, i32)> = stmt
        .query_map(params![a], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    // Sorted ASC by time → (-30m) first, absolute epoch second.
    assert_eq!(rows[0], (-30 * 60 * 1000, 2));
    assert_eq!(rows[1], (1_700_000_000_000, 0));

    // Clear.
    let empty: Vec<(i64, i32)> = Vec::new();
    let edit = TaskEdit {
        alarms: Some(&empty),
        ..edit_default("A", "")
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW + 1).unwrap());
    let db = Database::open_read_only(&db_path).unwrap();
    let n: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM alarms WHERE task = ?1",
            params![a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn update_task_fields_writes_and_clears_geofence() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    let (a, _) = seed_two_tasks(&db_path);

    // Seed a place so the INSERT has somewhere to point.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO places \
             (uid, name, latitude, longitude, place_color, place_order, radius) \
             VALUES ('home', 'Home', 37.77, -122.42, 0, 0, 250)",
            [],
        )
        .unwrap();
    }

    let edit = TaskEdit {
        geofence: Some(GeofenceEdit {
            place_uid: "home",
            arrival: true,
            departure: false,
        }),
        ..edit_default("A", "")
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let (place, arrival, departure): (String, i32, i32) = db
        .connection()
        .query_row(
            "SELECT place, arrival, departure FROM geofences WHERE task = ?1",
            params![a],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(place, "home");
    assert_eq!(arrival, 1);
    assert_eq!(departure, 0);

    // Empty place_uid clears the geofence.
    let edit = TaskEdit {
        geofence: Some(GeofenceEdit {
            place_uid: "",
            arrival: false,
            departure: false,
        }),
        ..edit_default("A", "")
    };
    assert!(update_task_fields(&db_path, a, &edit, NOW + 1).unwrap());

    let db = Database::open_read_only(&db_path).unwrap();
    let count: i64 = db
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM geofences WHERE task = ?1",
            params![a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
