//! Read-only core for the Tasks.org native desktop client.
//!
//! Milestone 1 scope: open the SQLite database that the Android app writes,
//! verify the Room schema identity hash, and run a small set of task list
//! queries. No sync, no writes — see `desktop-native/README.md` for the
//! broader roadmap.

pub mod db;
pub mod error;
pub mod models;
pub mod query;
pub mod watch;

pub use db::Database;
pub use error::{CoreError, Result};
pub use models::{
    AccountType, Alarm, AlarmType, CaldavAccount, CaldavCalendar, CaldavTask, CalendarAccess,
    Filter, Geofence, Place, Priority, RepeatFrom, Tag, TagData, Task,
};
