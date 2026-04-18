use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Mirrors `org.tasks.data.entity.TagData` (table `tagdata`). The user-visible
/// definition of a tag — its name, colour, icon, ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagData {
    pub id: Option<i64>,
    pub remote_id: Option<String>,
    pub name: Option<String>,
    pub color: Option<i32>,
    pub tag_ordering: Option<String>,
    pub icon: Option<String>,
    pub order: i32,
}

impl TagData {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(TagData {
            id: row.get("_id")?,
            remote_id: row.get("remoteId")?,
            name: row.get("name")?,
            color: row.get("color")?,
            tag_ordering: row.get("tagOrdering")?,
            icon: row.get("td_icon")?,
            order: row.get("td_order")?,
        })
    }
}

/// Mirrors `org.tasks.data.entity.Tag` (table `tags`). The many-to-many link
/// row connecting a task to a TagData by `tag_uid`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub task: i64,
    pub name: Option<String>,
    pub tag_uid: Option<String>,
    pub task_uid: Option<String>,
}

impl Tag {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Tag {
            id: row.get("_id")?,
            task: row.get("task")?,
            name: row.get("name")?,
            tag_uid: row.get("tag_uid")?,
            task_uid: row.get("task_uid")?,
        })
    }
}
