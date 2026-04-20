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
fn import_missing_file_reports_io_error() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    drop(Database::open_or_create_read_only(&db_path).unwrap());

    let missing: PathBuf = tmp.path().join("does-not-exist.json");
    let err = import_json_backup(&db_path, &missing).expect_err("should fail");
    assert!(err.to_string().contains("read"), "err = {err}");
}
