//! The engine seam: what the session needs from libmpv, and the libmpv adapter.

use std::sync::{Arc, Mutex, PoisonError};

use okp_core::playlist::PlaylistItem;
use okp_mpv::{
    Chapter, EntryEvent, Mpv, OpenGlRenderer, PlaybackState, RenderUpdate, Track, VideoDimensions,
};

use crate::protocol::{
    Command, MediaSource, SeekMode, SeekPrecision, SessionConfig, TrackKind, media_position,
    seek_seconds,
};

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
    /// Carry out a command the core state machine has already accepted. For an
    /// open, returns the engine's id for the new load when it reports one; the
    /// load's events carry the same id.
    ///
    /// An open's track choices are not applied here: the session selects them
    /// once the file has loaded and shows it has those tracks.
    fn forward(&self, command: &Command, request_id: u64) -> Result<Option<i64>, String>;
    fn observe(&self) -> Observation;
    /// Lifecycle events since the previous call, oldest first.
    fn take_events(&self) -> Vec<EntryEvent>;
    fn set_listener(&self, listener: Option<ChangeListener>);

    /// Create the OpenGL render context with advanced control and `on_update`
    /// installed; the caller's OpenGL context is current.
    fn render_create(&self, _on_update: RenderUpdate) -> Result<(), String> {
        Err(NO_RENDERING.to_owned())
    }
    /// Serve the render thread's share of the engine's work after `on_update`
    /// fired; true when a new frame is due.
    fn render_update(&self) -> bool {
        false
    }
    /// Draw the due (or current) frame into the bound framebuffer.
    fn render(&self, _width: i32, _height: i32) -> Result<(), String> {
        Err(NO_RENDERING.to_owned())
    }
    /// The frame drawn last has been presented. Returns the swaps reported to
    /// the render context so far, 0 without one.
    fn render_report_swap(&self) -> u64 {
        0
    }
    /// Free the render context, if there is one.
    fn render_destroy(&self) {}
}

const NO_RENDERING: &str = "this engine does not render";

/// libmpv through `okp-mpv`.
pub(crate) struct MpvEngine {
    /// The render context, behind its own lock so render calls never overlap.
    /// Declared before `mpv` so it is dropped first, as libmpv requires.
    renderer: Mutex<Option<OpenGlRenderer>>,
    mpv: Mpv,
}

// SAFETY: the libmpv client API is thread-safe, and `Mpv` serialises its own
// shared state (the event pump publishes behind mutexes). Across threads the
// session uses only `&self` methods: commands and `observed_*` reads, which
// libmpv allows from any thread, and the render context, which lives in
// `renderer` behind its own mutex so its calls never overlap and which libmpv
// lets run alongside client API calls. The `&mut Mpv` render methods are not
// reachable through this adapter. The render context is freed before the handle,
// and the handle is destroyed by `Drop` once no thread can use it.
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
        Ok(Self {
            renderer: Mutex::new(None),
            mpv,
        })
    }

    fn open(&self, source: &MediaSource, resume_from: Option<f64>) -> Result<Option<i64>, String> {
        // The start position is file-local, so libmpv applies it as this file
        // loads, not to whatever is loaded when a later seek would arrive.
        let options: Vec<(&str, String)> = resume_from
            .map(|seconds| ("start", media_position(seconds).to_string()))
            .into_iter()
            .collect();
        let target = match source.to_item() {
            PlaylistItem::Local(path) => {
                self.mpv.set_media_source(Some(path.clone()));
                path.to_string_lossy().into_owned()
            }
            PlaylistItem::Url(url) => {
                self.mpv.set_media_source(None);
                url
            }
        };
        self.mpv
            .load_with_file_options(&target, &options)
            .map_err(|error| error.to_string())
    }

    fn renderer(&self) -> std::sync::MutexGuard<'_, Option<OpenGlRenderer>> {
        self.renderer.lock().unwrap_or_else(PoisonError::into_inner)
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
        // Track and subtitle-delay choices belong to one file. libmpv keeps them
        // as engine options, so without this a track turned off, or a delay set,
        // would carry into the next file (which may then have nothing to play).
        (
            "reset-on-next-file".to_owned(),
            "aid,sid,sub-delay".to_owned(),
        ),
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
    fn forward(&self, command: &Command, request_id: u64) -> Result<Option<i64>, String> {
        let engine = &self.mpv;
        let result = match command {
            Command::Open {
                source,
                resume_from,
                ..
            } => return self.open(source, *resume_from),
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
                &[
                    "seek",
                    &seek_seconds(*seconds, *mode).to_string(),
                    seek_flags(*mode, *precision),
                ],
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
            // The session carries history actions out itself.
            Command::History { .. } => {
                return Err("a history action is not an engine command".to_owned());
            }
            // The session resolves an invoked registry command to the command it
            // stands for before anything reaches the engine.
            Command::InvokeCommand { id } => {
                return Err(format!("invoke_command `{id}` is not an engine command"));
            }
        };
        result.map(|()| None).map_err(|error| error.to_string())
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

    fn take_events(&self) -> Vec<EntryEvent> {
        self.mpv.take_lifecycle_events_with_entries()
    }

    fn set_listener(&self, listener: Option<ChangeListener>) {
        self.mpv.set_observation_listener(listener);
    }

    fn render_create(&self, on_update: RenderUpdate) -> Result<(), String> {
        let mut renderer = self.renderer();
        if renderer.is_none() {
            // SAFETY: the renderer lives in `self.renderer`, which is dropped
            // before `self.mpv`.
            let created = unsafe { self.mpv.create_opengl_renderer(on_update) };
            *renderer = Some(created.map_err(|error| error.to_string())?);
        }
        Ok(())
    }

    fn render_update(&self) -> bool {
        self.renderer().as_mut().is_some_and(OpenGlRenderer::update)
    }

    fn render(&self, width: i32, height: i32) -> Result<(), String> {
        match self.renderer().as_mut() {
            Some(renderer) => renderer
                .draw(width, height)
                .map_err(|error| error.to_string()),
            None => Err("the render context has not been created".to_owned()),
        }
    }

    fn render_report_swap(&self) -> u64 {
        self.renderer()
            .as_mut()
            .map_or(0, OpenGlRenderer::report_swap)
    }

    fn render_destroy(&self) {
        self.renderer().take();
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
                platform: okp_core::app::Platform::MacOs,
            },
            data_dir: "/data".into(),
            config_dir: "/config".into(),
            cache_dir: "/cache".into(),
            hwdec: None,
            mpv_options: vec![MpvOption {
                name: "input-vo-keyboard".to_owned(),
                value: "yes".to_owned(),
            }],
            shell_commands: Vec::new(),
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
