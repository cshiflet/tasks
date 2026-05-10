//! Integration test for `tasks_core::tune_writeback_connection`.
//!
//! The helper is the canonical writer-side concurrency-tuning
//! call site for every RW handle in the workspace
//! (tasks-core, tasks-sync, tasks-ui). Two settings:
//!   1. `journal_mode = WAL` — enables concurrent reads + a
//!      single writer without taking out a database-wide lock,
//!      preventing "database is locked" errors when multiple
//!      sync workers run in parallel.
//!   2. `busy_timeout = 5 s` — absorbs writer-vs-writer
//!      transient contention.
//!
//! Without these two, the parallel-account-sync case the user
//! reported degenerates into SQLITE_BUSY errors. Pin both so a
//! future tweak can't silently regress to rollback-journal
//! mode + a 100 ms timeout.

use rusqlite::Connection;

#[test]
fn applies_wal_mode_and_busy_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.db");
    let conn = Connection::open(&path).unwrap();
    tasks_core::tune_writeback_connection(&conn).unwrap();

    // journal_mode=wal sticks in the DB header; verify by
    // reading the pragma back.
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");

    // busy_timeout is per-connection, not persisted. A round-trip
    // confirms the call applied something non-trivial; we don't
    // pin the exact number because tweaking it shouldn't churn
    // this test.
    let timeout_ms: i64 = conn
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert!(
        timeout_ms >= 1_000,
        "busy_timeout = {timeout_ms} ms, expected at least 1 s"
    );
}

/// Re-applying the helper on a database that's already WAL is
/// a no-op — calling it on every RW open (which is what the
/// production code does) shouldn't churn or fail.
#[test]
fn idempotent_on_existing_wal_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.db");
    {
        let conn = Connection::open(&path).unwrap();
        tasks_core::tune_writeback_connection(&conn).unwrap();
    }
    // Reopen and apply again; should succeed without changing
    // the journal_mode away from WAL.
    let conn = Connection::open(&path).unwrap();
    tasks_core::tune_writeback_connection(&conn).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");
}
