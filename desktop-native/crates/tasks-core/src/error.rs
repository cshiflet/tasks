use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error(
        "database schema identity hash mismatch: expected {expected}, got {actual}. \
         The desktop client is pinned to Room schema version {expected_version}; upgrade \
         or downgrade the database to match."
    )]
    SchemaMismatch {
        expected: &'static str,
        actual: String,
        expected_version: u32,
    },

    #[error("database is missing the Room metadata table `room_master_table`")]
    MissingRoomMetadata,

    #[error("filesystem watcher error: {0}")]
    Watch(#[from] notify::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
