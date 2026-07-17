use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use gtk::cairo;
use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use okp_core::candidate_channel::{self, CandidateFeed};
use okp_core::clip_export::{self, ClipExportEligibility, ClipExportLimits, ClipExportTooling};
use okp_core::gapless::{GaplessPlaybackCapability, PlaylistTransitionPath};
use okp_core::hdr::HdrHandlingState;
use okp_core::playlist::{Playlist, PlaylistItem, QueueInsertMode, RepeatMode};
use okp_core::settings::{AppearanceTheme, UpdateChannel};
use okp_core::shortcuts::{
    self, ShortcutAction, ShortcutBinding, ShortcutChord, ShortcutModifiers, ShortcutSlot,
};
use okp_core::update_selection::{self, DebFeed, DebUpdate, SHA256SUMS_ASSET};
use okp_core::video_geometry::{VideoAspect, VideoGeometry, VideoGeometryAction};
use okp_core::{
    AppIdentity, chapter_math, fullscreen_toggle, launch_args, lrc, m3u, media_formats,
    natural_compare, network_media, ok_player_uri, progress_report, seek_readout, sha256sums,
    subtitle_delay, subtitle_search, time_code, timeline_buffer, video_click, volume, window_fit,
    youtube_open,
};
use okp_mpv::{
    AbLoopState, AudioDevice, Chapter, EndFileReason, InfoRow, InfoSection, InfoTrack, MediaInfo,
    Mpv, MpvEvent, NativeWaylandDisplay, PlaybackState, Track, TrackKind, VideoDimensions,
    current_render_target_size, resolve_render_target_size,
};
use velopack::{
    UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackApp, VelopackAsset,
    sources::HttpSource,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

mod about;
mod branding;
mod compact_mode;
mod controls;
mod css;
mod dialogs;
mod history;
mod history_view;
mod integration;
mod keyboard;
mod lyrics;
mod media_info;
mod mpris;
mod mpv_bridge;
mod native_video;
mod nfo_title;
mod osc_bar;
mod panels;
mod playback;
mod playlist_ops;
mod presentation;
mod screenshots;
mod settings;
mod settings_pages;
mod settings_window;
mod thumbnails;
mod track_popovers;
mod updates;
mod window;
pub(crate) use about::*;
pub(crate) use branding::*;
pub(crate) use compact_mode::*;
pub(crate) use controls::*;
pub(crate) use css::*;
pub(crate) use dialogs::*;
pub(crate) use history_view::*;
pub(crate) use integration::*;
pub(crate) use keyboard::*;
pub(crate) use lyrics::*;
pub(crate) use media_info::*;
pub(crate) use mpris::*;
pub(crate) use mpv_bridge::*;
pub(crate) use native_video::*;
pub(crate) use nfo_title::*;
pub(crate) use panels::*;
pub(crate) use playback::*;
pub(crate) use playlist_ops::*;
pub(crate) use presentation::*;
pub(crate) use settings_pages::*;
pub(crate) use settings_window::*;
pub(crate) use track_popovers::*;
pub(crate) use updates::*;
pub(crate) use window::*;

const SPEED_PRESETS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
const APP_BUILD_VERSION: &str = env!("OKP_BUILD_VERSION");
const APP_BUILD_SHA: &str = env!("OKP_BUILD_SHA");
const LINUX_DESKTOP_ID: &str = "com.befeast.okplayer.desktop";
const LINUX_ICON_NAME: &str = "com.befeast.okplayer";
const MPRIS_BUS_NAME: &str = "org.mpris.MediaPlayer2.okplayer";
const MPRIS_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_TRACK_PATH: &str = "/org/mpris/MediaPlayer2/Track/0";
const MPRIS_TRACKLIST_NO_TRACK_PATH: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const MPRIS_TRACKLIST_CONTEXT_LIMIT: usize = 21;
const MPRIS_SEEKED_DELTA_US: i64 = 750_000;
const MPRIS_ART_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
const MPRIS_FOLDER_ART_STEMS: &[&str] =
    &["cover", "folder", "front", "poster", "album", "albumart"];
const MPRIS_EMBEDDED_ART_TIMEOUT: Duration = Duration::from_secs(8);
// Grace period after toggling the A-B loop before the settled endpoints are read
// back from the event pump's snapshot (the change is observed asynchronously).
const AB_LOOP_SETTLE_DELAY: Duration = Duration::from_millis(60);
// Both Linux update lanes discover through a static feed on GitHub Pages
// (issue #162, symmetric to the Windows feed in #131) so a churn of releases on
// the other track can never bury the newest Linux release out of a discovery
// window. The Velopack AppImage lane reads releases.linux.json under this base
// (HttpSource appends the channel file name); the .deb lane reads deb.linux.json
// directly. Overridable for local testing via OKP_LINUX_UPDATE_FEED_URL and
// OKP_LINUX_DEB_FEED_URL.
const LINUX_UPDATE_FEED_BASE_URL: &str = "https://befeast.github.io/ok-player/updates/linux";
const LINUX_DEB_FEED_URL: &str = "https://befeast.github.io/ok-player/updates/linux/deb.linux.json";
// The rolling Linux candidate channel (issue #339). Only an explicitly enrolled
// QA install (Settings.updates.channel == Candidate, or OKP_LINUX_UPDATE_CHANNEL=
// candidate) fetches this; a default install never touches it, so the public
// feed above and its user behavior are untouched. Unlike deb.linux.json it is
// served from a single mutable "rolling" surface — one candidate at a time, no
// new GitHub Release per build. Overridable for local testing via
// OKP_LINUX_CANDIDATE_FEED_URL.
const LINUX_CANDIDATE_FEED_URL: &str =
    "https://github.com/BeFeast/ok-player/releases/download/linux-candidate/candidate.linux.json";
const LINUX_SHA256SUMS_MAX_BYTES: u64 = 1024 * 1024;
const DEB_SELF_INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const SETTINGS_REFERENCE_WIDTH: i32 = 760;
const SETTINGS_REFERENCE_HEIGHT: i32 = 560;
const SETTINGS_TITLEBAR_HEIGHT: i32 = 42;
const SETTINGS_RAIL_WIDTH: i32 = 192;
const SETTINGS_CONTENT_WIDTH: i32 = SETTINGS_REFERENCE_WIDTH - SETTINGS_RAIL_WIDTH;
const CAPTIONLESS_DRAG_HEIGHT: i32 = SETTINGS_TITLEBAR_HEIGHT;
const LINUX_KEY_MEDIA_MIME_TYPES: &[&str] = &[
    "video/mp4",
    "video/x-matroska",
    "video/quicktime",
    "video/webm",
    "video/x-msvideo",
    "audio/mpeg",
    "audio/flac",
    "audio/mp4",
    "audio/x-wav",
    "audio/ogg",
];
// The reserved ok-player:// control scheme (PRD §13.4). The MIME type is what a desktop
// entry advertises to claim the scheme; the display form is what diagnostics show.
const LINUX_URI_SCHEME_MIME: &str = "x-scheme-handler/ok-player";
const LINUX_RESERVED_URI_SCHEME: &str = "ok-player://";
const AUDIO_DEVICE_AUTO: &str = "auto";
const AUDIO_DEVICE_RESTORE_MAX_ATTEMPTS: u8 = 50;
const AB_LOOP_COMBINED_MARK_EPSILON_SECS: f64 = 0.5;
const OSC_CLEARANCE_DIP: f64 = 88.0;
const OSC_SUBTITLE_LIFT_PERCENT: f64 = 16.0;
const PROTECTED_MPV_OPTIONS: &[&str] = &["config", "terminal", "idle", "force-window", "vo"];
const LINUX_GAPLESS_CAPABILITY: GaplessPlaybackCapability =
    GaplessPlaybackCapability::for_transition_path(
        PlaylistTransitionPath::ShellManagedAfterEndFile,
    );
const LINUX_HDR_HANDLING: HdrHandlingState = HdrHandlingState::EngineManaged;

static MPRIS_SIDECAR_ART_CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
static MPRIS_EMBEDDED_ART_CACHE: OnceLock<
    Mutex<HashMap<MprisEmbeddedArtCacheKey, MprisEmbeddedArtCacheEntry>>,
> = OnceLock::new();
static MPRIS_APP_ICON_ART_URL: OnceLock<Option<String>> = OnceLock::new();

#[derive(Default)]
struct PlayerState {
    mpv: Option<Mpv>,
    current_file: Option<PathBuf>,
    current_url: Option<String>,
    current_nfo_title: okp_core::nfo_metadata::NfoTitleState,
    nfo_title_jobs: NfoTitleJobs,
    source_generation: u64,
    initial_window_fit: window_fit::InitialFitState,
    current_video_dimensions: Option<VideoDimensions>,
    seek_generation: u64,
    playlist: Playlist,
    pending_subtitles: Vec<PathBuf>,
    pending_resume: Option<PendingResume>,
    pending_launch_tracks: Option<PendingLaunchTracks>,
    next_launch_directives: Option<LaunchDirectives>,
    pending_preferences: Option<(PathBuf, history::PlaybackPreferences)>,
    thumbnail_request_key: Option<String>,
    hover_thumbnail_request_key: Option<String>,
    chapters_snapshot: Vec<Chapter>,
    private_session: bool,
    progress_reporter: progress_report::ProgressReporter,
    history: history::HistoryStore,
    settings: settings::SettingsStore,
    screenshot_jobs: screenshots::ScreenshotJobs,
    linux_update_status: LinuxUpdateStatus,
    pending_audio_device_restore: Option<PendingAudioDeviceRestore>,
    render_target_size: Option<okp_mpv::RenderTargetSize>,
    native_video_plane: Option<Arc<NativeVideoPlane>>,
    native_render_loop: Option<NativeRenderLoop>,
    presentation_recorder: Option<Arc<PresentationRecorder>>,
    presentation_exercise: Option<okp_core::presentation_evidence::PresentationExercise>,
    video_transform: VideoGeometry,
    ab_loop: AbLoopState,
    /// Last transient navigation projection, so rapid fine seeks / frame steps
    /// accumulate their readouts instead of re-projecting the same stale
    /// snapshot before mpv's pump republishes `time_pos`.
    pending_nav: Option<seek_readout::PendingNav>,
    /// The transport-surface state for the loaded source — the shared model the
    /// loading, buffering, and error surfaces read from. Pure core (see
    /// [`okp_core::network_media`]); the shell only transitions and renders it.
    media_load_state: network_media::MediaLoadState,
    /// Portable mute/restore memory shared by the OSC button and keyboard path.
    volume_state: volume::VolumeState,
    /// Latest optimistic UI/shortcut projection awaiting the matching mpv observation.
    /// This prevents the poll loop from rebasing rapid nudges on an older snapshot.
    pending_volume: Option<f64>,
    /// The source most recently handed to the engine, kept so the failure surface's
    /// Retry action can replay the same source. Cleared on close.
    retry_load_source: Option<network_media::LoadFailureSource>,
    /// The short, copyable reason for the most recent load failure — surfaced
    /// through the in-canvas card's Copy details action instead of raw logs.
    last_load_error: Option<String>,
    /// Intended fullscreen state for the double-click contract. The window's own
    /// `is_fullscreen` lags the Wayland compositor, so every toggle path decides
    /// from this eagerly-flipped intent and reconciles it with the `fullscreened`
    /// notify. See [`fullscreen_toggle`].
    fullscreen_toggle: fullscreen_toggle::FullscreenToggle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingResume {
    source_generation: u64,
    target: launch_args::ResumeTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingLaunchTracks {
    source_generation: u64,
    subtitle: Option<launch_args::TrackSelection>,
    audio: Option<launch_args::TrackSelection>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LaunchDirectives {
    resume_seconds: Option<f64>,
    subtitle: Option<launch_args::TrackSelection>,
    audio: Option<launch_args::TrackSelection>,
}

impl LaunchDirectives {
    fn has_tracks(self) -> bool {
        self.subtitle.is_some() || self.audio.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingAudioDeviceRestore {
    name: String,
    attempts: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MprisEmbeddedArtCacheKey {
    path: PathBuf,
    len: u64,
    modified_ns: u128,
}

#[derive(Clone, Debug)]
enum MprisEmbeddedArtCacheEntry {
    Pending,
    Ready(Option<PathBuf>),
}

impl PendingAudioDeviceRestore {
    fn new(name: String) -> Self {
        Self { name, attempts: 0 }
    }
}

#[derive(Clone, Default)]
struct LaunchArgs {
    items: Vec<PlaylistItem>,
    playlists: Vec<PathBuf>,
    subtitles: Vec<PathBuf>,
    directives: LaunchDirectives,
    /// Local diagnostics for reserved `ok-player://` requests seen on this launch — the
    /// scheme is recognized but its external-control commands are [Later], so each is
    /// reported rather than opened as media (PRD §13.4).
    reserved_notices: Vec<String>,
}

impl LaunchArgs {
    fn has_payload(&self) -> bool {
        !self.items.is_empty() || !self.playlists.is_empty() || !self.subtitles.is_empty()
    }

    /// The message to surface for the first reserved `ok-player://` request on this launch,
    /// if any. Only the first is shown so a batch of URIs cannot flood the toast.
    fn reserved_notice(&self) -> Option<&str> {
        self.reserved_notices.first().map(String::as_str)
    }

    fn has_media_payload(&self) -> bool {
        !self.items.is_empty() || !self.playlists.is_empty()
    }
}

#[derive(Clone)]
struct AppRuntime {
    window: gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
}

#[derive(Clone)]
struct MprisController {
    snapshot: Arc<Mutex<MprisSnapshot>>,
    commands: mpsc::Sender<MprisCommand>,
    signals: mpsc::Sender<MprisSignal>,
}

#[derive(Clone, Debug, PartialEq)]
struct MprisSnapshot {
    has_media: bool,
    paused: bool,
    position_us: i64,
    duration_us: Option<i64>,
    volume: f64,
    rate: f64,
    repeat_mode: RepeatMode,
    shuffle: bool,
    can_go_next: bool,
    can_go_previous: bool,
    track_id: OwnedObjectPath,
    title: String,
    uri: Option<String>,
    art_url: Option<String>,
    tracklist: Vec<MprisTrack>,
    current_track_id: Option<OwnedObjectPath>,
}

#[derive(Clone, Debug, PartialEq)]
struct MprisTrack {
    id: OwnedObjectPath,
    title: String,
    uri: Option<String>,
    duration_us: Option<i64>,
    art_url: Option<String>,
}

impl Default for MprisSnapshot {
    fn default() -> Self {
        Self {
            has_media: false,
            paused: true,
            position_us: 0,
            duration_us: None,
            volume: 1.0,
            rate: 1.0,
            repeat_mode: RepeatMode::Off,
            shuffle: false,
            can_go_next: false,
            can_go_previous: false,
            track_id: mpris_track_id(),
            title: "OK Player".to_owned(),
            uri: None,
            art_url: None,
            tracklist: Vec::new(),
            current_track_id: None,
        }
    }
}

impl MprisSnapshot {
    fn playback_status(&self) -> &'static str {
        if !self.has_media {
            "Stopped"
        } else if self.paused {
            "Paused"
        } else {
            "Playing"
        }
    }

    fn tracklist_track_ids(&self) -> Vec<OwnedObjectPath> {
        self.tracklist
            .iter()
            .map(|track| track.id.clone())
            .collect()
    }
}

#[derive(Clone, Debug)]
enum MprisCommand {
    Raise,
    Quit,
    Play,
    Pause,
    PlayPause,
    Stop,
    Previous,
    Next,
    SeekBy(i64),
    SetPosition(i64),
    SetVolume(f64),
    SetRate(f64),
    SetLoopStatus(String),
    SetShuffle(bool),
    GoToTrack(String),
    OpenUri(String),
}

#[derive(Clone, Debug)]
enum MprisSignal {
    PlayerPropertiesInvalidated(Vec<&'static str>),
    TrackListPropertiesInvalidated(Vec<&'static str>),
    TrackListReplaced {
        tracks: Vec<OwnedObjectPath>,
        current_track: OwnedObjectPath,
    },
    Seeked(i64),
}

#[derive(Clone)]
struct MprisRoot {
    commands: mpsc::Sender<MprisCommand>,
}

impl MprisRoot {
    fn send(&self, command: MprisCommand) -> zbus::fdo::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| zbus::fdo::Error::Failed("OK Player command channel is closed".to_owned()))
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    fn raise(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Raise)
    }

    fn quit(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Quit)
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn fullscreen(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_set_fullscreen(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "OK Player"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "com.befeast.okplayer"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        ["file", "http", "https"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        LINUX_KEY_MEDIA_MIME_TYPES
            .iter()
            .map(|mime| (*mime).to_owned())
            .collect()
    }
}

#[derive(Clone)]
struct MprisPlayer {
    snapshot: Arc<Mutex<MprisSnapshot>>,
    commands: mpsc::Sender<MprisCommand>,
}

impl MprisPlayer {
    fn send(&self, command: MprisCommand) -> zbus::fdo::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| zbus::fdo::Error::Failed("OK Player command channel is closed".to_owned()))
    }

    fn snapshot(&self) -> MprisSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone)]
struct MprisTrackList {
    snapshot: Arc<Mutex<MprisSnapshot>>,
    commands: mpsc::Sender<MprisCommand>,
}

impl MprisTrackList {
    fn send(&self, command: MprisCommand) -> zbus::fdo::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| zbus::fdo::Error::Failed("OK Player command channel is closed".to_owned()))
    }

    fn snapshot(&self) -> MprisSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.TrackList")]
impl MprisTrackList {
    fn get_tracks_metadata(
        &self,
        track_ids: Vec<OwnedObjectPath>,
    ) -> Vec<HashMap<String, OwnedValue>> {
        let snapshot = self.snapshot();
        track_ids
            .into_iter()
            .filter_map(|track_id| {
                snapshot
                    .tracklist
                    .iter()
                    .find(|track| track.id == track_id)
                    .map(mpris_track_metadata)
            })
            .collect()
    }

    fn add_track(
        &self,
        _uri: &str,
        _after_track: OwnedObjectPath,
        _set_as_current: bool,
    ) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::NotSupported(
            "OK Player exposes a read-only MPRIS track list".to_owned(),
        ))
    }

    fn remove_track(&self, _track_id: OwnedObjectPath) -> zbus::fdo::Result<()> {
        Err(zbus::fdo::Error::NotSupported(
            "OK Player exposes a read-only MPRIS track list".to_owned(),
        ))
    }

    fn go_to(&self, track_id: OwnedObjectPath) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::GoToTrack(track_id.to_string()))
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn tracks(&self) -> Vec<OwnedObjectPath> {
        self.snapshot()
            .tracklist
            .iter()
            .map(|track| track.id.clone())
            .collect()
    }

    #[zbus(property)]
    fn can_edit_tracks(&self) -> bool {
        false
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl MprisPlayer {
    fn next(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Next)
    }

    fn previous(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Previous)
    }

    fn pause(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Pause)
    }

    fn play_pause(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::PlayPause)
    }

    fn stop(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Stop)
    }

    fn play(&self) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::Play)
    }

    fn seek(&self, offset: i64) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::SeekBy(offset))
    }

    fn set_position(&self, _track_id: OwnedObjectPath, position: i64) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::SetPosition(position))
    }

    fn open_uri(&self, uri: &str) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::OpenUri(uri.to_owned()))
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.snapshot().playback_status().to_owned()
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn loop_status(&self) -> &str {
        mpris_loop_status(self.snapshot().repeat_mode)
    }

    #[zbus(property)]
    fn set_loop_status(&self, status: &str) -> zbus::fdo::Result<()> {
        if mpris_repeat_mode(status).is_none() {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "Unsupported LoopStatus: {status}"
            )));
        }
        self.send(MprisCommand::SetLoopStatus(status.to_owned()))
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn rate(&self) -> f64 {
        self.snapshot().rate
    }

    #[zbus(property)]
    fn set_rate(&self, rate: f64) -> zbus::fdo::Result<()> {
        if !rate.is_finite() {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Rate must be finite".to_owned(),
            ));
        }
        self.send(MprisCommand::SetRate(rate))
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn shuffle(&self) -> bool {
        self.snapshot().shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&self, shuffle: bool) -> zbus::fdo::Result<()> {
        self.send(MprisCommand::SetShuffle(shuffle))
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        mpris_metadata(&self.snapshot())
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn volume(&self) -> f64 {
        self.snapshot().volume
    }

    #[zbus(property)]
    fn set_volume(&self, volume: f64) -> zbus::fdo::Result<()> {
        if !volume.is_finite() {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Volume must be finite".to_owned(),
            ));
        }
        self.send(MprisCommand::SetVolume(volume))
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        self.snapshot().position_us
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        0.25
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        4.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.snapshot().can_go_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.snapshot().can_go_previous
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.snapshot().has_media
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.snapshot().has_media
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.snapshot().duration_us.is_some()
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

