mod ffi;
mod player;
pub mod version;

pub use player::{Mpv, MpvError, PlaybackState};
pub use version::BuildTimeMpv;
