mod ffi;
#[cfg(debug_assertions)]
mod guard;
mod player;
mod pump;
pub mod version;

pub use player::{
    AbLoopState, AudioDevice, Chapter, EndFileReason, InfoRow, InfoSection, InfoTrack, MediaInfo,
    Mpv, MpvError, MpvEvent, PlaybackPerformance, PlaybackState, RenderTargetSize, Track,
    TrackKind, VideoDimensions, current_render_target_size, resolve_render_target_size,
};
pub use version::BuildTimeMpv;