struct Controls {
    subtitle_button: gtk::MenuButton,
    audio_button: gtk::MenuButton,
    speed_button: gtk::MenuButton,
    previous_button: gtk::Button,
    play_button: gtk::Button,
    next_button: gtk::Button,
    chapters_button: gtk::Button,
    screenshot_button: gtk::Button,
    fullscreen_button: gtk::Button,
    more_button: gtk::MenuButton,
    // Mirrors the controls the adaptive OscBar folded into the overflow menu at
    // the current window width, so `controls_bar` can point the bar at the same
    // vec the `…` popover reads (issue #328).
    overflow_collapsed: Rc<RefCell<Vec<okp_core::osc_overflow::OscControlId>>>,
    timeline: gtk::Overlay,
    seek: gtk::Scale,
    timeline_rail: TimelineRail,
    elapsed_label: gtk::Label,
    duration_label: gtk::Label,
    trailing_time_mode: Rc<Cell<time_code::TrailingTimeMode>>,
    volume: VolumeControl,
    // Shared toast surface, kept so the side panel's own row handlers (add/remove a
    // bookmark) can report their outcome without threading a toast through every call.
    status_toast: Rc<StatusToast>,
    up_next_revealer: gtk::Revealer,
    side_panel_fade_revealer: gtk::Revealer,
    chapters_tab: gtk::Button,
    up_next_tab: gtk::Button,
    up_next_list: gtk::ListBox,
    side_panel_user_visible: Rc<Cell<bool>>,
    side_panel_pinned: Rc<Cell<bool>>,
    side_panel_mode: Rc<Cell<SidePanelMode>>,
    side_panel_manual_mode: Rc<Cell<bool>>,
    side_panel_snapshot: Rc<RefCell<SidePanelSnapshot>>,
    side_panel_actions: Rc<RefCell<Vec<SidePanelAction>>>,
    // State for the explicit scene-detection action. The portable transition model lives in
    // okp-core; this cell only mirrors the current media's UI state.
    chapter_detection: Rc<Cell<chapter_math::ChapterDetection>>,
    // When set, the live poll leaves the side panel alone so the visual smoke
    // hook (`OKP_OPEN_SIDE_PANEL_ON_STARTUP`) can render fixture rows that would
    // otherwise be cleared the moment the poll sees there is no loaded media.
    // The poll clears it as soon as real media loads, so a session that merely
    // inherited the env var falls back to live data instead of fixtures.
    side_panel_preview_frozen: Rc<Cell<bool>>,
    // The seek-bar hover tooltip (thumbnail + timecode + chapter). Kept so the visual
    // smoke hook (`OKP_OPEN_SEEK_PREVIEW_ON_STARTUP`) can pop it with fixture data to
    // screenshot the timecode-only fallback without loaded media or a thumbnail source.
    seek_hover_preview: Rc<SeekHoverPreview>,
    thumbnail_sender: mpsc::Sender<thumbnails::ThumbnailEvent>,
    thumbnail_events: RefCell<mpsc::Receiver<thumbnails::ThumbnailEvent>>,
}

