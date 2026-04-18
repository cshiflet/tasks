use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Mirrors `org.tasks.data.entity.CaldavAccount` (table `caldav_accounts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaldavAccount {
    pub id: i64,
    pub uuid: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub error: Option<String>,
    pub account_type: i32,
    pub is_collapsed: bool,
    pub server_type: i32,
    pub last_sync: i64,
}

impl CaldavAccount {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(CaldavAccount {
            id: row.get("cda_id")?,
            uuid: row.get("cda_uuid")?,
            name: row.get("cda_name")?,
            url: row.get("cda_url")?,
            username: row.get("cda_username")?,
            password: row.get("cda_password")?,
            error: row.get("cda_error")?,
            account_type: row.get("cda_account_type")?,
            is_collapsed: row.get::<_, i32>("cda_collapsed")? != 0,
            server_type: row.get("cda_server_type")?,
            last_sync: row.get("cda_last_sync")?,
        })
    }

    pub fn is_caldav(&self) -> bool { self.account_type == AccountType::CALDAV }
    pub fn is_local(&self) -> bool { self.account_type == AccountType::LOCAL }
    pub fn is_opentasks(&self) -> bool { self.account_type == AccountType::OPENTASKS }
    pub fn is_tasks_org(&self) -> bool { self.account_type == AccountType::TASKS_ORG }
    pub fn is_etebase(&self) -> bool { self.account_type == AccountType::ETEBASE }
    pub fn is_microsoft(&self) -> bool { self.account_type == AccountType::MICROSOFT }
    pub fn is_google_tasks(&self) -> bool { self.account_type == AccountType::GOOGLE_TASKS }
}

pub struct AccountType;

impl AccountType {
    pub const CALDAV: i32 = 0;
    // 1 was TYPE_ETESYNC, deprecated in favour of ETEBASE.
    pub const LOCAL: i32 = 2;
    pub const OPENTASKS: i32 = 3;
    pub const TASKS_ORG: i32 = 4;
    pub const ETEBASE: i32 = 5;
    pub const MICROSOFT: i32 = 6;
    pub const GOOGLE_TASKS: i32 = 7;
}

/// Mirrors `org.tasks.data.entity.CaldavCalendar` (table `caldav_lists`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaldavCalendar {
    pub id: i64,
    pub account: Option<String>,
    pub uuid: Option<String>,
    pub name: Option<String>,
    pub color: i32,
    pub ctag: Option<String>,
    pub url: Option<String>,
    pub icon: Option<String>,
    pub order: i32,
    pub access: i32,
    pub last_sync: i64,
}

impl CaldavCalendar {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(CaldavCalendar {
            id: row.get("cdl_id")?,
            account: row.get("cdl_account")?,
            uuid: row.get("cdl_uuid")?,
            name: row.get("cdl_name")?,
            color: row.get("cdl_color")?,
            ctag: row.get("cdl_ctag")?,
            url: row.get("cdl_url")?,
            icon: row.get("cdl_icon")?,
            order: row.get("cdl_order")?,
            access: row.get("cdl_access")?,
            last_sync: row.get("cdl_last_sync")?,
        })
    }

    pub fn is_read_only(&self) -> bool { self.access == CalendarAccess::READ_ONLY }
}

pub struct CalendarAccess;

impl CalendarAccess {
    pub const UNKNOWN: i32 = -1;
    pub const OWNER: i32 = 0;
    pub const READ_WRITE: i32 = 1;
    pub const READ_ONLY: i32 = 2;
}

/// Mirrors `org.tasks.data.entity.CaldavTask` (table `caldav_tasks`). The
/// sync-side metadata linking a local task row to a remote VTODO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaldavTask {
    pub id: i64,
    pub task: i64,
    pub calendar: Option<String>,
    pub remote_id: Option<String>,
    pub object: Option<String>,
    pub etag: Option<String>,
    pub last_sync: i64,
    pub deleted: i64,
    pub remote_parent: Option<String>,
    pub is_moved: bool,
    pub remote_order: i64,
}

impl CaldavTask {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(CaldavTask {
            id: row.get("cd_id")?,
            task: row.get("cd_task")?,
            calendar: row.get("cd_calendar")?,
            remote_id: row.get("cd_remote_id")?,
            object: row.get("cd_object")?,
            etag: row.get("cd_etag")?,
            last_sync: row.get("cd_last_sync")?,
            deleted: row.get("cd_deleted")?,
            remote_parent: row.get("cd_remote_parent")?,
            is_moved: row.get::<_, i32>("gt_moved")? != 0,
            remote_order: row.get("gt_remote_order")?,
        })
    }

    pub fn is_deleted(&self) -> bool { self.deleted > 0 }
}
