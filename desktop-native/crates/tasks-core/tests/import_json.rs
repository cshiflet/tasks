//! Integration test for `tasks_core::import::import_json_backup`.
//!
//! Builds a minimal JSON payload in the shape Tasks.org's
//! `TasksJsonExporter.kt` produces, runs the importer against a
//! fresh DB materialised by `Database::open_or_create_read_only`,
//! and verifies the expected rows land in each table.

use std::path::PathBuf;

use rusqlite::Connection;
use tasks_core::db::Database;
use tasks_core::import::{import_json_backup, ImportStats};

fn sample_backup() -> serde_json::Value {
    serde_json::json!({
        "version": 140200,
        "timestamp": 1_700_000_000_000_i64,
        "data": {
            "tasks": [
                {
                    "task": {
                        "title": "Ship Q1 plan",
                        "priority": 1,
                        "dueDate": 1_700_000_000_000_i64,
                        "hideUntil": 0,
                        "creationDate": 1_699_900_000_000_i64,
                        "modificationDate": 1_699_900_000_000_i64,
                        "completionDate": 0,
                        "deletionDate": 0,
                        "notes": "Outline + review",
                        "estimatedSeconds": 0,
                        "elapsedSeconds": 0,
                        "timerStart": 0,
                        "ringFlags": 0,
                        "reminderLast": 0,
                        "recurrence": null,
                        "repeatFrom": 0,
                        "calendarURI": null,
                        "remoteId": "ship-q1-uid",
                        "isCollapsed": false,
                        "order": null,
                        "readOnly": false
                    },
                    "alarms": [
                        { "time": 3_600_000, "type": 1, "repeat": 0, "interval": 0 }
                    ],
                    "geofences": [],
                    "tags": [
                        { "name": "urgent", "tagUid": "tag-urgent" }
                    ],
                    "caldavTasks": [
                        {
                            "calendar": "list-work",
                            "remoteId": "ship-q1-uid",
                            "object": "ship-q1-uid.ics",
                            "etag": "\"etag-1\"",
                            "lastSync": 1_700_000_000_000_i64,
                            "deleted": 0,
                            "remoteParent": null
                        }
                    ],
                    "attachments": [
                        { "ignore": "me" }
                    ],
                    "comments": [
                        { "also": "ignored" }
                    ]
                },
                {
                    "task": {
                        "title": "Buy milk",
                        "priority": 3,
                        "remoteId": "buy-milk-uid"
                    }
                }
            ],
            "places": [
                {
                    "uid": "place-home",
                    "name": "Home",
                    "address": "1 Main St",
                    "latitude": 37.77,
                    "longitude": -122.42,
                    "color": 0,
                    "order": -1,
                    "radius": 300
                }
            ],
            "tags": [
                { "remoteId": "tag-urgent", "name": "urgent", "color": 0, "order": 1 }
            ],
            "filters": [
                {
                    "title": "High priority",
                    "sql": "WHERE tasks.importance = 0",
                    "order": 1
                }
            ],
            "caldavAccounts": [
                {
                    "uuid": "acct-nextcloud",
                    "name": "Nextcloud",
                    "url": "https://cloud.example/caldav",
                    "accountType": 0,
                    "serverType": 4
                }
            ],
            "caldavCalendars": [
                {
                    "account": "acct-nextcloud",
                    "uuid": "list-work",
                    "name": "Work",
                    "color": 0,
                    "order": 0,
                    "access": 0,
                    "lastSync": 0
                }
            ]
        }
    })
}