#[derive(Clone)]
struct PlayerWindowChrome {
    revealer: gtk::Revealer,
    auto_hide_widgets: Vec<gtk::Widget>,
    persistent_widgets: Vec<gtk::Widget>,
    media_icon: gtk::DrawingArea,
    title_label: gtk::Label,
    always_on_top: Rc<Cell<bool>>,
}

#[derive(Clone)]
struct PlayerWindowBounds {
    monitor: Option<gdk::Monitor>,
    work_area: window_fit::WindowRect,
}

struct StatePollContext {
    updating_seek: Rc<Cell<bool>>,
    initial_map_pending: Rc<Cell<bool>>,
    chrome: Rc<ChromeVisibility>,
    compact_mode: CompactMode,
    window_chrome: PlayerWindowChrome,
    subtitle_position_snapshot: Rc<Cell<Option<i64>>>,
    empty_surface: EmptySurface,
    lyrics_surface: LyricsSurface,
    media_state_overlay: MediaStateOverlay,
    window_bounds: Rc<RefCell<Option<PlayerWindowBounds>>>,
    mpris_snapshot: Arc<Mutex<MprisSnapshot>>,
    mpris_signals: mpsc::Sender<MprisSignal>,
}

#[derive(Clone)]
struct PendingLinuxUpdate {
    manager: Option<UpdateManager>,
    target: LinuxUpdateTarget,
}

