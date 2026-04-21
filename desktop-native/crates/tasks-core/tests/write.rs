//! Integration tests for `tasks_core::write`.
//!
//! Stand up a fresh schema via `Database::open_or_create_read_only`,
//! insert a couple of task rows directly, then exercise the
//! completion/delete helpers and re-read through a fresh read-only
//! handle to verify the mutations land (and stay scoped to the
//! targeted row).

use rusqlite::params;
use tasks_core::db::Database;
use tasks_core::write::{set_task_completion, set_task_deleted, update_task_fields, TaskEdit};

const NOW: i64 = 1_700_000_000_000;

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
    let edit = TaskEdit {
        title: "x",
        notes: "y",
        due_ms: 0,
        hide_until_ms: 0,
        priority: 3,
    };
    assert!(!update_task_fields(&db_path, 9_999_999, &edit, NOW).unwrap());
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
    update_task_fields(
        &db_path,
        a,
        &TaskEdit {
            title: "A",
            notes: "some notes",
            due_ms: 0,
            hide_until_ms: 0,
            priority: 3,
        },
        NOW,
    )
    .unwrap();
    // Now clear.
    update_task_fields(
        &db_path,
        a,
        &TaskEdit {
            title: "A",
            notes: "",
            due_ms: 0,
            hide_until_ms: 0,
            priority: 3,
        },
        NOW + 1,
    )
    .unwrap();

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
