//! Entity models mirroring `data/src/commonMain/kotlin/org/tasks/data/entity/`.
//!
//! Only the fields read by Milestone 1 are mapped. Additional fields and
//! entities (Alarm, Tag, Filter, Place, CaldavCalendar, ...) are added as
//! the UI surfaces that need them land.

mod task;

pub use task::{Priority, RepeatFrom, Task};
