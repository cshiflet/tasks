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
        // L-5: reject symlinks on the final path component before
        // opening. A user-writable symlink at the default data dir
        // could otherwise redirect the DB handle to a location
        // outside the app's own sandbox (e.g. `/etc/shadow` on
        // Linux, `C:\Windows\System32\...` on Windows). We use
        // symlink_metadata so the check doesn't itself follow the
        // link. Missing files are fine — `open_or_create_read_only`
        // needs to bootstrap them — so we skip the check when the
        // path doesn't exist yet.
        reject_if_symlink(&path)?;
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
        // L-5: reject a symlink at the target even in the "does-not-
        // exist yet" branch — if the default data dir got pre-staged
        // with a symlink pointing elsewhere, `create_empty_db` would
        // otherwise follow it when creating the file.
        reject_if_symlink(path)?;
        if !path.exists() {
            create_empty_db(path)?;
        }
        Self::open_read_only(path)
    }

    /// Read-write equivalent of [`open_or_create_read_only`]. Used by
    /// the GUI now that M2+ writes (task create / edit / delete) and
    /// sync writeback share the same long-lived handle. The
    /// schema-hash check still runs so a stale Android backup can't
    /// silently corrupt the desktop's local DB.
    pub fn open_or_create_read_write(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        reject_if_symlink(&path)?;
        if !path.exists() {
            create_empty_db(&path)?;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(&path, flags)?;
        tune_writeback_connection(&conn)?;

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

/// Reject a path whose final component is a symlink. Non-existent
/// paths are OK (the caller may be about to create the file);
/// anything else that fails `symlink_metadata` is also surfaced
/// Apply the desktop client's writer-side concurrency tuning.
/// Used by every RW handle: the long-lived bridge connection,
/// the per-write transient handles in `tasks_core::write`, and
/// the per-sync engine handles in `tasks_sync::engine`.
///
/// Two settings:
///
/// * `journal_mode = WAL` so concurrent reads + a single writer
///   coexist without taking out a database-wide lock. Without
///   WAL, parallel `sync_account` calls (one per account fanned
///   out by the periodic auto-sync timer) deadlock against each
///   other with `SQLITE_BUSY` even when their write windows are
///   short — the rollback journal serialises everything through
///   the file-level lock. WAL allows concurrent readers + one
///   writer at a time, with the writer's commits going through
///   the WAL file rather than blocking readers. WAL is set
///   per-database, sticks across opens, so calling it on every
///   open is a no-op after the first.
///
/// * `busy_timeout = 5 s` so the rare transient contention that
///   does happen (writer-vs-writer when two syncs commit within
///   ms of each other) still surfaces as a delay rather than an
///   error. The bridge's interactive writes are sub-100ms, the
///   sync engine's per-task transactions are similar; 5 s is
///   enough headroom for any realistic batch the engine commits
///   while another worker is mid-commit. Tuned up from the
///   earlier 1 s after the user reported "database is locked"
///   on parallel sync of 4 accounts pulling 800+ tasks each.
pub fn tune_writeback_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Reject any `path` that points at a symlink so a malicious
/// link can't redirect our open into another file (L-5). IO
/// errors on the parent dir surface as `CoreError::Io` rather
/// than getting silently swallowed.
///
/// `symlink_metadata` does NOT follow symlinks — that's the whole
/// point; it reports on the link itself.
fn reject_if_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{} is a symlink; refusing to open for safety (L-5)",
                path.display()
            ),
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("stat {}: {e}", path.display()),
        ))),
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
