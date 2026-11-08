//! One playback session behind a small, document-shaped interface.
//!
//! A [`Session`] owns the engine (libmpv through `okp-mpv`), the core
//! [`okp_core::player::PlayerMachine`] that validates every command before the
//! engine sees it, an event thread, and a coalescing event queue. A shell talks
//! to it in JSON documents ([`protocol`]): it dispatches commands, queries the
//! full state, and drains batches of events after the session's wakeup fires.
//! The C ABI over this lives in `okp-ffi` (`okp_session_*`).
//!
//! The session decides; a shell only renders what it reports.

mod coalesce;
pub mod commands;
mod engine;
pub mod machine;
pub mod protocol;
pub mod reconcile;
mod session;

pub use coalesce::TIMELINE_INTERVAL;
pub use commands::{CommandRegistry, WindowState};
pub use machine::SessionMachine;
pub use protocol::SCHEMA_VERSION;
pub use session::{RenderUpdate, Session, Wakeup};
