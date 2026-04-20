//! Read-only SQLite access layer.
//!
//! Opens the database the desktop client owns, verifies the Room identity
//! hash against a pinned schema version, and exposes enough connection
//! plumbing for the query layer. Writes are intentionally unsupported in
//! Milestone 1, with the sole exception of creating an empty schema on
//! first launch (see `open_or_create_read_only`).

use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Room schema version and identity hash the client is pinned to.
///
/// When upstream Room migrations bump the schema, verify compatibility and
/// bump these constants together. The value is taken from
/// `data/schemas/org.tasks.data.db.Database/<VERSION>.json`. `build.rs`
/// reads the same JSON to emit `SCHEMA_SQL` below — keep them in sync.
pub const PINNED_SCHEMA_VERSION: u32 = 92;
pub const PINNED_IDENTITY_HASH: &str = "cb0b4ff7fd922686361fdbe58cf6bf55";

/// CREATE TABLE / CREATE INDEX statements for the pinned schema
/// version, generated at build time from the Room JSON. Used by
/// `open_or_create_read_only` to stand up a fresh DB when the file
/// doesn't exist yet.
const SCHEMA_SQL: &str = include_str!(concat!(env!("OUT_DIR"), "/schema_92.sql"));

/// Subdirectory name under the OS's per-user data directory. Matches
/// the package style used by the Android app (`org.tasks`) so a user
/// exporting a file from there won't collide with the desktop client's
/// managed directory.
const APP_DIR_NAME: &str = "tasks-desktop";
const DB_FILE_NAME: &str = "tasks.db";

/// Default per-OS location for the desktop client's SQLite file:
///
///   Linux   : `$XDG_DATA_HOME/tasks-desktop/tasks.db`
///             (fallback: `$HOME/.local/share/tasks-desktop/tasks.db`)
///   macOS   : `$HOME/Library/Application Support/tasks-desktop/tasks.db`
///   Windows : `%APPDATA%\tasks-desktop\tasks.db`
///
/// Returns `None` if the OS doesn't expose a data directory at all
/// (extremely rare — happens on stripped-down containers without
/// HOME/APPDATA set).
pub fn default_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_DIR_NAME).join(DB_FILE_NAME))
}

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

    /// If `path` exists, behaves exactly like `open_read_only`. If it
    /// doesn't, creates the parent directory, stands up an empty DB
    /// at `PINNED_SCHEMA_VERSION`, seeds `room_master_table` with
    /// `PINNED_IDENTITY_HASH`, and then reopens read-only.
    ///
    /// This is the entry point the GUI uses on launch so a first-
    /// time user doesn't need to pick a file to see an (empty) task
    /// list. Writes are only performed during the initial
    /// materialisation; once the file exists, every subsequent call
    /// takes the read-only path.
    pub fn open_or_create_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            create_empty_db(path)?;
        }
        Self::open_read_only(path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// Materialise an empty Room DB at `path`. Failures propagate the
/// underlying io / rusqlite errors; the partially-written file is
/// left in place for debugging — callers that want a clean slate
/// should remove it before retrying.
fn create_empty_db(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            tracing::warn!("mkdir -p {}: {e}", parent.display());
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!(
                    "couldn't create parent dir {}: {e}",
                    parent.display()
                )),
            )
        })?;
    }

    // Writable open just long enough to run the schema and seed the
    // identity hash. The connection closes when it drops; the
    // subsequent `open_read_only` on the same path is what the rest
    // of the app sees.
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    conn.execute(
        "INSERT OR REPLACE INTO room_master_table (id, identity_hash) VALUES (42, ?1)",
        [PINNED_IDENTITY_HASH],
    )?;
    tracing::info!(
        "created empty database at {} (schema v{PINNED_SCHEMA_VERSION})",
        path.display()
    );
    Ok(())
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
