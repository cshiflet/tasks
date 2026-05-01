//! Per-backend provider implementations.
//!
//! Each module exports a `struct` implementing [`crate::Provider`].
//! The skeleton versions all answer `Err(SyncError::NotYetImplemented)`
//! from the network methods so integration callers can compile
//! and wire the UI against the trait today without waiting for the
//! actual network code.

pub mod caldav;
pub mod caldav_xml;
pub mod etesync;
pub mod google;
pub mod google_json;
pub mod http_util;
pub mod microsoft;
pub mod microsoft_json;