#[test]
fn import_backup_populates_every_entity() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    let json_path = tmp.path().join("backup.json");

    // Bootstrap an empty schema, then drop the handle so the
    // importer's own writable connection doesn't contend.
    drop(Database::open_or_create_read_only(&db_path).unwrap());

    std::fs::write(&json_path, sample_backup().to_string()).unwrap();
    let stats = import_json_backup(&db_path, &json_path).unwrap();

    // Row counts match the fixture.
    assert_eq!(
        stats,
        ImportStats {
            tasks: 2,
            alarms: 1,
            geofences: 0,
            tags: 1,
            caldav_tasks: 1,
            places: 1,
            tag_data: 1,
            filters: 1,
            caldav_accounts: 1,
            caldav_calendars: 1,
            skipped_attachments: 1,
            skipped_comments: 1,
            // No remote_parent in this fixture.
            subtask_links: 0,
        },
        "row counts should match the fixture"
    );

    // Spot-check through a fresh read-only open.
    let db = Database::open_read_only(&db_path).unwrap();
    let conn = db.connection();

    let task_title: String = conn
        .query_row(
            "SELECT title FROM tasks WHERE remoteId = 'ship-q1-uid'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_title, "Ship Q1 plan");

    // Alarm points at the newly-inserted task id, not the original.
    let alarm_points_at_right_task: String = conn
        .query_row(
            "SELECT tasks.remoteId FROM alarms \
             JOIN tasks ON tasks._id = alarms.task \
             WHERE alarms.type = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alarm_points_at_right_task, "ship-q1-uid");

    // Tag (join row) references the imported task + the tagdata uid.
    let tag_task_uid: String = conn
        .query_row(
            "SELECT task_uid FROM tags WHERE tag_uid = 'tag-urgent'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tag_task_uid, "ship-q1-uid");

    // Idempotent: importing the same backup a second time should be
    // a no-op for top-level (INSERT OR REPLACE), and should duplicate
    // tasks (they don't have a natural key we use for replace). This
    // is worth pinning so we don't accidentally de-dupe tasks later.
    let stats2 = import_json_backup(&db_path, &json_path).unwrap();
    assert_eq!(stats2.tasks, 2);
    assert_eq!(stats2.places, 1);
}

#[test]
fn import_rolls_back_on_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    let json_path = tmp.path().join("backup.json");
    drop(Database::open_or_create_read_only(&db_path).unwrap());

    // Valid JSON but wrong shape — data.places[0].latitude is a
    // string instead of a number. Serde should reject before any
    // INSERT runs.
    std::fs::write(
        &json_path,
        r#"{"version":1,"timestamp":0,"data":{"places":[{"latitude":"nope"}]}}"#,
    )
    .unwrap();
    let err = import_json_backup(&db_path, &json_path).expect_err("bad input");
    assert!(err.to_string().contains("parse"), "err = {err}");

    // DB is unchanged.
    let conn = Connection::open(&db_path).unwrap();
    let places: i64 = conn
        .query_row("SELECT count(*) FROM places", [], |r| r.get(0))
        .unwrap();
    assert_eq!(places, 0);
}

#[test]
fn import_restores_parent_child_subtask_links() {
    // Three tasks in one calendar:
    //   root   → no remoteParent
    //   child  → remoteParent = root's cd_remote_id
    //   orphan → remoteParent = "missing-uid" (not in this backup)
    //
    // After import we expect tasks.parent to be:
    //   root   → 0
    //   child  → root's new _id
    //   orphan → 0  (parent UID doesn't resolve; log+skip)
    let payload = serde_json::json!({
        "version": 140200,
        "timestamp": 1_700_000_000_000_i64,
        "data": {
            "tasks": [
                {
                    "task": { "title": "root", "priority": 3, "remoteId": "root-uid" },
                    "caldavTasks": [{ "calendar": "list-1", "remoteId": "root-uid" }]
                },
                {
                    "task": { "title": "child", "priority": 3, "remoteId": "child-uid" },
                    "caldavTasks": [{
                        "calendar": "list-1",
                        "remoteId": "child-uid",
                        "remoteParent": "root-uid"
                    }]
                },
                {
                    "task": { "title": "orphan", "priority": 3, "remoteId": "orphan-uid" },
                    "caldavTasks": [{
                        "calendar": "list-1",
                        "remoteId": "orphan-uid",
                        "remoteParent": "missing-uid"
                    }]
                }
            ],
            "caldavCalendars": [
                { "uuid": "list-1", "name": "Work", "color": 0, "order": 0,
                  "access": 0, "lastSync": 0 }
            ]
        }
    });

    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    let json_path = tmp.path().join("backup.json");
    drop(Database::open_or_create_read_only(&db_path).unwrap());
    std::fs::write(&json_path, payload.to_string()).unwrap();

    let stats = import_json_backup(&db_path, &json_path).unwrap();
    assert_eq!(stats.tasks, 3);
    assert_eq!(stats.caldav_tasks, 3);
    assert_eq!(
        stats.subtask_links, 1,
        "only child→root should be linked; orphan's target isn't in the backup"
    );

    let db = Database::open_read_only(&db_path).unwrap();
    let conn = db.connection();

    let root_id: i64 = conn
        .query_row(
            "SELECT _id FROM tasks WHERE remoteId = 'root-uid'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let (child_parent, child_title): (i64, String) = conn
        .query_row(
            "SELECT parent, title FROM tasks WHERE remoteId = 'child-uid'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(child_title, "child");
    assert_eq!(
        child_parent, root_id,
        "child's parent should point at root's new _id"
    );

    let orphan_parent: i64 = conn
        .query_row(
            "SELECT parent FROM tasks WHERE remoteId = 'orphan-uid'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        orphan_parent, 0,
        "orphan stays flat when parent UID is absent"
    );

    let root_parent: i64 = conn
        .query_row(
            "SELECT parent FROM tasks WHERE remoteId = 'root-uid'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(root_parent, 0, "root has no parent");

    // Re-running the same backup should stay idempotent: task rows
    // INSERT-or-REPLACE by rowid (new _ids each time), but the
    // re-link pass picks the fresh ids up and stamps parent again.
    let stats2 = import_json_backup(&db_path, &json_path).unwrap();
    assert_eq!(stats2.subtask_links, 1);
}

#[test]
fn import_missing_file_reports_io_error() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());

    let missing: PathBuf = tmp.path().join("does-not-exist.json");
    let err = import_json_backup(&db_path, &missing).expect_err("should fail");
    assert!(err.to_string().contains("read"), "err = {err}");
}

/// C-1 regression: a hostile backup embeds an executable SQL fragment
/// in `filters.sql`. After import, that column must be NULL so the
/// saved-filter read path can't execute it.
#[test]
fn import_strips_filter_sql_fragment_against_c1() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    let json_path = tmp.path().join("backup.json");
    drop(Database::open_or_create_read_only(&db_path).unwrap());

    // The SQL fragment here is the exploit shape from C-1: an EXISTS
    // subquery over `caldav_accounts` that the Android recursive CTE
    // would happily splice in. We want it NOT to land in the DB.
    let hostile = serde_json::json!({
        "version": 140200,
        "timestamp": 1_700_000_000_000_i64,
        "data": {
            "filters": [{
                "title": "benign-looking",
                "sql": "EXISTS (SELECT 1 FROM caldav_accounts WHERE tasks.title = cda_password)",
                "criterion": "anything",
                "order": 1
            }]
        }
    });
    std::fs::write(&json_path, hostile.to_string()).unwrap();

    let stats = import_json_backup(&db_path, &json_path).unwrap();
    assert_eq!(stats.filters, 1, "filter row is still counted");

    let conn = Connection::open(&db_path).unwrap();
    let sql: Option<String> = conn
        .query_row("SELECT sql FROM filters", [], |r| r.get(0))
        .unwrap();
    assert!(
        sql.is_none(),
        "imported `sql` must be NULL after C-1 fix; got {:?}",
        sql
    );
    // `criterion` is preserved so a future trusted SQL-rebuilder has
    // what it needs to reconstruct the filter from structured data.
    let criterion: Option<String> = conn
        .query_row("SELECT criterion FROM filters", [], |r| r.get(0))
        .unwrap();
    assert_eq!(criterion.as_deref(), Some("anything"));
}
