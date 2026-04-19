//! Read-only SQLite access layer.
//!
//! Opens the database the Android app writes, verifies the Room identity
//! hash against a pinned schema version, and exposes enough connection
//! plumbing for the query layer. Writes are intentionally unsupported in
//! Milestone 1.

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Room schema version and identity hash the client is pinned to.
///
/// When upstream Room migrations bump the schema, verify compatibility and
/// bump these constants together. The value is taken from
/// `data/schemas/org.tasks.data.db.Database/<VERSION>.json`.
pub const PINNED_SCHEMA_VERSION: u32 = 92;
pub const PINNED_IDENTITY_HASH: &str = "cb0b4ff7fd922686361fdbe58cf6bf55";

/// Handle to an opened, schema-verified, read-only Tasks database.
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish()
    }
}

impl Database {
    /// Open `path` read-only and verify the Room identity hash.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(&path, flags)?;

        // Low-threshold busy_timeout as a defensive belt only: the
        // desktop client owns its SQLite file exclusively. Android's
        // copy lives in its app-sandbox at `/data/data/<pkg>/databases`,
        // is never shared over cloud sync (the Android backup format
        // is JSON, not SQLite), and can't be reached by another
        // process without root. Contention, if it ever happens, is
        // another desktop reader — not the Android app — and resolves
        // instantly.
        conn.busy_timeout(std::time::Duration::from_millis(50))?;

        let actual_hash = read_identity_hash(&conn)?;
        if actual_hash != PINNED_IDENTITY_HASH {
            return Err(CoreError::SchemaMismatch {
                expected: PINNED_IDENTITY_HASH,
                actual: actual_hash,
                expected_version: PINNED_SCHEMA_VERSION,
            });
        }

        Ok(Database { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn read_identity_hash(conn: &Connection) -> Result<String> {
    // Room stores its schema identity hash in `room_master_table`, row
    // `id = 42`. If the table is missing, the DB almost certainly wasn't
    // produced by Room.
    let table_exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'room_master_table'",
            [],
            |row| row.get(0),
        )
        .ok();

    if table_exists.is_none() {
        return Err(CoreError::MissingRoomMetadata);
    }

    let hash: String = conn.query_row(
        "SELECT identity_hash FROM room_master_table WHERE id = 42",
        [],
        |row| row.get(0),
    )?;
    Ok(hash)
}