#[derive(Clone)]
enum LinuxUpdateTarget {
    Info(Box<UpdateInfo>),
    Asset(Box<VelopackAsset>),
    Deb(DebUpdate),
}

enum LinuxUpdateCheckResult {
    UpToDate,
    Available(PendingLinuxUpdate),
    Failed(String),
}

#[derive(Clone, Default)]
enum LinuxUpdateStatus {
    #[default]
    NotChecked,
    Checking,
    UpToDate,
    Available(PendingLinuxUpdate),
    Failed(String),
}

impl LinuxUpdateStatus {
    fn from_check_result(result: &LinuxUpdateCheckResult) -> Self {
        match result {
            LinuxUpdateCheckResult::UpToDate => Self::UpToDate,
            LinuxUpdateCheckResult::Available(update) => Self::Available(update.clone()),
            LinuxUpdateCheckResult::Failed(error) => Self::Failed(error.clone()),
        }
    }

    fn pending_update(&self) -> Option<PendingLinuxUpdate> {
        match self {
            Self::Available(update) => Some(update.clone()),
            _ => None,
        }
    }

    fn action_label(&self) -> String {
        match self {
            Self::Available(update) => update.action_label().to_owned(),
            Self::Checking => "Checking...".to_owned(),
            _ => "Check for updates".to_owned(),
        }
    }

    fn settings_status_text(&self, auto_check_enabled: bool) -> String {
        match self {
            Self::NotChecked => update_status_intro(auto_check_enabled).to_owned(),
            Self::Checking => "Checking the update feed...".to_owned(),
            Self::UpToDate => "OK Player is up to date".to_owned(),
            Self::Available(update) => update.available_status(),
            Self::Failed(error) => format!("Update check failed: {error}"),
        }
    }
}

enum LinuxUpdateApplyResult {
    Restarting,
    DebInstalled(PathBuf),
    InstallerOpened(PathBuf),
}

#[derive(Clone)]
struct EmptySurface {
    revealer: gtk::Revealer,
    canvas: gtk::Box,
    stack: gtk::Stack,
    welcome_host: gtk::Box,
    history_host: gtk::Box,
    footer: gtk::Box,
    footer_left_icon: gtk::Image,
    footer_left_label: gtk::Label,
    footer_status: gtk::Label,
    page: Rc<Cell<IdlePage>>,
    model: Rc<RefCell<Option<okp_core::recents_shelf::WelcomeShelf>>>,
    history_model: Rc<RefCell<Option<HistorySurfaceModel>>>,
    welcome_history_button: Rc<RefCell<Option<gtk::Button>>>,
    is_preview_substrate: Rc<Cell<bool>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum IdlePage {
    #[default]
    Welcome,
    History,
}

#[derive(Clone, Debug, PartialEq)]
struct HistorySurfaceModel {
    items: Vec<okp_core::recents_shelf::HistoryItem>,
    private_session: bool,
    read_failed: bool,
    cleared: bool,
    no_match: bool,
}

impl EmptySurface {
    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn set_has_media(&self, has_media: bool) {
        if self.is_preview_substrate.get() {
            self.revealer.set_reveal_child(true);
            self.revealer.set_can_target(false);
            return;
        }
        self.revealer.set_reveal_child(!has_media);
        self.revealer.set_can_target(!has_media);
        if has_media {
            self.revealer.add_css_class("has-media");
        } else {
            self.revealer.remove_css_class("has-media");
        }
    }

    fn set_preview_substrate(&self, bright: bool) {
        self.is_preview_substrate.set(true);
        self.stack.set_visible(false);
        self.footer.set_visible(false);
        self.revealer.add_css_class("is-preview-substrate");
        self.canvas.add_css_class("is-preview-substrate");
        if bright {
            self.revealer.add_css_class("is-preview-bright");
            self.canvas.add_css_class("is-preview-bright");
        }
        self.revealer.set_reveal_child(true);
        self.revealer.set_can_target(false);
    }

    fn clear_preview_substrate(&self) {
        if !self.is_preview_substrate.replace(false) {
            return;
        }
        self.revealer.remove_css_class("is-preview-bright");
        self.revealer.remove_css_class("is-preview-substrate");
        self.canvas.remove_css_class("is-preview-bright");
        self.canvas.remove_css_class("is-preview-substrate");
        self.stack.set_visible(true);
        self.footer.set_visible(true);
    }

