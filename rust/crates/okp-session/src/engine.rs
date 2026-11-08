//! The engine seam: what the session needs from libmpv, and the libmpv adapter.

use std::sync::Arc;

use okp_core::playlist::PlaylistItem;
use okp_mpv::{Chapter, Mpv, MpvEvent, PlaybackState, Track, VideoDimensions};

use crate::protocol::{Command, SeekMode, SeekPrecision, SessionConfig, TrackKind};

/// Called by the engine, on an engine thread, whenever its observed state or
/// lifecycle queue may have changed.
pub(crate) type ChangeListener = Arc<dyn Fn() + Send + Sync>;

/// What the engine currently reports. Plain data, read without blocking.
#[derive(Debug, Clone, Default)]
pub(crate) struct Observation {
    pub(crate) playback: PlaybackState,
    pub(crate) subtitle_delay: f64,
    pub(crate) buffering: bool,
    pub(crate) tracks: Vec<Track>,
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) title: Option<String>,
    pub(crate) video: Option<VideoDimensions>,
}

pub(crate) trait Engine: Send + Sync {
    /// Carry out a command the core state machine has already accepted.
    fn forward(&self, command: &Command, request_id: u64) -> Result<(), String>;
    fn observe(&self) -> Observation;
    /// Lifecycle events since the previous call, oldest first.
    fn take_events(&self) -> Vec<MpvEvent>;
    fn set_listener(&self, listener: Option<ChangeListener>);
}

/// libmpv through `okp-mpv`. Rendering is not part of this adapter yet.
pub(crate) struct MpvEngine {
    mpv: Mpv,
}

// SAFETY: the libmpv client API is thread-safe, and `Mpv` serialises its own
// shared state (the event pump publishes behind mutexes). The session only uses
// the `&self` command and `observed_*` methods across threads; the methods that
// need `&mut Mpv` (render context management) are not reachable through this
// adapter, and the handle is destroyed by `Drop` once no thread can use it.
unsafe impl Send for MpvEngine {}
unsafe impl Sync for MpvEngine {}

impl MpvEngine {
    pub(crate) fn new(config: &SessionConfig) -> Result<Self, String> {
        let mut mpv = Mpv::new_with_options(
            config.hwdec.as_deref().unwrap_or(DEFAULT_HWDEC),
            &engine_options(config),
        )
        .map_err(|error| format!("libmpv could not start: {error}"))?;
        // Audio-device discovery is not part of the session contract yet, and
        // enumerating devices can stall on hosts without a desktop audio service.
        mpv.start_event_pump_without_audio_devices();
        Ok(Self { mpv })
    }
}

/// The decoder choice of the first macOS preview, kept until #875 supplies a
/// per-platform default through the config.
const DEFAULT_HWDEC: &str = "auto-safe";

/// Session defaults followed by the shell's options, which win on conflict.
fn engine_options(config: &SessionConfig) -> Vec<(String, String)> {
    let mut options = vec![
        #[cfg(target_os = "macos")]
        ("ao".to_owned(), "coreaudio".to_owned()),
        // The shell owns every key and mouse binding.
        ("input-default-bindings".to_owned(), "no".to_owned()),
        ("input-vo-keyboard".to_owned(), "no".to_owned()),
        // The pinned macOS runtime has no Lua, so it exposes no osc option.
        #[cfg(not(target_os = "macos"))]
        ("osc".to_owned(), "no".to_owned()),
    ];
    options.extend(
        config
            .mpv_options
            .iter()
            .map(|option| (option.name.clone(), option.value.clone())),
    );
    options
}

