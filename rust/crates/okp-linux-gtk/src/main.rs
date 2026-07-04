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
use okp_core::playlist::{Playlist, PlaylistItem, QueueInsertMode, RepeatMode};
use okp_core::shortcuts::{
    self, ShortcutAction, ShortcutBinding, ShortcutChord, ShortcutModifiers, ShortcutSlot,
};
use okp_core::update_selection::{self, DebFeed, DebUpdate, SHA256SUMS_ASSET};
use okp_core::{
    AppIdentity, chapter_math, m3u, media_formats, natural_compare, sha256sums, subtitle_delay,
    time_code,
};
use okp_mpv::{
    AbLoopState, AudioDevice, Chapter, InfoRow, InfoSection, InfoTrack, MediaInfo, Mpv, MpvEvent,
    PlaybackState, Track, TrackKind, current_render_target_size, resolve_render_target_size,
};
use velopack::{
    UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackApp, VelopackAsset,
    sources::HttpSource,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

mod about;
mod controls;
mod css;
mod dialogs;
mod history;
mod integration;
mod keyboard;
mod media_info;
mod mpris;
mod mpv_bridge;
mod panels;
mod playback;
mod playlist_ops;
mod screenshots;
mod settings;
mod settings_pages;
mod settings_window;
mod thumbnails;
mod track_popovers;
mod updates;
mod window;
pub(crate) use about::*;
pub(crate) use controls::*;
pub(crate) use css::*;
pub(crate) use dialogs::*;
pub(crate) use integration::*;
pub(crate) use keyboard::*;
pub(crate) use media_info::*;
pub(crate) use mpris::*;
pub(crate) use mpv_bridge::*;
pub(crate) use panels::*;
pub(crate) use playback::*;
pub(crate) use playlist_ops::*;
pub(crate) use settings_pages::*;
pub(crate) use settings_window::*;
pub(crate) use track_popovers::*;
pub(crate) use updates::*;
pub(crate) use window::*;

const SPEED_PRESETS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
const APP_BUILD_VERSION: &str = env!("OKP_BUILD_VERSION");
const APP_BUILD_SHA: &str = env!("OKP_BUILD_SHA");
const LINUX_DESKTOP_ID: &str = "com.befeast.okplayer.desktop";
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
const LINUX_SHA256SUMS_MAX_BYTES: u64 = 1024 * 1024;
const UPDATE_STATUS_NOT_CHECKED: &str = "Not checked yet";
const DEB_SELF_INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
const SETTINGS_REFERENCE_WIDTH: i32 = 744;
const SETTINGS_REFERENCE_HEIGHT: i32 = 1030;
const SETTINGS_RAIL_WIDTH: i32 = 192;
const SETTINGS_CONTENT_WIDTH: i32 = SETTINGS_REFERENCE_WIDTH - SETTINGS_RAIL_WIDTH;
const CAPTIONLESS_DRAG_HEIGHT: i32 = 32;
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
const VIDEO_ASPECT_AUTO: &str = "no";
const VIDEO_ASPECT_PRESETS: [(&str, &str); 4] = [
    ("Auto", VIDEO_ASPECT_AUTO),
    ("16:9", "16:9"),
    ("4:3", "4:3"),
    ("2.35:1", "2.35:1"),
];
const AUDIO_DEVICE_AUTO: &str = "auto";
const AUDIO_DEVICE_RESTORE_MAX_ATTEMPTS: u8 = 50;
const AB_LOOP_COMBINED_MARK_EPSILON_SECS: f64 = 0.5;
const PROTECTED_MPV_OPTIONS: &[&str] = &["config", "terminal", "idle", "force-window", "vo"];

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
    playlist: Playlist,
    pending_subtitles: Vec<PathBuf>,
    pending_resume: Option<(PathBuf, f64)>,
    pending_preferences: Option<(PathBuf, history::PlaybackPreferences)>,
    thumbnail_request_key: Option<String>,
    hover_thumbnail_request_key: Option<String>,
    chapters_snapshot: Vec<Chapter>,
    private_session: bool,
    history: history::HistoryStore,
    settings: settings::SettingsStore,
    linux_update_status: LinuxUpdateStatus,
    pending_audio_device_restore: Option<PendingAudioDeviceRestore>,
    render_target_size: Option<okp_mpv::RenderTargetSize>,
    video_transform: VideoTransformState,
    ab_loop: AbLoopState,
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

#[derive(Clone, PartialEq, Eq)]
struct VideoTransformState {
    rotation: i64,
    fill_screen: bool,
    aspect_override: String,
}

impl Default for VideoTransformState {
    fn default() -> Self {
        Self {
            rotation: 0,
            fill_screen: false,
            aspect_override: VIDEO_ASPECT_AUTO.to_owned(),
        }
    }
}

impl VideoTransformState {
    fn rotate_clockwise(&mut self) -> i64 {
        self.rotation = (self.rotation + 90).rem_euclid(360);
        self.rotation
    }

    fn set_aspect(&mut self, aspect: &str) {
        self.aspect_override = video_aspect_value(aspect).to_owned();
    }

    fn toggle_fill_screen(&mut self) -> bool {
        self.fill_screen = !self.fill_screen;
        self.fill_screen
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Default)]
struct LaunchArgs {
    items: Vec<PlaylistItem>,
    playlists: Vec<PathBuf>,
    subtitles: Vec<PathBuf>,
}

impl LaunchArgs {
    fn has_payload(&self) -> bool {
        !self.items.is_empty() || !self.playlists.is_empty() || !self.subtitles.is_empty()
    }
}

#[derive(Clone)]
struct AppRuntime {
    window: gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
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
    open_button: gtk::Button,
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
    seek: gtk::Scale,
    elapsed_label: gtk::Label,
    duration_label: gtk::Label,
    volume: gtk::Scale,
    timeline_marks_snapshot: RefCell<Vec<TimelineMark>>,
    up_next_revealer: gtk::Revealer,
    up_next_title: gtk::Label,
    up_next_summary: gtk::Label,
    chapters_tab: gtk::Button,
    up_next_tab: gtk::Button,
    up_next_list: gtk::ListBox,
    side_panel_user_visible: Rc<Cell<bool>>,
    side_panel_pinned: Rc<Cell<bool>>,
    side_panel_mode: Rc<Cell<SidePanelMode>>,
    side_panel_manual_mode: Rc<Cell<bool>>,
    side_panel_snapshot: Rc<RefCell<SidePanelSnapshot>>,
    side_panel_actions: Rc<RefCell<Vec<SidePanelAction>>>,
    // When set, the live poll leaves the side panel alone so the visual smoke
    // hook (`OKP_OPEN_SIDE_PANEL_ON_STARTUP`) can render fixture rows that would
    // otherwise be cleared the moment the poll sees there is no loaded media.
    side_panel_preview_frozen: Rc<Cell<bool>>,
    thumbnail_sender: mpsc::Sender<String>,
    thumbnail_events: RefCell<mpsc::Receiver<String>>,
}

struct StatePollContext {
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
    chrome: Rc<ChromeVisibility>,
    empty_surface: EmptySurface,
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

    fn about_status_text(&self) -> String {
        match self {
            Self::NotChecked => UPDATE_STATUS_NOT_CHECKED.to_owned(),
            Self::Checking => "Checking...".to_owned(),
            Self::UpToDate => "Up to date".to_owned(),
            Self::Available(update) => update.available_status(),
            Self::Failed(_) => "Update check failed".to_owned(),
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
    panel: gtk::Box,
}

impl EmptySurface {
    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn set_has_media(&self, has_media: bool) {
        self.revealer.set_reveal_child(!has_media);
        self.revealer.set_can_target(!has_media);
    }

    fn set_drop_active(&self, active: bool) {
        if active {
            self.panel.add_css_class("is-drop-target");
        } else {
            self.panel.remove_css_class("is-drop-target");
        }
    }
}

struct ChromeVisibility {
    revealer: gtk::Revealer,
    linked_revealers: Rc<RefCell<Vec<gtk::Revealer>>>,
    hide_source: Rc<RefCell<Option<glib::SourceId>>>,
    pin_count: Rc<Cell<u32>>,
    auto_hide_enabled: Rc<Cell<bool>>,
    is_revealed: Rc<Cell<bool>>,
}

impl ChromeVisibility {
    fn new() -> Self {
        let revealer = gtk::Revealer::new();
        revealer.add_css_class("okp-chrome-revealer");
        revealer.set_halign(gtk::Align::Fill);
        revealer.set_valign(gtk::Align::End);
        revealer.set_transition_duration(170);
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_reveal_child(true);
        revealer.set_can_target(true);

        Self {
            revealer,
            linked_revealers: Rc::new(RefCell::new(Vec::new())),
            hide_source: Rc::new(RefCell::new(None)),
            pin_count: Rc::new(Cell::new(0)),
            auto_hide_enabled: Rc::new(Cell::new(false)),
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
        Self::set_revealer_state(&self.revealer, revealed);
        for revealer in self.linked_revealers.borrow().iter() {
            Self::set_revealer_state(revealer, revealed);
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
        let hide_source = Rc::clone(&self.hide_source);
        let pin_count = Rc::clone(&self.pin_count);
        let auto_hide_enabled = Rc::clone(&self.auto_hide_enabled);
        let is_revealed = Rc::clone(&self.is_revealed);
        let source_id = glib::timeout_add_local(Duration::from_millis(2600), move || {
            hide_source.borrow_mut().take();
            if auto_hide_enabled.get() && pin_count.get() == 0 {
                is_revealed.set(false);
                Self::set_revealer_state(&revealer, false);
                for revealer in linked_revealers.borrow().iter() {
                    Self::set_revealer_state(revealer, false);
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
    label: gtk::Label,
    hide_source: Rc<RefCell<Option<glib::SourceId>>>,
}

impl StatusToast {
    fn new() -> Self {
        let label = gtk::Label::new(None);
        label.add_css_class("okp-status-toast");
        label.set_ellipsize(pango::EllipsizeMode::Middle);
        label.set_max_width_chars(72);

        let revealer = gtk::Revealer::new();
        revealer.set_halign(gtk::Align::Center);
        revealer.set_valign(gtk::Align::Start);
        revealer.set_margin_top(28);
        revealer.set_transition_duration(140);
        revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        revealer.set_reveal_child(false);
        revealer.set_can_target(false);
        revealer.set_child(Some(&label));

        Self {
            revealer,
            label,
            hide_source: Rc::new(RefCell::new(None)),
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn show(&self, message: &str) {
        self.label.set_text(message);
        self.revealer.set_reveal_child(true);

        if let Some(source_id) = self.hide_source.borrow_mut().take() {
            source_id.remove();
        }

        let revealer = self.revealer.clone();
        let hide_source = Rc::clone(&self.hide_source);
        let source_id = glib::timeout_add_local(Duration::from_secs(3), move || {
            revealer.set_reveal_child(false);
            hide_source.borrow_mut().take();
            glib::ControlFlow::Break
        });
        self.hide_source.borrow_mut().replace(source_id);
    }
}

struct SeekHoverPreview {
    popover: gtk::Popover,
    thumbnail: gtk::Picture,
    thumbnail_snapshot: RefCell<Option<PathBuf>>,
    time_label: gtk::Label,
    chapter_label: gtk::Label,
}

impl SeekHoverPreview {
    fn new(seek: &gtk::Scale) -> Self {
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
        content.append(&thumbnail);
        content.append(&time_label);
        content.append(&chapter_label);

        let popover = gtk::Popover::new();
        popover.set_autohide(false);
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Top);
        popover.set_child(Some(&content));
        popover.set_parent(seek);

        Self {
            popover,
            thumbnail,
            thumbnail_snapshot: RefCell::new(None),
            time_label,
            chapter_label,
        }
    }

    fn show(
        &self,
        seek: &gtk::Scale,
        x: f64,
        time: f64,
        chapter: Option<&Chapter>,
        thumbnail: Option<PathBuf>,
    ) {
        let width = seek.width().max(1);
        let height = seek.height().max(1);
        let x = x.clamp(0.0, f64::from(width)).round() as i32;
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
        self.popover
            .set_pointing_to(Some(&gdk::Rectangle::new(x, 0, 1, height)));
        self.popover.popup();
    }

    fn hide(&self) {
        self.popover.popdown();
    }
}

#[derive(Clone, Default, PartialEq)]
struct SidePanelSnapshot {
    has_media: bool,
    current_file: Option<PathBuf>,
    current_url: Option<String>,
    playlist: Vec<PlaylistItem>,
    chapters: Vec<Chapter>,
    // Index of the chapter the playhead currently sits in (via
    // `chapter_math::current_index`). Kept as the resolved index rather than the
    // raw position so the panel only re-renders when the playhead crosses a
    // chapter boundary, not on every poll tick.
    current_chapter: Option<usize>,
    ab_loop: AbLoopState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimelineMark {
    time: f64,
    kind: TimelineMarkKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TimelineMarkKind {
    Chapter,
    AbStart,
    AbEnd,
    AbLoop,
}

#[derive(Clone, Copy)]
enum SidePanelAction {
    None,
    Chapter(f64),
    Playlist(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidePanelMode {
    Chapters,
    UpNext,
}

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
    Scale(f64),
    SetScale(f64),
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
    updates: String,
}

impl AboutSnapshot {
    fn capture(state: &Rc<RefCell<PlayerState>>) -> Self {
        let state = state.borrow();
        let auto_updates = state.settings.auto_check_updates();
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
            updates: if auto_updates {
                "Automatic".to_owned()
            } else {
                "Manual".to_owned()
            },
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
    xdg_mime_available: bool,
    update_desktop_database_available: bool,
}

impl LinuxIntegrationSnapshot {
    fn capture() -> Self {
        let desktop_entry_path = linux_desktop_entry_path();
        let registered_key_mimes = desktop_entry_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|contents| count_registered_key_media_mimes(&contents))
            .unwrap_or_default();
        let xdg_mime_available = find_executable("xdg-mime").is_some();
        let default_key_mimes = xdg_mime_available.then(count_default_key_media_mimes);

        Self {
            desktop_entry_path,
            registered_key_mimes,
            default_key_mimes,
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
    scale: f64,
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
