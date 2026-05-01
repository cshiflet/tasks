use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Mirrors `org.tasks.data.entity.Alarm` (table `alarms`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: i64,
    pub task: i64,
    pub time: i64,
    pub alarm_type: i32,
    pub repeat: i32,
    pub interval: i64,
}

impl Alarm {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Alarm {
            id: row.get("_id")?,
            task: row.get("task")?,
            time: row.get("time")?,
            alarm_type: row.get("type")?,
            repeat: row.get("repeat")?,
            interval: row.get("interval")?,
        })
    }
}

pub struct AlarmType;

impl AlarmType {
    pub const DATE_TIME: i32 = 0;
    pub const REL_START: i32 = 1;
    pub const REL_END: i32 = 2;
    pub const RANDOM: i32 = 3;
    pub const SNOOZE: i32 = 4;
    pub const GEO_ENTER: i32 = 5;
    pub const GEO_EXIT: i32 = 6;
}
