mod ffi;
mod player;
pub mod version;

pub use player::{Mpv, MpvError};
pub use version::BuildTimeMpv;
