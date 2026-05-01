//! Entity models mirroring `data/src/commonMain/kotlin/org/tasks/data/entity/`.
//!
//! Each struct maps one Room entity. Column names follow the Room
//! `@ColumnInfo(name = ...)` attribute (which is what lands in the physical
//! schema), not the Kotlin property name. `from_row` helpers parse a single
//! `rusqlite::Row`; they're deliberately narrow — sort/filter logic lives in
//! `crate::query`, not here.

mod alarm;
mod caldav;
mod filter;
mod place;
mod tag;
mod task;

pub use alarm::{Alarm, AlarmType};
pub use caldav::{AccountType, CaldavAccount, CaldavCalendar, CaldavTask, CalendarAccess};
pub use filter::Filter;
pub use place::{Geofence, Place};
pub use tag::{Tag, TagData};
pub use task::{Priority, RepeatFrom, Task};
