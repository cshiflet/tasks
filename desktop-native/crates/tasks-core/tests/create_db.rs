//! Verifies that `Database::open_or_create_read_only` can stand up a
//! fresh DB from the build-time-generated schema SQL and then open it
//! back cleanly — i.e. that the schema the build.rs emits is
//! structurally valid and that the identity hash it seeds matches
//! `PINNED_IDENTITY_HASH`.

use tasks_core::db::{Database, PINNED_IDENTITY_HASH, PINNED_SCHEMA_VERSION};
use tasks_core::query::{self, TaskFilter};

#[test]
fn open_or_create_creates_empty_db_on_first_call() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("nested/tasks-desktop/tasks.db");
    assert!(!db_path.exists(), "precondition: file shouldn't exist yet");

    let db = Database::open_or_create_read_only(&db_path)
        .expect("first open should create + initialise");

    assert!(db_path.exists(), "file should exist after first open");

    // Active filter on an empty DB returns zero rows — no panic, no
    // schema-mismatch error, just an empty list.
    let rows = query::run(&db, TaskFilter::Active, 0).expect("query an empty DB");
    assert_eq!(rows.len(), 0);
}

#[test]
fn open_or_create_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");

    // Create.
    let _ = Database::open_or_create_read_only(&db_path).unwrap();
    let sz_after_first = std::fs::metadata(&db_path).unwrap().len();

    // Second call on the same path should just open it, not re-init.
    let _ = Database::open_or_create_read_only(&db_path).unwrap();
    let sz_after_second = std::fs::metadata(&db_path).unwrap().len();
    assert_eq!(
        sz_after_first, sz_after_second,
        "second open must not rewrite the file"
    );
}

#[test]
fn generated_schema_produces_matching_identity_hash() {
    // The build.rs emits CREATE statements straight from Room's own
    // JSON and seeds `room_master_table` with PINNED_IDENTITY_HASH.
    // `open_read_only` then verifies the seeded value against the
    // same constant — this test exists so a bad copy-paste on
    // PINNED_IDENTITY_HASH breaks the build.
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("tasks.db");
    let _ = Database::open_or_create_read_only(&db_path).unwrap();

    // Re-read through a raw sqlite connection to double-check the
    // identity_hash row is what we think it is.
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let actual: String = conn
        .query_row(
            "SELECT identity_hash FROM room_master_table WHERE id = 42",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        actual, PINNED_IDENTITY_HASH,
        "schema v{PINNED_SCHEMA_VERSION} identity hash mismatch"
    );
}

#[test]
fn default_db_path_exposes_a_usable_path() {
    // On every platform we support (Linux, macOS, Windows), `dirs`
    // resolves a data directory; only extremely stripped-down
    // containers without HOME/APPDATA set would hit None. CI always
    // has a HOME.
    let path = tasks_core::default_db_path().expect("default data dir should resolve");
    assert!(path.ends_with("tasks-desktop/tasks.db") || path.ends_with("tasks-desktop\\tasks.db"));
}

/// L-5 regression (Unix-only): opening a symlink at the DB path
/// must be refused. Windows' `mklink` requires admin unless
/// Developer Mode is on, so this test is gated to Unix; the
/// defence runs identically on all platforms via `symlink_metadata`.
#[cfg(unix)]
#[test]
fn open_refuses_symlink_against_l5() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real.db");
    // Stand up a real DB at one path, then symlink a second path to it.
    let db = Database::open_or_create_read_only(&real).unwrap();
    drop(db);

    let link = tmp.path().join("linked.db");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let err =
        Database::open_read_only(&link).expect_err("symlink should be refused at the DB path");
    let msg = err.to_string();
    assert!(msg.contains("symlink"), "err = {msg}");

    // And the create-or-open path rejects the symlink too, even
    // before it would attempt to create.
    let link2 = tmp.path().join("linked2.db");
    std::os::unix::fs::symlink(&real, &link2).unwrap();
    let err = Database::open_or_create_read_only(&link2)
        .expect_err("open_or_create should also refuse symlinks");
    assert!(err.to_string().contains("symlink"), "err = {err}");
}
