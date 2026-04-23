//! Sync providers for the Tasks.org native desktop client.
//!
//! Every provider (CalDAV, Google Tasks, Microsoft To Do, EteSync)
//! implements a common [`Provider`] trait so the UI layer can speak
//! to all four in a uniform way. Account identity, list discovery,
//! push + pull, and teardown all live on the trait; per-provider
//! specifics (OAuth flows, WebDAV quirks, libetebase FFI) stay
//! inside the implementing module.
//!
//! **Status:** skeleton only. Every provider here returns
//! [`SyncError::NotYetImplemented`] from its network-dependent
//! methods. The shapes are fixed enough to commit against from
//! the UI side while the individual providers land one at a time.
//!
//! See the workspace README for milestone mapping:
//!
//! * Milestone 3 — [`providers::caldav`]
//! * Milestone 4 — [`providers::google`], [`providers::microsoft`]
//! * Milestone 5 — [`providers::etesync`]

pub mod convert;
pub mod engine;
pub mod ical;
pub mod oauth;
pub mod provider;
pub mod providers;

pub use engine::SyncEngine;

pub use convert::{remote_task_to_vtodo, vtodo_to_remote_task};

pub use ical::{
    parse_vcalendar, serialize_vcalendar, AlarmTrigger, IcalError, IcalResult, VAlarm, VTodo,
    VTodoStatus,
};
pub use provider::{
    AccountCredentials, Provider, ProviderKind, RemoteCalendar, RemoteTask, SyncError, SyncOutcome,
    SyncResult,
};