impl Engine for MpvEngine {
    fn forward(&self, command: &Command, request_id: u64) -> Result<(), String> {
        let engine = &self.mpv;
        let result = match command {
            Command::Open {
                source,
                resume_from,
                audio_track,
                subtitle_track,
            } => {
                let loaded = match source.to_item() {
                    PlaylistItem::Local(path) => {
                        engine.set_media_source(Some(path.clone()));
                        engine.load_file(&path)
                    }
                    PlaylistItem::Url(url) => {
                        engine.set_media_source(None);
                        engine.load_url(&url)
                    }
                };
                loaded.and_then(|()| {
                    if let Some(seconds) = resume_from {
                        engine.seek_absolute(*seconds)?;
                    }
                    if let Some(id) = audio_track {
                        engine.select_audio(Some(*id))?;
                    }
                    if let Some(id) = subtitle_track {
                        engine.select_subtitle(Some(*id))?;
                    }
                    Ok(())
                })
            }
            Command::Close {} => engine.command_async_with_userdata(&["stop"], request_id),
            Command::SetPaused { paused } => engine.command_async_with_userdata(
                &["set", "pause", if *paused { "yes" } else { "no" }],
                request_id,
            ),
            Command::TogglePause {} => {
                engine.command_async_with_userdata(&["cycle", "pause"], request_id)
            }
            Command::Seek {
                seconds,
                mode,
                precision,
            } => engine.command_async_with_userdata(
                &["seek", &seconds.to_string(), seek_flags(*mode, *precision)],
                request_id,
            ),
            Command::SetVolume { volume } => engine.set_volume(*volume),
            Command::SetMute { muted } => engine.command_async_with_userdata(
                &["set", "mute", if *muted { "yes" } else { "no" }],
                request_id,
            ),
            Command::SetSpeed { speed } => engine.set_speed(*speed),
            Command::SelectTrack { kind, id } => match kind {
                TrackKind::Audio => engine.select_audio(*id),
                TrackKind::Subtitle => engine.select_subtitle(*id),
            },
            Command::SetSubtitleDelay { seconds } => engine.set_subtitle_delay(*seconds),
        };
        result.map_err(|error| error.to_string())
    }

    fn observe(&self) -> Observation {
        let engine = &self.mpv;
        Observation {
            playback: engine.observed_playback_state(),
            subtitle_delay: engine.observed_subtitle_delay(),
            buffering: engine.observed_playback_diagnostics().paused_for_cache,
            tracks: engine.observed_tracks(),
            chapters: engine.observed_chapters(),
            title: engine.observed_media_info().map(|info| info.title),
            video: engine.observed_video_dimensions(),
        }
    }

    fn take_events(&self) -> Vec<MpvEvent> {
        self.mpv.take_lifecycle_events()
    }

    fn set_listener(&self, listener: Option<ChangeListener>) {
        self.mpv.set_observation_listener(listener);
    }
}

fn seek_flags(mode: SeekMode, precision: SeekPrecision) -> &'static str {
    match (mode, precision) {
        (SeekMode::Absolute, SeekPrecision::Exact) => "absolute+exact",
        (SeekMode::Absolute, SeekPrecision::Keyframe) => "absolute+keyframes",
        (SeekMode::Relative, SeekPrecision::Exact) => "relative+exact",
        (SeekMode::Relative, SeekPrecision::Keyframe) => "relative+keyframes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::MpvOption;
    use okp_core::app::AppIdentity;

    #[test]
    fn shell_options_follow_and_override_the_session_defaults() {
        let config = SessionConfig {
            schema_version: crate::protocol::SCHEMA_VERSION,
            app: AppIdentity {
                name: "OK Player".to_owned(),
                platform: "macos".to_owned(),
            },
            data_dir: "/data".into(),
            config_dir: "/config".into(),
            cache_dir: "/cache".into(),
            hwdec: None,
            mpv_options: vec![MpvOption {
                name: "input-vo-keyboard".to_owned(),
                value: "yes".to_owned(),
            }],
        };
        let options = engine_options(&config);
        let last = options
            .iter()
            .rposition(|(name, _)| name == "input-vo-keyboard")
            .expect("the shell option is passed on");
        assert_eq!(options[last].1, "yes");
        assert_eq!(last, options.len() - 1, "shell options come last and win");
        assert!(options.contains(&("input-default-bindings".to_owned(), "no".to_owned())));
    }

    #[test]
    fn seek_flags_cover_mode_and_precision() {
        assert_eq!(
            seek_flags(SeekMode::Relative, SeekPrecision::Keyframe),
            "relative+keyframes"
        );
        assert_eq!(
            seek_flags(SeekMode::Absolute, SeekPrecision::Exact),
            "absolute+exact"
        );
    }
}
