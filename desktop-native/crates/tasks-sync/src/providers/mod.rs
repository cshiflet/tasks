//! Per-backend provider implementations.
//!
//! Each module exports a `struct` implementing [`crate::Provider`].
//! The skeleton versions all answer `Err(SyncError::NotYetImplemented)`
//! from the network methods so integration callers can compile
//! and wire the UI against the trait today without waiting for the
//! actual network code.

pub mod caldav;
pub mod etesync;
pub mod google;
pub mod microsoft;