    fn set_drop_active(&self, active: bool) {
        if active {
            self.revealer.add_css_class("is-drop-target");
        } else {
            self.revealer.remove_css_class("is-drop-target");
        }
    }
}

/// Canonical in-canvas playback state surface. The shared core owns the load
/// state; this widget only projects paused, loading, and failed presentations.
#[derive(Clone)]
struct MediaStateOverlay {
    revealer: gtk::Revealer,
    stack: gtk::Stack,
    spinner: gtk::Spinner,
    retry_button: gtk::Button,
}

impl MediaStateOverlay {
    fn new(
        parent: &gtk::ApplicationWindow,
        state: Rc<RefCell<PlayerState>>,
        status_toast: Rc<StatusToast>,
    ) -> Self {
        let paused = gtk::Label::new(Some("PAUSED"));
        paused.add_css_class("okp-paused-cue");
        paused.set_halign(gtk::Align::Center);
        paused.set_valign(gtk::Align::Center);

        let spinner = gtk::Spinner::new();
        spinner.add_css_class("okp-loading-ring");
        let loading_label = gtk::Label::new(Some("Loading"));
        loading_label.add_css_class("okp-loading-label");
        let loading = gtk::Box::new(gtk::Orientation::Vertical, 10);
        loading.add_css_class("okp-loading-state");
        loading.set_halign(gtk::Align::Center);
        loading.set_valign(gtk::Align::Center);
        loading.append(&spinner);
        loading.append(&loading_label);

        let error_icon = gtk::Image::from_icon_name("dialog-error-symbolic");
        error_icon.add_css_class("okp-error-icon");
        let error_title = gtk::Label::new(Some("Playback failed"));
        error_title.add_css_class("okp-error-title");
        let error_body = gtk::Label::new(Some(
            "OK Player could not open this source. You can retry or choose another.",
        ));
        error_body.add_css_class("okp-error-body");
        error_body.set_wrap(true);
        error_body.set_max_width_chars(46);
        error_body.set_justify(gtk::Justification::Center);

        let retry_button = gtk::Button::with_label("Retry");
        retry_button.add_css_class("okp-error-primary");
        let retry_state = Rc::clone(&state);
        retry_button.connect_clicked(move |_| {
            let source = retry_state.borrow().retry_load_source.clone();
            match source {
                Some(network_media::LoadFailureSource::Url(url)) => {
                    load_media_url(&retry_state, url)
                }
                Some(network_media::LoadFailureSource::Local(path)) => {
                    load_media_path(&retry_state, path)
                }
                None => {}
            }
        });

        let open_button = gtk::Button::with_label("Open another");
        open_button.add_css_class("okp-error-secondary");
        let open_parent = parent.clone();
        let open_state = Rc::clone(&state);
        let open_toast = Rc::clone(&status_toast);
        open_button.connect_clicked(move |_| {
            if open_state
                .borrow()
                .retry_load_source
                .as_ref()
                .is_some_and(network_media::LoadFailureSource::is_url)
            {
                open_url_dialog(&open_parent, Rc::clone(&open_state), Rc::clone(&open_toast));
            } else {
                open_media_dialog(&open_parent, Rc::clone(&open_state), Rc::clone(&open_toast));
            }
        });

        let copy_button = gtk::Button::with_label("Copy details");
        copy_button.add_css_class("okp-error-secondary");
        let copy_state = Rc::clone(&state);
        let copy_toast = Rc::clone(&status_toast);
        copy_button.connect_clicked(move |_| {
            let detail = {
                let state = copy_state.borrow();
                let reason = state.last_load_error.as_deref().unwrap_or("");
                state
                    .retry_load_source
                    .as_ref()
                    .map(|source| network_media::failure_detail(source, reason))
                    .unwrap_or_else(|| {
                        if reason.trim().is_empty() {
                            "OK Player could not open the media.".to_owned()
                        } else {
                            format!("OK Player could not open the media.\nReason: {reason}")
                        }
                    })
            };
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&detail);
                copy_toast.show("Copied details");
            }
        });

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::Center);
        actions.append(&retry_button);
        actions.append(&open_button);
        actions.append(&copy_button);

        let error = gtk::Box::new(gtk::Orientation::Vertical, 8);
        error.add_css_class("okp-error-card");
        error.set_halign(gtk::Align::Center);
        error.set_valign(gtk::Align::Center);
        error.append(&error_icon);
        error.append(&error_title);
        error.append(&error_body);
        error.append(&actions);

        let stack = gtk::Stack::new();
        stack.set_halign(gtk::Align::Center);
        stack.set_valign(gtk::Align::Center);
        stack.add_named(&paused, Some("paused"));
        stack.add_named(&loading, Some("loading"));
        stack.add_named(&error, Some("error"));

        let revealer = gtk::Revealer::new();
        revealer.add_css_class("okp-media-state-overlay");
        revealer.set_halign(gtk::Align::Fill);
        revealer.set_valign(gtk::Align::Fill);
        revealer.set_transition_duration(if playback_animations_enabled() {
            180
        } else {
            0
        });
        revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        revealer.set_can_target(false);
        revealer.set_reveal_child(false);
        revealer.set_child(Some(&stack));

        Self {
            revealer,
            stack,
            spinner,
            retry_button,
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn update(
        &self,
        load_state: network_media::MediaLoadState,
        has_media: bool,
        paused: bool,
        can_retry: bool,
    ) {
        let state = match load_state {
            network_media::MediaLoadState::Failed => Some("error"),
            network_media::MediaLoadState::Loading if has_media => Some("loading"),
            network_media::MediaLoadState::Playing if has_media && paused => Some("paused"),
            _ => None,
        };

        if let Some(state) = state {
            self.stack.set_visible_child_name(state);
            self.revealer.set_reveal_child(true);
            let is_error = state == "error";
            self.revealer.set_can_target(is_error);
            self.retry_button.set_sensitive(can_retry);
            if state == "loading" {
                self.spinner.start();
            } else {
                self.spinner.stop();
            }
        } else {
            self.spinner.stop();
            self.revealer.set_can_target(false);
            self.revealer.set_reveal_child(false);
        }
    }
}

struct ChromeVisibility {
    revealer: gtk::Revealer,
    linked_revealers: Rc<RefCell<Vec<gtk::Revealer>>>,
    linked_motion_widgets: Rc<RefCell<Vec<gtk::Widget>>>,
    linked_persistent_widgets: Rc<RefCell<Vec<gtk::Widget>>>,
    cursor_widgets: Rc<RefCell<Vec<gtk::Widget>>>,
    hide_source: Rc<RefCell<Option<glib::SourceId>>>,
    pin_count: Rc<Cell<u32>>,
    auto_hide_enabled: Rc<Cell<bool>>,
    surface_suppressed: Rc<Cell<bool>>,
    is_revealed: Rc<Cell<bool>>,
}

