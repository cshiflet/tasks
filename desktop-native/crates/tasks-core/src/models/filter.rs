use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Mirrors `org.tasks.data.entity.Filter` (table `filters`). A user-defined
/// saved query: a title plus the SQL fragment it expands to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub id: i64,
    pub title: Option<String>,
    pub sql: Option<String>,
    pub values: Option<String>,
    pub criterion: Option<String>,
    pub color: Option<i32>,
    pub icon: Option<String>,
    pub order: i32,
}

impl Filter {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Filter {
            id: row.get("_id")?,
            title: row.get("title")?,
            sql: row.get("sql")?,
            values: row.get("values")?,
            criterion: row.get("criterion")?,
            color: row.get("f_color")?,
            icon: row.get("f_icon")?,
            order: row.get("f_order")?,
        })
    }
}