impl ChromeVisibility {
    fn new() -> Self {
        let revealer = gtk::Revealer::new();
        revealer.add_css_class("okp-chrome-revealer");
        revealer.set_halign(gtk::Align::Fill);
        revealer.set_valign(gtk::Align::End);
        revealer.set_transition_duration(0);
        revealer.set_transition_type(gtk::RevealerTransitionType::None);
        revealer.set_reveal_child(true);
        revealer.set_can_target(false);
        revealer.set_visible(false);

        Self {
            revealer,
            linked_revealers: Rc::new(RefCell::new(Vec::new())),
            linked_motion_widgets: Rc::new(RefCell::new(Vec::new())),
            linked_persistent_widgets: Rc::new(RefCell::new(Vec::new())),
            cursor_widgets: Rc::new(RefCell::new(Vec::new())),
            hide_source: Rc::new(RefCell::new(None)),
            pin_count: Rc::new(Cell::new(0)),
            auto_hide_enabled: Rc::new(Cell::new(false)),
            surface_suppressed: Rc::new(Cell::new(false)),
            is_revealed: Rc::new(Cell::new(true)),
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        self.revealer.set_child(Some(child));
    }

    fn add_linked_revealer(&self, revealer: &gtk::Revealer) {
        Self::set_revealer_state(revealer, self.is_revealed.get());
        self.linked_revealers.borrow_mut().push(revealer.clone());
    }

    fn add_linked_motion_widget(&self, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.clone().upcast::<gtk::Widget>();
        Self::set_motion_widget_state(&widget, self.is_revealed.get());
        self.linked_motion_widgets.borrow_mut().push(widget);
    }

    fn add_persistent_widget(&self, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.clone().upcast::<gtk::Widget>();
        Self::set_persistent_widget_state(&widget, self.is_revealed.get());
        self.linked_persistent_widgets.borrow_mut().push(widget);
    }

    fn add_cursor_widget(&self, widget: &impl IsA<gtk::Widget>) {
        self.cursor_widgets
            .borrow_mut()
            .push(widget.clone().upcast::<gtk::Widget>());
    }

    fn is_revealed(&self) -> bool {
        self.is_revealed.get()
    }

    fn set_has_media(&self, has_media: bool) {
        self.revealer
            .set_visible(has_media && !self.surface_suppressed.get());
        self.revealer
            .set_can_target(has_media && self.is_revealed.get());
    }

    fn set_surface_suppressed(&self, suppressed: bool) {
        self.surface_suppressed.set(suppressed);
        if suppressed {
            self.revealer.set_visible(false);
        }
    }

    fn set_auto_hide_enabled(&self, enabled: bool) {
        let was_enabled = self.auto_hide_enabled.replace(enabled);
        if enabled && self.pin_count.get() == 0 {
            if !was_enabled || (self.is_revealed.get() && self.hide_source.borrow().is_none()) {
                self.schedule_hide();
            }
        } else {
            self.show_persistently();
        }
    }

    fn show_for_activity(&self) {
        self.show_now();
        if self.auto_hide_enabled.get() && self.pin_count.get() == 0 {
            self.schedule_hide();
        }
    }

    fn pin(&self) {
        self.pin_count.set(self.pin_count.get().saturating_add(1));
        self.show_persistently();
    }

    fn unpin(&self) {
        self.pin_count.set(self.pin_count.get().saturating_sub(1));
        if self.auto_hide_enabled.get() && self.pin_count.get() == 0 {
            self.schedule_hide();
        }
    }

    fn show_persistently(&self) {
        self.cancel_hide();
        self.show_now();
    }

    fn show_now(&self) {
        self.is_revealed.set(true);
        self.set_all_revealed(true);
    }

    fn set_all_revealed(&self, revealed: bool) {
        Self::set_motion_widget_state(&self.revealer, revealed);
        for widget in self.linked_motion_widgets.borrow().iter() {
            Self::set_motion_widget_state(widget, revealed);
        }
        for widget in self.linked_persistent_widgets.borrow().iter() {
            Self::set_persistent_widget_state(widget, revealed);
        }
        for revealer in self.linked_revealers.borrow().iter() {
            Self::set_revealer_state(revealer, revealed);
        }
        for widget in self.cursor_widgets.borrow().iter() {
            Self::set_cursor_revealed(widget, revealed);
        }
    }

    fn set_cursor_revealed(widget: &impl IsA<gtk::Widget>, revealed: bool) {
        widget.set_cursor_from_name(if revealed { None } else { Some("none") });
    }

    fn set_motion_widget_state(widget: &impl IsA<gtk::Widget>, revealed: bool) {
        widget.set_can_target(revealed);
        widget.set_sensitive(revealed);
        if revealed {
            widget.remove_css_class("is-hidden");
        } else {
            widget.add_css_class("is-hidden");
        }
    }

    fn set_persistent_widget_state(widget: &impl IsA<gtk::Widget>, chrome_revealed: bool) {
        if chrome_revealed {
            widget.remove_css_class("is-isolated");
        } else {
            widget.add_css_class("is-isolated");
        }
    }

    fn set_revealer_state(revealer: &gtk::Revealer, revealed: bool) {
        revealer.set_can_target(revealed);
        revealer.set_reveal_child(revealed);
    }

    fn schedule_hide(&self) {
        if !self.is_revealed.get() {
            return;
        }
        self.cancel_hide();

        let revealer = self.revealer.clone();
        let linked_revealers = Rc::clone(&self.linked_revealers);
        let linked_motion_widgets = Rc::clone(&self.linked_motion_widgets);
        let linked_persistent_widgets = Rc::clone(&self.linked_persistent_widgets);
        let cursor_widgets = Rc::clone(&self.cursor_widgets);
        let hide_source = Rc::clone(&self.hide_source);
        let pin_count = Rc::clone(&self.pin_count);
        let auto_hide_enabled = Rc::clone(&self.auto_hide_enabled);
        let is_revealed = Rc::clone(&self.is_revealed);
        let source_id = glib::timeout_add_local(Duration::from_millis(2500), move || {
            hide_source.borrow_mut().take();
            if auto_hide_enabled.get() && pin_count.get() == 0 {
                is_revealed.set(false);
                Self::set_motion_widget_state(&revealer, false);
                for widget in linked_motion_widgets.borrow().iter() {
                    Self::set_motion_widget_state(widget, false);
                }
                for widget in linked_persistent_widgets.borrow().iter() {
                    Self::set_persistent_widget_state(widget, false);
                }
                for revealer in linked_revealers.borrow().iter() {
                    Self::set_revealer_state(revealer, false);
                }
                for widget in cursor_widgets.borrow().iter() {
                    Self::set_cursor_revealed(widget, false);
                }
            }
            glib::ControlFlow::Break
        });
        self.hide_source.borrow_mut().replace(source_id);
    }

    fn cancel_hide(&self) {
        if let Some(source_id) = self.hide_source.borrow_mut().take() {
            source_id.remove();
        }
    }
}

struct StatusToast {
    revealer: gtk::Revealer,
    thumbnail: gtk::Image,
    label: gtk::Label,
    hide_source: Rc<RefCell<Option<glib::SourceId>>>,
}

impl StatusToast {
    fn new() -> Self {
        let thumbnail = gtk::Image::new();
        thumbnail.add_css_class("okp-status-toast-thumbnail");
        thumbnail.set_size_request(64, 36);
        thumbnail.set_pixel_size(64);
        thumbnail.set_halign(gtk::Align::Start);
        thumbnail.set_valign(gtk::Align::Center);
        thumbnail.set_visible(false);

        let label = gtk::Label::new(None);
        label.set_ellipsize(pango::EllipsizeMode::Middle);
        label.set_max_width_chars(72);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        content.add_css_class("okp-status-toast");
        content.append(&thumbnail);
        content.append(&label);

        let revealer = gtk::Revealer::new();
        revealer.set_halign(gtk::Align::Center);
        revealer.set_valign(gtk::Align::Start);
        revealer.set_margin_top(64);
        revealer.set_transition_duration(if playback_animations_enabled() {
            150
        } else {
            0
        });
        revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        revealer.set_reveal_child(false);
        revealer.set_can_target(false);
        revealer.set_child(Some(&content));

        Self {
            revealer,
            thumbnail,
            label,
            hide_source: Rc::new(RefCell::new(None)),
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn show(&self, message: &str) {
        self.thumbnail.set_visible(false);
        self.thumbnail.set_paintable(None::<&gdk::Paintable>);
        self.reveal(message);
    }

    fn show_screenshot(&self, message: &str, path: &Path) {
        if let Ok(texture) = gdk::Texture::from_filename(path) {
            self.thumbnail.set_paintable(Some(&texture));
            self.thumbnail.set_visible(true);
        } else {
            self.thumbnail.set_visible(false);
            self.thumbnail.set_paintable(None::<&gdk::Paintable>);
        }
        self.reveal(message);
    }

    fn reveal(&self, message: &str) {
        self.label.set_text(message);
        self.revealer.set_reveal_child(true);

        if let Some(source_id) = self.hide_source.borrow_mut().take() {
            source_id.remove();
        }

        let revealer = self.revealer.clone();
        let hide_source = Rc::clone(&self.hide_source);
        let source_id = glib::timeout_add_local(Duration::from_millis(1700), move || {
            revealer.set_reveal_child(false);
            hide_source.borrow_mut().take();
            glib::ControlFlow::Break
        });
        self.hide_source.borrow_mut().replace(source_id);
    }
}

struct SeekHoverPreview {
    root: gtk::Fixed,
    content: gtk::Box,
    thumbnail: gtk::Picture,
    thumbnail_snapshot: RefCell<Option<PathBuf>>,
    thumbnail_request_key: RefCell<Option<String>>,
    anchor: Cell<Option<(f64, f64)>>,
    time_label: gtk::Label,
    chapter_label: gtk::Label,
}

impl SeekHoverPreview {
    fn new() -> Self {
        let thumbnail = gtk::Picture::new();
        thumbnail.add_css_class("okp-seek-preview-thumb");
        thumbnail.set_size_request(144, 81);
        thumbnail.set_can_shrink(true);
        thumbnail.set_visible(false);

        let time_label = gtk::Label::new(Some("00:00"));
        time_label.add_css_class("okp-seek-preview-time");

        let chapter_label = gtk::Label::new(None);
        chapter_label.add_css_class("okp-seek-preview-chapter");
        chapter_label.set_ellipsize(pango::EllipsizeMode::End);
        chapter_label.set_max_width_chars(32);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("okp-seek-preview");
        content.set_can_target(false);
        content.set_visible(false);
        content.append(&thumbnail);
        content.append(&time_label);
        content.append(&chapter_label);

        let root = gtk::Fixed::new();
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_halign(gtk::Align::Fill);
        root.set_valign(gtk::Align::Fill);
        root.set_can_target(false);
        root.put(&content, 0.0, 0.0);

        Self {
            root,
            content,
            thumbnail,
            thumbnail_snapshot: RefCell::new(None),
            thumbnail_request_key: RefCell::new(None),
            anchor: Cell::new(None),
            time_label,
            chapter_label,
        }
    }

    fn widget(&self) -> &gtk::Fixed {
        &self.root
    }

    fn show(
        &self,
        seek: &gtk::Scale,
        x: f64,
        time: f64,
        chapter: Option<&Chapter>,
        thumbnail: Option<PathBuf>,
        thumbnail_request_key: Option<String>,
    ) {
        let width = seek.width().max(1);
        let x = x.clamp(0.0, f64::from(width));
        if let Some(thumbnail_path) = thumbnail {
            let mut snapshot = self.thumbnail_snapshot.borrow_mut();
            if snapshot.as_ref() != Some(&thumbnail_path) {
                self.thumbnail.set_filename(Some(&thumbnail_path));
                *snapshot = Some(thumbnail_path);
            }
            self.thumbnail.set_visible(true);
        } else {
            self.thumbnail.set_visible(false);
            self.thumbnail_snapshot.borrow_mut().take();
        }
        self.thumbnail_request_key.replace(thumbnail_request_key);

        self.time_label.set_text(&time_code::format_clock(time));
        if let Some(chapter) = chapter {
            let title = chapter
                .title
                .as_deref()
                .filter(|title| !title.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));
            self.chapter_label.set_text(&title);
            self.chapter_label.set_visible(true);
        } else {
            self.chapter_label.set_visible(false);
        }

        let Some(bounds) = seek.compute_bounds(&self.root) else {
            self.hide();
            return;
        };
        self.content.set_visible(true);
        self.anchor
            .set(Some((f64::from(bounds.x()) + x, f64::from(bounds.y()))));
        self.position_at_anchor();
    }

    fn show_thumbnail_if_current(&self, request_key: &str, path: &Path) {
        if self.thumbnail_request_key.borrow().as_deref() != Some(request_key) {
            return;
        }

        let path = path.to_path_buf();
        let mut snapshot = self.thumbnail_snapshot.borrow_mut();
        if snapshot.as_ref() != Some(&path) {
            self.thumbnail.set_filename(Some(&path));
            *snapshot = Some(path);
        }
        self.thumbnail.set_visible(true);
        self.position_at_anchor();
    }

    fn hide(&self) {
        self.thumbnail_request_key.borrow_mut().take();
        self.anchor.set(None);
        self.content.set_visible(false);
    }

    fn position_at_anchor(&self) {
        let Some((anchor_x, anchor_y)) = self.anchor.get() else {
            return;
        };
        let (_, width, _, _) = self.content.measure(gtk::Orientation::Horizontal, -1);
        let (_, height, _, _) = self.content.measure(gtk::Orientation::Vertical, width);
        let width = width.max(1);
        let height = height.max(1);
        let root_width = self.root.width().max(width + 16);
        let left = (anchor_x - f64::from(width) / 2.0)
            .clamp(8.0, f64::from((root_width - width - 8).max(8)));
        let top = (anchor_y - f64::from(height) - 8.0).max(8.0);
        self.root.move_(&self.content, left, top);
    }
}

#[derive(Clone, Default, PartialEq)]
struct SidePanelSnapshot {
    has_media: bool,
    current_file: Option<PathBuf>,
    current_url: Option<String>,
    current_title: Option<String>,
    playlist: Vec<PlaylistItem>,
    chapters: Vec<Chapter>,
    // Index of the chapter the playhead currently sits in (via
    // `chapter_math::current_index`). Kept as the resolved index rather than the
    // raw position so the panel only re-renders when the playhead crosses a
    // chapter boundary, not on every poll tick.
    current_chapter: Option<usize>,
    // Known media duration, used by the core model to synthesize interval fallback markers.
    duration: Option<f64>,
    // The user's saved position bookmarks for the current local file (empty for streams
    // and unbookmarked media). Carried in the snapshot so the panel re-renders the
    // Bookmarks section the moment a mark is added or removed.
    bookmarks: Vec<f64>,
    ab_loop: AbLoopState,
    detection: chapter_math::ChapterDetection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimelineMark {
    time: f64,
    kind: TimelineMarkKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TimelineMarkKind {
    Chapter,
    Interval,
    Bookmark,
    AbStart,
    AbEnd,
    AbLoop,
}

#[derive(Clone, Copy)]
enum SidePanelAction {
    None,
    Chapter(f64),
    Playlist(usize),
    AddBookmark,
    AddFiles,
    DetectChapters,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidePanelMode {
    Chapters,
    UpNext,
}

const SIDE_PANEL_WIDTH: i32 = 316;
const SIDE_PANEL_TOP_INSET: i32 = 44;
const SIDE_PANEL_BOTTOM_INSET: i32 = 80;
const SIDE_PANEL_TRANSITION_MS: u32 = 250;

fn main() -> glib::ExitCode {
    VelopackApp::build().set_auto_apply_on_startup(false).run();

    let app = gtk::Application::builder()
        .application_id("com.befeast.okplayer")
        .flags(gtk::gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    let runtime = Rc::new(RefCell::new(None::<AppRuntime>));
    app.connect_command_line(move |app, command_line| {
        let mut args = command_line.arguments().into_iter();
        let _argv0 = args.next();
        let cwd = command_line.cwd();
        let launch_args = parse_launch_args_from_cwd(args, cwd.as_deref());

        if let Some(runtime) = runtime.borrow().as_ref() {
            open_runtime_launch_args(runtime, &launch_args);
        } else {
            runtime.replace(Some(build_window(app, launch_args)));
        }

        glib::ExitCode::SUCCESS
    });
    app.run()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawMpvConfigError {
    line: usize,
    message: String,
}

#[derive(Clone, Copy)]
enum SubtitleAdjustment {
    Delay(f64),
    SetDelay(f64),
}

#[derive(Clone, Copy)]
enum SettingsNavIcon {
    Appearance,
    Playback,
    Subtitles,
    Video,
    Audio,
    Shortcuts,
    Integration,
    Advanced,
    About,
}

#[derive(Clone, Copy)]
enum WindowControlKind {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[derive(Clone)]
struct AboutSnapshot {
    version: String,
    package_version: String,
    channel: String,
    build: String,
    license: String,
    libmpv: String,
    ffmpeg: String,
    render_api: String,
    graphics: String,
    hwdec: String,
    os: String,
    gtk: String,
    cpu: String,
    install: String,
}

impl AboutSnapshot {
    fn capture(state: &Rc<RefCell<PlayerState>>) -> Self {
        let state = state.borrow();
        let hwdec = state.settings.hardware_decode_label().to_owned();
        Self {
            version: about_display_version(APP_BUILD_VERSION),
            package_version: APP_BUILD_VERSION.to_owned(),
            channel: about_display_channel(APP_BUILD_VERSION),
            build: APP_BUILD_SHA.to_owned(),
            license: "GPL-3.0-or-later".to_owned(),
            libmpv: pkg_config_version("mpv").unwrap_or_else(|| "system".to_owned()),
            ffmpeg: ffmpeg_version().unwrap_or_else(|| "system".to_owned()),
            render_api: "libmpv render".to_owned(),
            graphics: "OpenGL · GTK GLArea".to_owned(),
            hwdec,
            os: linux_os_label(),
            gtk: format!(
                "{}.{}.{}",
                gtk::major_version(),
                gtk::minor_version(),
                gtk::micro_version()
            ),
            cpu: env::consts::ARCH.to_owned(),
            install: linux_update_install_status().to_owned(),
        }
    }
}

impl PendingLinuxUpdate {
    fn target_version(&self) -> Option<String> {
        match &self.target {
            LinuxUpdateTarget::Info(info) => Some(info.TargetFullRelease.Version.clone()),
            LinuxUpdateTarget::Asset(asset) => Some(asset.Version.clone()),
            LinuxUpdateTarget::Deb(update) => Some(update.version.clone()),
        }
    }

    fn action_label(&self) -> &'static str {
        match &self.target {
            LinuxUpdateTarget::Info(_) | LinuxUpdateTarget::Asset(_) => "Download and Restart",
            LinuxUpdateTarget::Deb(_) => "Install .deb",
        }
    }

    fn available_status(&self) -> String {
        match &self.target {
            LinuxUpdateTarget::Info(_) | LinuxUpdateTarget::Asset(_) => format!(
                "Available: {}",
                self.target_version()
                    .unwrap_or_else(|| "new version".to_owned())
            ),
            LinuxUpdateTarget::Deb(update) => format!("Available: {}", update.version),
        }
    }
}

#[derive(Clone, Copy)]
enum IntegrationStatus {
    Good,
    Warning,
    Bad,
}

impl IntegrationStatus {
    fn css_class(self) -> &'static str {
        match self {
            Self::Good => "is-good",
            Self::Warning => "is-warning",
            Self::Bad => "is-bad",
        }
    }
}

#[derive(Clone, Debug)]
struct LinuxIntegrationSnapshot {
    desktop_entry_path: Option<PathBuf>,
    registered_key_mimes: usize,
    default_key_mimes: Option<usize>,
    uri_scheme_registered: bool,
    uri_scheme_default: Option<bool>,
    xdg_mime_available: bool,
    update_desktop_database_available: bool,
}

impl LinuxIntegrationSnapshot {
    fn capture() -> Self {
        let desktop_entry_path = linux_desktop_entry_path();
        let desktop_entry = desktop_entry_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok());
        let registered_key_mimes = desktop_entry
            .as_deref()
            .map(count_registered_key_media_mimes)
            .unwrap_or_default();
        let uri_scheme_registered = desktop_entry
            .as_deref()
            .map(desktop_registers_uri_scheme)
            .unwrap_or(false);
        let xdg_mime_available = find_executable("xdg-mime").is_some();
        let default_key_mimes = xdg_mime_available.then(count_default_key_media_mimes);
        let uri_scheme_default = xdg_mime_available.then(uri_scheme_default_is_ok_player);

        Self {
            desktop_entry_path,
            registered_key_mimes,
            default_key_mimes,
            uri_scheme_registered,
            uri_scheme_default,
            xdg_mime_available,
            update_desktop_database_available: find_executable("update-desktop-database").is_some(),
        }
    }
}

struct SettingsSubtitleSnapshot {
    has_media: bool,
    primary: String,
    secondary: String,
    delay_seconds: f64,
}

#[derive(Clone, Copy)]
enum VideoAdjustment {
    Brightness,
    Contrast,
    Saturation,
    Gamma,
}

impl VideoAdjustment {
    fn label(self) -> &'static str {
        match self {
            Self::Brightness => "Brightness",
            Self::Contrast => "Contrast",
            Self::Saturation => "Saturation",
            Self::Gamma => "Gamma",
        }
    }

    fn read(self, settings: &settings::SettingsStore) -> f64 {
        match self {
            Self::Brightness => settings.brightness(),
            Self::Contrast => settings.contrast(),
            Self::Saturation => settings.saturation(),
            Self::Gamma => settings.gamma(),
        }
    }

    fn write(self, settings: &mut settings::SettingsStore, value: f64) {
        match self {
            Self::Brightness => settings.set_brightness(value),
            Self::Contrast => settings.set_contrast(value),
            Self::Saturation => settings.set_saturation(value),
            Self::Gamma => settings.set_gamma(value),
        }
    }

    fn apply(self, mpv: &Mpv, value: f64) -> Result<(), okp_mpv::MpvError> {
        match self {
            Self::Brightness => mpv.set_brightness(value),
            Self::Contrast => mpv.set_contrast(value),
            Self::Saturation => mpv.set_saturation(value),
            Self::Gamma => mpv.set_gamma(value),
        }
    }
}

struct ShortcutEditorRow {
    action: ShortcutAction,
    default_chord: ShortcutChord,
    primary_chord: RefCell<ShortcutChord>,
    secondary_chord: RefCell<Option<ShortcutChord>>,
    container: gtk::Box,
    primary_chip: gtk::Button,
    primary_chip_label: gtk::Label,
    secondary_chip: gtk::Button,
    secondary_chip_label: gtk::Label,
    badge: gtk::Label,
    reset: gtk::Button,
}

/// Adapts the GDK keyval tables to the core shortcut model's key namespace: config tokens
/// beyond the portable set resolve exactly as the pre-extraction parser did
/// (`gdk::Key::from_name`, case-sensitive), and the canonical name comes back case-folded via
/// `to_lower()` so cased letter keysyms compare consistently with key events.
struct GdkKeyNames;

impl shortcuts::KeyNames for GdkKeyNames {
    fn canonicalize_extra(&self, token: &str) -> Option<String> {
        gdk::Key::from_name(token)
            .and_then(|key| key.to_lower().name())
            .map(Into::into)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum M3uPlaylistReadError {
    NotPlaylist,
    ReadFailed,
    Empty,
}

#[cfg(test)]
mod tests;
