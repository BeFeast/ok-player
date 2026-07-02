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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gtk::cairo;
use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use okp_core::{AppIdentity, m3u, media_formats, natural_compare, time_code};
use okp_mpv::{
    AbLoopState, AudioDevice, Chapter, InfoSection, InfoTrack, MediaInfo, Mpv, MpvEvent,
    PlaybackState, Track, TrackKind, current_render_target_size, resolve_render_target_size,
};
use serde::Deserialize;
use velopack::{
    UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackApp, VelopackAsset,
    sources::GithubSource,
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

mod history;
mod screenshots;
mod settings;
mod thumbnails;

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
const LINUX_UPDATE_REPO_URL: &str = "https://github.com/BeFeast/ok-player";
const LINUX_DEB_RELEASES_API_URL: &str = "https://api.github.com/repos/BeFeast/ok-player/releases";
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
    playlist: Vec<PlaylistItem>,
    pending_subtitles: Vec<PathBuf>,
    pending_resume: Option<(PathBuf, f64)>,
    pending_preferences: Option<(PathBuf, history::PlaybackPreferences)>,
    thumbnail_request_key: Option<String>,
    hover_thumbnail_request_key: Option<String>,
    chapters_snapshot: Vec<Chapter>,
    modes: PlayModes,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

impl RepeatMode {
    fn cycle(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat Off",
            Self::One => "Repeat One",
            Self::All => "Repeat All",
        }
    }

    fn settings_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::One => "one",
            Self::All => "all",
        }
    }

    fn from_settings_value(value: &str) -> Self {
        match value {
            "one" => Self::One,
            "all" => Self::All,
            _ => Self::Off,
        }
    }
}

struct PlayModes {
    repeat_mode: RepeatMode,
    shuffle_enabled: bool,
    auto_advance_enabled: bool,
    shuffle_order: Vec<usize>,
    shuffle_cursor: Option<usize>,
    shuffle_seed: u64,
}

impl Default for PlayModes {
    fn default() -> Self {
        Self {
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            auto_advance_enabled: true,
            shuffle_order: Vec::new(),
            shuffle_cursor: None,
            shuffle_seed: shuffle_seed(),
        }
    }
}

impl PlayModes {
    fn reset_shuffle_order(&mut self) {
        self.shuffle_order.clear();
        self.shuffle_cursor = None;
    }

    fn ensure_shuffle_order(&mut self, playlist_len: usize, current_index: usize) {
        if !self.shuffle_enabled || playlist_len == 0 {
            self.reset_shuffle_order();
            return;
        }

        if self.shuffle_order.len() != playlist_len {
            self.shuffle_order = (0..playlist_len).collect();
            for index in (1..playlist_len).rev() {
                let swap_with = (next_shuffle_value(&mut self.shuffle_seed) as usize) % (index + 1);
                self.shuffle_order.swap(index, swap_with);
            }
        }

        if let Some(position) = self
            .shuffle_order
            .iter()
            .position(|index| *index == current_index)
        {
            self.shuffle_cursor = Some(position);
        }
    }
}

fn apply_playback_settings_defaults(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.modes.repeat_mode = RepeatMode::from_settings_value(state.settings.repeat_mode());
    state.modes.auto_advance_enabled = state.settings.auto_advance_enabled();
    state.modes.shuffle_enabled = state.settings.shuffle_enabled();
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
    Deb(ManualDebUpdate),
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
            Self::Checking => "Checking GitHub Releases...".to_owned(),
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

#[derive(Clone, Copy)]
enum QueueInsertMode {
    Append,
    PlayNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PlaylistItem {
    Local(PathBuf),
    Url(String),
}

impl PlaylistItem {
    fn local(path: PathBuf) -> Option<Self> {
        is_media_path(&path).then_some(Self::Local(path))
    }

    fn from_m3u_entry(entry: &str) -> Option<Self> {
        if is_media_url(entry) {
            return Some(Self::Url(entry.to_owned()));
        }

        Self::local(PathBuf::from(entry))
    }

    fn is_current(&self, current_file: Option<&Path>, current_url: Option<&str>) -> bool {
        match self {
            Self::Local(path) => current_file.is_some_and(|current| current == path),
            Self::Url(url) => current_url.is_some_and(|current| current == url),
        }
    }

    fn display_name(&self) -> String {
        match self {
            Self::Local(path) => display_file_name(path),
            Self::Url(url) => url
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| url.to_owned()),
        }
    }

    fn display_location(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Url(url) => url.to_owned(),
        }
    }

    fn m3u_entry(&self) -> String {
        match self {
            Self::Local(path) => path.to_string_lossy().into_owned(),
            Self::Url(url) => url.to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
struct ManualDebUpdate {
    version: String,
    name: String,
    url: String,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
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

        self.time_label.set_text(&format_time(time));
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

fn create_mpris_controller() -> (
    MprisController,
    mpsc::Receiver<MprisCommand>,
    mpsc::Receiver<MprisSignal>,
) {
    let (commands, receiver) = mpsc::channel();
    let (signals, signal_receiver) = mpsc::channel();
    (
        MprisController {
            snapshot: Arc::new(Mutex::new(MprisSnapshot::default())),
            commands,
            signals,
        },
        receiver,
        signal_receiver,
    )
}

fn start_mpris_service(controller: MprisController, signal_receiver: mpsc::Receiver<MprisSignal>) {
    if env::var_os("OKP_DISABLE_MPRIS").is_some() {
        return;
    }

    let spawn_result = thread::Builder::new()
        .name("okp-mpris".to_owned())
        .spawn(move || {
            if let Err(error) = run_mpris_service(controller, signal_receiver) {
                eprintln!("MPRIS service unavailable: {error}");
            }
        });

    if let Err(error) = spawn_result {
        eprintln!("Failed to start MPRIS thread: {error}");
    }
}

fn run_mpris_service(
    controller: MprisController,
    signal_receiver: mpsc::Receiver<MprisSignal>,
) -> zbus::Result<()> {
    let root = MprisRoot {
        commands: controller.commands.clone(),
    };
    let player = MprisPlayer {
        snapshot: Arc::clone(&controller.snapshot),
        commands: controller.commands.clone(),
    };
    let track_list = MprisTrackList {
        snapshot: controller.snapshot,
        commands: controller.commands,
    };
    let connection = zbus::blocking::connection::Builder::session()?
        .serve_at(MPRIS_OBJECT_PATH, root)?
        .serve_at(MPRIS_OBJECT_PATH, player)?
        .serve_at(MPRIS_OBJECT_PATH, track_list)?
        .name(MPRIS_BUS_NAME)?
        .build()?;

    while let Ok(signal) = signal_receiver.recv() {
        emit_mpris_signal(&connection, signal)?;
    }

    Ok(())
}

fn emit_mpris_signal(
    connection: &zbus::blocking::Connection,
    signal: MprisSignal,
) -> zbus::Result<()> {
    match signal {
        MprisSignal::PlayerPropertiesInvalidated(properties) if !properties.is_empty() => {
            let changed: HashMap<&str, Value<'_>> = HashMap::new();
            connection.emit_signal(
                None::<&str>,
                MPRIS_OBJECT_PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    "org.mpris.MediaPlayer2.Player",
                    changed,
                    properties.as_slice(),
                ),
            )
        }
        MprisSignal::TrackListPropertiesInvalidated(properties) if !properties.is_empty() => {
            let changed: HashMap<&str, Value<'_>> = HashMap::new();
            connection.emit_signal(
                None::<&str>,
                MPRIS_OBJECT_PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(
                    "org.mpris.MediaPlayer2.TrackList",
                    changed,
                    properties.as_slice(),
                ),
            )
        }
        MprisSignal::TrackListReplaced {
            tracks,
            current_track,
        } => connection.emit_signal(
            None::<&str>,
            MPRIS_OBJECT_PATH,
            "org.mpris.MediaPlayer2.TrackList",
            "TrackListReplaced",
            &(tracks, current_track),
        ),
        MprisSignal::Seeked(position_us) => connection.emit_signal(
            None::<&str>,
            MPRIS_OBJECT_PATH,
            "org.mpris.MediaPlayer2.Player",
            "Seeked",
            &(position_us,),
        ),
        _ => Ok(()),
    }
}

fn connect_mpris_commands(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    commands: mpsc::Receiver<MprisCommand>,
) {
    let window = window.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(command) = commands.try_recv() {
            handle_mpris_command(&window, &state, &status_toast, command);
        }
        glib::ControlFlow::Continue
    });
}

fn handle_mpris_command(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    command: MprisCommand,
) {
    match command {
        MprisCommand::Raise => window.present(),
        MprisCommand::Quit => window.close(),
        MprisCommand::Play => set_playback_paused(state, false),
        MprisCommand::Pause => set_playback_paused(state, true),
        MprisCommand::PlayPause => {
            with_mpv(state, |mpv| mpv.cycle_pause());
        }
        MprisCommand::Stop => {
            close_current_media(state, status_toast);
        }
        MprisCommand::Previous => {
            navigate_playlist(state, -1);
        }
        MprisCommand::Next => {
            navigate_playlist(state, 1);
        }
        MprisCommand::SeekBy(offset_us) => {
            let seconds = offset_us as f64 / 1_000_000.0;
            with_mpv(state, |mpv| mpv.seek_relative(seconds));
        }
        MprisCommand::SetPosition(position_us) => {
            let seconds = position_us.max(0) as f64 / 1_000_000.0;
            with_mpv(state, |mpv| mpv.seek_absolute(seconds));
        }
        MprisCommand::SetVolume(volume) => {
            if let Some(volume) = mpris_volume_to_mpv_percent(volume) {
                set_volume_from_ui(state, volume);
            }
        }
        MprisCommand::SetRate(rate) => {
            if let Some(speed) = mpris_rate_to_mpv_speed(rate) {
                set_playback_speed_from_ui(state, speed);
            }
        }
        MprisCommand::SetLoopStatus(status) => {
            if let Some(repeat_mode) = mpris_repeat_mode(&status) {
                set_repeat_mode_from_ui(state, status_toast, repeat_mode);
            }
        }
        MprisCommand::SetShuffle(shuffle) => {
            set_shuffle_from_ui(state, status_toast, shuffle);
        }
        MprisCommand::GoToTrack(track_id) => {
            let target = {
                let state = state.borrow();
                mpris_tracklist_target_for_id(&state, &track_id)
            };
            if let Some((index, item)) = target {
                if state.borrow().playlist.is_empty() {
                    match item {
                        PlaylistItem::Local(path) => load_media_path(state, path),
                        PlaylistItem::Url(url) => load_media_url(state, url),
                    }
                } else {
                    jump_playlist_index(state, index);
                }
            }
        }
        MprisCommand::OpenUri(uri) => {
            if let Some(path) = file_uri_path(&uri) {
                load_media_path(state, path);
            } else if is_media_url(&uri) {
                load_media_url(state, uri);
            }
        }
    }
}

fn set_playback_paused(state: &Rc<RefCell<PlayerState>>, paused: bool) {
    let should_toggle = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        match mpv.playback_state() {
            Ok(playback) => playback.paused != paused,
            Err(error) => {
                eprintln!("Failed to read playback state for MPRIS command: {error}");
                false
            }
        }
    };

    if should_toggle {
        with_mpv(state, |mpv| mpv.cycle_pause());
    }
}

fn update_mpris_snapshot(
    snapshot: &Arc<Mutex<MprisSnapshot>>,
    signals: &mpsc::Sender<MprisSignal>,
    state: &PlayerState,
    playback: Option<PlaybackState>,
) {
    let next = mpris_snapshot_from_state(state, playback);
    let (invalidated, tracklist_invalidated, tracklist_replaced, seeked_position) =
        if let Ok(mut snapshot) = snapshot.lock() {
            let invalidated = mpris_invalidated_properties(&snapshot, &next);
            let tracklist_invalidated = mpris_tracklist_invalidated_properties(&snapshot, &next);
            let tracklist_replaced = mpris_tracklist_replaced_signal(&snapshot, &next);
            let seeked_position = mpris_seeked_position(&snapshot, &next);
            *snapshot = next;
            (
                invalidated,
                tracklist_invalidated,
                tracklist_replaced,
                seeked_position,
            )
        } else {
            (Vec::new(), Vec::new(), None, None)
        };

    if !invalidated.is_empty() {
        let _ = signals.send(MprisSignal::PlayerPropertiesInvalidated(invalidated));
    }

    if !tracklist_invalidated.is_empty() {
        let _ = signals.send(MprisSignal::TrackListPropertiesInvalidated(
            tracklist_invalidated,
        ));
    }

    if let Some((tracks, current_track)) = tracklist_replaced {
        let _ = signals.send(MprisSignal::TrackListReplaced {
            tracks,
            current_track,
        });
    }

    if let Some(position_us) = seeked_position {
        let _ = signals.send(MprisSignal::Seeked(position_us));
    }
}

fn mpris_tracklist_invalidated_properties(
    previous: &MprisSnapshot,
    next: &MprisSnapshot,
) -> Vec<&'static str> {
    (previous.tracklist_track_ids() != next.tracklist_track_ids())
        .then_some(vec!["Tracks"])
        .unwrap_or_default()
}

fn mpris_tracklist_replaced_signal(
    previous: &MprisSnapshot,
    next: &MprisSnapshot,
) -> Option<(Vec<OwnedObjectPath>, OwnedObjectPath)> {
    if previous.tracklist == next.tracklist && previous.current_track_id == next.current_track_id {
        return None;
    }

    Some((
        next.tracklist_track_ids(),
        next.current_track_id
            .clone()
            .unwrap_or_else(mpris_no_track_id),
    ))
}

fn mpris_invalidated_properties(
    previous: &MprisSnapshot,
    next: &MprisSnapshot,
) -> Vec<&'static str> {
    let mut properties = Vec::new();

    if previous.playback_status() != next.playback_status() {
        properties.push("PlaybackStatus");
    }

    if previous.has_media != next.has_media
        || previous.track_id != next.track_id
        || previous.title != next.title
        || previous.uri != next.uri
        || previous.art_url != next.art_url
        || previous.duration_us != next.duration_us
    {
        properties.push("Metadata");
    }

    if previous.has_media != next.has_media {
        properties.push("CanPlay");
        properties.push("CanPause");
    }

    if previous.duration_us != next.duration_us {
        properties.push("CanSeek");
    }

    if previous.can_go_next != next.can_go_next {
        properties.push("CanGoNext");
    }

    if previous.can_go_previous != next.can_go_previous {
        properties.push("CanGoPrevious");
    }

    if (previous.volume - next.volume).abs() > f64::EPSILON {
        properties.push("Volume");
    }

    if (previous.rate - next.rate).abs() > f64::EPSILON {
        properties.push("Rate");
    }

    if previous.repeat_mode != next.repeat_mode {
        properties.push("LoopStatus");
    }

    if previous.shuffle != next.shuffle {
        properties.push("Shuffle");
    }

    properties
}

fn mpris_seeked_position(previous: &MprisSnapshot, next: &MprisSnapshot) -> Option<i64> {
    let same_media = previous.has_media
        && next.has_media
        && previous.title == next.title
        && previous.uri == next.uri
        && previous.duration_us == next.duration_us;
    if !same_media {
        return None;
    }

    let delta = (previous.position_us - next.position_us).abs();
    (delta >= MPRIS_SEEKED_DELTA_US).then_some(next.position_us)
}

fn mpris_volume_to_mpv_percent(volume: f64) -> Option<f64> {
    volume
        .is_finite()
        .then(|| (volume * 100.0).clamp(0.0, 130.0))
}

fn mpris_rate_to_mpv_speed(rate: f64) -> Option<f64> {
    rate.is_finite().then(|| rate.clamp(0.25, 4.0))
}

fn mpris_loop_status(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "None",
        RepeatMode::One => "Track",
        RepeatMode::All => "Playlist",
    }
}

fn mpris_repeat_mode(status: &str) -> Option<RepeatMode> {
    match status {
        "None" => Some(RepeatMode::Off),
        "Track" => Some(RepeatMode::One),
        "Playlist" => Some(RepeatMode::All),
        _ => None,
    }
}

fn mpris_snapshot_from_state(
    state: &PlayerState,
    playback: Option<PlaybackState>,
) -> MprisSnapshot {
    let has_media = has_loaded_media_state(state);
    let (title, uri, art_url) = mpris_title_uri_and_art(state);
    let playback = playback.unwrap_or_default();
    let duration_us = playback
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(secs_to_mpris_us);
    let tracklist = mpris_tracklist_from_state(state, duration_us);
    let current_track_id = tracklist
        .iter()
        .find(|track| {
            track
                .uri
                .as_ref()
                .is_some_and(|track_uri| uri.as_ref() == Some(track_uri))
        })
        .map(|track| track.id.clone());
    let track_id = current_track_id.clone().unwrap_or_else(mpris_track_id);

    MprisSnapshot {
        has_media,
        paused: playback.paused || !has_media,
        position_us: playback
            .time_pos
            .filter(|position| position.is_finite() && *position > 0.0)
            .map(secs_to_mpris_us)
            .unwrap_or(0),
        duration_us,
        volume: playback.volume.unwrap_or(100.0).max(0.0) / 100.0,
        rate: playback.speed.unwrap_or(1.0).clamp(0.25, 4.0),
        repeat_mode: state.modes.repeat_mode,
        shuffle: state.modes.shuffle_enabled,
        can_go_next: state.playlist.len() > 1,
        can_go_previous: state.playlist.len() > 1,
        track_id,
        title,
        uri,
        art_url: has_media.then_some(art_url).flatten(),
        tracklist,
        current_track_id,
    }
}

fn mpris_tracklist_from_state(
    state: &PlayerState,
    current_duration_us: Option<i64>,
) -> Vec<MprisTrack> {
    let items = mpris_tracklist_items_from_state(state);
    if items.is_empty() {
        return Vec::new();
    }

    let current_index = mpris_current_tracklist_index(state, &items).unwrap_or(0);
    let (start, end) = mpris_tracklist_window(items.len(), current_index);
    items
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|(index, item)| {
            let id = mpris_tracklist_id_for_item(index, item);
            let uri = mpris_playlist_item_uri(item);
            let is_current = index == current_index;
            MprisTrack {
                id,
                title: item.display_name(),
                uri,
                duration_us: is_current.then_some(current_duration_us).flatten(),
                art_url: mpris_playlist_item_art_url(item),
            }
        })
        .collect()
}

fn mpris_tracklist_items_from_state(state: &PlayerState) -> Vec<PlaylistItem> {
    if !state.playlist.is_empty() {
        return state.playlist.clone();
    }

    if let Some(path) = state.current_file.as_ref() {
        return vec![PlaylistItem::Local(path.clone())];
    }

    if let Some(url) = state.current_url.as_ref() {
        return vec![PlaylistItem::Url(url.clone())];
    }

    Vec::new()
}

fn mpris_current_tracklist_index(state: &PlayerState, items: &[PlaylistItem]) -> Option<usize> {
    items.iter().position(|item| {
        item.is_current(state.current_file.as_deref(), state.current_url.as_deref())
    })
}

fn mpris_tracklist_window(len: usize, current_index: usize) -> (usize, usize) {
    if len <= MPRIS_TRACKLIST_CONTEXT_LIMIT {
        return (0, len);
    }

    let half = MPRIS_TRACKLIST_CONTEXT_LIMIT / 2;
    let start = current_index
        .saturating_sub(half)
        .min(len.saturating_sub(MPRIS_TRACKLIST_CONTEXT_LIMIT));
    (start, start + MPRIS_TRACKLIST_CONTEXT_LIMIT)
}

fn mpris_tracklist_id_for_item(index: usize, item: &PlaylistItem) -> OwnedObjectPath {
    let hash = mpris_playlist_item_hash(item);
    format!("/org/mpris/MediaPlayer2/TrackList/Track/t{index}_{hash:016x}")
        .try_into()
        .expect("generated MPRIS track id should be an object path")
}

fn mpris_tracklist_target_for_id(
    state: &PlayerState,
    track_id: &str,
) -> Option<(usize, PlaylistItem)> {
    mpris_tracklist_items_from_state(state)
        .into_iter()
        .enumerate()
        .find(|(index, item)| mpris_tracklist_id_for_item(*index, item).as_str() == track_id)
}

fn mpris_playlist_item_hash(item: &PlaylistItem) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut mix = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    };

    match item {
        PlaylistItem::Local(path) => {
            mix(b'L');
            for byte in path.to_string_lossy().as_bytes() {
                mix(*byte);
            }
        }
        PlaylistItem::Url(url) => {
            mix(b'U');
            for byte in url.as_bytes() {
                mix(*byte);
            }
        }
    }

    hash
}

fn mpris_playlist_item_uri(item: &PlaylistItem) -> Option<String> {
    match item {
        PlaylistItem::Local(path) => Some(local_file_uri(path)),
        PlaylistItem::Url(url) => Some(url.clone()),
    }
}

fn mpris_playlist_item_art_url(item: &PlaylistItem) -> Option<String> {
    match item {
        PlaylistItem::Local(path) => mpris_local_art_url(path),
        PlaylistItem::Url(_) => mpris_app_icon_art_url(),
    }
}

fn mpris_title_uri_and_art(state: &PlayerState) -> (String, Option<String>, Option<String>) {
    if let Some(path) = state.current_file.as_ref() {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        let uri = local_file_uri(path);
        let art_url = mpris_local_art_url(path);
        return (title, Some(uri), art_url);
    }

    if let Some(url) = state.current_url.as_ref() {
        return (
            url.to_owned(),
            Some(url.to_owned()),
            mpris_app_icon_art_url(),
        );
    }

    ("OK Player".to_owned(), None, None)
}

fn local_file_uri(path: &Path) -> String {
    gtk::gio::File::for_path(path).uri().to_string()
}

fn mpris_local_art_url(media_path: &Path) -> Option<String> {
    mpris_sidecar_art_url(media_path)
        .or_else(|| mpris_embedded_art_url(media_path))
        .or_else(mpris_app_icon_art_url)
}

fn mpris_sidecar_art_url(media_path: &Path) -> Option<String> {
    let cache = MPRIS_SIDECAR_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(cached) = cache.get(media_path) {
            return cached.clone();
        }
        let resolved = mpris_sidecar_art_path(media_path).map(|path| local_file_uri(&path));
        cache.insert(media_path.to_path_buf(), resolved.clone());
        return resolved;
    }

    mpris_sidecar_art_path(media_path).map(|path| local_file_uri(&path))
}

fn mpris_embedded_art_url(media_path: &Path) -> Option<String> {
    if !media_formats::is_audio(media_path) {
        return None;
    }

    let key = mpris_embedded_art_cache_key(media_path)?;
    let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };

    match cache.get(&key) {
        Some(MprisEmbeddedArtCacheEntry::Pending) => return None,
        Some(MprisEmbeddedArtCacheEntry::Ready(path)) => {
            return path.as_ref().map(|path| local_file_uri(path));
        }
        None => {}
    }

    cache.insert(key.clone(), MprisEmbeddedArtCacheEntry::Pending);
    drop(cache);
    spawn_mpris_embedded_art_extraction(key);
    None
}

fn spawn_mpris_embedded_art_extraction(key: MprisEmbeddedArtCacheKey) {
    let thread_key = key.clone();
    let spawn_result = thread::Builder::new()
        .name("okp-mpris-art".to_owned())
        .spawn(move || {
            let resolved = mpris_extract_embedded_art_path(&thread_key);
            let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
            if let Ok(mut cache) = cache.lock() {
                cache.insert(thread_key, MprisEmbeddedArtCacheEntry::Ready(resolved));
            }
        });

    if let Err(error) = spawn_result {
        eprintln!("Failed to spawn MPRIS embedded artwork extraction: {error}");
        let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, MprisEmbeddedArtCacheEntry::Ready(None));
        }
    }
}

fn mpris_extract_embedded_art_path(key: &MprisEmbeddedArtCacheKey) -> Option<PathBuf> {
    let output = mpris_embedded_art_cache_path(key);
    if output.is_file() {
        if mpris_has_image_header(&output) {
            return Some(output);
        }
        let _ = fs::remove_file(&output);
    }

    let parent = output.parent()?;
    fs::create_dir_all(parent).ok()?;
    let temp = mpris_embedded_art_temp_path(&output)?;
    let _ = fs::remove_file(&temp);

    let ffmpeg = find_executable("ffmpeg")?;
    let mut child = Command::new(ffmpeg)
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(&key.path)
        .args(["-map", "0:v:0", "-frames:v", "1", "-an", "-sn", "-dn"])
        .arg(&temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let status = wait_for_child_with_timeout(&mut child, MPRIS_EMBEDDED_ART_TIMEOUT).ok()?;
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(&temp);
        return None;
    };
    if !status.success() || !mpris_has_image_header(&temp) {
        let _ = fs::remove_file(&temp);
        return None;
    }

    fs::rename(&temp, &output).ok()?;
    Some(output)
}

fn mpris_embedded_art_cache_key(media_path: &Path) -> Option<MprisEmbeddedArtCacheKey> {
    let metadata = fs::metadata(media_path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    Some(MprisEmbeddedArtCacheKey {
        path: media_path.to_path_buf(),
        len: metadata.len(),
        modified_ns,
    })
}

fn mpris_embedded_art_cache_path(key: &MprisEmbeddedArtCacheKey) -> PathBuf {
    mpris_embedded_art_cache_path_in_dir(key, &mpris_embedded_art_cache_dir())
}

fn mpris_embedded_art_cache_path_in_dir(key: &MprisEmbeddedArtCacheKey, dir: &Path) -> PathBuf {
    dir.join(format!("{:016x}.png", mpris_embedded_art_cache_hash(key)))
}

fn mpris_embedded_art_cache_hash(key: &MprisEmbeddedArtCacheKey) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut mix = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    };

    for byte in key.path.to_string_lossy().as_bytes() {
        mix(*byte);
    }
    for byte in key.len.to_le_bytes() {
        mix(byte);
    }
    for byte in key.modified_ns.to_le_bytes() {
        mix(byte);
    }

    hash
}

fn mpris_embedded_art_cache_dir() -> PathBuf {
    if let Some(cache_dir) =
        env::var_os("OKP_MPRIS_ART_CACHE_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(cache_dir);
    }
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(cache_home).join("ok-player/mpris-art");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".cache/ok-player/mpris-art");
    }
    env::temp_dir().join("ok-player/mpris-art")
}

fn mpris_embedded_art_temp_path(output: &Path) -> Option<PathBuf> {
    let stem = output.file_stem()?.to_string_lossy();
    Some(output.with_file_name(format!("{stem}.part.{}.png", std::process::id())))
}

fn mpris_sidecar_art_path(media_path: &Path) -> Option<PathBuf> {
    let dir = media_path.parent()?;
    let media_stem = media_path.file_stem()?.to_str()?;
    let mut candidates: Vec<(i32, usize, PathBuf)> = Vec::new();

    for entry in fs::read_dir(dir).ok()? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(extension_rank) = mpris_art_extension_rank(&path) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let slot = if stem.eq_ignore_ascii_case(media_stem) {
            -1
        } else if let Some(index) = mpris_folder_art_stem_index(stem) {
            index as i32
        } else {
            continue;
        };
        if !mpris_has_image_header(&path) {
            continue;
        }
        candidates.push((slot, extension_rank, path));
    }

    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    candidates.into_iter().map(|(_, _, path)| path).next()
}

fn mpris_art_extension_rank(path: &Path) -> Option<usize> {
    let extension = path.extension()?.to_str()?;
    MPRIS_ART_EXTENSIONS
        .iter()
        .position(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn mpris_folder_art_stem_index(stem: &str) -> Option<usize> {
    MPRIS_FOLDER_ART_STEMS
        .iter()
        .position(|candidate| stem.eq_ignore_ascii_case(candidate))
}

fn mpris_has_image_header(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = [0_u8; 12];
    if file.read_exact(&mut bytes).is_err() {
        return false;
    }

    (bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff)
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

fn mpris_app_icon_art_url() -> Option<String> {
    MPRIS_APP_ICON_ART_URL
        .get_or_init(|| mpris_app_icon_art_path().map(|path| local_file_uri(&path)))
        .clone()
}

fn mpris_app_icon_art_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(
        "/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg",
    ));
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("com.befeast.okplayer.svg"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/linux/com.befeast.okplayer.svg"),
    );

    candidates.into_iter().find(|path| path.is_file())
}

fn secs_to_mpris_us(seconds: f64) -> i64 {
    (seconds.max(0.0) * 1_000_000.0).round() as i64
}

fn mpris_metadata(snapshot: &MprisSnapshot) -> HashMap<String, OwnedValue> {
    mpris_metadata_map(
        snapshot.track_id.clone(),
        &snapshot.title,
        snapshot.uri.as_deref(),
        snapshot.duration_us,
        snapshot.art_url.as_deref(),
    )
}

fn mpris_track_metadata(track: &MprisTrack) -> HashMap<String, OwnedValue> {
    mpris_metadata_map(
        track.id.clone(),
        &track.title,
        track.uri.as_deref(),
        track.duration_us,
        track.art_url.as_deref(),
    )
}

fn mpris_metadata_map(
    track_id: OwnedObjectPath,
    title: &str,
    uri: Option<&str>,
    duration_us: Option<i64>,
    art_url: Option<&str>,
) -> HashMap<String, OwnedValue> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "mpris:trackid".to_owned(),
        Value::from(track_id).try_into().expect("track id value"),
    );
    metadata.insert(
        "xesam:title".to_owned(),
        Value::from(title).try_into().expect("title value"),
    );
    if let Some(duration_us) = duration_us {
        metadata.insert(
            "mpris:length".to_owned(),
            Value::from(duration_us).try_into().expect("length value"),
        );
    }
    if let Some(uri) = uri {
        metadata.insert(
            "xesam:url".to_owned(),
            Value::from(uri).try_into().expect("url value"),
        );
    }
    if let Some(art_url) = art_url {
        metadata.insert(
            "mpris:artUrl".to_owned(),
            Value::from(art_url).try_into().expect("art url value"),
        );
    }
    metadata
}

fn mpris_track_id() -> OwnedObjectPath {
    MPRIS_TRACK_PATH
        .try_into()
        .expect("static MPRIS track path")
}

fn mpris_no_track_id() -> OwnedObjectPath {
    MPRIS_TRACKLIST_NO_TRACK_PATH
        .try_into()
        .expect("static MPRIS no-track path")
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

#[cfg(test)]
fn parse_launch_args_from(args: impl Iterator<Item = std::ffi::OsString>) -> LaunchArgs {
    let cwd = env::current_dir().ok();
    parse_launch_args_from_cwd(args, cwd.as_deref())
}

fn parse_launch_args_from_cwd(
    mut args: impl Iterator<Item = std::ffi::OsString>,
    cwd: Option<&Path>,
) -> LaunchArgs {
    let mut launch = LaunchArgs::default();
    while let Some(arg) = args.next() {
        if arg == "--sub" {
            if let Some(arg) = args.next() {
                add_launch_subtitle_arg(&mut launch, arg, cwd);
            }
            continue;
        }

        if let Some(text) = arg.to_str() {
            if media_formats::is_playable_url(Some(text)) {
                push_unique_playlist_item(&mut launch.items, PlaylistItem::Url(text.to_owned()));
                continue;
            }

            if let Some(path) = file_uri_path(text) {
                add_launch_path_arg(&mut launch, path);
                continue;
            }
        }

        add_launch_path_arg(&mut launch, launch_path_arg(arg, cwd));
    }

    launch
}

fn file_uri_path(text: &str) -> Option<PathBuf> {
    text.strip_prefix("file://")?;
    gtk::gio::File::for_uri(text).path()
}

fn add_launch_subtitle_arg(launch: &mut LaunchArgs, arg: std::ffi::OsString, cwd: Option<&Path>) {
    if let Some(text) = arg.to_str()
        && let Some(path) = file_uri_path(text)
    {
        add_unique_launch_subtitle(launch, path);
        return;
    }

    add_unique_launch_subtitle(launch, launch_path_arg(arg, cwd));
}

fn launch_path_arg(arg: std::ffi::OsString, cwd: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(arg);
    if path.is_relative()
        && let Some(cwd) = cwd
    {
        return cwd.join(path);
    }
    path
}

fn add_launch_path_arg(launch: &mut LaunchArgs, path: PathBuf) {
    if is_subtitle_path(&path) {
        add_unique_launch_subtitle(launch, path);
    } else if is_playlist_path(&path) {
        if !launch.playlists.iter().any(|existing| existing == &path) {
            launch.playlists.push(path);
        }
    } else if is_media_path(&path) {
        push_unique_playlist_item(&mut launch.items, PlaylistItem::Local(path));
    }
}

fn add_unique_launch_subtitle(launch: &mut LaunchArgs, path: PathBuf) {
    if !launch.subtitles.iter().any(|existing| existing == &path) {
        launch.subtitles.push(path);
    }
}

fn push_unique_playlist_item(items: &mut Vec<PlaylistItem>, item: PlaylistItem) {
    if !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

fn build_window(app: &gtk::Application, launch_args: LaunchArgs) -> AppRuntime {
    install_css();

    let identity = AppIdentity::linux();
    let state = Rc::new(RefCell::new(PlayerState::default()));
    apply_playback_settings_defaults(&state);
    let auto_check_updates = state.borrow().settings.auto_check_updates();
    let updating_seek = Rc::new(Cell::new(false));
    let updating_volume = Rc::new(Cell::new(false));
    let status_toast = Rc::new(StatusToast::new());
    let chrome = Rc::new(ChromeVisibility::new());
    let (mpris_controller, mpris_commands, mpris_signals) = create_mpris_controller();
    start_mpris_service(mpris_controller.clone(), mpris_signals);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&identity.name)
        .default_width(1120)
        .default_height(680)
        .decorated(false)
        .build();
    window.add_css_class("okp-player-window");

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-root");

    let video_area = gtk::GLArea::new();
    video_area.set_hexpand(true);
    video_area.set_vexpand(true);
    video_area.set_auto_render(false);
    video_area.set_required_version(3, 2);
    video_area.add_css_class("okp-video-plane");

    let controls = build_controls(
        &window,
        Rc::clone(&state),
        Rc::clone(&updating_seek),
        Rc::clone(&updating_volume),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    let control_bar = controls_bar(&controls);
    let window_chrome = build_player_window_chrome(&window);
    sync_player_window_chrome_fullscreen(&window_chrome, &window);
    let empty_surface = build_empty_surface(&window, Rc::clone(&state), Rc::clone(&status_toast));
    chrome.set_child(&control_bar);
    chrome.add_linked_revealer(&window_chrome);
    chrome.add_linked_revealer(&controls.up_next_revealer);

    overlay.set_child(Some(&video_area));
    overlay.add_overlay(empty_surface.widget());
    overlay.add_overlay(&window_chrome);
    overlay.add_overlay(chrome.widget());
    overlay.add_overlay(&controls.up_next_revealer);
    overlay.add_overlay(status_toast.widget());
    for resize_handle in build_player_resize_handles(&window) {
        overlay.add_overlay(&resize_handle);
    }
    window.set_child(Some(&overlay));
    connect_chrome_activity(&overlay, Rc::clone(&chrome));

    connect_mpv(&video_area, Rc::clone(&state), launch_args);
    connect_video_clicks(
        &video_area,
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    );
    connect_drop(&window, Rc::clone(&state), empty_surface.clone());
    connect_keyboard(
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    connect_mpris_commands(
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        mpris_commands,
    );
    connect_progress_persistence(&window, Rc::clone(&state));
    connect_state_poll(
        &window,
        Rc::clone(&state),
        controls,
        StatePollContext {
            updating_seek: Rc::clone(&updating_seek),
            updating_volume: Rc::clone(&updating_volume),
            chrome: Rc::clone(&chrome),
            empty_surface,
            mpris_snapshot: Arc::clone(&mpris_controller.snapshot),
            mpris_signals: mpris_controller.signals.clone(),
        },
    );

    window.present();
    if env::var_os("OKP_OPEN_SETTINGS_ON_STARTUP").is_some() {
        let settings_parent = window.clone();
        let settings_state = Rc::clone(&state);
        let settings_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            open_settings_window(&settings_parent, settings_state, settings_toast);
        });
    }
    if auto_check_updates {
        check_updates_on_startup(Rc::clone(&state), Rc::clone(&status_toast));
    }

    AppRuntime { window, state }
}

fn open_runtime_launch_args(runtime: &AppRuntime, launch_args: &LaunchArgs) {
    runtime.window.present();
    if launch_args.has_payload() {
        apply_launch_args(&runtime.state, launch_args);
    }
}

fn sync_player_window_chrome_fullscreen(
    window_chrome: &gtk::Revealer,
    window: &gtk::ApplicationWindow,
) {
    window_chrome.set_visible(!window.is_fullscreen());

    let fullscreen_chrome = window_chrome.clone();
    window.connect_notify_local(Some("fullscreened"), move |window, _| {
        fullscreen_chrome.set_visible(!window.is_fullscreen());
    });
}

fn build_player_window_chrome(window: &gtk::ApplicationWindow) -> gtk::Revealer {
    let revealer = gtk::Revealer::new();
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Start);
    revealer.set_transition_duration(140);
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_reveal_child(true);
    revealer.set_can_target(true);

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("okp-window-chrome");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::Start);
    bar.set_margin_top(0);

    let drag_zone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_zone.add_css_class("okp-window-drag-zone");
    drag_zone.set_hexpand(true);
    drag_zone.set_can_target(true);
    connect_player_window_drag(&drag_zone, window);
    bar.append(&drag_zone);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-player-window-controls");
    controls.set_halign(gtk::Align::End);
    controls.set_margin_top(4);
    controls.set_margin_end(6);

    let minimize = player_window_control(WindowControlKind::Minimize, "Minimize");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    controls.append(&minimize);

    let maximize = player_window_control(WindowControlKind::Maximize, "Maximize");
    sync_player_maximize_icon(&maximize, window);
    let maximize_window = window.clone();
    let maximize_button = maximize.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
        sync_player_maximize_icon(&maximize_button, &maximize_window);
    });
    let notify_button = maximize.clone();
    window.connect_maximized_notify(move |window| {
        sync_player_maximize_icon(&notify_button, window);
    });
    controls.append(&maximize);

    let close = player_window_control(WindowControlKind::Close, "Close");
    close.add_css_class("okp-player-window-close");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    controls.append(&close);

    bar.append(&controls);
    revealer.set_child(Some(&bar));
    revealer
}

fn player_window_control(kind: WindowControlKind, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-player-window-control");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&window_control_icon(
        kind,
        "okp-player-window-control-glyph",
    )));
    button
}

fn sync_player_maximize_icon(button: &gtk::Button, window: &gtk::ApplicationWindow) {
    if window.is_maximized() {
        set_player_window_control_kind(button, WindowControlKind::Restore);
        button.set_tooltip_text(Some("Restore"));
    } else {
        set_player_window_control_kind(button, WindowControlKind::Maximize);
        button.set_tooltip_text(Some("Maximize"));
    }
}

fn set_player_window_control_kind(button: &gtk::Button, kind: WindowControlKind) {
    if let Some(icon) = button.child().and_downcast::<gtk::DrawingArea>() {
        icon.set_draw_func(move |area, cr, width, height| {
            draw_window_control_icon(area, cr, width, height, kind);
        });
        icon.queue_draw();
    }
}

fn connect_player_window_drag(widget: &impl IsA<gtk::Widget>, window: &gtk::ApplicationWindow) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let drag_window = window.clone();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if n_press == 2 {
            if drag_window.is_maximized() {
                drag_window.unmaximize();
            } else {
                drag_window.maximize();
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(surface) = drag_window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };

        toplevel.begin_move(
            &device,
            gesture.current_button() as i32,
            x,
            y,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

fn build_player_resize_handles(window: &gtk::ApplicationWindow) -> Vec<gtk::Box> {
    let specs = [
        (
            gdk::SurfaceEdge::NorthWest,
            gtk::Align::Start,
            gtk::Align::Start,
            16,
            16,
            "nwse-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::North,
            gtk::Align::Fill,
            gtk::Align::Start,
            -1,
            6,
            "ns-resize",
            "okp-resize-edge-horizontal",
        ),
        (
            gdk::SurfaceEdge::NorthEast,
            gtk::Align::End,
            gtk::Align::Start,
            16,
            16,
            "nesw-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::West,
            gtk::Align::Start,
            gtk::Align::Fill,
            6,
            -1,
            "ew-resize",
            "okp-resize-edge-vertical",
        ),
        (
            gdk::SurfaceEdge::East,
            gtk::Align::End,
            gtk::Align::Fill,
            6,
            -1,
            "ew-resize",
            "okp-resize-edge-vertical",
        ),
        (
            gdk::SurfaceEdge::SouthWest,
            gtk::Align::Start,
            gtk::Align::End,
            16,
            16,
            "nesw-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::South,
            gtk::Align::Fill,
            gtk::Align::End,
            -1,
            6,
            "ns-resize",
            "okp-resize-edge-horizontal",
        ),
        (
            gdk::SurfaceEdge::SouthEast,
            gtk::Align::End,
            gtk::Align::End,
            16,
            16,
            "nwse-resize",
            "okp-resize-corner",
        ),
    ];

    specs
        .into_iter()
        .map(
            |(edge, halign, valign, width, height, cursor, class_name)| {
                let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                handle.add_css_class("okp-resize-handle");
                handle.add_css_class(class_name);
                handle.set_halign(halign);
                handle.set_valign(valign);
                handle.set_can_target(true);
                handle.set_cursor_from_name(Some(cursor));
                if width > 0 {
                    handle.set_width_request(width);
                }
                if height > 0 {
                    handle.set_height_request(height);
                }
                connect_player_window_resize(&handle, window, edge);
                handle
            },
        )
        .collect()
}

fn connect_player_window_resize(
    widget: &gtk::Box,
    window: &gtk::ApplicationWindow,
    edge: gdk::SurfaceEdge,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let resize_window = window.clone();
    let resize_widget = widget.clone();
    gesture.connect_pressed(move |gesture, _, x, y| {
        let debug_resize = env::var_os("OKP_DEBUG_WINDOW_RESIZE").is_some();
        if debug_resize {
            eprintln!("resize press edge={edge:?} local=({x:.1},{y:.1})");
        }

        if resize_window.is_fullscreen() || resize_window.is_maximized() {
            if debug_resize {
                eprintln!("resize ignored: fullscreen/maximized");
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            if debug_resize {
                eprintln!("resize ignored: no device");
            }
            return;
        };
        let Some(surface) = resize_window.surface() else {
            if debug_resize {
                eprintln!("resize ignored: no surface");
            }
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            if debug_resize {
                eprintln!("resize ignored: surface is not a toplevel");
            }
            return;
        };
        let window_point = resize_widget
            .compute_point(
                &resize_window,
                &gtk::graphene::Point::new(x as f32, y as f32),
            )
            .map(|point| (f64::from(point.x()), f64::from(point.y())))
            .unwrap_or((x, y));
        if debug_resize {
            eprintln!(
                "resize begin edge={edge:?} window=({:.1},{:.1}) button={}",
                window_point.0,
                window_point.1,
                gesture.current_button()
            );
        }

        toplevel.begin_resize(
            edge,
            Some(&device),
            gesture.current_button() as i32,
            window_point.0,
            window_point.1,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

fn build_empty_surface(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> EmptySurface {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 16);
    panel.add_css_class("okp-empty-panel");
    panel.set_halign(gtk::Align::Center);
    panel.set_valign(gtk::Align::Center);

    let logo = gtk::Image::from_icon_name("com.befeast.okplayer");
    logo.add_css_class("okp-empty-logo");
    logo.set_pixel_size(64);
    panel.append(&logo);

    let title = gtk::Label::new(Some("OK Player"));
    title.add_css_class("okp-empty-title");
    title.set_xalign(0.5);
    panel.append(&title);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_halign(gtk::Align::Center);

    let open_button = gtk::Button::with_label("Open media");
    open_button.add_css_class("okp-empty-primary-button");
    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    open_button.connect_clicked(move |_| open_media_dialog(&open_parent, Rc::clone(&open_state)));
    actions.append(&open_button);

    let folder_button = gtk::Button::with_label("Open folder");
    folder_button.add_css_class("okp-empty-secondary-button");
    let folder_parent = window.clone();
    let folder_state = Rc::clone(&state);
    let folder_toast = Rc::clone(&status_toast);
    folder_button.connect_clicked(move |_| {
        open_folder_dialog(
            &folder_parent,
            Rc::clone(&folder_state),
            Rc::clone(&folder_toast),
        );
    });
    actions.append(&folder_button);

    let url_button = gtk::Button::with_label("Open URL");
    url_button.add_css_class("okp-empty-secondary-button");
    let url_parent = window.clone();
    let url_state = Rc::clone(&state);
    let url_toast = Rc::clone(&status_toast);
    url_button.connect_clicked(move |_| {
        open_url_dialog(&url_parent, Rc::clone(&url_state), Rc::clone(&url_toast));
    });
    actions.append(&url_button);

    panel.append(&actions);

    let revealer = gtk::Revealer::new();
    revealer.add_css_class("okp-empty-surface");
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Fill);
    revealer.set_transition_duration(180);
    revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    revealer.set_reveal_child(true);
    revealer.set_child(Some(&panel));

    EmptySurface { revealer, panel }
}

fn build_controls(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) -> Controls {
    let play_button = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .build();
    play_button.set_has_frame(false);
    play_button.add_css_class("okp-control-button");
    play_button.add_css_class("okp-play-button");
    play_button.set_tooltip_text(Some("Play / Pause (Space)"));
    play_button.set_sensitive(false);

    let open_button = gtk::Button::with_label("Open");
    open_button.set_has_frame(false);
    open_button.add_css_class("okp-control-button");
    open_button.add_css_class("okp-chip-button");
    open_button.set_tooltip_text(Some("Open file (O)"));

    let subtitle_button = gtk::MenuButton::builder().label("Sub").build();
    subtitle_button.set_has_frame(false);
    subtitle_button.add_css_class("okp-control-button");
    subtitle_button.add_css_class("okp-chip-button");
    subtitle_button.set_tooltip_text(Some("Subtitles"));
    subtitle_button.set_sensitive(false);

    let audio_button = gtk::MenuButton::builder().label("Audio").build();
    audio_button.set_has_frame(false);
    audio_button.add_css_class("okp-control-button");
    audio_button.add_css_class("okp-chip-button");
    audio_button.set_tooltip_text(Some("Audio"));
    audio_button.set_sensitive(false);

    let speed_button = gtk::MenuButton::builder().label("1.00x").build();
    speed_button.set_has_frame(false);
    speed_button.add_css_class("okp-control-button");
    speed_button.add_css_class("okp-speed-chip");
    speed_button.set_tooltip_text(Some("Playback speed"));
    speed_button.set_sensitive(false);

    let previous_button = gtk::Button::builder()
        .icon_name("media-skip-backward-symbolic")
        .build();
    previous_button.set_has_frame(false);
    previous_button.add_css_class("okp-control-button");
    previous_button.add_css_class("okp-transport-button");
    previous_button.set_tooltip_text(Some("Previous item (Page Up)"));
    previous_button.set_sensitive(false);

    let elapsed_label = gtk::Label::new(Some("00:00"));
    elapsed_label.add_css_class("okp-time-label");

    let next_button = gtk::Button::builder()
        .icon_name("media-skip-forward-symbolic")
        .build();
    next_button.set_has_frame(false);
    next_button.add_css_class("okp-control-button");
    next_button.add_css_class("okp-transport-button");
    next_button.set_tooltip_text(Some("Next item (Page Down)"));
    next_button.set_sensitive(false);

    let chapters_button = gtk::Button::builder()
        .icon_name("view-list-symbolic")
        .build();
    chapters_button.set_has_frame(false);
    chapters_button.add_css_class("okp-control-button");
    chapters_button.add_css_class("okp-icon-button");
    chapters_button.set_tooltip_text(Some("Chapters / Up Next"));
    chapters_button.set_sensitive(false);

    let screenshot_button = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .build();
    screenshot_button.set_has_frame(false);
    screenshot_button.add_css_class("okp-control-button");
    screenshot_button.add_css_class("okp-icon-button");
    screenshot_button.set_tooltip_text(Some("Save frame to Pictures/OK Player (C)"));
    screenshot_button.set_sensitive(false);

    let fullscreen_button = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .build();
    fullscreen_button.set_has_frame(false);
    fullscreen_button.add_css_class("okp-control-button");
    fullscreen_button.add_css_class("okp-icon-button");
    fullscreen_button.set_tooltip_text(Some("Enter Fullscreen (F)"));
    fullscreen_button.set_sensitive(false);

    let more_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build();
    more_button.set_has_frame(false);
    more_button.add_css_class("okp-control-button");
    more_button.add_css_class("okp-icon-button");
    more_button.set_tooltip_text(Some("More commands"));

    let duration_label = gtk::Label::new(Some("00:00"));
    duration_label.add_css_class("okp-time-label");

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    seek.set_draw_value(false);
    seek.set_hexpand(true);
    seek.set_sensitive(false);
    seek.add_css_class("okp-seek");

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    volume.set_draw_value(false);
    volume.set_width_request(68);
    volume.set_value(100.0);
    volume.add_css_class("okp-volume");

    let up_next_title = gtk::Label::new(Some("Playback"));
    up_next_title.add_css_class("okp-up-next-title");
    up_next_title.set_xalign(0.0);

    let up_next_summary = gtk::Label::new(Some("No media loaded"));
    up_next_summary.add_css_class("okp-up-next-summary");
    up_next_summary.set_xalign(0.0);
    up_next_summary.set_ellipsize(pango::EllipsizeMode::End);

    let chapters_tab = side_panel_segment_button("Chapters", true);
    let up_next_tab = side_panel_segment_button("Up Next", false);
    let side_panel_mode = Rc::new(Cell::new(SidePanelMode::Chapters));
    let side_panel_tabs = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    side_panel_tabs.add_css_class("okp-side-panel-tabs");
    side_panel_tabs.append(&chapters_tab);
    side_panel_tabs.append(&up_next_tab);

    let up_next_list = gtk::ListBox::new();
    up_next_list.add_css_class("okp-up-next-list");
    up_next_list.set_selection_mode(gtk::SelectionMode::None);

    let up_next_scroller = gtk::ScrolledWindow::new();
    up_next_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    up_next_scroller.set_child(Some(&up_next_list));
    up_next_scroller.set_vexpand(true);

    let up_next_header = gtk::Box::new(gtk::Orientation::Vertical, 6);
    up_next_header.add_css_class("okp-side-panel-header");
    up_next_header.append(&up_next_title);
    up_next_header.append(&up_next_summary);
    up_next_header.append(&side_panel_tabs);

    let up_next_panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    up_next_panel.add_css_class("okp-up-next-panel");
    up_next_panel.set_width_request(344);
    up_next_panel.append(&up_next_header);
    up_next_panel.append(&up_next_scroller);

    let up_next_revealer = gtk::Revealer::new();
    up_next_revealer.set_halign(gtk::Align::End);
    up_next_revealer.set_valign(gtk::Align::Fill);
    up_next_revealer.set_margin_top(24);
    up_next_revealer.set_margin_end(24);
    up_next_revealer.set_margin_bottom(92);
    up_next_revealer.set_transition_duration(170);
    up_next_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    up_next_revealer.set_reveal_child(true);
    up_next_revealer.set_can_target(true);
    up_next_revealer.set_visible(false);
    up_next_revealer.set_child(Some(&up_next_panel));

    let side_panel_user_visible = Rc::new(Cell::new(false));
    let side_panel_pinned = Rc::new(Cell::new(false));
    let side_panel_manual_mode = Rc::new(Cell::new(false));
    let side_panel_snapshot = Rc::new(RefCell::new(SidePanelSnapshot::default()));

    let up_next_state = Rc::clone(&state);
    let up_next_actions = Rc::new(RefCell::new(Vec::<SidePanelAction>::new()));
    let row_actions = Rc::clone(&up_next_actions);
    let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
    up_next_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }

        match row_actions
            .borrow()
            .get(index as usize)
            .copied()
            .unwrap_or(SidePanelAction::None)
        {
            SidePanelAction::None => {}
            SidePanelAction::Chapter(time) => seek_to_chapter(&up_next_state, time),
            SidePanelAction::Playlist(index) => {
                jump_playlist_index(&up_next_state, index);
            }
        }
    });

    let chapters_tab_mode = Rc::clone(&side_panel_mode);
    let chapters_tab_manual_mode = Rc::clone(&side_panel_manual_mode);
    let chapters_tab_snapshot = Rc::clone(&side_panel_snapshot);
    let chapters_tab_button = chapters_tab.clone();
    let chapters_peer_tab = up_next_tab.clone();
    chapters_tab.connect_clicked(move |_| {
        chapters_tab_manual_mode.set(true);
        chapters_tab_mode.set(SidePanelMode::Chapters);
        chapters_tab_snapshot.borrow_mut().has_media = false;
        update_side_panel_tab_state(
            &chapters_tab_button,
            &chapters_peer_tab,
            SidePanelMode::Chapters,
        );
    });

    let up_next_tab_mode = Rc::clone(&side_panel_mode);
    let up_next_tab_manual_mode = Rc::clone(&side_panel_manual_mode);
    let up_next_tab_snapshot = Rc::clone(&side_panel_snapshot);
    let up_next_tab_button = up_next_tab.clone();
    let up_next_peer_tab = chapters_tab.clone();
    up_next_tab.connect_clicked(move |_| {
        up_next_tab_manual_mode.set(true);
        up_next_tab_mode.set(SidePanelMode::UpNext);
        up_next_tab_snapshot.borrow_mut().has_media = false;
        update_side_panel_tab_state(
            &up_next_peer_tab,
            &up_next_tab_button,
            SidePanelMode::UpNext,
        );
    });

    let subtitle_popover = gtk::Popover::new();
    prepare_track_popover(&subtitle_popover);
    connect_popover_chrome_pin(&subtitle_popover, Rc::clone(&chrome));
    subtitle_button.set_popover(Some(&subtitle_popover));
    let subtitle_parent = window.clone();
    let subtitle_state = Rc::clone(&state);
    subtitle_popover.connect_show(move |popover| {
        populate_subtitle_popover(popover, &subtitle_parent, Rc::clone(&subtitle_state));
    });

    let audio_popover = gtk::Popover::new();
    prepare_track_popover(&audio_popover);
    connect_popover_chrome_pin(&audio_popover, Rc::clone(&chrome));
    audio_button.set_popover(Some(&audio_popover));
    let audio_state = Rc::clone(&state);
    let audio_toast = Rc::clone(&status_toast);
    audio_popover.connect_show(move |popover| {
        populate_audio_popover(popover, Rc::clone(&audio_state), Rc::clone(&audio_toast));
    });

    let speed_popover = gtk::Popover::new();
    prepare_track_popover(&speed_popover);
    connect_popover_chrome_pin(&speed_popover, Rc::clone(&chrome));
    speed_button.set_popover(Some(&speed_popover));
    let speed_state = Rc::clone(&state);
    speed_popover.connect_show(move |popover| {
        populate_speed_popover(popover, Rc::clone(&speed_state));
    });

    let more_popover = gtk::Popover::new();
    prepare_track_popover(&more_popover);
    connect_popover_chrome_pin(&more_popover, Rc::clone(&chrome));
    more_button.set_popover(Some(&more_popover));
    let more_parent = window.clone();
    let more_state = Rc::clone(&state);
    let more_toast = Rc::clone(&status_toast);
    more_popover.connect_show(move |popover| {
        populate_command_popover(
            popover,
            &more_parent,
            Rc::clone(&more_state),
            Rc::clone(&more_toast),
        );
    });

    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    open_button.connect_clicked(move |_| open_media_dialog(&open_parent, Rc::clone(&open_state)));

    let previous_state = Rc::clone(&state);
    previous_button.connect_clicked(move |_| {
        navigate_playlist(&previous_state, -1);
    });

    let play_state = Rc::clone(&state);
    let play_open_parent = window.clone();
    play_button.connect_clicked(move |_| {
        let has_media = has_loaded_media(&play_state);
        if !has_media {
            open_media_dialog(&play_open_parent, Rc::clone(&play_state));
            return;
        }

        if let Some(mpv) = play_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.cycle_pause()
        {
            eprintln!("Failed to toggle playback: {error}");
        }
    });

    let next_state = Rc::clone(&state);
    next_button.connect_clicked(move |_| {
        navigate_playlist(&next_state, 1);
    });

    let chapters_panel = up_next_revealer.clone();
    let chapters_toggle = chapters_button.clone();
    let chapters_visible = Rc::clone(&side_panel_user_visible);
    let chapters_pinned = Rc::clone(&side_panel_pinned);
    let chapters_chrome = Rc::clone(&chrome);
    let chapters_state = Rc::clone(&state);
    let chapters_mode = Rc::clone(&side_panel_mode);
    let chapters_manual_mode = Rc::clone(&side_panel_manual_mode);
    let chapters_tab_for_toggle = chapters_tab.clone();
    let up_next_tab_for_toggle = up_next_tab.clone();
    let chapters_snapshot_for_toggle = Rc::clone(&side_panel_snapshot);
    chapters_button.connect_clicked(move |_| {
        let next_visible = !chapters_visible.get();
        if next_visible {
            let preferred_mode = preferred_side_panel_mode(&chapters_state);
            chapters_manual_mode.set(false);
            chapters_mode.set(preferred_mode);
            chapters_snapshot_for_toggle.borrow_mut().has_media = false;
            update_side_panel_tab_state(
                &chapters_tab_for_toggle,
                &up_next_tab_for_toggle,
                preferred_mode,
            );
        }
        set_side_panel_user_visible(
            &chapters_panel,
            &chapters_toggle,
            &chapters_visible,
            &chapters_pinned,
            &chapters_chrome,
            next_visible,
        );
    });

    let screenshot_state = Rc::clone(&state);
    let screenshot_toast = Rc::clone(&status_toast);
    screenshot_button
        .connect_clicked(move |_| save_screenshot(&screenshot_state, &screenshot_toast, false));

    let fullscreen_parent = window.clone();
    fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&fullscreen_parent));

    let seek_state = Rc::clone(&state);
    seek.connect_change_value(move |_, _, value| {
        if !updating_seek.get()
            && let Some(mpv) = seek_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.seek_absolute(value)
        {
            eprintln!("Failed to seek: {error}");
        }

        glib::Propagation::Proceed
    });
    connect_seek_hover(&seek, Rc::clone(&state), thumbnail_sender.clone());

    let volume_state = Rc::clone(&state);
    volume.connect_change_value(move |_, _, value| {
        if !updating_volume.get() {
            set_volume_from_ui(&volume_state, value);
        }

        glib::Propagation::Proceed
    });

    Controls {
        open_button,
        subtitle_button,
        audio_button,
        speed_button,
        previous_button,
        play_button,
        next_button,
        chapters_button,
        screenshot_button,
        fullscreen_button,
        more_button,
        seek,
        elapsed_label,
        duration_label,
        volume,
        timeline_marks_snapshot: RefCell::new(Vec::new()),
        up_next_revealer,
        up_next_title,
        up_next_summary,
        chapters_tab,
        up_next_tab,
        up_next_list,
        side_panel_user_visible,
        side_panel_pinned,
        side_panel_mode,
        side_panel_manual_mode,
        side_panel_snapshot,
        side_panel_actions: up_next_actions,
        thumbnail_sender,
        thumbnail_events: RefCell::new(thumbnail_receiver),
    }
}

fn controls_bar(controls: &Controls) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("okp-controls");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_start(18);
    bar.set_margin_end(18);
    bar.set_margin_bottom(18);

    let transport = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    transport.add_css_class("okp-transport-group");
    transport.append(&controls.previous_button);
    transport.append(&controls.play_button);
    transport.append(&controls.next_button);

    let primary = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    primary.add_css_class("okp-control-group");
    primary.append(&controls.open_button);
    primary.append(&transport);

    let timeline = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    timeline.add_css_class("okp-timeline-group");
    timeline.set_hexpand(true);
    timeline.append(&controls.elapsed_label);
    timeline.append(&controls.seek);
    timeline.append(&controls.duration_label);

    let secondary = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    secondary.add_css_class("okp-control-group");
    secondary.append(&controls.volume);
    secondary.append(&controls.speed_button);
    secondary.append(&controls.subtitle_button);
    secondary.append(&controls.audio_button);
    secondary.append(&controls.chapters_button);
    secondary.append(&controls.screenshot_button);
    secondary.append(&controls.fullscreen_button);
    secondary.append(&controls.more_button);

    bar.append(&primary);
    bar.append(&timeline);
    bar.append(&secondary);

    bar
}

fn connect_chrome_activity(overlay: &gtk::Overlay, chrome: Rc<ChromeVisibility>) {
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, _, _| {
        chrome.show_for_activity();
    });
    overlay.add_controller(motion);
}

fn connect_popover_chrome_pin(popover: &gtk::Popover, chrome: Rc<ChromeVisibility>) {
    let show_chrome = Rc::clone(&chrome);
    popover.connect_show(move |_| {
        show_chrome.pin();
    });

    popover.connect_closed(move |_| {
        chrome.unpin();
    });
}

fn prepare_track_popover(popover: &gtk::Popover) {
    popover.add_css_class("okp-track-popover");
    popover.set_has_arrow(false);
}

fn side_panel_segment_button(label: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("okp-side-panel-tab");
    button.set_has_frame(false);
    if selected {
        button.add_css_class("is-selected");
    }
    button
}

fn preferred_side_panel_mode(state: &Rc<RefCell<PlayerState>>) -> SidePanelMode {
    let state = state.borrow();
    let has_chapters = state
        .mpv
        .as_ref()
        .map(Mpv::chapters)
        .and_then(Result::ok)
        .is_some_and(|chapters| !chapters.is_empty());
    if has_chapters {
        SidePanelMode::Chapters
    } else {
        SidePanelMode::UpNext
    }
}

fn update_side_panel_tab_labels(
    chapters_tab: &gtk::Button,
    up_next_tab: &gtk::Button,
    chapters_count: usize,
    playlist_count: usize,
) {
    chapters_tab.set_label(&format!("Chapters {chapters_count}"));
    up_next_tab.set_label(&format!("Up Next {playlist_count}"));
}

fn update_side_panel_tab_state(
    chapters_tab: &gtk::Button,
    up_next_tab: &gtk::Button,
    mode: SidePanelMode,
) {
    match mode {
        SidePanelMode::Chapters => {
            chapters_tab.add_css_class("is-selected");
            up_next_tab.remove_css_class("is-selected");
        }
        SidePanelMode::UpNext => {
            up_next_tab.add_css_class("is-selected");
            chapters_tab.remove_css_class("is-selected");
        }
    }
}

fn side_panel_summary(snapshot: &SidePanelSnapshot) -> String {
    let current = snapshot
        .current_file
        .as_deref()
        .map(display_file_name)
        .or_else(|| {
            snapshot
                .current_url
                .as_ref()
                .map(|url| PlaylistItem::Url(url.clone()).display_name())
        })
        .unwrap_or_else(|| "No media loaded".to_owned());
    let chapter_label = match snapshot.chapters.len() {
        0 => "0 chapters".to_owned(),
        1 => "1 chapter".to_owned(),
        count => format!("{count} chapters"),
    };
    let item_label = match snapshot.playlist.len() {
        0 => "0 items".to_owned(),
        1 => "1 item".to_owned(),
        count => format!("{count} items"),
    };
    format!("{current} · {chapter_label} · {item_label}")
}

fn set_side_panel_user_visible(
    revealer: &gtk::Revealer,
    toggle: &gtk::Button,
    user_visible: &Rc<Cell<bool>>,
    pinned: &Rc<Cell<bool>>,
    chrome: &ChromeVisibility,
    visible: bool,
) {
    user_visible.set(visible);
    revealer.set_visible(visible);

    if visible {
        toggle.add_css_class("is-selected");
        if pinned.get() {
            chrome.show_persistently();
        } else {
            chrome.pin();
            pinned.set(true);
        }
    } else {
        toggle.remove_css_class("is-selected");
        if pinned.replace(false) {
            chrome.unpin();
        }
    }
}

fn update_fullscreen_button(button: &gtk::Button, is_fullscreen: bool) {
    if is_fullscreen {
        button.set_icon_name("view-restore-symbolic");
        button.set_tooltip_text(Some("Exit Fullscreen (F / Esc)"));
        button.add_css_class("is-selected");
    } else {
        button.set_icon_name("view-fullscreen-symbolic");
        button.set_tooltip_text(Some("Enter Fullscreen (F)"));
        button.remove_css_class("is-selected");
    }
}

fn connect_seek_hover(
    seek: &gtk::Scale,
    state: Rc<RefCell<PlayerState>>,
    thumbnail_sender: mpsc::Sender<String>,
) {
    let preview = Rc::new(SeekHoverPreview::new(seek));
    let motion = gtk::EventControllerMotion::new();

    let motion_seek = seek.clone();
    let motion_state = Rc::clone(&state);
    let motion_preview = Rc::clone(&preview);
    motion.connect_motion(move |_, x, _| {
        let Some((media_path, duration, chapters)) = seek_hover_snapshot(&motion_state) else {
            motion_preview.hide();
            return;
        };

        let width = f64::from(motion_seek.width().max(1));
        let time = (x.clamp(0.0, width) / width * duration).clamp(0.0, duration);
        let thumbnail = hover_thumbnail_for_time(
            &motion_state,
            &media_path,
            time,
            duration,
            &thumbnail_sender,
        );
        motion_preview.show(
            &motion_seek,
            x,
            time,
            chapter_at_time(&chapters, time),
            thumbnail,
        );
    });

    motion.connect_leave(move |_| {
        preview.hide();
    });

    seek.add_controller(motion);
}

fn seek_hover_snapshot(state: &Rc<RefCell<PlayerState>>) -> Option<(PathBuf, f64, Vec<Chapter>)> {
    let state = state.borrow();
    let current_file = state.current_file.clone()?;

    state
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.playback_state().ok())
        .and_then(|playback| playback.duration)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (current_file, duration, state.chapters_snapshot.clone()))
}

fn chapter_at_time(chapters: &[Chapter], time: f64) -> Option<&Chapter> {
    let mut current = None;
    for chapter in chapters {
        if chapter.time.is_finite() && chapter.time <= time {
            current = Some(chapter);
        } else {
            break;
        }
    }

    current
}

fn hover_thumbnail_for_time(
    state: &Rc<RefCell<PlayerState>>,
    media_path: &Path,
    time: f64,
    duration: f64,
    sender: &mpsc::Sender<String>,
) -> Option<PathBuf> {
    let thumbnail_time = thumbnails::hover_thumbnail_time(time, duration);
    if let Some(path) = thumbnails::existing_hover_thumbnail_path(media_path, thumbnail_time) {
        return Some(path);
    }

    let request_key = thumbnails::hover_request_key(media_path, thumbnail_time);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.hover_thumbnail_request_key.as_deref() == Some(request_key.as_str()) {
            false
        } else {
            state.hover_thumbnail_request_key = Some(request_key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_hover_thumbnail(
            media_path.to_path_buf(),
            thumbnail_time,
            request_key,
            sender.clone(),
        );
    }

    None
}

fn connect_mpv(video_area: &gtk::GLArea, state: Rc<RefCell<PlayerState>>, launch_args: LaunchArgs) {
    let realize_state = Rc::clone(&state);
    video_area.connect_realize(move |area| {
        area.make_current();
        if let Some(error) = area.error() {
            eprintln!("GTK GLArea error: {error}");
            return;
        }

        let (hwdec, raw_mpv_config) = {
            let state = realize_state.borrow();
            (
                state.settings.hardware_decode_mpv_option().to_owned(),
                state.settings.raw_mpv_config().to_owned(),
            )
        };
        let raw_mpv_options = match parse_raw_mpv_config(&raw_mpv_config) {
            Ok(options) => options,
            Err(error) => {
                eprintln!(
                    "Ignoring custom mpv.conf option at line {}: {}",
                    error.line, error.message
                );
                Vec::new()
            }
        };

        let mut mpv = match Mpv::new_with_options(&hwdec, &raw_mpv_options) {
            Ok(mpv) => mpv,
            Err(error) if !raw_mpv_options.is_empty() => {
                eprintln!(
                    "Failed to create mpv with custom mpv.conf options: {error}; retrying without them"
                );
                match Mpv::new_with_hwdec(&hwdec) {
                    Ok(mpv) => mpv,
                    Err(error) => {
                        eprintln!("Failed to create mpv: {error}");
                        return;
                    }
                }
            }
            Err(error) => {
                eprintln!("Failed to create mpv: {error}");
                return;
            }
        };
        // The realize handler runs on the GLib main context: arm the debug
        // tripwire so blocking property reads issued from this thread are
        // hard-logged with a backtrace (the deadlock class from the Windows
        // #33 postmortem). No-op in release builds.
        mpv.mark_ui_thread();
        let saved_volume = realize_state.borrow().settings.volume();
        if let Err(error) = mpv.set_volume(saved_volume) {
            eprintln!("Failed to restore saved volume: {error}");
        }
        let video_adjustments = realize_state.borrow().settings.video_adjustments();
        if let Err(error) = mpv.set_video_adjustments(
            video_adjustments.brightness,
            video_adjustments.contrast,
            video_adjustments.saturation,
            video_adjustments.gamma,
        ) {
            eprintln!("Failed to restore video adjustments: {error}");
        }
        let audio_normalization = realize_state
            .borrow()
            .settings
            .audio_normalization_enabled();
        if let Err(error) = mpv.set_audio_normalization(audio_normalization) {
            eprintln!("Failed to restore audio normalization: {error}");
        }

        if let Err(error) = mpv.create_render_context() {
            eprintln!("Failed to create mpv render context: {error}");
            return;
        }

        realize_state.borrow_mut().mpv = Some(mpv);
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);

        apply_launch_args(&realize_state, &launch_args);
    });

    let resize_state = Rc::clone(&state);
    video_area.connect_resize(move |_, width, height| {
        resize_state.borrow_mut().render_target_size =
            (width > 0 && height > 0).then_some(okp_mpv::RenderTargetSize { width, height });
    });

    let render_state = Rc::clone(&state);
    video_area.connect_render(move |area, _context| {
        area.make_current();
        area.attach_buffers();
        let viewport_size = current_render_target_size();
        let widget_width = area.width();
        let widget_height = area.height();
        let scale_factor = area.scale_factor();
        let mut state = render_state.borrow_mut();
        let target_size = resolve_render_target_size(
            viewport_size,
            state.render_target_size,
            widget_width,
            widget_height,
            scale_factor,
        );
        if let Some(mpv) = state.mpv.as_mut()
            && let Err(error) = mpv.render(target_size.width, target_size.height)
        {
            eprintln!("mpv render failed: {error}");
        }

        glib::Propagation::Stop
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |area| {
        area.make_current();
        if let Some(mpv) = unrealize_state.borrow_mut().mpv.as_mut() {
            mpv.destroy_render_context();
        }
    });

    let tick_area = video_area.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_area.queue_render();
        glib::ControlFlow::Continue
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawMpvConfigError {
    line: usize,
    message: String,
}

fn parse_raw_mpv_config(text: &str) -> Result<Vec<(String, String)>, RawMpvConfigError> {
    let mut options = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let option = trimmed.strip_prefix("--").unwrap_or(trimmed);
        let Some((name, value)) = option.split_once('=') else {
            return Err(raw_mpv_config_error(
                line_number,
                "Use key=value syntax, one option per line.",
            ));
        };
        let name = name.trim();
        let value = value.trim();

        if name.is_empty() {
            return Err(raw_mpv_config_error(line_number, "Option name is empty."));
        }
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(raw_mpv_config_error(
                line_number,
                "Option names can use letters, numbers, hyphen, or underscore.",
            ));
        }
        if name.contains('\0') || value.contains('\0') {
            return Err(raw_mpv_config_error(
                line_number,
                "NUL bytes are not valid in mpv options.",
            ));
        }
        if PROTECTED_MPV_OPTIONS
            .iter()
            .any(|protected| name.eq_ignore_ascii_case(protected))
        {
            return Err(raw_mpv_config_error(
                line_number,
                &format!("{name} is managed by OK Player."),
            ));
        }

        options.push((name.to_owned(), value.to_owned()));
    }

    Ok(options)
}

fn raw_mpv_config_error(line: usize, message: &str) -> RawMpvConfigError {
    RawMpvConfigError {
        line,
        message: message.to_owned(),
    }
}

fn apply_launch_args(state: &Rc<RefCell<PlayerState>>, launch_args: &LaunchArgs) -> bool {
    if launch_args.has_payload() {
        eprintln!(
            "Launch request: {} item(s), {} playlist(s), {} subtitle(s)",
            launch_args.items.len(),
            launch_args.playlists.len(),
            launch_args.subtitles.len()
        );
    }

    let loaded = load_launch_args(state, launch_args);
    let subtitles_loaded = apply_launch_subtitles(state, &launch_args.subtitles);
    loaded || subtitles_loaded
}

fn load_launch_args(state: &Rc<RefCell<PlayerState>>, launch_args: &LaunchArgs) -> bool {
    match launch_args.items.as_slice() {
        [PlaylistItem::Local(path)] => {
            load_media_path(state, path.clone());
            true
        }
        [PlaylistItem::Url(url)] => {
            load_media_url(state, url.clone());
            true
        }
        [] => launch_args
            .playlists
            .first()
            .is_some_and(|path| load_m3u_playlist_silent(state, path)),
        items => {
            let playlist = items.to_vec();
            let Some(first_item) = playlist.first().cloned() else {
                return false;
            };
            load_playlist_item_with_playlist(state, first_item, playlist, true)
        }
    }
}

fn apply_launch_subtitles(state: &Rc<RefCell<PlayerState>>, subtitles: &[PathBuf]) -> bool {
    let mut applied = false;
    for path in subtitles {
        if load_subtitle_path(state, path.clone()) {
            applied = true;
        } else if !has_loaded_media(state) {
            let mut state = state.borrow_mut();
            if !state
                .pending_subtitles
                .iter()
                .any(|existing| existing == path)
            {
                state.pending_subtitles.push(path.clone());
            }
        }
    }
    applied
}

fn connect_state_poll(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    controls: Controls,
    context: StatePollContext,
) {
    let window = window.clone();
    let StatePollContext {
        updating_seek,
        updating_volume,
        chrome,
        empty_surface,
        mpris_snapshot,
        mpris_signals,
    } = context;
    glib::timeout_add_local(Duration::from_millis(200), move || {
        drain_mpv_events(&state);
        try_pending_audio_device_restore(&state);

        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok());
        let has_media = has_loaded_media(&state);
        let has_playlist = state.borrow().playlist.len() > 1;
        {
            let state = state.borrow();
            update_mpris_snapshot(&mpris_snapshot, &mpris_signals, &state, playback);
        }
        sync_ab_loop_state(&state, has_media);
        empty_surface.set_has_media(has_media);
        drain_thumbnail_events(&controls);
        update_up_next_panel(&controls, &state, &chrome);

        if let Some(playback) = playback {
            try_pending_subtitles(&state);
            chrome.set_auto_hide_enabled(has_media && !playback.paused);

            let duration = playback.duration.unwrap_or(0.0).max(0.0);
            let raw_time = playback.time_pos.unwrap_or(0.0).max(0.0);
            let time_pos = if duration > 0.0 {
                raw_time.min(duration)
            } else {
                raw_time
            };
            try_pending_resume(&state, duration);

            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls.play_button.set_icon_name(if playback.paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            });
            controls
                .play_button
                .set_tooltip_text(Some(if playback.paused {
                    "Play (Space)"
                } else {
                    "Pause (Space)"
                }));
            controls
                .speed_button
                .set_label(&format_speed(playback.speed.unwrap_or(1.0)));
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(has_media && duration > 0.0);

            updating_seek.set(true);
            controls.seek.set_range(0.0, duration.max(1.0));
            controls.seek.set_value(time_pos);
            updating_seek.set(false);

            if let Some(volume) = playback.volume {
                updating_volume.set(true);
                controls.volume.set_value(volume.clamp(0.0, 130.0));
                updating_volume.set(false);
            }

            controls.elapsed_label.set_text(&format_time(time_pos));
            controls.duration_label.set_text(&format_time(duration));
        } else {
            chrome.set_auto_hide_enabled(false);
            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls
                .play_button
                .set_icon_name("media-playback-start-symbolic");
            controls.play_button.set_tooltip_text(Some("Play (Space)"));
            controls.speed_button.set_label("1.00x");
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(false);
            updating_seek.set(true);
            controls.seek.set_range(0.0, 1.0);
            controls.seek.set_value(0.0);
            updating_seek.set(false);
            controls.elapsed_label.set_text("00:00");
            controls.duration_label.set_text("00:00");
        }

        glib::ControlFlow::Continue
    });
}

fn connect_video_clicks(
    video_area: &gtk::GLArea,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(1);

    let click_window = window.clone();
    click.connect_released(move |_, press_count, _, _| {
        if press_count == 2 {
            toggle_fullscreen(&click_window);
        }
    });

    video_area.add_controller(click);

    let context_click = gtk::GestureClick::new();
    context_click.set_button(3);

    let context_area = video_area.clone();
    let context_window = window.clone();
    let context_state = Rc::clone(&state);
    let context_toast = Rc::clone(&status_toast);
    context_click.connect_pressed(move |_, _, x, y| {
        show_video_context_menu(
            &context_area,
            &context_window,
            Rc::clone(&context_state),
            Rc::clone(&context_toast),
            x,
            y,
        );
    });

    video_area.add_controller(context_click);
}

fn show_video_context_menu(
    video_area: &gtk::GLArea,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    x: f64,
    y: f64,
) {
    let popover = gtk::Popover::new();
    prepare_track_popover(&popover);
    popover.set_parent(video_area);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let content = command_popover_content(&popover, parent, state, status_toast);
    set_track_popover_child(&popover, content);
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn update_up_next_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    chrome: &ChromeVisibility,
) {
    let snapshot = {
        let state = state.borrow();
        let has_media = has_loaded_media_state(&state);
        let chapters = state
            .mpv
            .as_ref()
            .map(Mpv::chapters)
            .and_then(Result::ok)
            .unwrap_or_default();

        SidePanelSnapshot {
            has_media,
            current_file: state.current_file.clone(),
            current_url: state.current_url.clone(),
            playlist: state.playlist.clone(),
            chapters,
            ab_loop: state.ab_loop,
        }
    };

    {
        let mut state = state.borrow_mut();
        if state.chapters_snapshot != snapshot.chapters {
            state.chapters_snapshot = snapshot.chapters.clone();
        }
    }

    controls.chapters_button.set_sensitive(snapshot.has_media);
    if !snapshot.has_media {
        set_side_panel_user_visible(
            &controls.up_next_revealer,
            &controls.chapters_button,
            &controls.side_panel_user_visible,
            &controls.side_panel_pinned,
            chrome,
            false,
        );
        controls.side_panel_snapshot.replace(snapshot);
        controls.side_panel_actions.borrow_mut().clear();
        update_timeline_marks(
            &controls.seek,
            &controls.timeline_marks_snapshot,
            &[],
            AbLoopState::default(),
        );
        clear_list_box(&controls.up_next_list);
        return;
    }

    let panel_visible = controls.side_panel_user_visible.get();
    controls.up_next_revealer.set_visible(panel_visible);
    if panel_visible {
        controls.chapters_button.add_css_class("is-selected");
    } else {
        controls.chapters_button.remove_css_class("is-selected");
    }

    let previous_snapshot = controls.side_panel_snapshot.borrow().clone();
    request_chapter_thumbnail_warm(controls, state, &snapshot);

    if previous_snapshot == snapshot {
        return;
    }

    if panel_visible
        && !controls.side_panel_manual_mode.get()
        && previous_snapshot.chapters.is_empty()
        && !snapshot.chapters.is_empty()
    {
        controls.side_panel_mode.set(SidePanelMode::Chapters);
    }

    controls.side_panel_snapshot.replace(snapshot.clone());
    update_timeline_marks(
        &controls.seek,
        &controls.timeline_marks_snapshot,
        &snapshot.chapters,
        snapshot.ab_loop,
    );

    let current_index = snapshot.playlist.iter().position(|item| {
        item.is_current(
            snapshot.current_file.as_deref(),
            snapshot.current_url.as_deref(),
        )
    });

    let mode = controls.side_panel_mode.get();
    update_side_panel_tab_labels(
        &controls.chapters_tab,
        &controls.up_next_tab,
        snapshot.chapters.len(),
        snapshot.playlist.len(),
    );
    update_side_panel_tab_state(&controls.chapters_tab, &controls.up_next_tab, mode);
    controls.up_next_title.set_text(match mode {
        SidePanelMode::Chapters => "Chapters",
        SidePanelMode::UpNext => "Up Next",
    });
    controls
        .up_next_summary
        .set_text(&side_panel_summary(&snapshot));
    clear_list_box(&controls.up_next_list);
    let mut actions = Vec::new();

    match mode {
        SidePanelMode::Chapters => render_chapters_panel(controls, &snapshot, &mut actions),
        SidePanelMode::UpNext => {
            render_playlist_panel(controls, state, &snapshot, current_index, &mut actions)
        }
    }

    controls.side_panel_actions.replace(actions);
}

fn render_chapters_panel(
    controls: &Controls,
    snapshot: &SidePanelSnapshot,
    actions: &mut Vec<SidePanelAction>,
) {
    if snapshot.chapters.is_empty() {
        controls
            .up_next_list
            .append(&panel_empty_row("No chapters in this media yet."));
        actions.push(SidePanelAction::None);
        return;
    }

    controls.up_next_list.append(&panel_heading_row(&format!(
        "Chapters · {}",
        snapshot.chapters.len()
    )));
    actions.push(SidePanelAction::None);

    for chapter in &snapshot.chapters {
        let thumbnail = snapshot
            .current_file
            .as_ref()
            .and_then(|path| thumbnails::existing_thumbnail_path(path, chapter));
        controls
            .up_next_list
            .append(&chapter_row(chapter, thumbnail));
        actions.push(SidePanelAction::Chapter(chapter.time));
    }
}

fn render_playlist_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
    current_index: Option<usize>,
    actions: &mut Vec<SidePanelAction>,
) {
    if snapshot.playlist.len() <= 1 {
        controls
            .up_next_list
            .append(&panel_empty_row("No folder queue for this media yet."));
        actions.push(SidePanelAction::None);
        return;
    }

    controls.up_next_list.append(&panel_heading_row(&format!(
        "Up Next · {}",
        snapshot.playlist.len()
    )));
    actions.push(SidePanelAction::None);

    for (index, item) in snapshot.playlist.iter().enumerate() {
        controls.up_next_list.append(&playlist_row(
            item,
            index,
            current_index,
            snapshot.playlist.len(),
            Rc::clone(state),
        ));
        actions.push(SidePanelAction::Playlist(index));
    }
}

fn drain_thumbnail_events(controls: &Controls) {
    let mut changed = false;
    while controls.thumbnail_events.borrow().try_recv().is_ok() {
        changed = true;
    }

    if changed {
        controls
            .side_panel_snapshot
            .replace(SidePanelSnapshot::default());
    }
}

fn request_chapter_thumbnail_warm(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
) {
    let Some(media_path) = snapshot.current_file.as_ref() else {
        return;
    };
    if snapshot.chapters.is_empty() {
        return;
    }

    let key = thumbnails::request_key(media_path, &snapshot.chapters);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.thumbnail_request_key.as_deref() == Some(key.as_str()) {
            false
        } else {
            state.thumbnail_request_key = Some(key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_chapter_thumbnails(
            media_path.clone(),
            snapshot.chapters.clone(),
            key,
            controls.thumbnail_sender.clone(),
        );
    }
}

fn update_timeline_marks(
    seek: &gtk::Scale,
    snapshot: &RefCell<Vec<TimelineMark>>,
    chapters: &[Chapter],
    ab_loop: AbLoopState,
) {
    let marks = timeline_marks(chapters, ab_loop);
    if *snapshot.borrow() == marks {
        return;
    }

    seek.clear_marks();
    for mark in &marks {
        let (position, label) = match mark.kind {
            TimelineMarkKind::Chapter => (gtk::PositionType::Top, None),
            TimelineMarkKind::AbStart => (gtk::PositionType::Bottom, Some("A")),
            TimelineMarkKind::AbEnd => (gtk::PositionType::Bottom, Some("B")),
            TimelineMarkKind::AbLoop => (gtk::PositionType::Bottom, Some("A-B")),
        };
        seek.add_mark(mark.time, position, label);
    }
    snapshot.replace(marks);
}

fn timeline_marks(chapters: &[Chapter], ab_loop: AbLoopState) -> Vec<TimelineMark> {
    let mut marks = chapters
        .iter()
        .map(|chapter| TimelineMark {
            time: chapter.time,
            kind: TimelineMarkKind::Chapter,
        })
        .filter(|mark| mark.time.is_finite() && mark.time > 0.0)
        .collect::<Vec<_>>();

    let ab_start = ab_loop.a.filter(|time| time.is_finite() && *time >= 0.0);
    let ab_end = ab_loop.b.filter(|time| time.is_finite() && *time >= 0.0);
    match (ab_start, ab_end) {
        (Some(a), Some(b)) if should_combine_ab_loop_marks(a, b) => marks.push(TimelineMark {
            time: a + ((b - a) / 2.0),
            kind: TimelineMarkKind::AbLoop,
        }),
        (Some(a), Some(b)) => {
            marks.push(TimelineMark {
                time: a,
                kind: TimelineMarkKind::AbStart,
            });
            marks.push(TimelineMark {
                time: b,
                kind: TimelineMarkKind::AbEnd,
            });
        }
        (Some(time), None) => marks.push(TimelineMark {
            time,
            kind: TimelineMarkKind::AbStart,
        }),
        (None, Some(time)) => marks.push(TimelineMark {
            time,
            kind: TimelineMarkKind::AbEnd,
        }),
        (None, None) => {}
    }

    marks
}

fn should_combine_ab_loop_marks(a: f64, b: f64) -> bool {
    (a - b).abs() <= AB_LOOP_COMBINED_MARK_EPSILON_SECS
}

fn panel_heading_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-heading-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-heading");
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

fn panel_empty_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-empty-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-empty");
    label.set_wrap(true);
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

fn chapter_row(chapter: &Chapter, thumbnail: Option<PathBuf>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_selectable(false);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let thumbnail_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    thumbnail_box.add_css_class("okp-chapter-thumb");
    thumbnail_box.set_size_request(88, 50);
    if let Some(thumbnail) = thumbnail {
        let picture = gtk::Picture::for_filename(thumbnail);
        picture.set_size_request(88, 50);
        picture.set_can_shrink(true);
        thumbnail_box.append(&picture);
    }

    let title_text = chapter
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));

    let label_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    label_box.set_hexpand(true);

    let time = gtk::Label::new(Some(&format_time(chapter.time)));
    time.add_css_class("okp-up-next-marker");
    time.set_xalign(0.0);

    let title = gtk::Label::new(Some(&title_text));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    label_box.append(&time);
    label_box.append(&title);
    row_box.append(&thumbnail_box);
    row_box.append(&label_box);
    row.set_child(Some(&row_box));
    row
}

fn clear_list_box(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn playlist_row(
    item: &PlaylistItem,
    index: usize,
    current_index: Option<usize>,
    playlist_len: usize,
    state: Rc<RefCell<PlayerState>>,
) -> gtk::ListBoxRow {
    let is_current = current_index == Some(index);
    let is_next = current_index.is_some_and(|current| index == current + 1);
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_activatable(!is_current);
    row.set_selectable(false);
    row.set_tooltip_text(Some(&item.display_location()));
    if is_current {
        row.add_css_class("is-current");
    }

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let marker = gtk::Label::new(Some(if is_current {
        "Now"
    } else if is_next {
        "Next"
    } else {
        ""
    }));
    marker.add_css_class("okp-up-next-marker");
    marker.set_width_chars(4);
    marker.set_xalign(0.0);

    let title = gtk::Label::new(Some(&item.display_name()));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    let drag_handle = playlist_drag_handle();

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    actions.add_css_class("okp-up-next-actions");

    let move_up = playlist_action_button("go-up-symbolic", "Move up", index > 0);
    let move_up_state = Rc::clone(&state);
    move_up.connect_clicked(move |_| {
        move_playlist_item(&move_up_state, index, index.saturating_sub(1));
    });
    actions.append(&move_up);

    let play_next_sensitive =
        current_index.is_some_and(|current| index != current && index != current + 1);
    let play_next = playlist_action_button(
        "media-skip-forward-symbolic",
        "Play next",
        play_next_sensitive,
    );
    let play_next_state = Rc::clone(&state);
    play_next.connect_clicked(move |_| {
        if let Some(current) = current_index {
            let target = if index < current {
                current
            } else {
                current + 1
            };
            move_playlist_item(&play_next_state, index, target);
        }
    });
    actions.append(&play_next);

    let move_down =
        playlist_action_button("go-down-symbolic", "Move down", index + 1 < playlist_len);
    let move_down_state = Rc::clone(&state);
    move_down.connect_clicked(move |_| {
        move_playlist_item(&move_down_state, index, index + 1);
    });
    actions.append(&move_down);

    let remove = playlist_action_button(
        "list-remove-symbolic",
        "Remove from queue",
        !is_current && playlist_len > 1,
    );
    let remove_state = Rc::clone(&state);
    remove.connect_clicked(move |_| {
        remove_playlist_item(&remove_state, index);
    });
    actions.append(&remove);

    connect_playlist_row_drag_reorder(&row, &drag_handle, index, state);

    row_box.append(&drag_handle);
    row_box.append(&marker);
    row_box.append(&title);
    row_box.append(&actions);
    row.set_child(Some(&row_box));
    row
}

fn playlist_drag_handle() -> gtk::Box {
    let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    handle.add_css_class("okp-up-next-drag-handle");
    handle.set_tooltip_text(Some("Drag to reorder"));
    handle.set_valign(gtk::Align::Center);
    handle.set_can_target(true);

    let icon = gtk::Image::from_icon_name("view-more-symbolic");
    icon.add_css_class("okp-up-next-drag-handle-icon");
    handle.append(&icon);

    handle
}

fn playlist_action_button(icon_name: &str, tooltip: &str, sensitive: bool) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon_name);
    button.add_css_class("okp-up-next-action-button");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_sensitive(sensitive);
    button
}

fn connect_playlist_row_drag_reorder(
    row: &gtk::ListBoxRow,
    handle: &impl IsA<gtk::Widget>,
    index: usize,
    state: Rc<RefCell<PlayerState>>,
) {
    let drag = gtk::DragSource::builder()
        .actions(gdk::DragAction::MOVE)
        .build();
    drag.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&(index as u32).to_value()))
    });
    handle.add_controller(drag);

    let drop = gtk::DropTarget::new(u32::static_type(), gdk::DragAction::MOVE);
    let enter_row = row.clone();
    drop.connect_enter(move |_, _, _| {
        enter_row.add_css_class("is-drop-target");
        gdk::DragAction::MOVE
    });
    let leave_row = row.clone();
    drop.connect_leave(move |_| {
        leave_row.remove_css_class("is-drop-target");
    });
    let drop_row = row.clone();
    drop.connect_drop(move |_, value, _, y| {
        drop_row.remove_css_class("is-drop-target");
        let Ok(source_index) = value.get::<u32>() else {
            return false;
        };
        let drop_after = y >= f64::from(drop_row.allocated_height()) / 2.0;
        let Some(target_index) =
            playlist_drop_target_index(source_index as usize, index, drop_after)
        else {
            return false;
        };
        move_playlist_item(&state, source_index as usize, target_index)
    });
    row.add_controller(drop);
}

fn playlist_drop_target_index(
    source_index: usize,
    row_index: usize,
    drop_after: bool,
) -> Option<usize> {
    if source_index == row_index {
        return None;
    }

    let target = match (drop_after, source_index < row_index) {
        (false, true) => row_index.saturating_sub(1),
        (false, false) => row_index,
        (true, true) => row_index,
        (true, false) => row_index + 1,
    };

    (target != source_index).then_some(target)
}

fn populate_subtitle_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let content = track_popover_content("Subtitles");
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let any_selected = tracks.iter().any(|track| track.selected);
    let secondary_subtitle_id = read_secondary_subtitle_id(&state);

    let off_button = track_button("Off", !any_selected);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_subtitle(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No subtitle tracks"));
    } else {
        for track in &tracks {
            let button = track_button(&track_label(track), track.selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| mpv.select_subtitle(Some(track_id))) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    content.append(&track_section_title("Secondary"));

    let secondary_off_button = track_button("Off", secondary_subtitle_id.is_none());
    let secondary_off_state = Rc::clone(&state);
    let secondary_off_popover = popover.clone();
    secondary_off_button.connect_clicked(move |_| {
        if with_mpv(&secondary_off_state, |mpv| {
            mpv.select_secondary_subtitle(None)
        }) {
            save_current_preferences(&secondary_off_state);
        }
        secondary_off_popover.popdown();
    });
    content.append(&secondary_off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No subtitle tracks"));
    } else {
        for track in &tracks {
            let selected = secondary_subtitle_id == Some(track.id);
            let button = track_button(&track_label_for(track, selected), selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| {
                    mpv.select_secondary_subtitle(Some(track_id))
                }) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    let add_button = track_button("Add subtitle file...", false);
    let add_state = Rc::clone(&state);
    let add_parent = parent.clone();
    let add_popover = popover.clone();
    add_button.connect_clicked(move |_| {
        add_popover.popdown();
        open_subtitle_dialog(&add_parent, Rc::clone(&add_state));
    });
    content.append(&add_button);

    content.append(&divider());
    content.append(&subtitle_adjustment_rows(popover, parent, &state));

    set_track_popover_child(popover, content);
}

fn populate_audio_popover(
    popover: &gtk::Popover,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = track_popover_content("Audio");
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect::<Vec<_>>();
    let any_selected = tracks.iter().any(|track| track.selected);

    let off_button = track_button("Off", !any_selected);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_audio(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No audio tracks"));
    } else {
        for track in tracks {
            let button = track_button(&track_label(&track), track.selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| mpv.select_audio(Some(track_id))) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    content.append(&track_section_title("Output Device"));
    let devices = read_audio_devices(&state);
    if devices.is_empty() {
        content.append(&empty_track_label("No output devices"));
    } else {
        for device in devices {
            let button = track_button(&device.label, device.selected);
            let device_state = Rc::clone(&state);
            let device_popover = popover.clone();
            let device_toast = Rc::clone(&status_toast);
            let device_name = device.name.clone();
            let device_label = device.label.clone();
            button.connect_clicked(move |_| {
                if with_mpv(&device_state, |mpv| mpv.set_audio_device(&device_name)) {
                    save_audio_device_setting(
                        &device_state,
                        &device_name,
                        Some(device_toast.as_ref()),
                    );
                    device_toast.show(&format!("Audio output: {device_label}"));
                }
                device_popover.popdown();
            });
            content.append(&button);
        }
    }

    set_track_popover_child(popover, content);
}

fn populate_speed_popover(popover: &gtk::Popover, state: Rc<RefCell<PlayerState>>) {
    let content = track_popover_content("Speed");
    let current_speed = read_playback_speed(&state);

    for speed in SPEED_PRESETS {
        let button = track_button(&format_speed(speed), speed_matches(current_speed, speed));
        let speed_state = Rc::clone(&state);
        let speed_popover = popover.clone();
        button.connect_clicked(move |_| {
            set_playback_speed_from_ui(&speed_state, speed);
            speed_popover.popdown();
        });
        content.append(&button);
    }

    set_track_popover_child(popover, content);
}

fn populate_command_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = command_popover_content(popover, parent, state, status_toast);
    set_track_popover_child(popover, content);
}

fn command_popover_content(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = track_popover_content("More");
    let (
        has_media,
        repeat_mode,
        shuffle_enabled,
        auto_advance_enabled,
        private_session,
        playlist_count,
        has_local_media,
        video_transform,
        ab_loop_active,
    ) = {
        let state = state.borrow();
        (
            has_loaded_media_state(&state),
            state.modes.repeat_mode,
            state.modes.shuffle_enabled,
            state.modes.auto_advance_enabled,
            state.private_session,
            state.playlist.len(),
            state.current_file.is_some(),
            state.video_transform.clone(),
            state.ab_loop.is_active(),
        )
    };

    let open_url_button = track_button("Open URL...", false);
    let open_url_parent = parent.clone();
    let open_url_state = Rc::clone(&state);
    let open_url_toast = Rc::clone(&status_toast);
    let open_url_popover = popover.clone();
    open_url_button.connect_clicked(move |_| {
        open_url_popover.popdown();
        open_url_dialog(
            &open_url_parent,
            Rc::clone(&open_url_state),
            Rc::clone(&open_url_toast),
        );
    });
    content.append(&open_url_button);

    let open_folder_button = track_button("Open Folder...", false);
    let open_folder_parent = parent.clone();
    let open_folder_state = Rc::clone(&state);
    let open_folder_toast = Rc::clone(&status_toast);
    let open_folder_popover = popover.clone();
    open_folder_button.connect_clicked(move |_| {
        open_folder_popover.popdown();
        open_folder_dialog(
            &open_folder_parent,
            Rc::clone(&open_folder_state),
            Rc::clone(&open_folder_toast),
        );
    });
    content.append(&open_folder_button);

    let open_playlist_button = track_button("Open Playlist...", false);
    let open_playlist_parent = parent.clone();
    let open_playlist_state = Rc::clone(&state);
    let open_playlist_toast = Rc::clone(&status_toast);
    let open_playlist_popover = popover.clone();
    open_playlist_button.connect_clicked(move |_| {
        open_playlist_popover.popdown();
        open_playlist_dialog(
            &open_playlist_parent,
            Rc::clone(&open_playlist_state),
            Rc::clone(&open_playlist_toast),
        );
    });
    content.append(&open_playlist_button);

    let add_queue_button = track_button("Add to Queue...", false);
    add_queue_button.set_sensitive(has_local_media);
    add_queue_button.set_tooltip_text(Some("Append local media files to Up Next"));
    let add_queue_parent = parent.clone();
    let add_queue_state = Rc::clone(&state);
    let add_queue_toast = Rc::clone(&status_toast);
    let add_queue_popover = popover.clone();
    add_queue_button.connect_clicked(move |_| {
        add_queue_popover.popdown();
        open_queue_media_dialog(
            &add_queue_parent,
            Rc::clone(&add_queue_state),
            Rc::clone(&add_queue_toast),
            QueueInsertMode::Append,
        );
    });
    content.append(&add_queue_button);

    let play_next_button = track_button("Play Next...", false);
    play_next_button.set_sensitive(has_local_media);
    play_next_button.set_tooltip_text(Some("Insert local media files after the current item"));
    let play_next_parent = parent.clone();
    let play_next_state = Rc::clone(&state);
    let play_next_toast = Rc::clone(&status_toast);
    let play_next_popover = popover.clone();
    play_next_button.connect_clicked(move |_| {
        play_next_popover.popdown();
        open_queue_media_dialog(
            &play_next_parent,
            Rc::clone(&play_next_state),
            Rc::clone(&play_next_toast),
            QueueInsertMode::PlayNext,
        );
    });
    content.append(&play_next_button);

    let save_playlist_button = track_button("Save Playlist...", false);
    save_playlist_button.set_sensitive(playlist_count > 0);
    save_playlist_button.set_tooltip_text(Some("Save current Up Next list as M3U"));
    let save_playlist_parent = parent.clone();
    let save_playlist_state = Rc::clone(&state);
    let save_playlist_toast = Rc::clone(&status_toast);
    let save_playlist_popover = popover.clone();
    save_playlist_button.connect_clicked(move |_| {
        save_playlist_popover.popdown();
        save_playlist_dialog(
            &save_playlist_parent,
            Rc::clone(&save_playlist_state),
            Rc::clone(&save_playlist_toast),
        );
    });
    content.append(&save_playlist_button);

    let settings_button = track_button("Settings...", false);
    let settings_parent = parent.clone();
    let settings_state = Rc::clone(&state);
    let settings_toast = Rc::clone(&status_toast);
    let settings_popover = popover.clone();
    settings_button.connect_clicked(move |_| {
        settings_popover.popdown();
        open_settings_window(
            &settings_parent,
            Rc::clone(&settings_state),
            Rc::clone(&settings_toast),
        );
    });
    content.append(&settings_button);

    let info_button = track_button("Media Info...", false);
    info_button.set_sensitive(has_media);
    info_button.set_tooltip_text(Some("Media Information (I)"));
    let info_parent = parent.clone();
    let info_state = Rc::clone(&state);
    let info_toast = Rc::clone(&status_toast);
    let info_popover = popover.clone();
    info_button.connect_clicked(move |_| {
        info_popover.popdown();
        open_media_info_window(&info_parent, &info_state, Rc::clone(&info_toast));
    });
    content.append(&info_button);

    let location_button = track_button("Open File Location", false);
    location_button.set_sensitive(has_local_media);
    location_button.set_tooltip_text(Some("Open the current file in the file manager"));
    let location_state = Rc::clone(&state);
    let location_toast = Rc::clone(&status_toast);
    let location_popover = popover.clone();
    location_button.connect_clicked(move |_| {
        location_popover.popdown();
        open_current_file_location(&location_state, &location_toast);
    });
    content.append(&location_button);

    let go_to_time_button = track_button("Go to Time...", false);
    go_to_time_button.set_sensitive(has_media);
    go_to_time_button.set_tooltip_text(Some("Go to timecode (J)"));
    let go_to_time_parent = parent.clone();
    let go_to_time_state = Rc::clone(&state);
    let go_to_time_toast = Rc::clone(&status_toast);
    let go_to_time_popover = popover.clone();
    go_to_time_button.connect_clicked(move |_| {
        go_to_time_popover.popdown();
        open_go_to_time_dialog(
            &go_to_time_parent,
            Rc::clone(&go_to_time_state),
            Rc::clone(&go_to_time_toast),
        );
    });
    content.append(&go_to_time_button);

    let copy_time_button = track_button("Copy Current Time", false);
    copy_time_button.set_sensitive(has_media);
    copy_time_button.set_tooltip_text(Some("Copy the current timecode"));
    let copy_time_state = Rc::clone(&state);
    let copy_time_toast = Rc::clone(&status_toast);
    let copy_time_popover = popover.clone();
    copy_time_button.connect_clicked(move |_| {
        copy_time_popover.popdown();
        copy_current_time(&copy_time_state, &copy_time_toast);
    });
    content.append(&copy_time_button);

    let ab_loop_button = track_button("A-B loop", ab_loop_active);
    ab_loop_button.set_sensitive(has_media);
    ab_loop_button.set_tooltip_text(Some("Set A, set B, clear (L)"));
    let ab_loop_state = Rc::clone(&state);
    let ab_loop_toast = Rc::clone(&status_toast);
    let ab_loop_popover = popover.clone();
    ab_loop_button.connect_clicked(move |_| {
        ab_loop_popover.popdown();
        toggle_ab_loop(&ab_loop_state, &ab_loop_toast);
    });
    content.append(&ab_loop_button);

    content.append(&divider());
    content.append(&track_section_title("Video"));
    content.append(&track_section_title("Aspect ratio"));
    for (label, aspect) in VIDEO_ASPECT_PRESETS {
        let button = track_button(label, video_transform.aspect_override == aspect);
        button.set_sensitive(has_media);
        let aspect_state = Rc::clone(&state);
        let aspect_toast = Rc::clone(&status_toast);
        let aspect_popover = popover.clone();
        button.connect_clicked(move |_| {
            aspect_popover.popdown();
            set_video_aspect(&aspect_state, aspect, &aspect_toast);
        });
        content.append(&button);
    }

    let rotate_button = track_button("Rotate 90°", false);
    rotate_button.set_sensitive(has_media);
    let rotate_state = Rc::clone(&state);
    let rotate_toast = Rc::clone(&status_toast);
    let rotate_popover = popover.clone();
    rotate_button.connect_clicked(move |_| {
        rotate_popover.popdown();
        rotate_video_clockwise(&rotate_state, &rotate_toast);
    });
    content.append(&rotate_button);

    let fill_button = track_button("Fill screen (crop bars)", video_transform.fill_screen);
    fill_button.set_sensitive(has_media);
    let fill_state = Rc::clone(&state);
    let fill_toast = Rc::clone(&status_toast);
    let fill_popover = popover.clone();
    fill_button.connect_clicked(move |_| {
        fill_popover.popdown();
        toggle_video_fill_screen(&fill_state, &fill_toast);
    });
    content.append(&fill_button);

    let reset_video_button = track_button("Reset video", false);
    reset_video_button.set_sensitive(has_media);
    let reset_video_state = Rc::clone(&state);
    let reset_video_toast = Rc::clone(&status_toast);
    let reset_video_popover = popover.clone();
    reset_video_button.connect_clicked(move |_| {
        reset_video_popover.popdown();
        reset_video_transform(&reset_video_state, &reset_video_toast);
    });
    content.append(&reset_video_button);

    content.append(&divider());
    content.append(&track_section_title("Screenshot"));

    let save_frame_button = track_button("Save frame", false);
    save_frame_button.set_sensitive(has_media);
    let save_frame_state = Rc::clone(&state);
    let save_frame_toast = Rc::clone(&status_toast);
    let save_frame_popover = popover.clone();
    save_frame_button.connect_clicked(move |_| {
        save_frame_popover.popdown();
        save_screenshot(&save_frame_state, &save_frame_toast, false);
    });
    content.append(&save_frame_button);

    let save_subs_button = track_button("Save frame with subtitles", false);
    save_subs_button.set_sensitive(has_media);
    let save_subs_state = Rc::clone(&state);
    let save_subs_toast = Rc::clone(&status_toast);
    let save_subs_popover = popover.clone();
    save_subs_button.connect_clicked(move |_| {
        save_subs_popover.popdown();
        save_screenshot(&save_subs_state, &save_subs_toast, true);
    });
    content.append(&save_subs_button);

    let copy_frame_button = track_button("Copy frame to clipboard", false);
    copy_frame_button.set_sensitive(has_media);
    let copy_frame_state = Rc::clone(&state);
    let copy_frame_toast = Rc::clone(&status_toast);
    let copy_frame_popover = popover.clone();
    copy_frame_button.connect_clicked(move |_| {
        copy_frame_popover.popdown();
        copy_frame_to_clipboard(&copy_frame_state, &copy_frame_toast);
    });
    content.append(&copy_frame_button);

    let close_button = track_button("Close Media", false);
    close_button.set_sensitive(has_media);
    let close_state = Rc::clone(&state);
    let close_toast = Rc::clone(&status_toast);
    let close_popover = popover.clone();
    close_button.connect_clicked(move |_| {
        close_popover.popdown();
        close_current_media(&close_state, &close_toast);
    });
    content.append(&close_button);

    let fullscreen_label = if parent.is_fullscreen() {
        "Exit Fullscreen"
    } else {
        "Enter Fullscreen"
    };
    let fullscreen_button = track_button(fullscreen_label, parent.is_fullscreen());
    let fullscreen_parent = parent.clone();
    let fullscreen_popover = popover.clone();
    fullscreen_button.connect_clicked(move |_| {
        fullscreen_popover.popdown();
        toggle_fullscreen(&fullscreen_parent);
    });
    content.append(&fullscreen_button);

    content.append(&divider());

    let private_button = track_button(
        if private_session {
            "Private Session On"
        } else {
            "Private Session Off"
        },
        private_session,
    );
    let private_state = Rc::clone(&state);
    let private_toast = Rc::clone(&status_toast);
    let private_popover = popover.clone();
    private_button.connect_clicked(move |_| {
        toggle_private_session(&private_state, &private_toast);
        private_popover.popdown();
    });
    content.append(&private_button);

    let clear_history_button = track_button("Clear History...", false);
    let clear_history_parent = parent.clone();
    let clear_history_state = Rc::clone(&state);
    let clear_history_toast = Rc::clone(&status_toast);
    let clear_history_popover = popover.clone();
    clear_history_button.connect_clicked(move |_| {
        clear_history_popover.popdown();
        open_clear_history_dialog(
            &clear_history_parent,
            Rc::clone(&clear_history_state),
            Rc::clone(&clear_history_toast),
        );
    });
    content.append(&clear_history_button);

    content.append(&divider());

    let repeat_button = track_button(repeat_mode.label(), repeat_mode != RepeatMode::Off);
    let repeat_state = Rc::clone(&state);
    let repeat_toast = Rc::clone(&status_toast);
    let repeat_popover = popover.clone();
    repeat_button.connect_clicked(move |_| {
        cycle_repeat_mode(&repeat_state, &repeat_toast);
        repeat_popover.popdown();
    });
    content.append(&repeat_button);

    let shuffle_button = track_button(
        if shuffle_enabled {
            "Shuffle On"
        } else {
            "Shuffle Off"
        },
        shuffle_enabled,
    );
    let shuffle_state = Rc::clone(&state);
    let shuffle_toast = Rc::clone(&status_toast);
    let shuffle_popover = popover.clone();
    shuffle_button.connect_clicked(move |_| {
        toggle_shuffle(&shuffle_state, &shuffle_toast);
        shuffle_popover.popdown();
    });
    content.append(&shuffle_button);

    let auto_advance_button = track_button(
        if auto_advance_enabled {
            "Auto-advance On"
        } else {
            "Auto-advance Off"
        },
        auto_advance_enabled,
    );
    let auto_advance_state = Rc::clone(&state);
    let auto_advance_toast = Rc::clone(&status_toast);
    let auto_advance_popover = popover.clone();
    auto_advance_button.connect_clicked(move |_| {
        toggle_auto_advance(&auto_advance_state, &auto_advance_toast);
        auto_advance_popover.popdown();
    });
    content.append(&auto_advance_button);

    content
}

fn track_popover_content(title: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.add_css_class("okp-track-popover-content");
    content.set_width_request(320);

    content.append(&track_section_title(title));
    content
}

fn set_track_popover_child(popover: &gtk::Popover, content: gtk::Box) {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("okp-track-popover-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_width(320);
    scroll.set_max_content_height(520);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&content));
    popover.set_child(Some(&scroll));
}

fn track_section_title(title: &str) -> gtk::Label {
    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-track-popover-title");
    title.set_xalign(0.0);
    title
}

fn subtitle_adjustment_rows(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);

    let (delay_seconds, scale) = read_subtitle_adjustments(state);
    content.append(&subtitle_delay_adjustment_row(
        delay_seconds,
        popover,
        parent,
        state,
    ));
    content.append(&subtitle_adjustment_row(
        "Size",
        &format_scale(scale),
        [
            ("-", SubtitleAdjustment::Scale(-0.1)),
            ("100%", SubtitleAdjustment::SetScale(1.0)),
            ("+", SubtitleAdjustment::Scale(0.1)),
        ],
        popover,
        parent,
        state,
    ));

    content
}

fn subtitle_delay_adjustment_row(
    delay_seconds: f64,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("okp-sub-adjust-row");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some("Delay"));
    label.add_css_class("okp-sub-adjust-label");
    label.set_xalign(0.0);
    label.set_width_chars(6);
    top.append(&label);

    let entry = gtk::Entry::new();
    entry.add_css_class("okp-sub-adjust-entry");
    gtk::prelude::EntryExt::set_alignment(&entry, 1.0);
    entry.set_input_purpose(gtk::InputPurpose::Number);
    entry.set_text(&format_delay_entry(delay_seconds));
    entry.set_width_chars(8);
    entry.set_placeholder_text(Some("0"));
    top.append(&entry);

    let unit = gtk::Label::new(Some("ms"));
    unit.add_css_class("okp-sub-adjust-unit");
    top.append(&unit);

    let apply_button = gtk::Button::with_label("Apply");
    apply_button.add_css_class("okp-sub-adjust-button");
    top.append(&apply_button);

    let reset_button = gtk::Button::with_label("Reset");
    reset_button.add_css_class("okp-sub-adjust-button");
    top.append(&reset_button);

    row.append(&top);

    let quick = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    quick.set_halign(gtk::Align::End);
    for (text, adjustment) in [
        ("-50", SubtitleAdjustment::Delay(-0.05)),
        ("+50", SubtitleAdjustment::Delay(0.05)),
    ] {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_popover = popover.clone();
        let button_parent = parent.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            populate_subtitle_popover(&button_popover, &button_parent, Rc::clone(&button_state));
        });
        quick.append(&button);
    }
    row.append(&quick);

    let apply_state = Rc::clone(state);
    let apply_popover = popover.clone();
    let apply_parent = parent.clone();
    let apply_entry = entry.clone();
    apply_button.connect_clicked(move |_| {
        apply_subtitle_delay_entry(
            &apply_entry,
            &apply_popover,
            &apply_parent,
            Rc::clone(&apply_state),
        );
    });

    let activate_state = Rc::clone(state);
    let activate_popover = popover.clone();
    let activate_parent = parent.clone();
    entry.connect_activate(move |entry| {
        apply_subtitle_delay_entry(
            entry,
            &activate_popover,
            &activate_parent,
            Rc::clone(&activate_state),
        );
    });

    let reset_state = Rc::clone(state);
    let reset_popover = popover.clone();
    let reset_parent = parent.clone();
    reset_button.connect_clicked(move |_| {
        apply_subtitle_adjustment(&reset_state, SubtitleAdjustment::SetDelay(0.0));
        populate_subtitle_popover(&reset_popover, &reset_parent, Rc::clone(&reset_state));
    });

    entry.connect_changed(|entry| {
        entry.remove_css_class("is-error");
    });

    row
}

#[derive(Clone, Copy)]
enum SubtitleAdjustment {
    Delay(f64),
    SetDelay(f64),
    Scale(f64),
    SetScale(f64),
}

fn subtitle_adjustment_row(
    title: &str,
    value: &str,
    actions: [(&str, SubtitleAdjustment); 3],
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("okp-sub-adjust-row");

    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-sub-adjust-label");
    label.set_xalign(0.0);
    label.set_width_chars(6);
    row.append(&label);

    let value_label = gtk::Label::new(Some(value));
    value_label.add_css_class("okp-sub-adjust-value");
    value_label.set_xalign(1.0);
    value_label.set_width_chars(7);
    row.append(&value_label);

    for (text, adjustment) in actions {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_popover = popover.clone();
        let button_parent = parent.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            populate_subtitle_popover(&button_popover, &button_parent, Rc::clone(&button_state));
        });
        row.append(&button);
    }

    row
}

fn read_subtitle_adjustments(state: &Rc<RefCell<PlayerState>>) -> (f64, f64) {
    let values = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| {
            (
                mpv.subtitle_delay().unwrap_or(0.0),
                mpv.subtitle_scale().unwrap_or(1.0),
            )
        })
    };

    values.unwrap_or((0.0, 1.0))
}

fn apply_subtitle_adjustment(state: &Rc<RefCell<PlayerState>>, adjustment: SubtitleAdjustment) {
    if with_mpv(state, |mpv| match adjustment {
        SubtitleAdjustment::Delay(delta) => mpv.adjust_subtitle_delay(delta),
        SubtitleAdjustment::SetDelay(value) => mpv.set_subtitle_delay(value),
        SubtitleAdjustment::Scale(delta) => mpv.adjust_subtitle_scale(delta),
        SubtitleAdjustment::SetScale(value) => mpv.set_subtitle_scale(value),
    }) {
        save_current_preferences(state);
    }
}

fn apply_subtitle_delay_entry(
    entry: &gtk::Entry,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let Some(delay_seconds) = parse_delay_entry_seconds(entry.text().as_str()) else {
        entry.add_css_class("is-error");
        entry.grab_focus();
        return;
    };

    apply_subtitle_adjustment(&state, SubtitleAdjustment::SetDelay(delay_seconds));
    populate_subtitle_popover(popover, parent, state);
}

fn parse_delay_entry_seconds(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let seconds = if let Some(value) = lower.strip_suffix("ms") {
        value.trim().parse::<f64>().ok()? / 1000.0
    } else if let Some(value) = lower.strip_suffix('s') {
        value.trim().parse::<f64>().ok()?
    } else {
        lower.parse::<f64>().ok()? / 1000.0
    };

    seconds.is_finite().then(|| seconds.clamp(-600.0, 600.0))
}

fn format_delay_entry(seconds: f64) -> String {
    ((seconds * 1000.0).round() as i64).to_string()
}

fn format_scale(scale: f64) -> String {
    format!("{:.0}%", scale * 100.0)
}

fn read_tracks(state: &Rc<RefCell<PlayerState>>) -> Vec<Track> {
    let tracks = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::tracks)
    };

    match tracks {
        Some(Ok(tracks)) => tracks,
        Some(Err(error)) => {
            eprintln!("Failed to read tracks: {error}");
            Vec::new()
        }
        None => Vec::new(),
    }
}

fn read_audio_devices(state: &Rc<RefCell<PlayerState>>) -> Vec<AudioDevice> {
    let devices = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::audio_devices)
    };

    match devices {
        Some(Ok(devices)) => devices,
        Some(Err(error)) => {
            eprintln!("Failed to read audio devices: {error}");
            Vec::new()
        }
        None => Vec::new(),
    }
}

fn schedule_audio_device_restore(state: &Rc<RefCell<PlayerState>>) {
    let device = state.borrow().settings.audio_device().trim().to_owned();
    state.borrow_mut().pending_audio_device_restore =
        should_restore_audio_device(&device).then(|| PendingAudioDeviceRestore::new(device));
}

fn try_pending_audio_device_restore(state: &Rc<RefCell<PlayerState>>) {
    let Some(pending) = state.borrow().pending_audio_device_restore.clone() else {
        return;
    };

    let restore_result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.restore_audio_device(&pending.name)
    };

    match restore_result {
        Ok(true) => state.borrow_mut().pending_audio_device_restore = None,
        Ok(false) => record_audio_device_restore_miss(state, pending, None),
        Err(error) => record_audio_device_restore_miss(state, pending, Some(error.to_string())),
    }
}

fn record_audio_device_restore_miss(
    state: &Rc<RefCell<PlayerState>>,
    pending: PendingAudioDeviceRestore,
    error: Option<String>,
) {
    let next = next_audio_device_restore_retry(pending.clone(), AUDIO_DEVICE_RESTORE_MAX_ATTEMPTS);
    if next.is_none() {
        if let Some(error) = error {
            eprintln!(
                "Failed to restore saved audio output '{}': {error}",
                pending.name
            );
        } else {
            eprintln!(
                "Saved audio output '{}' is not available after {AUDIO_DEVICE_RESTORE_MAX_ATTEMPTS} attempts",
                pending.name
            );
        }
    }
    state.borrow_mut().pending_audio_device_restore = next;
}

fn should_restore_audio_device(device: &str) -> bool {
    let device = device.trim();
    !device.is_empty() && device != AUDIO_DEVICE_AUTO
}

fn next_audio_device_restore_retry(
    mut pending: PendingAudioDeviceRestore,
    max_attempts: u8,
) -> Option<PendingAudioDeviceRestore> {
    pending.attempts = pending.attempts.saturating_add(1);
    (pending.attempts < max_attempts).then_some(pending)
}

fn read_secondary_subtitle_id(state: &Rc<RefCell<PlayerState>>) -> Option<i64> {
    let value = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::secondary_subtitle_id)
    };

    match value {
        Some(Ok(value)) => value,
        Some(Err(error)) => {
            eprintln!("Failed to read secondary subtitle track: {error}");
            None
        }
        None => None,
    }
}

fn track_button(text: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-track-row");
    if selected {
        button.add_css_class("is-selected");
    }

    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_ellipsize(pango::EllipsizeMode::End);
    button.set_child(Some(&label));
    button
}

fn empty_track_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-track-empty");
    label.set_xalign(0.0);
    label
}

fn divider() -> gtk::Separator {
    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-track-divider");
    divider
}

fn track_label(track: &Track) -> String {
    track_label_for(track, track.selected)
}

fn track_label_for(track: &Track, selected: bool) -> String {
    let mut parts = Vec::new();
    parts.push(track_base_label(track));

    if track.kind == TrackKind::Audio {
        if let Some(channels) = track.audio_channels.as_deref() {
            parts.push(channels.to_owned());
        }
        if let Some(codec) = track.codec.as_deref() {
            parts.push(codec.to_ascii_uppercase());
        }
    } else if track.external {
        parts.push("EXT".to_owned());
    } else if track.default {
        parts.push("Default".to_owned());
    }

    let label = parts.join(" · ");
    if selected {
        format!("On  {label}")
    } else {
        label
    }
}

fn track_base_label(track: &Track) -> String {
    track
        .title
        .as_deref()
        .or(track.lang.as_deref())
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Track {}", track.id))
}

fn drain_mpv_events(state: &Rc<RefCell<PlayerState>>) {
    let events = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(Mpv::drain_events)
            .unwrap_or_default()
    };

    for event in events {
        match event {
            MpvEvent::FileLoaded => {
                try_pending_audio_device_restore(state);
                try_pending_playback_preferences(state);
            }
            MpvEvent::EndFile { reason } if reason.is_eof() => {
                if state.borrow().modes.repeat_mode != RepeatMode::One {
                    save_current_progress(state, true);
                }
                advance_playlist_on_eof(state);
            }
            _ => {}
        }
    }
}

fn open_media_dialog(parent: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open media"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_select_multiple(true);
    dialog.add_filter(&media_file_filter());
    dialog.add_filter(&playlist_file_filter());
    dialog.add_filter(&subtitle_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            load_selected_local_paths(&state, file_chooser_paths(dialog));
        }
        dialog.close();
    });

    dialog.present();
}

fn open_folder_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open folder"),
        Some(parent),
        gtk::FileChooserAction::SelectFolder,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_select_multiple(true);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && !load_selected_local_paths(&state, file_chooser_paths(dialog))
        {
            status_toast.show("Folder has no playable media");
        }
        dialog.close();
    });

    dialog.present();
}

fn open_url_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::Dialog::builder()
        .title("Open URL")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Open", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_end(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.append(&command_dialog_title("Open URL"));

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://example.com/video.mkv"));
    entry.set_activates_default(true);
    entry.set_width_chars(52);
    content.append(&entry);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            let url = entry.text().trim().to_owned();
            if media_formats::is_playable_url(Some(&url)) {
                load_media_url(&state, url);
            } else {
                status_toast.show("Enter a valid stream URL");
            }
        }
        dialog.close();
    });

    dialog.present();
}

fn open_go_to_time_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let Some((position, duration)) = go_to_time_snapshot(&state) else {
        status_toast.show("Open media first");
        return;
    };

    let dialog = gtk::Dialog::builder()
        .title("Go to Time")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Go", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_end(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.append(&command_dialog_title("Go to Time"));

    let label = gtk::Label::new(Some("Enter a timecode or seconds."));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    content.append(&label);

    let entry = gtk::Entry::new();
    entry.add_css_class("okp-sub-adjust-entry");
    gtk::prelude::EntryExt::set_alignment(&entry, 1.0);
    entry.set_input_purpose(gtk::InputPurpose::Number);
    entry.set_text(&time_code::format(position));
    entry.set_placeholder_text(Some("1:23 or 90"));
    entry.set_activates_default(true);
    entry.set_width_chars(18);
    content.append(&entry);

    let range = if duration.is_finite() && duration > 0.0 {
        format!("Duration {}", time_code::format(duration))
    } else {
        "Duration unknown".to_owned()
    };
    let hint = gtk::Label::new(Some(&range));
    hint.add_css_class("okp-info-label");
    hint.set_xalign(0.0);
    content.append(&hint);

    let focus_entry = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response != gtk::ResponseType::Accept {
            dialog.close();
            return;
        }

        let text = entry.text();
        let Some(mut target) = time_code::parse(Some(text.as_str())) else {
            entry.add_css_class("is-error");
            status_toast.show("Enter a valid timecode");
            return;
        };

        if let Some((_, duration)) = go_to_time_snapshot(&state) {
            if duration.is_finite() && duration > 0.0 {
                target = target.min(duration);
            }
        } else {
            status_toast.show("Open media first");
            dialog.close();
            return;
        }

        if seek_to_time(&state, target) {
            status_toast.show(&format!("Jumped to {}", time_code::format(target)));
            dialog.close();
        } else {
            status_toast.show("Could not seek");
        }
    });

    dialog.present();
    focus_entry.grab_focus();
    focus_entry.select_region(0, -1);
}

fn go_to_time_snapshot(state: &Rc<RefCell<PlayerState>>) -> Option<(f64, f64)> {
    let state = state.borrow();
    if !has_loaded_media_state(&state) {
        return None;
    }

    let playback = state
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.playback_state().ok())?;
    let position = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    Some((position, duration))
}

fn open_clear_history_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::Dialog::builder()
        .title("Clear History")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Clear", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(14);
    content.set_margin_end(14);
    content.set_margin_bottom(14);
    content.set_margin_start(14);
    content.append(&command_dialog_title("Clear History"));

    let message = gtk::Label::new(Some(
        "Clear saved resume positions and per-file playback preferences?",
    ));
    message.set_xalign(0.0);
    message.set_wrap(true);
    content.append(&message);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            clear_history(&state, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

fn command_dialog_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-command-dialog-title");
    label.set_xalign(0.0);
    label
}

fn captionless_transient_window(
    parent: &gtk::ApplicationWindow,
    title: &str,
    default_width: i32,
    default_height: i32,
    resizable: bool,
) -> gtk::Window {
    let window = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .default_width(default_width)
        .default_height(default_height)
        .resizable(resizable)
        .decorated(false)
        .build();
    window.set_decorated(false);
    window
}

fn open_settings_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let initial_page = env::var("OKP_OPEN_SETTINGS_PAGE_ON_STARTUP")
        .ok()
        .and_then(|page| normalized_settings_page(&page))
        .unwrap_or("about");
    let window = captionless_transient_window(
        parent,
        "Settings",
        SETTINGS_REFERENCE_WIDTH,
        SETTINGS_REFERENCE_HEIGHT,
        false,
    );
    window.add_css_class("okp-settings-window");

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("okp-settings-root");

    let stack = gtk::Stack::new();
    stack.add_css_class("okp-settings-stack");
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let about_page = settings_scroller(&settings_about_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&about_page, Some("about"));

    let appearance_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    appearance_page.add_css_class("okp-settings-page");
    appearance_page.append(&settings_appearance_section());
    stack.add_named(&settings_scroller(&appearance_page), Some("appearance"));

    let advanced_page = settings_advanced_page(Rc::clone(&state), Rc::clone(&status_toast));
    stack.add_named(&settings_scroller(&advanced_page), Some("advanced"));

    let playback_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    playback_page.add_css_class("okp-settings-page");
    let playback = settings_section("Playback");
    playback.append(&settings_resume_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_auto_advance_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_repeat_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_shuffle_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_volume_row(Rc::clone(&state)));
    playback_page.append(&playback);
    stack.add_named(&settings_scroller(&playback_page), Some("playback"));

    let subtitles_page =
        settings_subtitles_page(parent, Rc::clone(&state), Rc::clone(&status_toast));
    stack.add_named(&settings_scroller(&subtitles_page), Some("subtitles"));

    let video_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    video_page.add_css_class("okp-settings-page");
    let video = settings_section("Video");
    video.append(&settings_hwdec_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Brightness,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Contrast,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Saturation,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Gamma,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video_page.append(&video);
    stack.add_named(&settings_scroller(&video_page), Some("video"));

    let audio_page = settings_audio_page(Rc::clone(&state), Rc::clone(&status_toast));
    stack.add_named(&settings_scroller(&audio_page), Some("audio"));

    let shortcuts_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    shortcuts_page.add_css_class("okp-settings-page");
    shortcuts_page.append(&settings_shortcuts_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&settings_scroller(&shortcuts_page), Some("shortcuts"));

    let integration_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    integration_page.add_css_class("okp-settings-page");
    integration_page.append(&settings_integration_section(Rc::clone(&status_toast)));

    let privacy = settings_section("Privacy");
    privacy.append(&settings_private_session_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    privacy.append(&settings_clear_history_row(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    integration_page.append(&privacy);

    let storage = settings_section("Storage");
    let settings_path = state
        .borrow()
        .settings
        .path()
        .to_string_lossy()
        .into_owned();
    storage.append(&settings_value_row("Settings file", &settings_path));
    integration_page.append(&storage);
    stack.add_named(&settings_scroller(&integration_page), Some("integration"));

    stack.set_visible_child_name(initial_page);
    root.append(&settings_nav_rail_frame(settings_nav_rail(
        &stack,
        initial_page,
    )));

    stack.set_size_request(SETTINGS_CONTENT_WIDTH, SETTINGS_REFERENCE_HEIGHT);
    root.append(&stack);

    let window_overlay = gtk::Overlay::new();
    window_overlay.set_child(Some(&root));
    window_overlay.add_overlay(&captionless_window_drag_layer(&window));
    window_overlay.add_overlay(&settings_window_controls(&window));
    window.set_child(Some(&window_overlay));
    window.present();
}

fn normalized_settings_page(page: &str) -> Option<&'static str> {
    match page.trim().to_ascii_lowercase().as_str() {
        "appearance" => Some("appearance"),
        "playback" => Some("playback"),
        "subtitles" => Some("subtitles"),
        "video" => Some("video"),
        "audio" => Some("audio"),
        "shortcuts" => Some("shortcuts"),
        "integration" => Some("integration"),
        "advanced" => Some("advanced"),
        "about" => Some("about"),
        _ => None,
    }
}

fn settings_scroller<T: IsA<gtk::Widget>>(child: &T) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-settings-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_min_content_width(SETTINGS_CONTENT_WIDTH);
    scroller.set_max_content_width(SETTINGS_CONTENT_WIDTH);
    scroller.set_propagate_natural_width(false);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(child));
    scroller
}

fn settings_nav_rail_frame(rail: gtk::Box) -> gtk::ScrolledWindow {
    let frame = gtk::ScrolledWindow::new();
    frame.add_css_class("okp-settings-rail-frame");
    frame.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    frame.set_min_content_width(SETTINGS_RAIL_WIDTH);
    frame.set_max_content_width(SETTINGS_RAIL_WIDTH);
    frame.set_propagate_natural_width(false);
    frame.set_size_request(SETTINGS_RAIL_WIDTH, SETTINGS_REFERENCE_HEIGHT);
    frame.set_child(Some(&rail));
    frame
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

fn settings_nav_rail(stack: &gtk::Stack, selected_page: &str) -> gtk::Box {
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 2);
    rail.add_css_class("okp-settings-rail");
    rail.set_size_request(SETTINGS_RAIL_WIDTH, SETTINGS_REFERENCE_HEIGHT);

    let title = gtk::Label::new(Some("Settings"));
    title.add_css_class("okp-settings-rail-title");
    title.set_xalign(0.0);
    rail.append(&title);

    let search = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    search.add_css_class("okp-settings-search");
    search.set_size_request(171, 30);
    let search_icon = gtk::Image::from_icon_name("system-search-symbolic");
    search_icon.set_pixel_size(13);
    search.append(&search_icon);
    let search_label = gtk::Label::new(Some("Search"));
    search_label.add_css_class("okp-settings-search-label");
    search_label.set_xalign(0.0);
    search.append(&search_label);
    rail.append(&search);

    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));
    let nav_items = [
        (
            "Appearance",
            SettingsNavIcon::Appearance,
            Some("appearance"),
        ),
        ("Playback", SettingsNavIcon::Playback, Some("playback")),
        ("Subtitles", SettingsNavIcon::Subtitles, Some("subtitles")),
        ("Video", SettingsNavIcon::Video, Some("video")),
        ("Audio", SettingsNavIcon::Audio, Some("audio")),
        ("Shortcuts", SettingsNavIcon::Shortcuts, Some("shortcuts")),
        (
            "Integration",
            SettingsNavIcon::Integration,
            Some("integration"),
        ),
        ("Advanced", SettingsNavIcon::Advanced, Some("advanced")),
    ];

    for (label, icon, page) in nav_items {
        let row = settings_nav_row(label, icon, page == Some(selected_page));
        if let Some(page) = page {
            connect_settings_nav_row(&row, page, stack, &buttons);
            buttons.borrow_mut().push(row.clone());
        }
        rail.append(&row);
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-settings-rail-divider");
    rail.append(&divider);

    let about = settings_nav_row("About", SettingsNavIcon::About, selected_page == "about");
    connect_settings_nav_row(&about, "about", stack, &buttons);
    buttons.borrow_mut().push(about.clone());
    rail.append(&about);

    rail
}

fn settings_window_controls(window: &gtk::Window) -> gtk::Box {
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-settings-window-controls");
    controls.set_halign(gtk::Align::End);
    controls.set_valign(gtk::Align::Start);

    let minimize = settings_window_control(WindowControlKind::Minimize, "Minimize");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    controls.append(&minimize);

    let maximize = settings_window_control(WindowControlKind::Maximize, "Maximize");
    sync_settings_maximize_icon(&maximize, window);
    let maximize_window = window.clone();
    let maximize_button = maximize.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
        sync_settings_maximize_icon(&maximize_button, &maximize_window);
    });
    let notify_button = maximize.clone();
    window.connect_maximized_notify(move |window| {
        sync_settings_maximize_icon(&notify_button, window);
    });
    controls.append(&maximize);

    let close = settings_window_control(WindowControlKind::Close, "Close");
    close.add_css_class("okp-settings-window-close");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    controls.append(&close);

    controls
}

fn captionless_window_drag_layer(window: &gtk::Window) -> gtk::Box {
    let drag_layer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_layer.add_css_class("okp-captionless-window-drag-layer");
    drag_layer.set_halign(gtk::Align::Fill);
    drag_layer.set_valign(gtk::Align::Start);
    drag_layer.set_can_target(true);
    drag_layer.set_height_request(CAPTIONLESS_DRAG_HEIGHT);
    connect_captionless_window_drag(&drag_layer, window);
    drag_layer
}

fn connect_captionless_window_drag(widget: &impl IsA<gtk::Widget>, window: &gtk::Window) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let drag_window = window.clone();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if n_press == 2 {
            if drag_window.is_maximized() {
                drag_window.unmaximize();
            } else {
                drag_window.maximize();
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(surface) = drag_window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };

        toplevel.begin_move(
            &device,
            gesture.current_button() as i32,
            x,
            y,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

#[derive(Clone, Copy)]
enum WindowControlKind {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn settings_window_control(kind: WindowControlKind, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-window-control");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));

    let glyph = window_control_icon(kind, "okp-settings-window-control-glyph");
    button.set_child(Some(&glyph));
    button
}

fn sync_settings_maximize_icon(button: &gtk::Button, window: &gtk::Window) {
    if window.is_maximized() {
        set_settings_window_control_kind(button, WindowControlKind::Restore);
        button.set_tooltip_text(Some("Restore"));
    } else {
        set_settings_window_control_kind(button, WindowControlKind::Maximize);
        button.set_tooltip_text(Some("Maximize"));
    }
}

fn window_control_icon(kind: WindowControlKind, css_class: &str) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.add_css_class(css_class);
    icon.set_size_request(10, 10);
    icon.set_draw_func(move |area, cr, width, height| {
        draw_window_control_icon(area, cr, width, height, kind);
    });
    icon
}

fn set_settings_window_control_kind(button: &gtk::Button, kind: WindowControlKind) {
    if let Some(icon) = button.child().and_downcast::<gtk::DrawingArea>() {
        icon.set_draw_func(move |area, cr, width, height| {
            draw_window_control_icon(area, cr, width, height, kind);
        });
        icon.queue_draw();
    }
}

fn draw_window_control_icon(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    kind: WindowControlKind,
) {
    let color = area.style_context().color();
    let _ = cr.save();
    cr.translate(
        ((width as f64) - 10.0) / 2.0,
        ((height as f64) - 10.0) / 2.0,
    );
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.set_line_width(1.0);
    cr.set_line_cap(cairo::LineCap::Square);

    match kind {
        WindowControlKind::Minimize => {
            cr.move_to(1.0, 5.0);
            cr.line_to(9.0, 5.0);
            let _ = cr.stroke();
        }
        WindowControlKind::Maximize => {
            cr.rectangle(1.5, 1.5, 7.0, 7.0);
            let _ = cr.stroke();
        }
        WindowControlKind::Restore => {
            cr.rectangle(2.7, 1.5, 5.8, 5.8);
            let _ = cr.stroke();
            cr.move_to(1.5, 2.8);
            cr.line_to(1.5, 8.5);
            cr.line_to(7.2, 8.5);
            let _ = cr.stroke();
        }
        WindowControlKind::Close => {
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(2.0, 2.0);
            cr.line_to(8.0, 8.0);
            cr.move_to(8.0, 2.0);
            cr.line_to(2.0, 8.0);
            let _ = cr.stroke();
        }
    }

    let _ = cr.restore();
}

fn settings_nav_row(label: &str, icon: SettingsNavIcon, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-nav-row");
    button.set_has_frame(false);
    button.set_size_request(171, 36);
    if selected {
        button.add_css_class("is-selected");
    }

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_halign(gtk::Align::Fill);
    content.append(&settings_nav_icon(icon));
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    content.append(&text);
    button.set_child(Some(&content));
    button
}

fn settings_nav_icon(icon: SettingsNavIcon) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class("okp-settings-nav-icon");
    area.set_size_request(16, 16);
    area.set_draw_func(move |area, cr, width, height| {
        draw_settings_nav_icon(area, cr, width, height, icon);
    });
    area
}

fn draw_settings_nav_icon(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    icon: SettingsNavIcon,
) {
    let color = area.style_context().color();
    let scale = f64::min(width as f64, height as f64) / 16.0;
    let _ = cr.save();
    cr.translate(
        ((width as f64) - (16.0 * scale)) / 2.0,
        ((height as f64) - (16.0 * scale)) / 2.0,
    );
    cr.scale(scale, scale);
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.set_line_width(1.25);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    match icon {
        SettingsNavIcon::Appearance => {
            cr.arc(8.0, 8.0, 3.0, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            for index in 0..8 {
                let angle = (index as f64) * std::f64::consts::FRAC_PI_4;
                cr.move_to(8.0 + angle.cos() * 5.2, 8.0 + angle.sin() * 5.2);
                cr.line_to(8.0 + angle.cos() * 6.7, 8.0 + angle.sin() * 6.7);
            }
            let _ = cr.stroke();
        }
        SettingsNavIcon::Playback => {
            cr.move_to(5.25, 3.35);
            cr.line_to(12.3, 8.0);
            cr.line_to(5.25, 12.65);
            cr.close_path();
            let _ = cr.stroke();
        }
        SettingsNavIcon::Subtitles => {
            cairo_rounded_rect(cr, 2.5, 4.0, 11.0, 8.0, 1.2);
            let _ = cr.stroke();
            cr.move_to(5.0, 8.8);
            cr.line_to(7.3, 8.8);
            cr.move_to(8.7, 8.8);
            cr.line_to(11.0, 8.8);
            cr.move_to(5.0, 10.7);
            cr.line_to(10.2, 10.7);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Video => {
            cairo_rounded_rect(cr, 2.5, 3.5, 11.0, 8.2, 1.1);
            let _ = cr.stroke();
            cr.move_to(8.0, 11.7);
            cr.line_to(8.0, 13.2);
            cr.move_to(5.7, 13.2);
            cr.line_to(10.3, 13.2);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Audio => {
            cr.move_to(2.6, 6.2);
            cr.line_to(5.0, 6.2);
            cr.line_to(8.6, 3.7);
            cr.line_to(8.6, 12.3);
            cr.line_to(5.0, 9.8);
            cr.line_to(2.6, 9.8);
            cr.close_path();
            let _ = cr.stroke();
            cr.arc(8.7, 8.0, 3.3, -0.72, 0.72);
            let _ = cr.stroke();
            cr.arc(8.7, 8.0, 5.1, -0.62, 0.62);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Shortcuts => {
            cairo_rounded_rect(cr, 2.2, 4.1, 11.6, 7.8, 1.1);
            let _ = cr.stroke();
            for y in [6.7, 9.2] {
                for x in [4.5, 6.8, 9.1, 11.4] {
                    cairo_rounded_rect(cr, x - 0.45, y - 0.35, 0.9, 0.7, 0.2);
                    let _ = cr.fill();
                }
            }
        }
        SettingsNavIcon::Integration => {
            let _ = cr.save();
            cr.translate(8.0, 8.0);
            cr.rotate(-std::f64::consts::FRAC_PI_4);
            cairo_rounded_rect(cr, -6.0, -2.2, 7.3, 4.4, 2.2);
            let _ = cr.stroke();
            cairo_rounded_rect(cr, -1.3, -2.2, 7.3, 4.4, 2.2);
            let _ = cr.stroke();
            let _ = cr.restore();
        }
        SettingsNavIcon::Advanced => {
            cr.move_to(6.6, 2.6);
            cr.curve_to(4.6, 2.6, 5.2, 5.2, 4.0, 6.2);
            cr.curve_to(3.4, 6.8, 3.4, 7.2, 4.0, 7.8);
            cr.curve_to(5.2, 8.8, 4.6, 13.4, 6.6, 13.4);
            cr.move_to(9.4, 2.6);
            cr.curve_to(11.4, 2.6, 10.8, 5.2, 12.0, 6.2);
            cr.curve_to(12.6, 6.8, 12.6, 7.2, 12.0, 7.8);
            cr.curve_to(10.8, 8.8, 11.4, 13.4, 9.4, 13.4);
            let _ = cr.stroke();
        }
        SettingsNavIcon::About => {
            cr.arc(8.0, 8.0, 5.8, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.arc(8.0, 5.2, 0.55, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.move_to(8.0, 7.4);
            cr.line_to(8.0, 11.0);
            let _ = cr.stroke();
        }
    }

    let _ = cr.restore();
}

fn cairo_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let right = x + w;
    let bottom = y + h;
    cr.new_sub_path();
    cr.arc(right - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(right - r, bottom - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        bottom - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

fn connect_settings_nav_row(
    button: &gtk::Button,
    page: &str,
    stack: &gtk::Stack,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let page = page.to_owned();
    let stack = stack.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |button| {
        stack.set_visible_child_name(&page);
        for row in buttons.borrow().iter() {
            row.remove_css_class("is-selected");
        }
        button.add_css_class("is-selected");
    });
}

fn settings_section(title: &str) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
    section.add_css_class("okp-info-section");

    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    section.append(&title);
    section
}

fn settings_about_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let snapshot = AboutSnapshot::capture(&state);
    let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pane.add_css_class("okp-about-pane");

    pane.append(&about_identity_hero(&snapshot));

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-about-identity-divider");
    pane.append(&divider);

    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 11);
    sheet.add_css_class("okp-about-sheet");
    sheet.append(&about_app_card(&snapshot));
    sheet.append(&about_updates_card(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    sheet.append(&about_engine_card(&snapshot));
    sheet.append(&about_host_card(&snapshot));
    pane.append(&sheet);

    pane.append(&about_footer(snapshot, status_toast));
    pane
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

fn about_identity_hero(snapshot: &AboutSnapshot) -> gtk::Box {
    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    hero.add_css_class("okp-about-identity");

    let illustration = gtk::Box::new(gtk::Orientation::Vertical, 0);
    illustration.add_css_class("okp-about-illustration");
    illustration.set_halign(gtk::Align::Center);
    illustration.set_valign(gtk::Align::Center);
    illustration.append(&about_illustration());
    hero.append(&illustration);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);

    let wordmark = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    wordmark.add_css_class("okp-about-wordmark");
    let ok = gtk::Label::new(Some("OK"));
    ok.add_css_class("okp-about-wordmark-ok");
    let player = gtk::Label::new(Some(" Player"));
    player.add_css_class("okp-about-wordmark-player");
    wordmark.append(&ok);
    wordmark.append(&player);
    text.append(&wordmark);

    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    chips.add_css_class("okp-about-chip-row");
    let version = gtk::Label::new(Some(&snapshot.version));
    version.add_css_class("okp-about-version-chip");
    chips.append(&version);
    let channel = gtk::Label::new(Some(&about_hero_channel(&snapshot.channel)));
    channel.add_css_class("okp-about-channel-chip");
    chips.append(&channel);
    text.append(&chips);

    let tagline = gtk::Label::new(Some("The most elegant media player on Linux."));
    tagline.add_css_class("okp-about-tagline");
    tagline.set_xalign(0.0);
    text.append(&tagline);

    let byline = gtk::Label::new(Some("Open source · by Oleg Kossoy"));
    byline.add_css_class("okp-about-byline");
    byline.set_xalign(0.0);
    text.append(&byline);

    hero.append(&text);
    hero
}

fn about_illustration() -> gtk::Widget {
    if let Some(path) = about_illustration_path() {
        let image = gtk::Image::from_file(path);
        image.set_size_request(116, 90);
        image.set_pixel_size(116);
        return image.upcast();
    }

    let image = gtk::Image::from_icon_name("com.befeast.okplayer");
    image.set_size_request(116, 90);
    image.set_pixel_size(90);
    image.upcast()
}

fn about_illustration_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(
        "/usr/share/ok-player/com.befeast.okplayer.about.svg",
    ));
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("com.befeast.okplayer.about.svg"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/linux/com.befeast.okplayer.about.svg"),
    );

    candidates.into_iter().find(|path| path.is_file())
}

fn about_display_version(version: &str) -> String {
    version
        .split_once("-linux-")
        .map(|(base, _)| base)
        .unwrap_or(version)
        .to_owned()
}

fn about_display_channel(version: &str) -> String {
    if version.contains("-linux-alpha") {
        "Linux alpha"
    } else if version.contains("-linux-beta") {
        "Linux beta"
    } else {
        "Linux"
    }
    .to_owned()
}

fn about_hero_channel(channel: &str) -> String {
    channel
        .split_whitespace()
        .last()
        .unwrap_or(channel)
        .to_uppercase()
}

fn about_app_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 9);
    rows.append(&about_spec_row("Version", &snapshot.version, true, None));
    rows.append(&about_spec_row("Channel", &snapshot.channel, false, None));
    rows.append(&about_spec_row("Build", &snapshot.build, true, None));
    rows.append(&about_spec_row("License", &snapshot.license, true, None));
    about_card("APP", &rows)
}

fn about_engine_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 9);
    let hwdec_tag = if snapshot.hwdec == "off" {
        ("OFF", false)
    } else {
        ("ON", true)
    };
    rows.append(&about_spec_row("libmpv", &snapshot.libmpv, true, None));
    rows.append(&about_spec_row(
        "FFmpeg",
        &snapshot.ffmpeg,
        true,
        Some(("SYSTEM", false)),
    ));
    rows.append(&about_spec_row(
        "Render API",
        &snapshot.render_api,
        true,
        None,
    ));
    rows.append(&about_spec_row("Graphics", &snapshot.graphics, true, None));
    rows.append(&about_spec_row(
        "Hardware decode",
        &snapshot.hwdec,
        false,
        Some(hwdec_tag),
    ));
    about_card("ENGINE", &rows)
}

fn about_updates_card(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let initial_update_status = state.borrow().linux_update_status.clone();

    let status_row = about_spec_row(
        "Status",
        &initial_update_status.about_status_text(),
        false,
        None,
    );
    let status_label = status_row
        .last_child()
        .and_then(|wrap| wrap.first_child())
        .and_then(|widget| widget.downcast::<gtk::Label>().ok())
        .unwrap_or_else(|| gtk::Label::new(Some("Not checked")));
    content.append(&status_row);

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    auto_row.add_css_class("okp-about-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Check automatically"));
    auto_label.add_css_class("okp-about-row-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some("On launch"));
    auto_detail.add_css_class("okp-about-row-detail");
    auto_detail.set_xalign(0.0);
    auto_text.append(&auto_detail);
    auto_row.append(&auto_text);

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let auto_switch = about_toggle_button(auto_check_enabled);
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    auto_switch.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        if enabled {
            button.add_css_class("is-active");
        } else {
            button.remove_css_class("is-active");
        }
        if let Some(knob) = button.first_child() {
            knob.set_halign(if enabled {
                gtk::Align::End
            } else {
                gtk::Align::Start
            });
        }
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_toast.show(if enabled {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        });
    });
    auto_row.append(&auto_switch);
    content.append(&auto_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    actions.set_halign(gtk::Align::Start);
    let pending_update = Rc::new(RefCell::new(initial_update_status.pending_update()));
    let check_button = gtk::Button::with_label(&initial_update_status.action_label());
    check_button.add_css_class("okp-about-check-button");
    check_button.set_has_frame(false);
    check_button.set_size_request(132, 34);
    check_button.set_sensitive(!matches!(
        initial_update_status,
        LinuxUpdateStatus::Checking
    ));
    let check_status = status_label.clone();
    let check_pending = Rc::clone(&pending_update);
    let check_state = Rc::clone(&state);
    let check_toast = Rc::clone(&status_toast);
    check_button.connect_clicked(move |button| {
        if let Some(update) = check_pending.borrow().clone() {
            start_update_download(
                button,
                &check_status,
                update,
                Rc::clone(&check_state),
                Rc::clone(&check_toast),
            );
            return;
        }

        start_update_check_for_ui(
            button,
            &check_status,
            &check_pending,
            Rc::clone(&check_state),
            Rc::clone(&check_toast),
            "Checking...",
            true,
        );
    });
    actions.append(&check_button);
    if auto_check_enabled && matches!(initial_update_status, LinuxUpdateStatus::NotChecked) {
        let auto_button = check_button.clone();
        let auto_status = status_label.clone();
        let auto_pending = Rc::clone(&pending_update);
        let auto_state = Rc::clone(&state);
        let auto_toast = Rc::clone(&status_toast);
        glib::idle_add_local_once(move || {
            start_update_check_for_ui(
                &auto_button,
                &auto_status,
                &auto_pending,
                auto_state,
                auto_toast,
                "Checking...",
                false,
            );
        });
    }
    content.append(&actions);

    about_card("UPDATES", &content)
}

fn about_host_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let grid = gtk::Grid::new();
    grid.add_css_class("okp-about-host-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(26);
    grid.set_row_spacing(8);
    grid.attach(
        &about_spec_row("Linux", &snapshot.os, true, None),
        0,
        0,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("GTK", &snapshot.gtk, true, None),
        1,
        0,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("CPU", &snapshot.cpu, true, None),
        0,
        1,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("Install", &snapshot.install, false, None),
        1,
        1,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("Updates", &snapshot.updates, false, Some(("ON", true))),
        0,
        2,
        1,
        1,
    );
    about_card("HOST", &grid)
}

fn about_toggle_button(active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-toggle");
    button.set_has_frame(false);
    button.set_size_request(39, 22);
    button.set_halign(gtk::Align::End);
    button.set_valign(gtk::Align::Center);

    let knob = gtk::Box::new(gtk::Orientation::Vertical, 0);
    knob.add_css_class("okp-about-toggle-knob");
    knob.set_valign(gtk::Align::Center);
    button.set_child(Some(&knob));
    set_about_toggle_active(&button, active);
    button
}

fn set_about_toggle_active(button: &gtk::Button, active: bool) {
    if active {
        button.add_css_class("is-active");
    } else {
        button.remove_css_class("is-active");
    }
    if let Some(knob) = button.first_child() {
        knob.set_halign(if active {
            gtk::Align::End
        } else {
            gtk::Align::Start
        });
    }
}

fn about_card<T: IsA<gtk::Widget>>(title: &str, content: &T) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("okp-about-card");
    match title {
        "APP" => {
            card.add_css_class("okp-about-card-app");
            card.set_size_request(-1, 151);
        }
        "UPDATES" => {
            card.add_css_class("okp-about-card-updates");
            card.set_size_request(-1, 164);
        }
        "ENGINE" => {
            card.add_css_class("okp-about-card-engine");
            card.set_size_request(-1, 176);
        }
        "HOST" => {
            card.add_css_class("okp-about-card-host");
            card.set_size_request(-1, 125);
        }
        _ => {}
    }

    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-about-card-title");
    label.set_xalign(0.0);
    card.append(&label);
    card.append(content);
    card
}

fn about_spec_row(label: &str, value: &str, mono: bool, tag: Option<(&str, bool)>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("okp-about-row");
    row.set_hexpand(true);

    let key = gtk::Label::new(Some(label));
    key.add_css_class("okp-about-row-label");
    key.set_xalign(0.0);
    key.set_hexpand(true);
    row.append(&key);

    let value_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    value_wrap.set_halign(gtk::Align::End);

    let val = gtk::Label::new(Some(value));
    val.add_css_class(if mono {
        "okp-about-row-value-mono"
    } else {
        "okp-about-row-value"
    });
    val.set_xalign(1.0);
    val.set_width_chars(1);
    val.set_max_width_chars(34);
    val.set_ellipsize(pango::EllipsizeMode::End);
    val.set_selectable(true);
    value_wrap.append(&val);

    if let Some((text, accent)) = tag {
        let tag = gtk::Label::new(Some(text));
        tag.add_css_class("okp-about-tag");
        if accent {
            tag.add_css_class("is-accent");
        }
        value_wrap.append(&tag);
    }

    row.append(&value_wrap);
    row
}

fn about_footer(snapshot: AboutSnapshot, status_toast: Rc<StatusToast>) -> gtk::Box {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("okp-about-footer");

    let copy = about_action_button("Copy diagnostics", "edit-copy-symbolic");
    let copy_snapshot = snapshot.clone();
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display
                .clipboard()
                .set_text(&about_diagnostics_text(&copy_snapshot));
        }
        status_toast.show("Diagnostics copied");
    });
    footer.append(&copy);

    let links = gtk::Box::new(gtk::Orientation::Horizontal, 13);
    links.set_halign(gtk::Align::End);
    links.set_hexpand(true);

    let github = about_link_button("GitHub");
    github.connect_clicked(|_| open_external_url("https://github.com/BeFeast/ok-player"));
    links.append(&github);

    let dot = gtk::Label::new(Some("•"));
    dot.add_css_class("okp-about-link-dot");
    dot.set_valign(gtk::Align::Center);
    links.append(&dot);

    let license = about_link_button("License");
    license.connect_clicked(|_| {
        open_external_url("https://github.com/BeFeast/ok-player/blob/main/LICENSE")
    });
    links.append(&license);
    footer.append(&links);

    footer
}

fn about_action_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-copy-button");
    button.set_has_frame(false);
    button.set_size_request(147, 34);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

fn about_link_button(label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-link-button");
    button.set_has_frame(false);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    content.append(&gtk::Label::new(Some(label)));
    let icon = gtk::Label::new(Some("↗"));
    icon.add_css_class("okp-about-link-arrow");
    content.append(&icon);
    button.set_child(Some(&content));
    button
}

fn about_diagnostics_text(snapshot: &AboutSnapshot) -> String {
    format!(
        "OK Player {} ({})\nBuild {} - current\nLicense {}\n\nEngine\n  libmpv           {}\n  FFmpeg           {}\n  Render API       {}\n  Graphics         {}\n  Hardware decode  {}\n\nHost\n  Linux            {}\n  GTK              {}\n  CPU              {}\n  Install          {}\n  Updates          {}",
        snapshot.package_version,
        snapshot.channel,
        snapshot.build,
        snapshot.license,
        snapshot.libmpv,
        snapshot.ffmpeg,
        snapshot.render_api,
        snapshot.graphics,
        snapshot.hwdec,
        snapshot.os,
        snapshot.gtk,
        snapshot.cpu,
        snapshot.install,
        snapshot.updates
    )
}

fn pkg_config_version(package: &str) -> Option<String> {
    Command::new("pkg-config")
        .args(["--modversion", package])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn ffmpeg_version() -> Option<String> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| {
            output
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(2))
                .map(str::to_owned)
        })
}

fn linux_os_label() -> String {
    if let Ok(os_release) = fs::read_to_string("/etc/os-release")
        && let Some(pretty_name) = os_release.lines().find_map(|line| {
            line.strip_prefix("PRETTY_NAME=")
                .map(|value| value.trim_matches('"').to_owned())
        })
        && !pretty_name.is_empty()
    {
        return pretty_name;
    }

    Command::new("uname")
        .arg("-sr")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Linux".to_owned())
}

fn linux_update_install_status() -> &'static str {
    if linux_update_manager().is_ok() {
        "Self-update enabled"
    } else if deb_self_install_available() {
        "Deb self-install"
    } else {
        "Deb installer"
    }
}

fn settings_advanced_page(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");
    page.append(&settings_raw_mpv_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    page.append(&settings_updates_section(state, status_toast));
    page
}

fn settings_raw_mpv_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("mpv.conf");

    let detail = gtk::Label::new(Some(
        "Raw mpv key=value options. Startup-only options apply when playback starts.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(58);
    detail.set_wrap(true);
    section.append(&detail);

    let editor = gtk::TextView::new();
    editor.add_css_class("okp-mpv-conf-editor");
    editor.set_monospace(true);
    editor.set_wrap_mode(gtk::WrapMode::None);
    editor.set_accepts_tab(true);
    editor
        .buffer()
        .set_text(state.borrow().settings.raw_mpv_config());

    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-mpv-conf-scroller");
    scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroller.set_min_content_height(132);
    scroller.set_child(Some(&editor));
    section.append(&scroller);

    let status = gtk::Label::new(Some(
        "Managed by OK Player: config, terminal, idle, force-window, vo.",
    ));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);
    section.append(&status);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("okp-settings-button");
    let reset_buffer = editor.buffer();
    let reset_state = Rc::clone(&state);
    let reset_toast = Rc::clone(&status_toast);
    let reset_status = status.clone();
    reset.connect_clicked(move |_| {
        reset_buffer.set_text("");
        apply_raw_mpv_config_setting("", &reset_status, &reset_state, &reset_toast);
    });
    actions.append(&reset);

    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("okp-settings-button");
    let apply_buffer = editor.buffer();
    let apply_state = Rc::clone(&state);
    let apply_toast = Rc::clone(&status_toast);
    let apply_status = status.clone();
    apply.connect_clicked(move |_| {
        let text = text_buffer_string(&apply_buffer);
        apply_raw_mpv_config_setting(&text, &apply_status, &apply_state, &apply_toast);
    });
    actions.append(&apply);

    section.append(&actions);
    section
}

fn apply_raw_mpv_config_setting(
    text: &str,
    status: &gtk::Label,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
) {
    let options = match parse_raw_mpv_config(text) {
        Ok(options) => options,
        Err(error) => {
            status.set_text(&format!("Line {}: {}", error.line, error.message));
            status_toast.show("mpv.conf has an error");
            return;
        }
    };

    let live_result = {
        let state = state.borrow();
        (!options.is_empty())
            .then(|| state.mpv.as_ref().map(|mpv| mpv.apply_options(&options)))
            .flatten()
    };

    let save_result = {
        let mut state = state.borrow_mut();
        state.settings.set_raw_mpv_config(text);
        state.settings.save()
    };
    if let Err(error) = save_result {
        eprintln!("Failed to save custom mpv.conf setting: {error}");
        status.set_text("Could not save mpv.conf.");
        status_toast.show("Could not save mpv.conf");
        return;
    }

    match live_result {
        Some(Ok(())) => {
            status.set_text("Saved and applied to the current mpv session.");
            status_toast.show("mpv.conf applied");
        }
        Some(Err(error)) => {
            eprintln!("Failed to hot-apply custom mpv.conf options: {error}");
            status.set_text("Saved. Live apply failed; restart playback to retry.");
            status_toast.show("Saved. Restart playback to retry");
        }
        None if options.is_empty() => {
            status.set_text("Reset saved. Restart playback to clear hot-applied options.");
            status_toast.show("mpv.conf reset");
        }
        None => {
            status.set_text("Saved. It applies when playback starts.");
            status_toast.show("mpv.conf saved");
        }
    }
}

fn text_buffer_string(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

fn settings_updates_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Updates");
    section.append(&settings_value_row("Current version", APP_BUILD_VERSION));
    section.append(&settings_value_row("Channel", "linux"));
    section.append(&settings_value_row("Feed", "GitHub Releases"));
    section.append(&settings_value_row(
        "Install",
        linux_update_install_status(),
    ));

    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let initial_update_status = state.borrow().linux_update_status.clone();
    let status = gtk::Label::new(Some(
        &initial_update_status.settings_status_text(auto_check_enabled),
    ));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);
    row.append(&status);

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    auto_row.add_css_class("okp-settings-switch-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Automatic checks"));
    auto_label.add_css_class("okp-info-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some(
        "Check the linux pre-release feed on startup and show a toast when an update is ready.",
    ));
    auto_detail.add_css_class("okp-update-status");
    auto_detail.set_xalign(0.0);
    auto_detail.set_width_chars(1);
    auto_detail.set_max_width_chars(50);
    auto_detail.set_wrap(true);
    auto_text.append(&auto_detail);
    auto_row.append(&auto_text);

    let auto_state_label = gtk::Label::new(Some(if auto_check_enabled { "On" } else { "Off" }));
    auto_state_label.add_css_class("okp-settings-state-pill");
    auto_state_label.set_valign(gtk::Align::Center);
    auto_row.append(&auto_state_label);

    let auto_switch = about_toggle_button(auto_check_enabled);
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    let auto_status = status.clone();
    let auto_state_text = auto_state_label.clone();
    auto_switch.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_status.set_text(update_status_intro(enabled));
        auto_state_text.set_text(if enabled { "On" } else { "Off" });
        auto_toast.show(if enabled {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        });
    });
    auto_row.append(&auto_switch);
    row.append(&auto_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);

    let pending_update = Rc::new(RefCell::new(initial_update_status.pending_update()));

    let check_button = gtk::Button::with_label(&initial_update_status.action_label());
    check_button.add_css_class("okp-settings-button");
    check_button.set_sensitive(!matches!(
        initial_update_status,
        LinuxUpdateStatus::Checking
    ));
    let check_status = status.clone();
    let check_pending = Rc::clone(&pending_update);
    let check_state = Rc::clone(&state);
    let check_toast = Rc::clone(&status_toast);
    check_button.connect_clicked(move |button| {
        if let Some(update) = check_pending.borrow().clone() {
            start_update_download(
                button,
                &check_status,
                update,
                Rc::clone(&check_state),
                Rc::clone(&check_toast),
            );
            return;
        }

        start_update_check_for_ui(
            button,
            &check_status,
            &check_pending,
            Rc::clone(&check_state),
            Rc::clone(&check_toast),
            "Checking GitHub Releases...",
            true,
        );
    });
    actions.append(&check_button);
    if auto_check_enabled && matches!(initial_update_status, LinuxUpdateStatus::NotChecked) {
        let auto_button = check_button.clone();
        let auto_status = status.clone();
        let auto_pending = Rc::clone(&pending_update);
        let auto_state = Rc::clone(&state);
        let auto_toast = Rc::clone(&status_toast);
        glib::idle_add_local_once(move || {
            start_update_check_for_ui(
                &auto_button,
                &auto_status,
                &auto_pending,
                auto_state,
                auto_toast,
                "Checking GitHub Releases...",
                false,
            );
        });
    }

    let releases_button = gtk::Button::with_label("Open Releases");
    releases_button.add_css_class("okp-settings-button");
    releases_button.connect_clicked(move |_| {
        open_external_url("https://github.com/BeFeast/ok-player/releases")
    });
    actions.append(&releases_button);

    row.append(&actions);
    section.append(&row);

    section
}

fn update_status_intro(auto_check_enabled: bool) -> &'static str {
    if auto_check_enabled {
        "Automatic update checks are on. AppImage installs restart in place; .deb installs request admin approval and fall back to opening the installer."
    } else {
        "Automatic update checks are off. Use Check for updates any time."
    }
}

fn start_update_check_for_ui(
    button: &gtk::Button,
    status: &gtk::Label,
    pending: &Rc<RefCell<Option<PendingLinuxUpdate>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    checking_status: &str,
    show_toast: bool,
) {
    button.set_sensitive(false);
    button.set_label("Checking...");
    status.set_text(checking_status);
    pending.borrow_mut().take();
    state.borrow_mut().linux_update_status = LinuxUpdateStatus::Checking;

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check_for_linux_update());
    });

    let button = button.clone();
    let status = status.clone();
    let pending = Rc::clone(pending);
    glib::timeout_add_local(Duration::from_millis(120), move || {
        match receiver.try_recv() {
            Ok(result) => {
                apply_update_check_result(
                    &button,
                    &status,
                    &pending,
                    Rc::clone(&state),
                    &status_toast,
                    show_toast,
                    result,
                );
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Update check failed");
                state.borrow_mut().linux_update_status =
                    LinuxUpdateStatus::Failed("update check channel closed".to_owned());
                glib::ControlFlow::Break
            }
        }
    });
}

fn start_update_download(
    button: &gtk::Button,
    status: &gtk::Label,
    update: PendingLinuxUpdate,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    save_current_progress(&state, false);
    button.set_sensitive(false);
    button.set_label("Downloading...");
    status.set_text(&format!(
        "Downloading {}...",
        update
            .target_version()
            .unwrap_or_else(|| "update".to_owned())
    ));
    status_toast.show("Downloading update");

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(download_and_apply_linux_update(update));
    });

    let button = button.clone();
    let status = status.clone();
    let toast = Rc::clone(&status_toast);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        match receiver.try_recv() {
            Ok(Ok(LinuxUpdateApplyResult::Restarting)) => {
                button.set_label("Restarting...");
                status.set_text("Restarting to apply update...");
                glib::ControlFlow::Break
            }
            Ok(Ok(LinuxUpdateApplyResult::DebInstalled(_path))) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Installed. Restart OK Player to finish.");
                toast.show("Update installed");
                glib::ControlFlow::Break
            }
            Ok(Ok(LinuxUpdateApplyResult::InstallerOpened(_path))) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Installer opened. Complete it to update.");
                toast.show("Installer opened");
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text(&format!("Update failed: {error}"));
                toast.show("Update failed");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Update failed.");
                glib::ControlFlow::Break
            }
        }
    });
}

fn apply_update_check_result(
    button: &gtk::Button,
    status: &gtk::Label,
    pending: &Rc<RefCell<Option<PendingLinuxUpdate>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    show_toast: bool,
    result: LinuxUpdateCheckResult,
) {
    state.borrow_mut().linux_update_status = LinuxUpdateStatus::from_check_result(&result);
    button.set_sensitive(true);
    match result {
        LinuxUpdateCheckResult::UpToDate => {
            pending.borrow_mut().take();
            button.set_label("Check for updates");
            status.set_text("Up to date");
            if show_toast {
                status_toast.show("OK Player is up to date");
            }
        }
        LinuxUpdateCheckResult::Available(update) => {
            let status_text = update.available_status();
            let action_label = update.action_label();
            pending.borrow_mut().replace(update);
            button.set_label(action_label);
            status.set_text(&status_text);
            if show_toast {
                status_toast.show("Update available");
            }
        }
        LinuxUpdateCheckResult::Failed(error) => {
            pending.borrow_mut().take();
            button.set_label("Check for updates");
            status.set_text(&format!("Update check failed: {error}"));
            if show_toast {
                status_toast.show("Update check failed");
            }
        }
    }
}

fn check_updates_on_startup(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check_for_linux_update());
    });

    glib::timeout_add_local(Duration::from_millis(500), move || {
        match receiver.try_recv() {
            Ok(result) => {
                state.borrow_mut().linux_update_status =
                    LinuxUpdateStatus::from_check_result(&result);
                match result {
                    LinuxUpdateCheckResult::Available(update) => {
                        let version = update
                            .target_version()
                            .unwrap_or_else(|| "new version".to_owned());
                        status_toast.show(&format!("Update available: {version}"));
                    }
                    LinuxUpdateCheckResult::Failed(error) => {
                        eprintln!("Startup update check failed: {error}");
                    }
                    LinuxUpdateCheckResult::UpToDate => {}
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn check_for_linux_update() -> LinuxUpdateCheckResult {
    let manager = match linux_update_manager() {
        Ok(manager) => manager,
        Err(manager_error) => {
            return match check_for_linux_deb_update() {
                Ok(Some(update)) => LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                    manager: None,
                    target: LinuxUpdateTarget::Deb(update),
                }),
                Ok(None) => LinuxUpdateCheckResult::UpToDate,
                Err(deb_error) => {
                    LinuxUpdateCheckResult::Failed(format!("{manager_error}; {deb_error}"))
                }
            };
        }
    };

    if let Some(asset) = manager.get_update_pending_restart() {
        return LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
            manager: Some(manager),
            target: LinuxUpdateTarget::Asset(Box::new(asset)),
        });
    }

    match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => {
            LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                manager: Some(manager),
                target: LinuxUpdateTarget::Info(update),
            })
        }
        Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
            LinuxUpdateCheckResult::UpToDate
        }
        Err(error) => LinuxUpdateCheckResult::Failed(error.to_string()),
    }
}

fn linux_update_manager() -> Result<UpdateManager, String> {
    let source = GithubSource::new(LINUX_UPDATE_REPO_URL, None, true);
    let options = UpdateOptions {
        ExplicitChannel: Some("linux".to_owned()),
        ..Default::default()
    };
    UpdateManager::new(source, Some(options), None).map_err(|error| match error {
        velopack::Error::NotInstalled(_) => "This install cannot self-update.".to_owned(),
        other => other.to_string(),
    })
}

fn download_and_apply_linux_update(
    update: PendingLinuxUpdate,
) -> Result<LinuxUpdateApplyResult, String> {
    match update.target {
        LinuxUpdateTarget::Info(info) => {
            let info = info.as_ref();
            let manager = update
                .manager
                .as_ref()
                .ok_or_else(|| "Self-update manager unavailable.".to_owned())?;
            manager
                .download_updates(info, None)
                .map_err(|error| error.to_string())?;
            manager
                .apply_updates_and_restart(info)
                .map_err(|error| error.to_string())?;
            Ok(LinuxUpdateApplyResult::Restarting)
        }
        LinuxUpdateTarget::Asset(asset) => {
            let asset = asset.as_ref();
            let manager = update
                .manager
                .as_ref()
                .ok_or_else(|| "Self-update manager unavailable.".to_owned())?;
            manager
                .apply_updates_and_restart(asset)
                .map_err(|error| error.to_string())?;
            Ok(LinuxUpdateApplyResult::Restarting)
        }
        LinuxUpdateTarget::Deb(update) => {
            let path = download_deb_update(update)?;
            if try_install_deb_update(&path)? {
                Ok(LinuxUpdateApplyResult::DebInstalled(path))
            } else {
                open_deb_installer(&path)?;
                Ok(LinuxUpdateApplyResult::InstallerOpened(path))
            }
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

fn check_for_linux_deb_update() -> Result<Option<ManualDebUpdate>, String> {
    let url = linux_deb_releases_url();
    let mut response = ureq::get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("GitHub .deb update check failed: {error}"))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("GitHub .deb update check failed: {error}"))?;
    let releases: Vec<GitHubRelease> = serde_json::from_str(&body)
        .map_err(|error| format!("GitHub .deb update feed was invalid: {error}"))?;

    Ok(select_latest_linux_deb_update(releases, APP_BUILD_VERSION))
}

fn linux_deb_releases_url() -> String {
    env::var("OKP_LINUX_DEB_RELEASES_URL").unwrap_or_else(|_| LINUX_DEB_RELEASES_API_URL.to_owned())
}

fn select_latest_linux_deb_update(
    releases: Vec<GitHubRelease>,
    current_version: &str,
) -> Option<ManualDebUpdate> {
    let mut best = None::<ManualDebUpdate>;
    for release in releases {
        if release.draft || !release.prerelease {
            continue;
        }
        let version = release
            .tag_name
            .strip_prefix("linux-v")
            .unwrap_or(&release.tag_name)
            .to_owned();
        if compare_linux_versions(&version, current_version) != std::cmp::Ordering::Greater {
            continue;
        }
        let Some(asset) = release.assets.into_iter().find(|asset| {
            asset.name.starts_with("ok-player_") && asset.name.ends_with("_amd64.deb")
        }) else {
            continue;
        };
        let candidate = ManualDebUpdate {
            version,
            name: asset.name,
            url: asset.browser_download_url,
            size: asset.size,
        };
        if best.as_ref().is_none_or(|current| {
            compare_linux_versions(&candidate.version, &current.version)
                == std::cmp::Ordering::Greater
        }) {
            best = Some(candidate);
        }
    }

    best
}

fn download_deb_update(update: ManualDebUpdate) -> Result<PathBuf, String> {
    let cache_dir = linux_update_cache_dir();
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Could not create update cache: {error}"))?;
    let target = cache_dir.join(&update.name);
    let temp = cache_dir.join(format!("{}.part", update.name));

    let mut response = ureq::get(&update.url)
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("Download failed: {error}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| format!("Download failed: {error}"))?;
    if let Some(expected) = update.size
        && expected > 0
        && bytes.len() as u64 != expected
    {
        return Err(format!(
            "Download size mismatch: expected {expected} bytes, got {}.",
            bytes.len()
        ));
    }

    fs::write(&temp, bytes).map_err(|error| format!("Could not save update: {error}"))?;
    fs::rename(&temp, &target).map_err(|error| format!("Could not finalize update: {error}"))?;
    Ok(target)
}

fn linux_update_cache_dir() -> PathBuf {
    if let Some(cache_dir) =
        env::var_os("OKP_LINUX_UPDATE_CACHE_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(cache_dir);
    }
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache_home).join("ok-player/updates");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".cache/ok-player/updates");
    }
    env::temp_dir().join("ok-player/updates")
}

fn deb_self_install_available() -> bool {
    find_executable("pkexec").is_some()
        && (find_executable("apt-get").is_some() || find_executable("apt").is_some())
}

fn try_install_deb_update(path: &Path) -> Result<bool, String> {
    if env::var_os("OKP_SKIP_DEB_SELF_INSTALL").is_some() {
        return Ok(false);
    }

    let Some(pkexec) = find_executable("pkexec") else {
        return Ok(false);
    };
    let Some(apt) = find_executable("apt-get").or_else(|| find_executable("apt")) else {
        return Ok(false);
    };

    let mut child = Command::new(pkexec)
        .arg(apt)
        .arg("install")
        .arg("-y")
        .arg(path)
        .spawn()
        .map_err(|error| {
            format!(
                "Downloaded to {}, but could not request administrator approval: {error}",
                path.display()
            )
        })?;

    let timeout = deb_self_install_timeout();
    match wait_for_child_with_timeout(&mut child, timeout).map_err(|error| {
        format!(
            "Downloaded to {}, but could not wait for administrator approval: {error}",
            path.display()
        )
    })? {
        Some(status) if status.success() => Ok(true),
        Some(status) => {
            eprintln!(
                "Privileged .deb install exited with {status}; falling back to installer open."
            );
            Ok(false)
        }
        None => {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!(
                "Privileged .deb install timed out after {}s; falling back to installer open.",
                timeout.as_secs()
            );
            Ok(false)
        }
    }
}

fn deb_self_install_timeout() -> Duration {
    parse_deb_self_install_timeout(
        env::var("OKP_DEB_SELF_INSTALL_TIMEOUT_SECS")
            .ok()
            .as_deref(),
    )
}

fn parse_deb_self_install_timeout(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEB_SELF_INSTALL_TIMEOUT)
}

fn wait_for_child_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<ExitStatus>, std::io::Error> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Ok(None);
        }
        let remaining = timeout.saturating_sub(elapsed);
        std::thread::sleep(remaining.min(Duration::from_millis(100)));
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    if name.contains(std::path::MAIN_SEPARATOR) {
        let path = PathBuf::from(name);
        return path.is_file().then_some(path);
    }

    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn open_deb_installer(path: &Path) -> Result<(), String> {
    if env::var_os("OKP_SKIP_OPEN_INSTALLER").is_some() {
        return Ok(());
    }

    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|error| {
            format!(
                "Downloaded to {}, but could not open installer: {error}",
                path.display()
            )
        })?;
    Ok(())
}

fn compare_linux_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_key = linux_version_sort_key(left);
    let right_key = linux_version_sort_key(right);
    let max_len = left_key.len().max(right_key.len());
    for index in 0..max_len {
        let left_part = left_key.get(index).copied().unwrap_or_default();
        let right_part = right_key.get(index).copied().unwrap_or_default();
        match left_part.cmp(&right_part) {
            std::cmp::Ordering::Equal => {}
            order => return order,
        }
    }
    left.cmp(right)
}

fn linux_version_sort_key(version: &str) -> Vec<u64> {
    let mut key = Vec::new();
    let mut current = String::new();
    for character in version.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            key.push(current.parse().unwrap_or_default());
            current.clear();
        }
    }
    if !current.is_empty() {
        key.push(current.parse().unwrap_or_default());
    }
    key
}

fn open_external_url(url: &str) {
    if let Err(error) = Command::new("xdg-open").arg(url).spawn() {
        eprintln!("Failed to open {url}: {error}");
    }
}

fn settings_appearance_section() -> gtk::Box {
    let section = settings_section("Appearance");
    section.append(&settings_value_row("App theme", "Canonical light"));
    section.append(&settings_value_row("Player surface", "Dark video plane"));
    section.append(&settings_value_row(
        "Window chrome",
        "Custom captionless controls",
    ));
    section.append(&settings_value_row("Fullscreen caption", "Hidden"));
    section.append(&settings_value_row("Accent", "OK teal"));
    section
}

fn settings_integration_section(status_toast: Rc<StatusToast>) -> gtk::Box {
    let snapshot = LinuxIntegrationSnapshot::capture();
    let section = settings_section("Integration");

    let desktop_detail = snapshot
        .desktop_entry_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{LINUX_DESKTOP_ID} was not found in XDG application dirs"));
    section.append(&integration_status_row(
        "Desktop entry",
        if snapshot.desktop_entry_path.is_some() {
            "Installed"
        } else {
            "Missing"
        },
        &desktop_detail,
        if snapshot.desktop_entry_path.is_some() {
            IntegrationStatus::Good
        } else {
            IntegrationStatus::Bad
        },
    ));

    let registered = snapshot.registered_key_mimes;
    section.append(&integration_status_row(
        "Media types",
        &format!(
            "{registered}/{} key types",
            LINUX_KEY_MEDIA_MIME_TYPES.len()
        ),
        "Key audio/video MIME types advertised through the desktop entry.",
        if registered == LINUX_KEY_MEDIA_MIME_TYPES.len() {
            IntegrationStatus::Good
        } else if registered > 0 {
            IntegrationStatus::Warning
        } else {
            IntegrationStatus::Bad
        },
    ));

    let (defaults_value, defaults_detail, defaults_status) = match snapshot.default_key_mimes {
        Some(count) => {
            let remaining = LINUX_KEY_MEDIA_MIME_TYPES.len().saturating_sub(count);
            (
                format!("{count}/{} key types", LINUX_KEY_MEDIA_MIME_TYPES.len()),
                if remaining == 0 {
                    "OK Player is the default handler for the checked key media types.".to_owned()
                } else {
                    format!("{remaining} checked key media types still point elsewhere.")
                },
                if remaining == 0 {
                    IntegrationStatus::Good
                } else if count > 0 {
                    IntegrationStatus::Warning
                } else {
                    IntegrationStatus::Bad
                },
            )
        }
        None => (
            "Unavailable".to_owned(),
            "xdg-mime is not available, so default handlers cannot be checked.".to_owned(),
            IntegrationStatus::Warning,
        ),
    };
    let (defaults_row, defaults_value_label) = integration_status_row_with_value(
        "Default app",
        &defaults_value,
        &defaults_detail,
        defaults_status,
    );
    section.append(&defaults_row);

    section.append(&integration_status_row(
        "System tools",
        linux_integration_tools_value(&snapshot),
        linux_integration_tools_detail(&snapshot),
        if snapshot.xdg_mime_available && snapshot.update_desktop_database_available {
            IntegrationStatus::Good
        } else {
            IntegrationStatus::Warning
        },
    ));

    let status = gtk::Label::new(Some("Ready"));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);
    section.append(&status);

    let actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let primary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    primary.set_halign(gtk::Align::End);

    let make_default = gtk::Button::with_label("Make Default");
    make_default.add_css_class("okp-settings-button");
    make_default
        .set_sensitive(snapshot.xdg_mime_available && snapshot.desktop_entry_path.is_some());
    let make_default_status = status.clone();
    let make_default_value = defaults_value_label.clone();
    let make_default_toast = Rc::clone(&status_toast);
    make_default.connect_clicked(move |button| {
        button.set_sensitive(false);
        match set_linux_default_app_for_key_mimes() {
            Ok(count) => {
                set_integration_state_pill(
                    &make_default_value,
                    &format!("{count}/{} key types", LINUX_KEY_MEDIA_MIME_TYPES.len()),
                    if count == LINUX_KEY_MEDIA_MIME_TYPES.len() {
                        IntegrationStatus::Good
                    } else {
                        IntegrationStatus::Warning
                    },
                );
                make_default_status.set_text(&format!(
                    "OK Player set as default for {count} key media types."
                ));
                make_default_toast.show("Default media app updated");
            }
            Err(error) => {
                make_default_status.set_text(&format!("Could not update defaults: {error}"));
                make_default_toast.show("Could not update defaults");
            }
        }
        button.set_sensitive(true);
    });
    primary.append(&make_default);

    let default_apps = gtk::Button::with_label("Default Apps");
    default_apps.add_css_class("okp-settings-button");
    let default_apps_status = status.clone();
    let default_apps_toast = Rc::clone(&status_toast);
    default_apps.connect_clicked(move |_| match open_linux_default_apps_settings() {
        Ok(()) => {
            default_apps_status.set_text("Opened system Default Apps settings.");
            default_apps_toast.show("Default Apps opened");
        }
        Err(error) => {
            default_apps_status.set_text(&format!("Could not open Default Apps: {error}"));
            default_apps_toast.show("Could not open Default Apps");
        }
    });
    primary.append(&default_apps);
    actions.append(&primary);

    let secondary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    secondary.set_halign(gtk::Align::End);

    let refresh = gtk::Button::with_label("Refresh Database");
    refresh.add_css_class("okp-settings-button");
    refresh.set_sensitive(snapshot.update_desktop_database_available);
    let refresh_status = status.clone();
    let refresh_toast = Rc::clone(&status_toast);
    refresh.connect_clicked(move |_| match refresh_linux_desktop_database() {
        Ok(detail) => {
            refresh_status.set_text(&detail);
            refresh_toast.show("Desktop database refreshed");
        }
        Err(error) => {
            refresh_status.set_text(&format!("Desktop database refresh failed: {error}"));
            refresh_toast.show("Desktop database refresh failed");
        }
    });
    secondary.append(&refresh);

    let copy = gtk::Button::with_label("Copy Diagnostics");
    copy.add_css_class("okp-settings-button");
    let copy_status = status.clone();
    let copy_toast = Rc::clone(&status_toast);
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            let snapshot = LinuxIntegrationSnapshot::capture();
            display
                .clipboard()
                .set_text(&linux_integration_diagnostics(&snapshot));
            copy_status.set_text("Integration diagnostics copied.");
            copy_toast.show("Diagnostics copied");
        }
    });
    secondary.append(&copy);
    actions.append(&secondary);

    section.append(&actions);
    section
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

fn integration_status_row(
    label: &str,
    value: &str,
    detail: &str,
    status: IntegrationStatus,
) -> gtk::Box {
    integration_status_row_with_value(label, value, detail, status).0
}

fn integration_status_row_with_value(
    label: &str,
    value: &str,
    detail: &str,
    status: IntegrationStatus,
) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);

    let detail = gtk::Label::new(Some(detail));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let value = gtk::Label::new(Some(value));
    set_integration_state_pill(&value, value.text().as_ref(), status);
    value.set_valign(gtk::Align::Center);
    row.append(&value);

    (row, value)
}

fn set_integration_state_pill(label: &gtk::Label, value: &str, status: IntegrationStatus) {
    label.set_text(value);
    label.add_css_class("okp-integration-state-pill");
    for css_class in ["is-good", "is-warning", "is-bad"] {
        label.remove_css_class(css_class);
    }
    label.add_css_class(status.css_class());
}

fn linux_integration_tools_value(snapshot: &LinuxIntegrationSnapshot) -> &'static str {
    match (
        snapshot.xdg_mime_available,
        snapshot.update_desktop_database_available,
    ) {
        (true, true) => "Available",
        (true, false) | (false, true) => "Partial",
        (false, false) => "Missing",
    }
}

fn linux_integration_tools_detail(snapshot: &LinuxIntegrationSnapshot) -> &'static str {
    match (
        snapshot.xdg_mime_available,
        snapshot.update_desktop_database_available,
    ) {
        (true, true) => "xdg-mime and update-desktop-database are available.",
        (true, false) => "xdg-mime is available; update-desktop-database is missing.",
        (false, true) => "update-desktop-database is available; xdg-mime is missing.",
        (false, false) => "xdg-mime and update-desktop-database are missing.",
    }
}

fn linux_desktop_entry_path() -> Option<PathBuf> {
    linux_desktop_entry_paths()
        .into_iter()
        .find(|path| path.is_file())
}

fn linux_desktop_entry_paths() -> Vec<PathBuf> {
    linux_application_dirs()
        .into_iter()
        .map(|dir| dir.join(LINUX_DESKTOP_ID))
        .collect()
}

fn linux_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(data_home));
    } else if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }

    if let Some(data_dirs) = env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty()) {
        dirs.extend(env::split_paths(&data_dirs));
    } else {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }

    let mut application_dirs = Vec::new();
    for dir in dirs {
        let applications = dir.join("applications");
        if !application_dirs
            .iter()
            .any(|existing: &PathBuf| existing == &applications)
        {
            application_dirs.push(applications);
        }
    }
    application_dirs
}

fn parse_desktop_mime_types(contents: &str) -> Vec<String> {
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("MimeType="))
        .map(|types| {
            types
                .split(';')
                .map(str::trim)
                .filter(|mime| !mime.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn count_registered_key_media_mimes(desktop_entry: &str) -> usize {
    let registered = parse_desktop_mime_types(desktop_entry);
    LINUX_KEY_MEDIA_MIME_TYPES
        .iter()
        .filter(|mime| registered.iter().any(|registered| registered == *mime))
        .count()
}

fn count_default_key_media_mimes() -> usize {
    LINUX_KEY_MEDIA_MIME_TYPES
        .iter()
        .filter(|mime| {
            query_default_app_for_mime(mime)
                .as_deref()
                .is_some_and(default_app_matches_ok_player)
        })
        .count()
}

fn query_default_app_for_mime(mime: &str) -> Option<String> {
    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(mime)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn default_app_matches_ok_player(desktop_id: &str) -> bool {
    desktop_id.trim() == LINUX_DESKTOP_ID
}

fn set_linux_default_app_for_key_mimes() -> Result<usize, String> {
    if linux_desktop_entry_path().is_none() {
        return Err(format!("{LINUX_DESKTOP_ID} is not installed"));
    }
    let xdg_mime =
        find_executable("xdg-mime").ok_or_else(|| "xdg-mime is not installed".to_owned())?;
    let mut failures = Vec::new();
    for mime in LINUX_KEY_MEDIA_MIME_TYPES {
        match Command::new(&xdg_mime)
            .arg("default")
            .arg(LINUX_DESKTOP_ID)
            .arg(mime)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => failures.push(format!("{mime} ({status})")),
            Err(error) => failures.push(format!("{mime} ({error})")),
        }
    }
    if failures.is_empty() {
        Ok(LINUX_KEY_MEDIA_MIME_TYPES.len())
    } else {
        Err(failures.join(", "))
    }
}

fn refresh_linux_desktop_database() -> Result<String, String> {
    let updater = find_executable("update-desktop-database")
        .ok_or_else(|| "update-desktop-database is not installed".to_owned())?;
    let mut attempted = Vec::new();
    for dir in linux_application_dirs()
        .into_iter()
        .filter(|dir| dir.is_dir())
        .filter(|dir| dir.starts_with(user_data_home()))
    {
        attempted.push(dir.clone());
        match Command::new(&updater).arg(&dir).status() {
            Ok(status) if status.success() => {
                return Ok(format!("Refreshed {}.", dir.to_string_lossy()));
            }
            Ok(status) => eprintln!(
                "update-desktop-database failed for {}: {status}",
                dir.display()
            ),
            Err(error) => eprintln!(
                "update-desktop-database failed for {}: {error}",
                dir.display()
            ),
        }
    }

    if linux_desktop_entry_path().is_some() {
        Ok(
            "System desktop entry is installed; package manager owns the system database."
                .to_owned(),
        )
    } else if attempted.is_empty() {
        Err("no user application directory found".to_owned())
    } else {
        Err("no application database could be refreshed".to_owned())
    }
}

fn user_data_home() -> PathBuf {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(data_home)
    } else if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(home).join(".local/share")
    } else {
        env::temp_dir()
    }
}

fn open_linux_default_apps_settings() -> Result<(), String> {
    for (program, args) in [
        ("gnome-control-center", &["default-apps"][..]),
        ("kcmshell6", &["componentchooser"][..]),
        ("kcmshell5", &["componentchooser"][..]),
        ("systemsettings", &["kcm_componentchooser"][..]),
    ] {
        if find_executable(program).is_some() && Command::new(program).args(args).spawn().is_ok() {
            return Ok(());
        }
    }

    Command::new("xdg-open")
        .arg("settings://default-apps")
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn linux_integration_diagnostics(snapshot: &LinuxIntegrationSnapshot) -> String {
    let desktop_entry = snapshot
        .desktop_entry_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "missing".to_owned());
    let defaults = snapshot
        .default_key_mimes
        .map(|count| format!("{count}/{}", LINUX_KEY_MEDIA_MIME_TYPES.len()))
        .unwrap_or_else(|| "unavailable".to_owned());
    format!(
        "OK Player Linux Integration\nVersion: {APP_BUILD_VERSION}\nBuild: {APP_BUILD_SHA}\nDesktop ID: {LINUX_DESKTOP_ID}\nDesktop entry: {desktop_entry}\nRegistered key MIME types: {}/{}\nDefault key MIME types: {defaults}\nxdg-mime: {}\nupdate-desktop-database: {}\n",
        snapshot.registered_key_mimes,
        LINUX_KEY_MEDIA_MIME_TYPES.len(),
        if snapshot.xdg_mime_available {
            "available"
        } else {
            "missing"
        },
        if snapshot.update_desktop_database_available {
            "available"
        } else {
            "missing"
        },
    )
}

fn settings_subtitles_page(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");

    let snapshot = settings_subtitle_snapshot(&state);
    let summary = settings_section("Subtitles");
    summary.append(&settings_value_row("Primary", &snapshot.primary));
    summary.append(&settings_value_row("Secondary", &snapshot.secondary));
    let (delay_row, delay_label) =
        settings_value_row_with_label("Delay", &format_delay_label(snapshot.delay_seconds));
    summary.append(&delay_row);
    let (scale_row, scale_label) =
        settings_value_row_with_label("Size", &format_scale(snapshot.scale));
    summary.append(&scale_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let add_button = gtk::Button::with_label("Add subtitle...");
    add_button.add_css_class("okp-settings-button");
    add_button.set_sensitive(snapshot.has_media);
    let add_parent = parent.clone();
    let add_state = Rc::clone(&state);
    add_button.connect_clicked(move |_| open_subtitle_dialog(&add_parent, Rc::clone(&add_state)));
    actions.append(&add_button);

    for (label, adjustment) in [
        ("-50 ms", SubtitleAdjustment::Delay(-0.05)),
        ("+50 ms", SubtitleAdjustment::Delay(0.05)),
        ("Reset", SubtitleAdjustment::SetDelay(0.0)),
        ("Smaller", SubtitleAdjustment::Scale(-0.1)),
        ("100%", SubtitleAdjustment::SetScale(1.0)),
        ("Larger", SubtitleAdjustment::Scale(0.1)),
    ] {
        let button = gtk::Button::with_label(label);
        button.add_css_class("okp-settings-button");
        button.set_sensitive(snapshot.has_media);
        let button_state = Rc::clone(&state);
        let button_toast = Rc::clone(&status_toast);
        let button_delay = delay_label.clone();
        let button_scale = scale_label.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            refresh_settings_subtitle_values(&button_state, &button_delay, &button_scale);
            button_toast.show("Subtitle settings updated");
        });
        actions.append(&button);
    }
    summary.append(&actions);
    page.append(&summary);

    page.append(&settings_subtitle_track_section(
        "Primary Track",
        false,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    page.append(&settings_subtitle_track_section(
        "Secondary Track",
        true,
        state,
        status_toast,
    ));

    page
}

fn settings_audio_page(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");

    let summary = settings_section("Audio");
    summary.append(&settings_value_row(
        "Current track",
        &selected_track_summary(&state, TrackKind::Audio),
    ));
    summary.append(&settings_volume_row(Rc::clone(&state)));
    summary.append(&settings_audio_normalization_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    page.append(&summary);
    page.append(&settings_audio_device_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    page.append(&settings_audio_track_section(state, status_toast));

    page
}

fn settings_resume_row(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let active = state.borrow().settings.resume_enabled();
    settings_playback_switch_row(
        "Resume playback",
        "Reopen files at the saved position, skipping the first 5% and final stretch.",
        active,
        state,
        status_toast,
        |state, enabled| state.settings.set_resume_enabled(enabled),
        "Resume playback",
    )
}

fn settings_auto_advance_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().modes.auto_advance_enabled;
    settings_playback_switch_row(
        "Auto-advance",
        "Continue to the next item in the folder or playlist when a file ends.",
        active,
        state,
        status_toast,
        |state, enabled| {
            state.modes.auto_advance_enabled = enabled;
            state.settings.set_auto_advance_enabled(enabled);
        },
        "Auto-advance",
    )
}

fn settings_shuffle_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().modes.shuffle_enabled;
    settings_playback_switch_row(
        "Shuffle default",
        "Start folders and playlists in shuffled order, without immediate repeats.",
        active,
        state,
        status_toast,
        |state, enabled| {
            state.modes.shuffle_enabled = enabled;
            state.modes.reset_shuffle_order();
            if enabled && let Some(current_index) = current_playlist_index(state) {
                state
                    .modes
                    .ensure_shuffle_order(state.playlist.len(), current_index);
            }
            state.settings.set_shuffle_enabled(enabled);
        },
        "Shuffle",
    )
}

fn settings_playback_switch_row<F>(
    title: &str,
    detail: &str,
    active: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    apply: F,
    toast_subject: &'static str,
) -> gtk::Box
where
    F: Fn(&mut PlayerState, bool) + 'static,
{
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(detail));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(if active { "On" } else { "Off" }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let toggle = about_toggle_button(active);
    let toggle_state = Rc::clone(&state);
    let toggle_toast = Rc::clone(&status_toast);
    let toggle_state_label = state_label.clone();
    toggle.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);
        {
            let mut state = toggle_state.borrow_mut();
            apply(&mut state, enabled);
            save_settings_or_toast(&mut state, &toggle_toast);
        }
        toggle_state_label.set_text(if enabled { "On" } else { "Off" });
        toggle_toast.show(&format!(
            "{toast_subject} {}",
            if enabled { "on" } else { "off" }
        ));
    });
    row.append(&toggle);

    row
}

fn settings_repeat_row(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some("Repeat default"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(
        "Choose how folders and playlists repeat when they reach the end.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let current = state.borrow().modes.repeat_mode;
    let state_label = gtk::Label::new(Some(match current {
        RepeatMode::Off => "Off",
        RepeatMode::One => "One",
        RepeatMode::All => "All",
    }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let button = gtk::Button::with_label(current.label());
    button.add_css_class("okp-settings-button");
    button.set_valign(gtk::Align::Center);
    let repeat_state = Rc::clone(&state);
    let repeat_toast = Rc::clone(&status_toast);
    let repeat_state_label = state_label.clone();
    button.connect_clicked(move |button| {
        let mode = {
            let mut state = repeat_state.borrow_mut();
            state.modes.repeat_mode = state.modes.repeat_mode.cycle();
            let mode = state.modes.repeat_mode;
            state.settings.set_repeat_mode(mode.settings_value());
            save_settings_or_toast(&mut state, &repeat_toast);
            mode
        };
        button.set_label(mode.label());
        repeat_state_label.set_text(match mode {
            RepeatMode::Off => "Off",
            RepeatMode::One => "One",
            RepeatMode::All => "All",
        });
        repeat_toast.show(mode.label());
    });
    row.append(&button);

    row
}

fn save_settings_or_toast(state: &mut PlayerState, status_toast: &StatusToast) {
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save settings: {error}");
        status_toast.show("Could not save settings");
    }
}

struct SettingsSubtitleSnapshot {
    has_media: bool,
    primary: String,
    secondary: String,
    delay_seconds: f64,
    scale: f64,
}

fn settings_subtitle_snapshot(state: &Rc<RefCell<PlayerState>>) -> SettingsSubtitleSnapshot {
    let has_media = has_loaded_media(state);
    let tracks = read_tracks(state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let primary = tracks
        .iter()
        .find(|track| track.selected)
        .map(track_base_label)
        .unwrap_or_else(|| {
            if has_media {
                "Off".to_owned()
            } else {
                "No media loaded".to_owned()
            }
        });
    let secondary_id = read_secondary_subtitle_id(state);
    let secondary = secondary_id
        .and_then(|id| tracks.iter().find(|track| track.id == id))
        .map(track_base_label)
        .unwrap_or_else(|| {
            if has_media {
                "Off".to_owned()
            } else {
                "No media loaded".to_owned()
            }
        });
    let (delay_seconds, scale) = read_subtitle_adjustments(state);

    SettingsSubtitleSnapshot {
        has_media,
        primary,
        secondary,
        delay_seconds,
        scale,
    }
}

fn refresh_settings_subtitle_values(
    state: &Rc<RefCell<PlayerState>>,
    delay_label: &gtk::Label,
    scale_label: &gtk::Label,
) {
    let (delay_seconds, scale) = read_subtitle_adjustments(state);
    delay_label.set_text(&format_delay_label(delay_seconds));
    scale_label.set_text(&format_scale(scale));
}

fn settings_subtitle_track_section(
    title: &str,
    secondary: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section(title);
    if !has_loaded_media(&state) {
        section.append(&settings_empty_state("No media loaded"));
        return section;
    }

    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let selected_id = if secondary {
        read_secondary_subtitle_id(&state)
    } else {
        tracks
            .iter()
            .find(|track| track.selected)
            .map(|track| track.id)
    };
    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));

    let off_button = settings_track_button("Off", selected_id.is_none());
    connect_settings_subtitle_track_button(
        &off_button,
        None,
        secondary,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        &buttons,
    );
    buttons.borrow_mut().push(off_button.clone());
    section.append(&off_button);

    if tracks.is_empty() {
        section.append(&settings_empty_state("No subtitle tracks"));
    } else {
        for track in tracks {
            let button = settings_track_button(
                &track_label_for(&track, false),
                selected_id == Some(track.id),
            );
            connect_settings_subtitle_track_button(
                &button,
                Some(track.id),
                secondary,
                Rc::clone(&state),
                Rc::clone(&status_toast),
                &buttons,
            );
            buttons.borrow_mut().push(button.clone());
            section.append(&button);
        }
    }

    section
}

fn settings_audio_track_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Audio Tracks");
    if !has_loaded_media(&state) {
        section.append(&settings_empty_state("No media loaded"));
        return section;
    }

    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect::<Vec<_>>();
    let selected_id = tracks
        .iter()
        .find(|track| track.selected)
        .map(|track| track.id);
    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));

    let off_button = settings_track_button("Off", selected_id.is_none());
    connect_settings_audio_track_button(
        &off_button,
        None,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        &buttons,
    );
    buttons.borrow_mut().push(off_button.clone());
    section.append(&off_button);

    if tracks.is_empty() {
        section.append(&settings_empty_state("No audio tracks"));
    } else {
        for track in tracks {
            let button = settings_track_button(
                &track_label_for(&track, false),
                selected_id == Some(track.id),
            );
            connect_settings_audio_track_button(
                &button,
                Some(track.id),
                Rc::clone(&state),
                Rc::clone(&status_toast),
                &buttons,
            );
            buttons.borrow_mut().push(button.clone());
            section.append(&button);
        }
    }

    section
}

fn connect_settings_subtitle_track_button(
    button: &gtk::Button,
    track_id: Option<i64>,
    secondary: bool,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let selected_button = button.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |_| {
        let ok = with_mpv(&state, |mpv| {
            if secondary {
                mpv.select_secondary_subtitle(track_id)
            } else {
                mpv.select_subtitle(track_id)
            }
        });
        if ok {
            save_current_preferences(&state);
            mark_settings_track_selected(&buttons, &selected_button);
            status_toast.show("Subtitle track updated");
        }
    });
}

fn connect_settings_audio_track_button(
    button: &gtk::Button,
    track_id: Option<i64>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let selected_button = button.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |_| {
        if with_mpv(&state, |mpv| mpv.select_audio(track_id)) {
            save_current_preferences(&state);
            mark_settings_track_selected(&buttons, &selected_button);
            status_toast.show("Audio track updated");
        }
    });
}

fn connect_settings_audio_device_button(
    button: &gtk::Button,
    device_name: String,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let selected_button = button.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |_| {
        if with_mpv(&state, |mpv| mpv.set_audio_device(&device_name)) {
            save_audio_device_setting(&state, &device_name, Some(status_toast.as_ref()));
            mark_settings_track_selected(&buttons, &selected_button);
            status_toast.show("Audio output updated");
        }
    });
}

fn mark_settings_track_selected(buttons: &Rc<RefCell<Vec<gtk::Button>>>, selected: &gtk::Button) {
    for button in buttons.borrow().iter() {
        button.remove_css_class("is-selected");
    }
    selected.add_css_class("is-selected");
}

fn settings_track_button(text: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("okp-settings-track-row");
    button.set_has_frame(false);
    if selected {
        button.add_css_class("is-selected");
    }
    button
}

fn settings_empty_state(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-update-status");
    label.set_xalign(0.0);
    label
}

fn selected_track_summary(state: &Rc<RefCell<PlayerState>>, kind: TrackKind) -> String {
    if !has_loaded_media(state) {
        return "No media loaded".to_owned();
    }

    read_tracks(state)
        .into_iter()
        .find(|track| track.kind == kind && track.selected)
        .map(|track| track_label_for(&track, false))
        .unwrap_or_else(|| "Off".to_owned())
}

fn format_delay_label(seconds: f64) -> String {
    let milliseconds = (seconds * 1000.0).round() as i64;
    if milliseconds > 0 {
        format!("+{milliseconds} ms")
    } else {
        format!("{milliseconds} ms")
    }
}

fn settings_volume_row(state: Rc<RefCell<PlayerState>>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some("Volume"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);

    let current_volume = state.borrow().settings.volume();
    let value = gtk::Label::new(Some(&format!("{current_volume:.0}%")));
    value.add_css_class("okp-info-value");
    value.set_xalign(1.0);
    header.append(&label);
    header.append(&value);
    row.append(&header);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    scale.set_draw_value(false);
    scale.set_value(current_volume);
    scale.add_css_class("okp-settings-scale");

    let value_label = value.clone();
    scale.connect_change_value(move |_, _, volume| {
        value_label.set_text(&format!("{volume:.0}%"));
        set_volume_from_ui(&state, volume);
        glib::Propagation::Proceed
    });
    row.append(&scale);

    row
}

fn settings_audio_normalization_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let active = state.borrow().settings.audio_normalization_enabled();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let label = gtk::Label::new(Some("Loudness normalization"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);
    let detail = gtk::Label::new(Some(
        "Night mode: smooths quiet dialogue and loud effects using mpv dynaudnorm.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(50);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let state_label = gtk::Label::new(Some(if active { "On" } else { "Off" }));
    state_label.add_css_class("okp-settings-state-pill");
    state_label.set_valign(gtk::Align::Center);
    row.append(&state_label);

    let toggle = about_toggle_button(active);
    let toggle_state = Rc::clone(&state);
    let toggle_toast = Rc::clone(&status_toast);
    let toggle_state_label = state_label.clone();
    toggle.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);

        let (save_result, live_result) = {
            let mut state = toggle_state.borrow_mut();
            state.settings.set_audio_normalization_enabled(enabled);
            let save_result = state.settings.save();
            let live_result = state
                .mpv
                .as_ref()
                .map(|mpv| mpv.set_audio_normalization(enabled));
            (save_result, live_result)
        };

        toggle_state_label.set_text(if enabled { "On" } else { "Off" });

        if let Err(error) = save_result {
            eprintln!("Failed to save audio normalization setting: {error}");
            toggle_toast.show("Could not save audio normalization");
        } else if let Some(Err(error)) = live_result {
            eprintln!("Failed to update audio normalization: {error}");
            toggle_toast.show("Could not update audio normalization");
        } else {
            toggle_toast.show(if enabled {
                "Loudness normalization on"
            } else {
                "Loudness normalization off"
            });
        }
    });
    row.append(&toggle);

    row
}

fn settings_audio_device_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Output Device");
    let devices = read_audio_devices(&state);
    if devices.is_empty() {
        section.append(&settings_empty_state("Audio engine not ready"));
        return section;
    }

    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));
    for device in devices {
        let button = settings_track_button(&device.label, device.selected);
        connect_settings_audio_device_button(
            &button,
            device.name,
            Rc::clone(&state),
            Rc::clone(&status_toast),
            &buttons,
        );
        buttons.borrow_mut().push(button.clone());
        section.append(&button);
    }

    section
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

fn settings_video_adjustment_row(
    adjustment: VideoAdjustment,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some(adjustment.label()));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);

    let current = adjustment.read(&state.borrow().settings);
    let value = gtk::Label::new(Some(&format_video_adjustment(current)));
    value.add_css_class("okp-info-value");
    value.set_xalign(1.0);

    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("okp-settings-button");

    header.append(&label);
    header.append(&value);
    header.append(&reset);
    row.append(&header);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -100.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_value(current);
    scale.add_css_class("okp-settings-scale");

    let value_label = value.clone();
    let slider_state = Rc::clone(&state);
    let slider_toast = Rc::clone(&status_toast);
    scale.connect_change_value(move |_, _, raw_value| {
        let value = raw_value.round().clamp(-100.0, 100.0);
        value_label.set_text(&format_video_adjustment(value));
        set_video_adjustment_from_ui(&slider_state, adjustment, value, &slider_toast);
        glib::Propagation::Proceed
    });

    let reset_scale = scale.clone();
    let reset_state = Rc::clone(&state);
    let reset_toast = Rc::clone(&status_toast);
    let reset_value = value.clone();
    reset.connect_clicked(move |_| {
        reset_scale.set_value(0.0);
        reset_value.set_text(&format_video_adjustment(0.0));
        set_video_adjustment_from_ui(&reset_state, adjustment, 0.0, &reset_toast);
    });

    row.append(&scale);
    row
}

fn set_video_adjustment_from_ui(
    state: &Rc<RefCell<PlayerState>>,
    adjustment: VideoAdjustment,
    value: f64,
    status_toast: &StatusToast,
) {
    let (stored_value, save_ok) = {
        let mut state = state.borrow_mut();
        adjustment.write(&mut state.settings, value);
        let save_ok = if let Err(error) = state.settings.save() {
            eprintln!("Failed to save video adjustment: {error}");
            false
        } else {
            true
        };
        (adjustment.read(&state.settings), save_ok)
    };

    let live_result = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| adjustment.apply(mpv, stored_value))
    };

    match live_result {
        Some(Err(error)) => {
            eprintln!("Failed to update video adjustment: {error}");
            status_toast.show("Could not update video adjustment");
        }
        _ if !save_ok => status_toast.show("Could not save video adjustment"),
        _ => {}
    }
}

fn format_video_adjustment(value: f64) -> String {
    if value > 0.0 {
        format!("+{value:.0}")
    } else {
        format!("{value:.0}")
    }
}

fn settings_hwdec_row(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-switch-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);

    let label = gtk::Label::new(Some("Hardware decode"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    text.append(&label);

    let detail = gtk::Label::new(Some(
        "Use mpv auto-safe decoding when the driver stack supports it.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    text.append(&detail);
    row.append(&text);

    let enabled = state.borrow().settings.hardware_decode_enabled();
    let state_label = gtk::Label::new(Some(if enabled { "Auto-safe" } else { "Off" }));
    state_label.add_css_class("okp-settings-state-pill");
    row.append(&state_label);

    let toggle = gtk::Switch::new();
    toggle.set_active(enabled);
    let switch_state = Rc::clone(&state);
    let switch_toast = Rc::clone(&status_toast);
    let switch_label = state_label.clone();
    toggle.connect_state_set(move |_, enabled| {
        let (hwdec_option, save_ok) = {
            let mut state = switch_state.borrow_mut();
            state.settings.set_hardware_decode_enabled(enabled);
            let save_ok = if let Err(error) = state.settings.save() {
                eprintln!("Failed to save hardware decode setting: {error}");
                false
            } else {
                true
            };
            (state.settings.hardware_decode_mpv_option(), save_ok)
        };

        switch_label.set_text(if enabled { "Auto-safe" } else { "Off" });

        let live_result = {
            let state = switch_state.borrow();
            state.mpv.as_ref().map(|mpv| mpv.set_hwdec(hwdec_option))
        };

        match live_result {
            Some(Err(error)) => {
                eprintln!("Failed to update hardware decode: {error}");
                switch_toast.show("Could not update hardware decode");
            }
            _ if !save_ok => switch_toast.show("Could not save hardware decode setting"),
            _ => switch_toast.show(if enabled {
                "Hardware decode auto-safe"
            } else {
                "Hardware decode off"
            }),
        }

        glib::Propagation::Proceed
    });
    row.append(&toggle);

    row
}

fn settings_shortcuts_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&settings_shortcut_editor_section(state, status_toast));
    content
}

struct ShortcutEditorRow {
    action: ShortcutAction,
    default_chord: ShortcutChord,
    primary_chord: Cell<ShortcutChord>,
    secondary_chord: Cell<Option<ShortcutChord>>,
    container: gtk::Box,
    primary_chip: gtk::Button,
    primary_chip_label: gtk::Label,
    secondary_chip: gtk::Button,
    secondary_chip_label: gtk::Label,
    badge: gtk::Label,
    reset: gtk::Button,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShortcutEditorSlot {
    Primary,
    Secondary,
}

fn settings_shortcut_editor_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Keyboard Shortcuts");
    let rows = Rc::new(RefCell::new(Vec::<Rc<ShortcutEditorRow>>::new()));
    let bindings = shortcut_editor_initial_bindings(&state.borrow().settings);

    let search = gtk::Entry::new();
    search.add_css_class("okp-shortcuts-search");
    search.set_placeholder_text(Some("Search"));
    section.append(&search);

    let status = gtk::Label::new(Some("Ready"));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("okp-shortcuts-list");

    for action in SHORTCUT_ACTIONS {
        let current_chords = shortcut_chords_for_action(&bindings, *action);
        let primary_chord = current_chords
            .first()
            .copied()
            .unwrap_or_else(|| default_chord_for_action(*action));
        let secondary_chord = current_chords.get(1).copied();
        let row = shortcut_editor_row(
            *action,
            primary_chord,
            secondary_chord,
            Rc::clone(&rows),
            Rc::clone(&state),
            Rc::clone(&status_toast),
            status.clone(),
        );
        list.append(&row.container);
        rows.borrow_mut().push(row);
    }
    section.append(&list);

    section.append(&status);

    let search_rows = Rc::clone(&rows);
    search.connect_changed(move |entry| {
        let query = entry.text().trim().to_ascii_lowercase();
        for row in search_rows.borrow().iter() {
            let visible = query.is_empty()
                || row.action.label().to_ascii_lowercase().contains(&query)
                || row.action.id().contains(&query)
                || shortcut_chord_label(row.primary_chord.get())
                    .to_ascii_lowercase()
                    .contains(&query)
                || row
                    .secondary_chord
                    .get()
                    .map(shortcut_chord_label)
                    .is_some_and(|label| label.to_ascii_lowercase().contains(&query));
            row.container.set_visible(visible);
        }
    });

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let reset = gtk::Button::with_label("Reset All");
    reset.add_css_class("okp-settings-button");
    let reset_rows = Rc::clone(&rows);
    let reset_state = state;
    let reset_toast = status_toast;
    let reset_status = status;
    reset.connect_clicked(move |_| {
        shortcut_editor_clear_capture(&reset_rows.borrow());
        shortcut_editor_clear_conflicts(&reset_rows.borrow());
        for row in reset_rows.borrow().iter() {
            row.primary_chord.set(row.default_chord);
            row.secondary_chord.set(None);
            shortcut_editor_refresh_row(row);
        }
        save_shortcut_editor_rows(
            &reset_rows,
            &reset_state,
            &reset_status,
            &reset_toast,
            "All shortcuts reset",
        );
    });
    actions.append(&reset);

    section.append(&actions);
    section
}

fn shortcut_editor_row(
    action: ShortcutAction,
    primary_chord: ShortcutChord,
    secondary_chord: Option<ShortcutChord>,
    rows: Rc<RefCell<Vec<Rc<ShortcutEditorRow>>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    status: gtk::Label,
) -> Rc<ShortcutEditorRow> {
    let default_chord =
        parse_shortcut_chord(action.default_shortcut(), 0).expect("default shortcuts should parse");
    let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    container.add_css_class("okp-shortcut-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);

    let title = gtk::Label::new(Some(action.label()));
    title.add_css_class("okp-shortcut-action-title");
    title.set_xalign(0.0);
    text.append(&title);

    let subtitle = gtk::Label::new(Some(action.id()));
    subtitle.add_css_class("okp-shortcut-action-id");
    subtitle.set_xalign(0.0);
    text.append(&subtitle);
    container.append(&text);

    let badge = gtk::Label::new(Some("CUSTOM"));
    badge.add_css_class("okp-shortcut-badge");
    badge.set_valign(gtk::Align::Center);
    container.append(&badge);

    let primary_chip = gtk::Button::new();
    primary_chip.add_css_class("okp-shortcut-chip");
    primary_chip.set_has_frame(false);
    primary_chip.set_focus_on_click(true);
    primary_chip.set_tooltip_text(Some("Change primary shortcut"));
    let primary_chip_label = gtk::Label::new(None);
    primary_chip_label.add_css_class("okp-shortcut-chip-label");
    primary_chip.set_child(Some(&primary_chip_label));
    container.append(&primary_chip);

    let secondary_chip = gtk::Button::new();
    secondary_chip.add_css_class("okp-shortcut-chip");
    secondary_chip.add_css_class("is-secondary");
    secondary_chip.set_has_frame(false);
    secondary_chip.set_focus_on_click(true);
    secondary_chip.set_tooltip_text(Some("Add secondary shortcut"));
    let secondary_chip_label = gtk::Label::new(None);
    secondary_chip_label.add_css_class("okp-shortcut-chip-label");
    secondary_chip.set_child(Some(&secondary_chip_label));
    container.append(&secondary_chip);

    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("okp-shortcut-reset");
    reset.set_has_frame(false);
    reset.set_valign(gtk::Align::Center);
    container.append(&reset);

    let row = Rc::new(ShortcutEditorRow {
        action,
        default_chord,
        primary_chord: Cell::new(primary_chord),
        secondary_chord: Cell::new(secondary_chord),
        container,
        primary_chip,
        primary_chip_label,
        secondary_chip,
        secondary_chip_label,
        badge,
        reset,
    });
    shortcut_editor_refresh_row(&row);

    connect_shortcut_editor_chip(
        &row,
        ShortcutEditorSlot::Primary,
        Rc::clone(&rows),
        Rc::clone(&state),
        Rc::clone(&status_toast),
        status.clone(),
    );
    connect_shortcut_editor_chip(
        &row,
        ShortcutEditorSlot::Secondary,
        Rc::clone(&rows),
        Rc::clone(&state),
        Rc::clone(&status_toast),
        status.clone(),
    );

    let reset_row = Rc::clone(&row);
    let reset_rows = rows;
    let reset_state = state;
    let reset_toast = status_toast;
    let reset_status = status;
    row.reset.connect_clicked(move |_| {
        shortcut_editor_clear_capture(&reset_rows.borrow());
        shortcut_editor_clear_conflicts(&reset_rows.borrow());
        reset_row.primary_chord.set(reset_row.default_chord);
        reset_row.secondary_chord.set(None);
        shortcut_editor_refresh_row(&reset_row);
        save_shortcut_editor_rows(
            &reset_rows,
            &reset_state,
            &reset_status,
            &reset_toast,
            "Shortcut reset",
        );
    });

    row
}

fn connect_shortcut_editor_chip(
    row: &Rc<ShortcutEditorRow>,
    slot: ShortcutEditorSlot,
    rows: Rc<RefCell<Vec<Rc<ShortcutEditorRow>>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    status: gtk::Label,
) {
    let chip = shortcut_editor_chip_for(row, slot);
    let chip_row = Rc::clone(row);
    let chip_rows = Rc::clone(&rows);
    let chip_status = status.clone();
    chip.connect_clicked(move |button| {
        shortcut_editor_clear_capture(&chip_rows.borrow());
        shortcut_editor_clear_conflicts(&chip_rows.borrow());
        button.add_css_class("is-capturing");
        shortcut_editor_chip_label_for(&chip_row, slot).set_text("Press keys");
        chip_status.set_text(&format!("Recording {}", chip_row.action.label()));
        button.grab_focus();
    });

    let key_row = Rc::clone(row);
    let key_rows = rows;
    let key_state = state;
    let key_toast = status_toast;
    let key_status = status;
    let key_chip = shortcut_editor_chip_for(row, slot);
    let key_controller = gtk::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, modifiers| {
        if !key_chip.has_css_class("is-capturing") {
            return glib::Propagation::Proceed;
        }

        let chord = match shortcut_chord_from_event(key, modifiers) {
            Ok(chord) => chord,
            Err(message) => {
                key_status.set_text(message);
                return glib::Propagation::Stop;
            }
        };

        if let Some(conflict) =
            shortcut_editor_conflict(&key_rows.borrow(), key_row.action, slot, chord)
        {
            shortcut_editor_mark_conflict(&key_rows.borrow(), key_row.action, conflict);
            key_status.set_text(&format!(
                "{} already uses {}",
                conflict.label(),
                shortcut_chord_label(chord)
            ));
            key_toast.show("Shortcut conflict");
            return glib::Propagation::Stop;
        }

        shortcut_editor_clear_conflicts(&key_rows.borrow());
        key_chip.remove_css_class("is-capturing");
        shortcut_editor_set_chord(&key_row, slot, chord);
        shortcut_editor_refresh_row(&key_row);
        save_shortcut_editor_rows(
            &key_rows,
            &key_state,
            &key_status,
            &key_toast,
            "Shortcut saved",
        );
        glib::Propagation::Stop
    });
    shortcut_editor_chip_for(row, slot).add_controller(key_controller);
}

fn shortcut_editor_initial_bindings(settings: &settings::SettingsStore) -> Vec<ShortcutBinding> {
    resolved_shortcut_bindings(settings).unwrap_or_else(|error| {
        eprintln!(
            "Ignoring custom keybindings at line {} while building Settings UI: {}",
            error.line, error.message
        );
        default_shortcut_bindings()
    })
}

fn shortcut_editor_refresh_row(row: &ShortcutEditorRow) {
    let secondary = row.secondary_chord.get();
    let is_custom = row.primary_chord.get() != row.default_chord || secondary.is_some();
    row.primary_chip_label
        .set_text(&shortcut_chord_label(row.primary_chord.get()));
    if let Some(chord) = secondary {
        row.secondary_chip_label
            .set_text(&shortcut_chord_label(chord));
        row.secondary_chip.remove_css_class("is-empty");
        row.secondary_chip
            .set_tooltip_text(Some("Change secondary shortcut"));
    } else {
        row.secondary_chip_label.set_text("Add");
        row.secondary_chip.add_css_class("is-empty");
        row.secondary_chip
            .set_tooltip_text(Some("Add secondary shortcut"));
    }
    row.badge.set_visible(is_custom);
    row.reset.set_sensitive(is_custom);
}

fn shortcut_editor_clear_capture(rows: &[Rc<ShortcutEditorRow>]) {
    for row in rows {
        let was_capturing = row.primary_chip.has_css_class("is-capturing")
            || row.secondary_chip.has_css_class("is-capturing");
        row.primary_chip.remove_css_class("is-capturing");
        row.secondary_chip.remove_css_class("is-capturing");
        if was_capturing {
            shortcut_editor_refresh_row(row);
        }
    }
}

fn shortcut_editor_clear_conflicts(rows: &[Rc<ShortcutEditorRow>]) {
    for row in rows {
        row.container.remove_css_class("is-conflict");
        row.primary_chip.remove_css_class("is-conflict");
        row.secondary_chip.remove_css_class("is-conflict");
    }
}

fn shortcut_editor_mark_conflict(
    rows: &[Rc<ShortcutEditorRow>],
    left: ShortcutAction,
    right: ShortcutAction,
) {
    shortcut_editor_clear_conflicts(rows);
    for row in rows {
        if row.action == left || row.action == right {
            row.container.add_css_class("is-conflict");
            row.primary_chip.add_css_class("is-conflict");
            row.secondary_chip.add_css_class("is-conflict");
        }
    }
}

fn shortcut_editor_conflict(
    rows: &[Rc<ShortcutEditorRow>],
    action: ShortcutAction,
    slot: ShortcutEditorSlot,
    chord: ShortcutChord,
) -> Option<ShortcutAction> {
    for row in rows {
        if !(row.action == action && slot == ShortcutEditorSlot::Primary)
            && row.primary_chord.get() == chord
        {
            return Some(row.action);
        }
        if !(row.action == action && slot == ShortcutEditorSlot::Secondary)
            && row.secondary_chord.get() == Some(chord)
        {
            return Some(row.action);
        }
    }
    None
}

fn save_shortcut_editor_rows(
    rows: &Rc<RefCell<Vec<Rc<ShortcutEditorRow>>>>,
    state: &Rc<RefCell<PlayerState>>,
    status: &gtk::Label,
    status_toast: &StatusToast,
    success_message: &str,
) {
    let bindings = rows
        .borrow()
        .iter()
        .flat_map(|row| {
            let mut bindings = vec![ShortcutBinding {
                action: row.action,
                chord: row.primary_chord.get(),
            }];
            if let Some(chord) = row.secondary_chord.get() {
                bindings.push(ShortcutBinding {
                    action: row.action,
                    chord,
                });
            }
            bindings
        })
        .collect::<Vec<_>>();
    if let Err(error) = validate_shortcut_conflicts(&bindings) {
        status.set_text(&error.message);
        status_toast.show("Shortcut conflict");
        return;
    }

    let text = shortcut_config_text_from_bindings(&bindings);
    let save_result = {
        let mut state = state.borrow_mut();
        state.settings.set_raw_keybindings_config(&text);
        state.settings.save()
    };
    if let Err(error) = save_result {
        eprintln!("Failed to save keybinding remap setting: {error}");
        status.set_text("Could not save keybindings.");
        status_toast.show("Could not save keybindings");
        return;
    }

    status.set_text(success_message);
    status_toast.show(success_message);
}

fn shortcut_editor_chip_for(row: &ShortcutEditorRow, slot: ShortcutEditorSlot) -> gtk::Button {
    match slot {
        ShortcutEditorSlot::Primary => row.primary_chip.clone(),
        ShortcutEditorSlot::Secondary => row.secondary_chip.clone(),
    }
}

fn shortcut_editor_chip_label_for(row: &ShortcutEditorRow, slot: ShortcutEditorSlot) -> gtk::Label {
    match slot {
        ShortcutEditorSlot::Primary => row.primary_chip_label.clone(),
        ShortcutEditorSlot::Secondary => row.secondary_chip_label.clone(),
    }
}

fn shortcut_editor_set_chord(
    row: &ShortcutEditorRow,
    slot: ShortcutEditorSlot,
    chord: ShortcutChord,
) {
    match slot {
        ShortcutEditorSlot::Primary => row.primary_chord.set(chord),
        ShortcutEditorSlot::Secondary => row.secondary_chord.set(Some(chord)),
    }
}

fn settings_private_session_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some("Private Session"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let private_session = state.borrow().private_session;
    let button = gtk::Button::with_label(if private_session { "On" } else { "Off" });
    button.add_css_class("okp-settings-button");
    button.connect_clicked(move |button| {
        toggle_private_session(&state, &status_toast);
        let private_session = state.borrow().private_session;
        button.set_label(if private_session { "On" } else { "Off" });
    });
    row.append(&button);

    row
}

fn settings_clear_history_row(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some("History"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let button = gtk::Button::with_label("Clear...");
    button.add_css_class("okp-settings-button");
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        open_clear_history_dialog(&parent, Rc::clone(&state), Rc::clone(&status_toast));
    });
    row.append(&button);

    row
}

fn settings_value_row(label: &str, value: &str) -> gtk::Box {
    settings_value_row_with_label(label, value).0
}

fn settings_value_row_with_label(label: &str, value: &str) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_width_chars(14);
    row.append(&label);

    let value = gtk::Label::new(Some(value));
    value.add_css_class("okp-info-value");
    value.set_xalign(0.0);
    value.set_hexpand(true);
    value.set_width_chars(1);
    value.set_max_width_chars(44);
    value.set_ellipsize(pango::EllipsizeMode::Middle);
    value.set_selectable(true);
    row.append(&value);

    (row, value)
}

fn open_subtitle_dialog(parent: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Add subtitle"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Add", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.add_filter(&subtitle_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            load_subtitle_path(&state, path);
        }
        dialog.close();
    });

    dialog.present();
}

fn open_playlist_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open playlist"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.add_filter(&playlist_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            load_m3u_playlist(&state, &path, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

fn save_playlist_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Save playlist"),
        Some(parent),
        gtk::FileChooserAction::Save,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Save", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_current_name("OK Player Playlist.m3u");
    dialog.add_filter(&playlist_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            save_m3u_playlist(&state, playlist_save_path(path), &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

fn open_queue_media_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    mode: QueueInsertMode,
) {
    let (title, accept_label) = match mode {
        QueueInsertMode::Append => ("Add to Queue", "Add"),
        QueueInsertMode::PlayNext => ("Play Next", "Add"),
    };
    let dialog = gtk::FileChooserDialog::new(
        Some(title),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            (accept_label, gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    dialog.set_decorated(false);
    dialog.set_select_multiple(true);
    dialog.add_filter(&media_file_filter());
    dialog.add_filter(&all_files_filter());

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            queue_media_paths(&state, file_chooser_paths(dialog), mode, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

fn file_chooser_paths(dialog: &gtk::FileChooserDialog) -> Vec<PathBuf> {
    let files = dialog.files();
    let mut paths = Vec::new();
    for index in 0..files.n_items() {
        if let Some(path) = files
            .item(index)
            .and_then(|object| object.downcast::<gtk::gio::File>().ok())
            .and_then(|file| file.path())
        {
            paths.push(path);
        }
    }

    if paths.is_empty()
        && let Some(path) = dialog.file().and_then(|file| file.path())
    {
        paths.push(path);
    }

    paths
}

fn playlist_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("M3U playlists"));
    filter.add_pattern("*.m3u");
    filter.add_pattern("*.m3u8");
    filter
}

fn media_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Media files"));
    for extension in media_formats::extensions() {
        let pattern = format!("*{extension}");
        filter.add_pattern(&pattern);
        filter.add_pattern(&pattern.to_ascii_uppercase());
    }
    filter
}

fn subtitle_file_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Subtitle files"));
    for extension in media_formats::SUBTITLE_EXTENSIONS {
        let pattern = format!("*{extension}");
        filter.add_pattern(&pattern);
        filter.add_pattern(&pattern.to_ascii_uppercase());
    }
    filter
}

fn all_files_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("All files"));
    filter.add_pattern("*");
    filter
}

fn connect_drop(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    empty_surface: EmptySurface,
) {
    let drop_target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let enter_surface = empty_surface.clone();
    drop_target.connect_enter(move |_, _, _| {
        enter_surface.set_drop_active(true);
        gdk::DragAction::COPY
    });
    let leave_surface = empty_surface.clone();
    drop_target.connect_leave(move |_| {
        leave_surface.set_drop_active(false);
    });
    let drop_surface = empty_surface;
    drop_target.connect_drop(move |_, value, _, _| {
        drop_surface.set_drop_active(false);
        let Ok(files) = value.get::<gdk::FileList>() else {
            return false;
        };

        load_selected_local_paths(&state, dropped_file_list_paths(&files))
    });
    window.add_controller(drop_target);
}

fn dropped_file_list_paths(files: &gdk::FileList) -> Vec<PathBuf> {
    files
        .files()
        .into_iter()
        .filter_map(|file| file.path())
        .collect()
}

fn load_selected_local_paths(state: &Rc<RefCell<PlayerState>>, paths: Vec<PathBuf>) -> bool {
    let media_paths = selected_media_paths(&paths);
    match media_paths.as_slice() {
        [path] => {
            load_media_path(state, path.clone());
            load_selected_subtitles(state, selected_subtitle_paths(&paths));
            return true;
        }
        [] => {}
        _ => {
            let playlist = media_paths
                .into_iter()
                .map(PlaylistItem::Local)
                .collect::<Vec<_>>();
            let Some(first_item) = playlist.first().cloned() else {
                return false;
            };
            let loaded = load_playlist_item_with_playlist(state, first_item, playlist, true);
            if loaded {
                load_selected_subtitles(state, selected_subtitle_paths(&paths));
            }
            return loaded;
        }
    }

    if let Some(path) = selected_playlist_path(&paths) {
        return load_m3u_playlist_silent(state, &path);
    }

    load_selected_subtitles(state, selected_subtitle_paths(&paths))
}

fn selected_media_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut media_paths = Vec::new();
    for path in paths {
        if path.is_dir() {
            media_paths.extend(media_paths_in_directory(path));
        } else if is_media_path(path) {
            media_paths.push(path.clone());
        }
    }
    unique_media_paths(media_paths)
}

fn selected_subtitle_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut subtitles = Vec::new();
    for path in paths {
        if is_subtitle_path(path) && !subtitles.iter().any(|existing| existing == path) {
            subtitles.push(path.clone());
        }
    }
    subtitles
}

fn selected_playlist_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| is_playlist_path(path)).cloned()
}

fn load_selected_subtitles(state: &Rc<RefCell<PlayerState>>, paths: Vec<PathBuf>) -> bool {
    let mut loaded = false;
    for path in paths {
        loaded |= load_subtitle_path(state, path);
    }
    loaded
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    PlayPause,
    SeekBack,
    SeekForward,
    FrameForward,
    FrameBack,
    PreviousItem,
    NextItem,
    VolumeDown,
    VolumeUp,
    OpenFile,
    AddSubtitle,
    OpenUrl,
    CloseMedia,
    SaveScreenshot,
    CopyFrame,
    MediaInfo,
    GoToTime,
    AbLoop,
    SubtitleDelayForward,
    SubtitleDelayBack,
    SubtitleSizeDown,
    SubtitleSizeUp,
    Fullscreen,
    EscapeFullscreen,
    OpenSettings,
}

impl ShortcutAction {
    fn id(self) -> &'static str {
        match self {
            Self::PlayPause => "play-pause",
            Self::SeekBack => "seek-back",
            Self::SeekForward => "seek-forward",
            Self::FrameForward => "frame-forward",
            Self::FrameBack => "frame-back",
            Self::PreviousItem => "previous-item",
            Self::NextItem => "next-item",
            Self::VolumeDown => "volume-down",
            Self::VolumeUp => "volume-up",
            Self::OpenFile => "open-file",
            Self::AddSubtitle => "add-subtitle",
            Self::OpenUrl => "open-url",
            Self::CloseMedia => "close-media",
            Self::SaveScreenshot => "save-screenshot",
            Self::CopyFrame => "copy-frame",
            Self::MediaInfo => "media-info",
            Self::GoToTime => "go-to-time",
            Self::AbLoop => "ab-loop",
            Self::SubtitleDelayForward => "subtitle-delay-forward",
            Self::SubtitleDelayBack => "subtitle-delay-back",
            Self::SubtitleSizeDown => "subtitle-size-down",
            Self::SubtitleSizeUp => "subtitle-size-up",
            Self::Fullscreen => "fullscreen",
            Self::EscapeFullscreen => "escape-fullscreen",
            Self::OpenSettings => "open-settings",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::PlayPause => "Play / Pause",
            Self::SeekBack => "Seek Back",
            Self::SeekForward => "Seek Forward",
            Self::FrameForward => "Frame Forward",
            Self::FrameBack => "Frame Back",
            Self::PreviousItem => "Previous Item",
            Self::NextItem => "Next Item",
            Self::VolumeDown => "Volume Down",
            Self::VolumeUp => "Volume Up",
            Self::OpenFile => "Open File",
            Self::AddSubtitle => "Add Subtitle",
            Self::OpenUrl => "Open URL",
            Self::CloseMedia => "Close Media",
            Self::SaveScreenshot => "Save Screenshot",
            Self::CopyFrame => "Copy Frame",
            Self::MediaInfo => "Media Info",
            Self::GoToTime => "Go to Time",
            Self::AbLoop => "A-B Loop",
            Self::SubtitleDelayForward => "Subtitle Delay Forward",
            Self::SubtitleDelayBack => "Subtitle Delay Back",
            Self::SubtitleSizeDown => "Subtitle Size Down",
            Self::SubtitleSizeUp => "Subtitle Size Up",
            Self::Fullscreen => "Fullscreen",
            Self::EscapeFullscreen => "Exit Fullscreen",
            Self::OpenSettings => "Settings",
        }
    }

    fn default_shortcut(self) -> &'static str {
        match self {
            Self::PlayPause => "Space",
            Self::SeekBack => "Left",
            Self::SeekForward => "Right",
            Self::FrameForward => ".",
            Self::FrameBack => ",",
            Self::PreviousItem => "PageUp",
            Self::NextItem => "PageDown",
            Self::VolumeDown => "Down",
            Self::VolumeUp => "Up",
            Self::OpenFile => "O",
            Self::AddSubtitle => "S",
            Self::OpenUrl => "U",
            Self::CloseMedia => "X",
            Self::SaveScreenshot => "C",
            Self::CopyFrame => "Shift+C",
            Self::MediaInfo => "I",
            Self::GoToTime => "J",
            Self::AbLoop => "L",
            Self::SubtitleDelayForward => "Z",
            Self::SubtitleDelayBack => "Shift+Z",
            Self::SubtitleSizeDown => "[",
            Self::SubtitleSizeUp => "]",
            Self::Fullscreen => "F",
            Self::EscapeFullscreen => "Escape",
            Self::OpenSettings => "Ctrl+,",
        }
    }
}

const SHORTCUT_ACTIONS: &[ShortcutAction] = &[
    ShortcutAction::PlayPause,
    ShortcutAction::SeekBack,
    ShortcutAction::SeekForward,
    ShortcutAction::FrameForward,
    ShortcutAction::FrameBack,
    ShortcutAction::PreviousItem,
    ShortcutAction::NextItem,
    ShortcutAction::VolumeDown,
    ShortcutAction::VolumeUp,
    ShortcutAction::OpenFile,
    ShortcutAction::AddSubtitle,
    ShortcutAction::OpenUrl,
    ShortcutAction::CloseMedia,
    ShortcutAction::SaveScreenshot,
    ShortcutAction::CopyFrame,
    ShortcutAction::MediaInfo,
    ShortcutAction::GoToTime,
    ShortcutAction::AbLoop,
    ShortcutAction::SubtitleDelayForward,
    ShortcutAction::SubtitleDelayBack,
    ShortcutAction::SubtitleSizeDown,
    ShortcutAction::SubtitleSizeUp,
    ShortcutAction::Fullscreen,
    ShortcutAction::EscapeFullscreen,
    ShortcutAction::OpenSettings,
];

const MAX_SHORTCUTS_PER_ACTION: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShortcutChord {
    key: gdk::Key,
    modifiers: gdk::ModifierType,
}

impl ShortcutChord {
    fn new(key: gdk::Key, modifiers: gdk::ModifierType) -> Self {
        Self {
            key: key.to_lower(),
            modifiers: shortcut_modifiers(modifiers),
        }
    }

    fn matches(self, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
        self.key == key.to_lower() && self.modifiers == shortcut_modifiers(modifiers)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShortcutBinding {
    action: ShortcutAction,
    chord: ShortcutChord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShortcutConfigError {
    line: usize,
    message: String,
}

fn shortcut_modifiers(modifiers: gdk::ModifierType) -> gdk::ModifierType {
    modifiers
        & (gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::ALT_MASK)
}

fn shortcut_action_by_id(id: &str) -> Option<ShortcutAction> {
    SHORTCUT_ACTIONS
        .iter()
        .copied()
        .find(|action| action.id() == id)
}

fn default_shortcut_bindings() -> Vec<ShortcutBinding> {
    SHORTCUT_ACTIONS
        .iter()
        .copied()
        .map(|action| ShortcutBinding {
            action,
            chord: parse_shortcut_chord(action.default_shortcut(), 0)
                .expect("default shortcuts should parse"),
        })
        .collect()
}

fn resolved_shortcut_bindings(
    settings: &settings::SettingsStore,
) -> Result<Vec<ShortcutBinding>, ShortcutConfigError> {
    resolved_shortcut_bindings_from_text(settings.raw_keybindings_config())
}

fn resolved_shortcut_bindings_from_text(
    text: &str,
) -> Result<Vec<ShortcutBinding>, ShortcutConfigError> {
    let mut bindings = default_shortcut_bindings();
    let overrides = parse_raw_keybindings_config(text)?;
    for action in SHORTCUT_ACTIONS {
        let action_overrides = overrides
            .iter()
            .filter(|(override_action, _)| override_action == action)
            .map(|(_, chord)| *chord)
            .collect::<Vec<_>>();
        if action_overrides.is_empty() {
            continue;
        }

        bindings.retain(|binding| binding.action != *action);
        bindings.extend(action_overrides.into_iter().map(|chord| ShortcutBinding {
            action: *action,
            chord,
        }));
    }
    validate_shortcut_conflicts(&bindings)?;
    Ok(bindings)
}

fn shortcut_config_text_from_bindings(bindings: &[ShortcutBinding]) -> String {
    let mut lines = Vec::new();
    for action in SHORTCUT_ACTIONS {
        let default_chord = default_chord_for_action(*action);
        let chords = shortcut_chords_for_action(bindings, *action);
        if chords.len() == 1 && chords[0] == default_chord {
            continue;
        }

        for chord in chords.into_iter().take(MAX_SHORTCUTS_PER_ACTION) {
            lines.push(format!("{}={}", action.id(), shortcut_chord_label(chord)));
        }
    }
    lines.join("\n")
}

fn shortcut_chords_for_action(
    bindings: &[ShortcutBinding],
    action: ShortcutAction,
) -> Vec<ShortcutChord> {
    let chords = bindings
        .iter()
        .filter(|binding| binding.action == action)
        .map(|binding| binding.chord)
        .take(MAX_SHORTCUTS_PER_ACTION)
        .collect::<Vec<_>>();

    if chords.is_empty() {
        vec![default_chord_for_action(action)]
    } else {
        chords
    }
}

fn default_chord_for_action(action: ShortcutAction) -> ShortcutChord {
    parse_shortcut_chord(action.default_shortcut(), 0).expect("default shortcuts should parse")
}

fn keyboard_action_for_event(
    settings: &settings::SettingsStore,
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> Option<ShortcutAction> {
    let bindings = resolved_shortcut_bindings(settings).unwrap_or_else(|error| {
        eprintln!(
            "Ignoring custom keybindings at line {}: {}",
            error.line, error.message
        );
        default_shortcut_bindings()
    });

    shortcut_action_for_bindings(&bindings, key, modifiers)
}

fn shortcut_action_for_bindings(
    bindings: &[ShortcutBinding],
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> Option<ShortcutAction> {
    bindings
        .iter()
        .find(|binding| binding.chord.matches(key, modifiers))
        .map(|binding| binding.action)
}

fn parse_raw_keybindings_config(
    text: &str,
) -> Result<Vec<(ShortcutAction, ShortcutChord)>, ShortcutConfigError> {
    let mut overrides = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let Some((action_id, shortcut)) = trimmed.split_once('=') else {
            return Err(shortcut_config_error(
                line_number,
                "Use action=shortcut syntax, one binding per line.",
            ));
        };
        let action_id = action_id.trim();
        let shortcut = shortcut.trim();
        let Some(action) = shortcut_action_by_id(action_id) else {
            return Err(shortcut_config_error(
                line_number,
                &format!("Unknown action `{action_id}`."),
            ));
        };
        let existing_count = overrides
            .iter()
            .filter(|(existing_action, _)| *existing_action == action)
            .count();
        if existing_count >= MAX_SHORTCUTS_PER_ACTION {
            return Err(shortcut_config_error(
                line_number,
                &format!("Action `{action_id}` supports at most two shortcuts."),
            ));
        }

        overrides.push((action, parse_shortcut_chord(shortcut, line_number)?));
    }

    Ok(overrides)
}

fn parse_shortcut_chord(text: &str, line: usize) -> Result<ShortcutChord, ShortcutConfigError> {
    let mut modifiers = gdk::ModifierType::empty();
    let mut key = None::<gdk::Key>;

    for token in text
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= gdk::ModifierType::CONTROL_MASK,
            "alt" | "option" => modifiers |= gdk::ModifierType::ALT_MASK,
            "shift" => modifiers |= gdk::ModifierType::SHIFT_MASK,
            _ if key.is_none() => {
                key = Some(shortcut_key_from_token(token).ok_or_else(|| {
                    shortcut_config_error(line, &format!("Unknown key `{token}`."))
                })?);
            }
            _ => {
                return Err(shortcut_config_error(
                    line,
                    "Shortcut can only contain one non-modifier key.",
                ));
            }
        }
    }

    let Some(key) = key else {
        return Err(shortcut_config_error(line, "Shortcut key is empty."));
    };

    Ok(ShortcutChord::new(key, modifiers))
}

fn shortcut_key_from_token(token: &str) -> Option<gdk::Key> {
    let normalized = match token.to_ascii_lowercase().as_str() {
        "," | "comma" => "comma".to_owned(),
        "." | "period" => "period".to_owned(),
        "[" | "bracketleft" => "bracketleft".to_owned(),
        "]" | "bracketright" => "bracketright".to_owned(),
        "esc" | "escape" => "Escape".to_owned(),
        "pageup" | "page_up" => "Page_Up".to_owned(),
        "pagedown" | "page_down" => "Page_Down".to_owned(),
        "space" => "space".to_owned(),
        "left" => "Left".to_owned(),
        "right" => "Right".to_owned(),
        "up" => "Up".to_owned(),
        "down" => "Down".to_owned(),
        single if single.chars().count() == 1 => single.to_owned(),
        _ => token.to_owned(),
    };

    gdk::Key::from_name(normalized)
}

fn shortcut_chord_from_event(
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> Result<ShortcutChord, &'static str> {
    if shortcut_is_modifier_key(key) {
        return Err("Press a non-modifier key.");
    }

    Ok(ShortcutChord::new(key, modifiers))
}

fn shortcut_is_modifier_key(key: gdk::Key) -> bool {
    key.name()
        .map(|name| {
            matches!(
                name.as_str(),
                "Shift_L"
                    | "Shift_R"
                    | "Control_L"
                    | "Control_R"
                    | "Alt_L"
                    | "Alt_R"
                    | "Meta_L"
                    | "Meta_R"
                    | "Super_L"
                    | "Super_R"
                    | "Hyper_L"
                    | "Hyper_R"
                    | "ISO_Level3_Shift"
                    | "Caps_Lock"
            )
        })
        .unwrap_or(false)
}

fn validate_shortcut_conflicts(bindings: &[ShortcutBinding]) -> Result<(), ShortcutConfigError> {
    for (index, left) in bindings.iter().enumerate() {
        if let Some(right) = bindings
            .iter()
            .skip(index + 1)
            .find(|right| right.chord == left.chord)
        {
            return Err(shortcut_config_error(
                0,
                &format!(
                    "{} conflicts with {} on {}.",
                    right.action.id(),
                    left.action.id(),
                    shortcut_chord_label(left.chord)
                ),
            ));
        }
    }

    Ok(())
}

fn shortcut_config_error(line: usize, message: &str) -> ShortcutConfigError {
    ShortcutConfigError {
        line,
        message: message.to_owned(),
    }
}

fn shortcut_chord_label(chord: ShortcutChord) -> String {
    let mut parts = Vec::new();
    if chord.modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
        parts.push("Ctrl".to_owned());
    }
    if chord.modifiers.contains(gdk::ModifierType::ALT_MASK) {
        parts.push("Alt".to_owned());
    }
    if chord.modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
        parts.push("Shift".to_owned());
    }
    parts.push(
        chord
            .key
            .name()
            .map(|name| shortcut_display_key(name.as_str()))
            .unwrap_or_else(|| "Unknown".to_owned()),
    );
    parts.join("+")
}

fn shortcut_display_key(name: &str) -> String {
    match name {
        "space" => "Space".to_owned(),
        "comma" => ",".to_owned(),
        "period" => ".".to_owned(),
        "bracketleft" => "[".to_owned(),
        "bracketright" => "]".to_owned(),
        "Page_Up" => "PageUp".to_owned(),
        "Page_Down" => "PageDown".to_owned(),
        key if key.len() == 1 => key.to_ascii_uppercase(),
        key => key.to_owned(),
    }
}

fn connect_keyboard(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) {
    let controller = gtk::EventControllerKey::new();
    let shortcut_window = window.clone();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        chrome.show_for_activity();

        let action = {
            let state = state.borrow();
            keyboard_action_for_event(&state.settings, key, modifiers)
        };

        match action {
            Some(ShortcutAction::PlayPause) => {
                with_mpv(&state, |mpv| mpv.cycle_pause());
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SeekBack) => {
                with_mpv(&state, |mpv| mpv.seek_relative(-5.0));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SeekForward) => {
                with_mpv(&state, |mpv| mpv.seek_relative(5.0));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::FrameForward) => {
                with_mpv(&state, |mpv| mpv.frame_step());
                glib::Propagation::Stop
            }
            Some(ShortcutAction::FrameBack) => {
                with_mpv(&state, |mpv| mpv.frame_back_step());
                glib::Propagation::Stop
            }
            Some(ShortcutAction::PreviousItem) => {
                navigate_playlist(&state, -1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::NextItem) => {
                navigate_playlist(&state, 1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::VolumeDown) => {
                adjust_volume(&state, -5.0);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::VolumeUp) => {
                adjust_volume(&state, 5.0);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::OpenFile) => {
                open_media_dialog(&shortcut_window, Rc::clone(&state));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::AddSubtitle) => {
                open_subtitle_dialog(&shortcut_window, Rc::clone(&state));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::OpenUrl) => {
                open_url_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            Some(ShortcutAction::CloseMedia) => {
                close_current_media(&state, &status_toast);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::CopyFrame) => {
                copy_frame_to_clipboard(&state, &status_toast);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SaveScreenshot) => {
                save_screenshot(&state, &status_toast, false);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::MediaInfo) => {
                open_media_info_window(&shortcut_window, &state, Rc::clone(&status_toast));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::GoToTime) => {
                open_go_to_time_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            Some(ShortcutAction::AbLoop) => {
                toggle_ab_loop(&state, &status_toast);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleDelayForward) => {
                adjust_subtitle_delay(&state, 0.05);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleDelayBack) => {
                adjust_subtitle_delay(&state, -0.05);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleSizeDown) => {
                adjust_subtitle_scale(&state, -0.1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleSizeUp) => {
                adjust_subtitle_scale(&state, 0.1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::Fullscreen) => {
                toggle_fullscreen(&shortcut_window);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::EscapeFullscreen) if shortcut_window.is_fullscreen() => {
                shortcut_window.unfullscreen();
                glib::Propagation::Stop
            }
            Some(ShortcutAction::OpenSettings) => {
                open_settings_window(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(controller);
}

fn connect_progress_persistence(window: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let timer_state = Rc::clone(&state);
    glib::timeout_add_local(Duration::from_secs(10), move || {
        save_current_progress(&timer_state, false);
        glib::ControlFlow::Continue
    });

    let close_state = Rc::clone(&state);
    window.connect_close_request(move |_| {
        save_current_progress(&close_state, false);
        glib::Propagation::Proceed
    });
}

fn with_mpv(
    state: &Rc<RefCell<PlayerState>>,
    command: impl FnOnce(&Mpv) -> Result<(), okp_mpv::MpvError>,
) -> bool {
    if let Some(mpv) = state.borrow().mpv.as_ref()
        && let Err(error) = command(mpv)
    {
        eprintln!("mpv command failed: {error}");
        return false;
    }

    state.borrow().mpv.is_some()
}

fn has_loaded_media(state: &Rc<RefCell<PlayerState>>) -> bool {
    has_loaded_media_state(&state.borrow())
}

fn has_loaded_media_state(state: &PlayerState) -> bool {
    state.current_file.is_some() || state.current_url.is_some()
}

fn set_volume_from_ui(state: &Rc<RefCell<PlayerState>>, volume: f64) {
    let result = state
        .borrow()
        .mpv
        .as_ref()
        .map(|mpv| mpv.set_volume(volume));
    match result {
        Some(Ok(())) | None => save_volume_setting(state, volume),
        Some(Err(error)) => eprintln!("Failed to set volume: {error}"),
    }
}

fn adjust_volume(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    let updated_volume = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        let volume = match mpv.playback_state() {
            Ok(playback) => playback.volume.unwrap_or(100.0),
            Err(error) => {
                eprintln!("Failed to read volume: {error}");
                return;
            }
        };
        let updated_volume = (volume + delta).clamp(0.0, 130.0);
        if let Err(error) = mpv.set_volume(updated_volume) {
            eprintln!("Failed to set volume: {error}");
            return;
        }
        updated_volume
    };

    save_volume_setting(state, updated_volume);
}

fn save_volume_setting(state: &Rc<RefCell<PlayerState>>, volume: f64) {
    let mut state = state.borrow_mut();
    state.settings.set_volume(volume);
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save settings: {error}");
    }
}

fn save_audio_device_setting(
    state: &Rc<RefCell<PlayerState>>,
    device: &str,
    status_toast: Option<&StatusToast>,
) {
    let mut state = state.borrow_mut();
    state.settings.set_audio_device(device);
    state.pending_audio_device_restore = None;
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save audio device setting: {error}");
        if let Some(status_toast) = status_toast {
            status_toast.show("Could not save audio output");
        }
    }
}

fn adjust_subtitle_delay(state: &Rc<RefCell<PlayerState>>, delta_seconds: f64) {
    if with_mpv(state, |mpv| mpv.adjust_subtitle_delay(delta_seconds)) {
        save_current_preferences(state);
    }
}

fn adjust_subtitle_scale(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    if with_mpv(state, |mpv| mpv.adjust_subtitle_scale(delta)) {
        save_current_preferences(state);
    }
}

fn screenshot_context(state: &Rc<RefCell<PlayerState>>) -> Option<(Option<PathBuf>, Option<f64>)> {
    let (has_mpv, current_file, position) = {
        let state = state.borrow();
        let position = state
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok())
            .and_then(|playback| playback.time_pos);
        (state.mpv.is_some(), state.current_file.clone(), position)
    };

    if !has_mpv {
        return None;
    }

    Some((current_file, position))
}

fn save_screenshot(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    include_subtitles: bool,
) {
    let Some((current_file, position)) = screenshot_context(state) else {
        return;
    };
    let path = screenshots::next_screenshot_path(current_file.as_deref(), position);

    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.screenshot_to_file(&path, include_subtitles)
    };

    match result {
        Ok(()) => {
            let filename = path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| "screenshot.png".into());
            eprintln!("Screenshot saved to {}", path.display());
            status_toast.show(&format!("Screenshot saved: {filename}"));
        }
        Err(error) => {
            eprintln!("Failed to save screenshot to {}: {error}", path.display());
            status_toast.show("Screenshot failed");
        }
    }
}

fn copy_frame_to_clipboard(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    if screenshot_context(state).is_none() {
        return;
    }

    let path = screenshots::next_clipboard_frame_path();
    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.screenshot_to_file(&path, false)
    };

    if let Err(error) = result {
        eprintln!(
            "Failed to capture frame for clipboard at {}: {error}",
            path.display()
        );
        status_toast.show("Couldn't copy the frame");
        let _ = fs::remove_file(&path);
        return;
    }

    match gdk::Texture::from_filename(&path) {
        Ok(texture) => {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_texture(&texture);
                eprintln!("Frame copied to clipboard from {}", path.display());
                status_toast.show("Frame copied");
            } else {
                status_toast.show("Clipboard unavailable");
            }
        }
        Err(error) => {
            eprintln!("Failed to load clipboard frame {}: {error}", path.display());
            status_toast.show("Couldn't copy the frame");
        }
    }
    let _ = fs::remove_file(&path);
}

fn copy_current_time(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let time = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok())
            .and_then(|playback| playback.time_pos)
            .filter(|time| time.is_finite() && *time >= 0.0)
    };

    let Some(time) = time else {
        status_toast.show("Open media first");
        return;
    };

    let text = time_code::format(time);
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(&text);
        status_toast.show(&format!("Copied {text}"));
    } else {
        status_toast.show("Clipboard unavailable");
    }
}

fn open_current_file_location(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let path = state.borrow().current_file.clone();
    let Some(path) = path else {
        status_toast.show("Not a local file");
        return;
    };

    if show_file_in_file_manager(&path) {
        status_toast.show("Opened file location");
    } else {
        status_toast.show("Could not open the folder");
    }
}

fn show_file_in_file_manager(path: &Path) -> bool {
    if try_file_manager_show_items(path) {
        return true;
    }

    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return false;
    };

    Command::new("xdg-open").arg(parent).spawn().is_ok()
}

fn try_file_manager_show_items(path: &Path) -> bool {
    let uri = gtk::gio::File::for_path(path).uri().to_string();
    Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .spawn()
        .is_ok()
}

fn open_media_info_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };

        mpv.media_info(state.current_file.as_deref())
    };

    match result {
        Ok(media_info) => show_media_info_window(parent, &media_info, status_toast),
        Err(error) => {
            eprintln!("Failed to read media information: {error}");
            status_toast.show("Media information unavailable");
        }
    }
}

fn show_media_info_window(
    parent: &gtk::ApplicationWindow,
    media_info: &MediaInfo,
    status_toast: Rc<StatusToast>,
) {
    let window = captionless_transient_window(parent, "Media Information", 680, 820, true);
    window.add_css_class("okp-info-window");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("okp-info-root");

    let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    page.add_css_class("okp-info-page");
    page.set_margin_top(54);
    page.set_margin_end(36);
    page.set_margin_bottom(24);
    page.set_margin_start(36);

    let header = gtk::Box::new(gtk::Orientation::Vertical, 5);
    header.add_css_class("okp-info-hero");
    let eyebrow = gtk::Label::new(Some("MEDIA INFO"));
    eyebrow.add_css_class("okp-info-eyebrow");
    eyebrow.set_xalign(0.0);
    header.append(&eyebrow);

    let title = gtk::Label::new(Some(&media_info.title));
    title.add_css_class("okp-info-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    header.append(&title);

    if let Some(path) = media_info.path.as_deref() {
        let path_label = gtk::Label::new(Some(path));
        path_label.add_css_class("okp-info-path");
        path_label.set_xalign(0.0);
        path_label.set_ellipsize(pango::EllipsizeMode::Middle);
        header.append(&path_label);
    }
    page.append(&header);

    if let Some(summary) = media_info_summary_widget(media_info) {
        page.append(&summary);
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("okp-info-content");
    for section in &media_info.sections {
        content.append(&media_info_section_widget(section));
    }
    if !media_info.tracks.is_empty() {
        content.append(&media_info_tracks_section(&media_info.tracks));
    }

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&content));
    page.append(&scroller);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    footer.add_css_class("okp-info-footer");

    let copy_button = media_info_action_button("Copy info", "edit-copy-symbolic");
    copy_button.add_css_class("okp-info-footer-button");
    let copy_text = Rc::new(media_info_copy_text(media_info));
    let copy_toast = Rc::clone(&status_toast);
    copy_button.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(copy_text.as_str());
            copy_toast.show("Media information copied");
        }
    });
    footer.append(&copy_button);

    let done_button = gtk::Button::with_label("Done");
    done_button.add_css_class("okp-info-footer-button");
    done_button.set_has_frame(false);
    done_button.set_halign(gtk::Align::End);
    done_button.set_hexpand(true);
    let close_window = window.clone();
    done_button.connect_clicked(move |_| close_window.close());
    footer.append(&done_button);
    page.append(&footer);
    root.append(&page);

    let content_overlay = gtk::Overlay::new();
    content_overlay.set_child(Some(&root));
    content_overlay.add_overlay(&captionless_window_drag_layer(&window));
    content_overlay.add_overlay(&settings_window_controls(&window));
    window.set_child(Some(&content_overlay));
    window.present();
}

fn media_info_action_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_has_frame(false);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));

    button
}

fn media_info_summary_widget(media_info: &MediaInfo) -> Option<gtk::Box> {
    let chips = media_info_summary_chips(media_info);
    if chips.is_empty() {
        return None;
    }

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    summary.add_css_class("okp-info-summary");
    summary.set_halign(gtk::Align::Start);
    for (label, value) in chips {
        summary.append(&media_info_summary_chip(label, &value));
    }
    Some(summary)
}

fn media_info_summary_chips(media_info: &MediaInfo) -> Vec<(&'static str, String)> {
    let mut chips = Vec::new();

    if let Some(container) = media_info_value(media_info, "File", "Container") {
        chips.push(("Container", container.to_owned()));
    }
    if let Some(duration) = media_info_value(media_info, "File", "Duration") {
        chips.push(("Duration", duration.to_owned()));
    }
    if let Some(resolution) = media_info_value(media_info, "Video", "Resolution") {
        chips.push(("Video", resolution.to_owned()));
    }
    if let Some(codec) = media_info_value(media_info, "Video", "Codec") {
        chips.push(("Codec", codec.to_owned()));
    }

    let audio_count = media_info
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .count();
    if audio_count > 0 {
        chips.push(("Audio", audio_count.to_string()));
    }

    let subtitle_count = media_info
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .count();
    if subtitle_count > 0 {
        chips.push(("Subtitles", subtitle_count.to_string()));
    }

    chips
}

fn media_info_value<'a>(
    media_info: &'a MediaInfo,
    section_title: &str,
    row_label: &str,
) -> Option<&'a str> {
    media_info
        .sections
        .iter()
        .find(|section| section.title == section_title)?
        .rows
        .iter()
        .find(|row| row.label == row_label)
        .map(|row| row.value.as_str())
}

fn media_info_summary_chip(label: &str, value: &str) -> gtk::Box {
    let chip = gtk::Box::new(gtk::Orientation::Vertical, 2);
    chip.add_css_class("okp-info-chip");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-chip-label");
    label.set_xalign(0.0);
    chip.append(&label);

    let value = gtk::Label::new(Some(value));
    value.add_css_class("okp-info-chip-value");
    value.set_xalign(0.0);
    value.set_ellipsize(pango::EllipsizeMode::End);
    value.set_max_width_chars(18);
    chip.append(&value);

    chip
}

fn media_info_section_widget(section: &InfoSection) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("okp-info-section");

    let section_title = section.title.to_uppercase();
    let title = gtk::Label::new(Some(&section_title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for row in &section.rows {
        content.append(&media_info_row(&row.label, &row.value));
    }

    content
}

fn media_info_row(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("okp-info-row");
    row.set_hexpand(true);

    let label_widget = gtk::Label::new(Some(label));
    label_widget.add_css_class("okp-info-label");
    label_widget.set_xalign(0.0);
    label_widget.set_width_chars(15);
    row.append(&label_widget);

    let value_widget = gtk::Label::new(Some(value));
    value_widget.add_css_class("okp-info-value");
    value_widget.set_xalign(0.0);
    value_widget.set_hexpand(true);
    value_widget.set_wrap(true);
    value_widget.set_wrap_mode(pango::WrapMode::WordChar);
    row.append(&value_widget);

    row
}

fn media_info_tracks_section(tracks: &[InfoTrack]) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("okp-info-section");

    let title = gtk::Label::new(Some("Tracks"));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for track in tracks {
        content.append(&media_info_track_row(track));
    }

    content
}

fn media_info_track_row(track: &InfoTrack) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-info-track-row");
    if track.selected {
        row.add_css_class("is-selected");
    }

    let kind_text = media_info_track_kind_label(track.kind).to_uppercase();
    let kind = gtk::Label::new(Some(&kind_text));
    kind.add_css_class("okp-info-track-kind");
    kind.set_width_chars(8);
    kind.set_xalign(0.0);
    row.append(&kind);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
    body.set_hexpand(true);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    title_row.set_hexpand(true);

    let title = gtk::Label::new(Some(&format!("#{} {}", track.id, track.title)));
    title.add_css_class("okp-info-track-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title_row.append(&title);

    if track.selected {
        let current = gtk::Label::new(Some("CURRENT"));
        current.add_css_class("okp-info-track-current");
        title_row.append(&current);
    }
    body.append(&title_row);

    if !track.detail.is_empty() {
        let detail = gtk::Label::new(Some(&track.detail));
        detail.add_css_class("okp-info-track-detail");
        detail.set_xalign(0.0);
        detail.set_wrap(true);
        detail.set_wrap_mode(pango::WrapMode::WordChar);
        body.append(&detail);
    }

    row.append(&body);
    row
}

fn media_info_track_kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitle",
    }
}

fn media_info_copy_text(media_info: &MediaInfo) -> String {
    let mut lines = vec![
        "OK Player Media Information".to_owned(),
        format!("App: OK Player {APP_BUILD_VERSION} ({APP_BUILD_SHA})"),
        "Platform: Linux GTK4 / libmpv".to_owned(),
        String::new(),
        media_info.title.clone(),
    ];
    if let Some(path) = media_info.path.as_deref() {
        lines.push(format!("Path: {path}"));
    }

    for section in &media_info.sections {
        lines.push(String::new());
        lines.push(section.title.clone());
        for row in &section.rows {
            lines.push(format!("{}: {}", row.label, row.value));
        }
    }

    if !media_info.tracks.is_empty() {
        lines.push(String::new());
        lines.push("Tracks".to_owned());
        for track in &media_info.tracks {
            let detail = if track.detail.is_empty() {
                String::new()
            } else {
                format!(" - {}", track.detail)
            };
            lines.push(format!(
                "{} #{}: {}{}",
                media_info_track_kind_label(track.kind),
                track.id,
                track.title,
                detail
            ));
        }
    }

    lines.join("\n")
}

fn seek_to_chapter(state: &Rc<RefCell<PlayerState>>, time: f64) {
    if time.is_finite() && time >= 0.0 {
        with_mpv(state, |mpv| mpv.seek_absolute(time));
    }
}

fn seek_to_time(state: &Rc<RefCell<PlayerState>>, time: f64) -> bool {
    time.is_finite() && time >= 0.0 && with_mpv(state, |mpv| mpv.seek_absolute(time))
}

fn toggle_fullscreen(window: &gtk::ApplicationWindow) {
    if window.is_fullscreen() {
        window.unfullscreen();
    } else {
        window.fullscreen();
    }
}

fn toggle_ab_loop(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let was_active = state.borrow().ab_loop.is_active();
    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| {
            mpv.toggle_ab_loop()?;
            mpv.ab_loop_state()
        })
    };

    match result {
        Some(Ok(ab_loop)) => {
            state.borrow_mut().ab_loop = ab_loop;
            if let Some(message) = ab_loop_message(ab_loop, was_active) {
                status_toast.show(&message);
            }
        }
        Some(Err(error)) => {
            eprintln!("Failed to toggle A-B loop: {error}");
            status_toast.show("Could not update A-B loop");
        }
        None => status_toast.show("Open media first"),
    }
}

fn sync_ab_loop_state(state: &Rc<RefCell<PlayerState>>, has_media: bool) {
    let ab_loop = if has_media {
        state
            .borrow()
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.ab_loop_state().ok())
            .unwrap_or_default()
    } else {
        AbLoopState::default()
    };
    state.borrow_mut().ab_loop = ab_loop;
}

fn ab_loop_message(ab_loop: AbLoopState, was_active: bool) -> Option<String> {
    match (ab_loop.a, ab_loop.b) {
        (Some(a), Some(b)) => Some(format!("A-B loop: {} - {}", format_time(a), format_time(b))),
        (Some(a), None) => Some(format!("A-B loop: start at {}", format_time(a))),
        (None, Some(b)) => Some(format!("A-B loop: end at {}", format_time(b))),
        (None, None) if was_active => Some("A-B loop cleared".to_owned()),
        _ => None,
    }
}

fn set_video_aspect(state: &Rc<RefCell<PlayerState>>, aspect: &str, status_toast: &StatusToast) {
    let aspect = video_aspect_value(aspect);
    if with_mpv(state, |mpv| mpv.set_video_aspect_override(aspect)) {
        state.borrow_mut().video_transform.set_aspect(aspect);
        if aspect == VIDEO_ASPECT_AUTO {
            status_toast.show("Aspect: Auto");
        } else {
            status_toast.show(&format!("Aspect: {aspect}"));
        }
    } else {
        status_toast.show("Could not update video");
    }
}

fn rotate_video_clockwise(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let rotation = {
        let state = state.borrow();
        (state.video_transform.rotation + 90).rem_euclid(360)
    };
    if with_mpv(state, |mpv| mpv.set_video_rotation(rotation)) {
        state.borrow_mut().video_transform.rotate_clockwise();
        status_toast.show("Rotated 90°");
    } else {
        status_toast.show("Could not rotate video");
    }
}

fn toggle_video_fill_screen(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let enabled = {
        let state = state.borrow();
        !state.video_transform.fill_screen
    };
    if with_mpv(state, |mpv| mpv.set_video_fill_screen(enabled)) {
        state.borrow_mut().video_transform.toggle_fill_screen();
        status_toast.show(if enabled {
            "Fill screen on"
        } else {
            "Fill screen off"
        });
    } else {
        status_toast.show("Could not update video");
    }
}

fn reset_video_transform(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    if with_mpv(state, |mpv| mpv.reset_video_transform()) {
        state.borrow_mut().video_transform.reset();
        status_toast.show("Video reset");
    } else {
        status_toast.show("Could not reset video");
    }
}

fn reset_video_transform_for_new_media(state: &mut PlayerState) {
    state.video_transform.reset();
    if let Some(mpv) = state.mpv.as_ref()
        && let Err(error) = mpv.reset_video_transform()
    {
        eprintln!("Failed to reset video transform: {error}");
    }
}

fn cycle_repeat_mode(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let repeat_mode = state.borrow().modes.repeat_mode.cycle();
    set_repeat_mode_from_ui(state, status_toast, repeat_mode);
}

fn set_repeat_mode_from_ui(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    repeat_mode: RepeatMode,
) {
    let mut state = state.borrow_mut();
    state.modes.repeat_mode = repeat_mode;
    let repeat = state.modes.repeat_mode.settings_value();
    state.settings.set_repeat_mode(repeat);
    save_settings_or_toast(&mut state, status_toast);
}

fn toggle_shuffle(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let enabled = !state.borrow().modes.shuffle_enabled;
    set_shuffle_from_ui(state, status_toast, enabled);
}

fn set_shuffle_from_ui(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    enabled: bool,
) {
    let mut state = state.borrow_mut();
    state.modes.shuffle_enabled = enabled;
    state.modes.reset_shuffle_order();

    if state.modes.shuffle_enabled
        && let Some(current_index) = current_playlist_index(&state)
    {
        let playlist_len = state.playlist.len();
        state
            .modes
            .ensure_shuffle_order(playlist_len, current_index);
    }
    state.settings.set_shuffle_enabled(enabled);
    save_settings_or_toast(&mut state, status_toast);
}

fn set_playback_speed_from_ui(state: &Rc<RefCell<PlayerState>>, speed: f64) {
    if with_mpv(state, |mpv| mpv.set_speed(speed)) {
        save_current_preferences(state);
    }
}

fn toggle_auto_advance(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let mut state = state.borrow_mut();
    state.modes.auto_advance_enabled = !state.modes.auto_advance_enabled;
    let enabled = state.modes.auto_advance_enabled;
    state.settings.set_auto_advance_enabled(enabled);
    save_settings_or_toast(&mut state, status_toast);
}

fn toggle_private_session(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let enabled = {
        let mut state = state.borrow_mut();
        state.private_session = !state.private_session;
        if state.private_session {
            state.pending_resume = None;
            state.pending_preferences = None;
        }
        state.private_session
    };

    status_toast.show(if enabled {
        "Private session on"
    } else {
        "Private session off"
    });
}

fn clear_history(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let mut state = state.borrow_mut();
    state.history.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
    match state.history.save() {
        Ok(()) => status_toast.show("History cleared"),
        Err(error) => {
            eprintln!("Failed to clear history: {error}");
            status_toast.show("Could not clear history");
        }
    }
}

fn close_current_media(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) -> bool {
    if !has_loaded_media(state) {
        return false;
    }

    save_current_progress(state, false);

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::stop)
    };

    match result {
        Some(Ok(())) | None => {
            clear_loaded_media_state(state);
            status_toast.show("Media closed");
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to close media: {error}");
            status_toast.show("Could not close media");
            false
        }
    }
}

fn clear_loaded_media_state(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.current_file = None;
    state.current_url = None;
    state.playlist.clear();
    state.modes.reset_shuffle_order();
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
    state.video_transform.reset();
    state.ab_loop = AbLoopState::default();
}

fn load_media_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    load_media_path_internal(state, path, true);
}

fn load_media_url(state: &Rc<RefCell<PlayerState>>, url: String) {
    if !is_media_url(&url) {
        return;
    }

    save_current_progress(state, false);

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => remember_loaded_url(state, url),
        Some(Err(error)) => eprintln!("Failed to load URL '{url}': {error}"),
        None => remember_loaded_url(state, url),
    }
}

fn load_media_path_internal(state: &Rc<RefCell<PlayerState>>, path: PathBuf, save_previous: bool) {
    if !is_media_path(&path) {
        return;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => remember_loaded_media(state, path),
        Some(Err(error)) => eprintln!("Failed to load media '{}': {error}", path.display()),
        None => remember_loaded_media(state, path),
    }
}

fn remember_loaded_media(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    let playlist = build_folder_playlist(&path);
    remember_loaded_media_with_playlist(state, path, playlist);
}

fn load_media_path_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    playlist: Vec<PlaylistItem>,
    save_previous: bool,
) -> bool {
    if !is_media_path(&path) {
        return false;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => {
            remember_loaded_media_with_playlist(state, path, playlist);
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to load media '{}': {error}", path.display());
            false
        }
        None => {
            remember_loaded_media_with_playlist(state, path, playlist);
            true
        }
    }
}

fn remember_loaded_media_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    playlist: Vec<PlaylistItem>,
) {
    let mut playlist = playlist
        .into_iter()
        .filter(|item| match item {
            PlaylistItem::Local(path) => is_media_path(path),
            PlaylistItem::Url(url) => is_media_url(url),
        })
        .collect::<Vec<_>>();
    if !playlist
        .iter()
        .any(|item| matches!(item, PlaylistItem::Local(item_path) if item_path == &path))
    {
        playlist.insert(0, PlaylistItem::Local(path.clone()));
    }
    let resume_path = path.clone();
    let preferences_path = path.clone();
    let mut state = state.borrow_mut();
    let resume = if state.private_session || !state.settings.resume_enabled() {
        None
    } else {
        state.history.resume_position(&path)
    };
    let preferences = if state.private_session {
        None
    } else {
        state.history.playback_preferences(&path)
    };
    let playlist_changed = state.playlist != playlist;
    reset_video_transform_for_new_media(&mut state);
    state.ab_loop = AbLoopState::default();
    state.current_file = Some(path);
    state.current_url = None;
    state.playlist = playlist;
    if playlist_changed {
        state.modes.reset_shuffle_order();
    }
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    if let Some(current_index) = current_playlist_index(&state) {
        let playlist_len = state.playlist.len();
        state
            .modes
            .ensure_shuffle_order(playlist_len, current_index);
    }
    state.pending_subtitles.clear();
    state.pending_resume = resume.map(|position| (resume_path, position));
    state.pending_preferences = preferences.map(|preferences| (preferences_path, preferences));
}

fn remember_loaded_url(state: &Rc<RefCell<PlayerState>>, url: String) {
    remember_loaded_url_with_playlist(state, url.clone(), vec![PlaylistItem::Url(url)]);
}

fn load_media_url_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    url: String,
    playlist: Vec<PlaylistItem>,
    save_previous: bool,
) -> bool {
    if !is_media_url(&url) {
        return false;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => {
            remember_loaded_url_with_playlist(state, url, playlist);
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to load URL '{url}': {error}");
            false
        }
        None => {
            remember_loaded_url_with_playlist(state, url, playlist);
            true
        }
    }
}

fn remember_loaded_url_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    url: String,
    playlist: Vec<PlaylistItem>,
) {
    let mut playlist = playlist
        .into_iter()
        .filter(|item| match item {
            PlaylistItem::Local(path) => is_media_path(path),
            PlaylistItem::Url(url) => is_media_url(url),
        })
        .collect::<Vec<_>>();
    if !playlist
        .iter()
        .any(|item| matches!(item, PlaylistItem::Url(item_url) if item_url == &url))
    {
        playlist.insert(0, PlaylistItem::Url(url.clone()));
    }

    let mut state = state.borrow_mut();
    let playlist_changed = state.playlist != playlist;
    reset_video_transform_for_new_media(&mut state);
    state.ab_loop = AbLoopState::default();
    state.current_file = None;
    state.current_url = Some(url);
    state.playlist = playlist;
    if playlist_changed {
        state.modes.reset_shuffle_order();
    }
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
    if let Some(current_index) = current_playlist_index(&state) {
        let playlist_len = state.playlist.len();
        state
            .modes
            .ensure_shuffle_order(playlist_len, current_index);
    }
}

fn load_playlist_item_with_playlist(
    state: &Rc<RefCell<PlayerState>>,
    item: PlaylistItem,
    playlist: Vec<PlaylistItem>,
    save_previous: bool,
) -> bool {
    match item {
        PlaylistItem::Local(path) => {
            load_media_path_with_playlist(state, path, playlist, save_previous)
        }
        PlaylistItem::Url(url) => load_media_url_with_playlist(state, url, playlist, save_previous),
    }
}

fn load_m3u_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: &Path,
    status_toast: &StatusToast,
) -> bool {
    let playlist = match read_m3u_playlist_items(path) {
        Ok(playlist) => playlist,
        Err(M3uPlaylistReadError::NotPlaylist) => {
            status_toast.show("Choose an M3U playlist");
            return false;
        }
        Err(M3uPlaylistReadError::ReadFailed) => {
            status_toast.show("Could not read playlist");
            return false;
        }
        Err(M3uPlaylistReadError::Empty) => {
            status_toast.show("Playlist has no playable media");
            return false;
        }
    };

    let count = playlist.len();
    if let Some(first_item) = playlist.first().cloned()
        && load_playlist_item_with_playlist(state, first_item, playlist, true)
    {
        status_toast.show(&format!("Playlist opened: {count} item{}", plural_s(count)));
        return true;
    }

    status_toast.show("Could not open playlist media");
    false
}

fn load_m3u_playlist_silent(state: &Rc<RefCell<PlayerState>>, path: &Path) -> bool {
    let Ok(playlist) = read_m3u_playlist_items(path) else {
        return false;
    };

    let Some(first_item) = playlist.first().cloned() else {
        return false;
    };
    load_playlist_item_with_playlist(state, first_item, playlist, true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum M3uPlaylistReadError {
    NotPlaylist,
    ReadFailed,
    Empty,
}

fn read_m3u_playlist_items(path: &Path) -> Result<Vec<PlaylistItem>, M3uPlaylistReadError> {
    if !is_playlist_path(path) {
        return Err(M3uPlaylistReadError::NotPlaylist);
    }

    let text = fs::read_to_string(path).map_err(|_| M3uPlaylistReadError::ReadFailed)?;
    let entries = m3u::parse(&text, path.parent());
    let playlist = playlist_items_from_m3u_entries(&entries);
    if playlist.is_empty() {
        Err(M3uPlaylistReadError::Empty)
    } else {
        Ok(playlist)
    }
}

fn save_m3u_playlist(
    state: &Rc<RefCell<PlayerState>>,
    path: PathBuf,
    status_toast: &StatusToast,
) -> bool {
    let paths = {
        let state = state.borrow();
        state
            .playlist
            .iter()
            .map(PlaylistItem::m3u_entry)
            .collect::<Vec<_>>()
    };

    if paths.is_empty() {
        status_toast.show("No playlist to save");
        return false;
    }

    let text = m3u::write(paths.iter().map(String::as_str));
    match fs::write(&path, text) {
        Ok(()) => {
            status_toast.show(&format!(
                "Playlist saved: {} item{}",
                paths.len(),
                plural_s(paths.len())
            ));
            true
        }
        Err(error) => {
            eprintln!("Failed to save playlist '{}': {error}", path.display());
            status_toast.show("Could not save playlist");
            false
        }
    }
}

fn queue_media_paths(
    state: &Rc<RefCell<PlayerState>>,
    paths: Vec<PathBuf>,
    mode: QueueInsertMode,
    status_toast: &StatusToast,
) -> bool {
    let additions = unique_media_paths(paths);
    if additions.is_empty() {
        status_toast.show("Choose media files");
        return false;
    }

    let count = {
        let mut state = state.borrow_mut();
        let Some(current_file) = state.current_file.clone() else {
            status_toast.show("Open local media first");
            return false;
        };
        let Some((playlist, count)) =
            queue_playlist_insert(state.playlist.clone(), &current_file, additions, mode)
        else {
            status_toast.show("Already in queue");
            return false;
        };

        state.playlist = playlist;
        state.modes.reset_shuffle_order();
        if let Some(current_index) = current_playlist_index(&state) {
            let playlist_len = state.playlist.len();
            state
                .modes
                .ensure_shuffle_order(playlist_len, current_index);
        }
        count
    };

    let action = match mode {
        QueueInsertMode::Append => "Queued",
        QueueInsertMode::PlayNext => "Will play next",
    };
    status_toast.show(&format!("{action}: {count} item{}", plural_s(count)));
    true
}

fn playlist_items_from_m3u_entries(entries: &[String]) -> Vec<PlaylistItem> {
    entries
        .iter()
        .filter_map(|entry| PlaylistItem::from_m3u_entry(entry))
        .collect()
}

fn playlist_save_path(mut path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_none_or(|extension| extension.is_empty())
    {
        path.set_extension("m3u");
    }
    path
}

fn plural_s(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn unique_media_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if is_media_path(&path) && !unique.iter().any(|existing| existing == &path) {
            unique.push(path);
        }
    }
    unique
}

fn queue_playlist_insert(
    mut playlist: Vec<PlaylistItem>,
    current_file: &Path,
    additions: Vec<PathBuf>,
    mode: QueueInsertMode,
) -> Option<(Vec<PlaylistItem>, usize)> {
    if playlist.is_empty() {
        playlist.push(PlaylistItem::Local(current_file.to_path_buf()));
    }
    if !playlist
        .iter()
        .any(|item| matches!(item, PlaylistItem::Local(path) if path.as_path() == current_file))
    {
        playlist.insert(0, PlaylistItem::Local(current_file.to_path_buf()));
    }

    let additions = additions
        .into_iter()
        .filter(|path| path.as_path() != current_file)
        .collect::<Vec<_>>();
    if additions.is_empty() {
        return None;
    }

    match mode {
        QueueInsertMode::Append => {
            let additions = additions
                .into_iter()
                .filter(|path| {
                    !playlist.iter().any(
                        |item| matches!(item, PlaylistItem::Local(item_path) if item_path == path),
                    )
                })
                .map(PlaylistItem::Local)
                .collect::<Vec<_>>();
            if additions.is_empty() {
                return None;
            }

            let count = additions.len();
            playlist.extend(additions);
            Some((playlist, count))
        }
        QueueInsertMode::PlayNext => {
            playlist.retain(|item| {
                !additions
                    .iter()
                    .any(|addition| matches!(item, PlaylistItem::Local(path) if path == addition))
            });
            let current_index = playlist
                .iter()
                .position(|item| matches!(item, PlaylistItem::Local(path) if path.as_path() == current_file))
                .unwrap_or(0);
            let count = additions.len();
            playlist.splice(
                current_index + 1..current_index + 1,
                additions.into_iter().map(PlaylistItem::Local),
            );
            Some((playlist, count))
        }
    }
}

fn navigate_playlist(state: &Rc<RefCell<PlayerState>>, direction: isize) -> bool {
    let Some(item) = playlist_target_item(state, direction, true) else {
        return false;
    };
    let playlist = state.borrow().playlist.clone();

    load_playlist_item_with_playlist(state, item, playlist, true)
}

fn jump_playlist_index(state: &Rc<RefCell<PlayerState>>, index: usize) -> bool {
    let (item, playlist) = {
        let state = state.borrow();
        (state.playlist.get(index).cloned(), state.playlist.clone())
    };

    let Some(item) = item else {
        return false;
    };

    {
        let mut state = state.borrow_mut();
        if state.modes.shuffle_enabled {
            state.modes.shuffle_cursor = state
                .modes
                .shuffle_order
                .iter()
                .position(|item| *item == index);
        }
    }

    load_playlist_item_with_playlist(state, item, playlist, true)
}

fn advance_playlist_on_eof(state: &Rc<RefCell<PlayerState>>) -> bool {
    let repeat_mode = state.borrow().modes.repeat_mode;
    if repeat_mode == RepeatMode::One {
        return restart_current_file(state);
    }

    if !state.borrow().modes.auto_advance_enabled {
        return false;
    }

    let wrap = repeat_mode == RepeatMode::All;
    let Some(next_item) = playlist_target_item(state, 1, wrap) else {
        return false;
    };
    let playlist = state.borrow().playlist.clone();

    load_playlist_item_with_playlist(state, next_item, playlist, false)
}

fn move_playlist_item(state: &Rc<RefCell<PlayerState>>, from: usize, to: usize) -> bool {
    let mut state = state.borrow_mut();
    let Some(playlist) = reorder_playlist(state.playlist.clone(), from, to) else {
        return false;
    };
    state.playlist = playlist;
    state.modes.reset_shuffle_order();
    true
}

fn remove_playlist_item(state: &Rc<RefCell<PlayerState>>, index: usize) -> bool {
    let mut state = state.borrow_mut();
    if state.playlist.get(index).is_some_and(|item| {
        item.is_current(state.current_file.as_deref(), state.current_url.as_deref())
    }) {
        return false;
    }
    let Some(playlist) = remove_playlist_index(state.playlist.clone(), index) else {
        return false;
    };
    state.playlist = playlist;
    state.modes.reset_shuffle_order();
    true
}

fn reorder_playlist(
    mut playlist: Vec<PlaylistItem>,
    from: usize,
    to: usize,
) -> Option<Vec<PlaylistItem>> {
    if from >= playlist.len() || from == to {
        return None;
    }
    let item = playlist.remove(from);
    let target = to.min(playlist.len());
    playlist.insert(target, item);
    Some(playlist)
}

fn remove_playlist_index(
    mut playlist: Vec<PlaylistItem>,
    index: usize,
) -> Option<Vec<PlaylistItem>> {
    if playlist.len() <= 1 || index >= playlist.len() {
        return None;
    }
    playlist.remove(index);
    Some(playlist)
}

fn restart_current_file(state: &Rc<RefCell<PlayerState>>) -> bool {
    let path = {
        let state = state.borrow();
        let Some(path) = state.current_file.clone() else {
            return false;
        };
        let Some(mpv) = state.mpv.as_ref() else {
            return false;
        };
        if let Err(error) = mpv.load_file(&path) {
            eprintln!("Failed to repeat '{}': {error}", path.display());
            return false;
        }
        path
    };

    let preferences = state.borrow().history.playback_preferences(&path);
    let mut state = state.borrow_mut();
    state.pending_resume = None;
    state.pending_preferences = preferences.map(|preferences| (path, preferences));
    true
}

fn playlist_target_item(
    state: &Rc<RefCell<PlayerState>>,
    direction: isize,
    wrap: bool,
) -> Option<PlaylistItem> {
    let mut state = state.borrow_mut();
    if state.playlist.len() < 2 {
        return None;
    }

    let current_index = current_playlist_index(&state).unwrap_or(0);
    let next_index = if state.modes.shuffle_enabled {
        shuffled_target_index(&mut state, current_index, direction, wrap)?
    } else {
        ordered_target_index(state.playlist.len(), current_index, direction, wrap)?
    };

    state.playlist.get(next_index).cloned()
}

fn ordered_target_index(
    playlist_len: usize,
    current_index: usize,
    direction: isize,
    wrap: bool,
) -> Option<usize> {
    let target = current_index as isize + direction;
    if wrap {
        Some(target.rem_euclid(playlist_len as isize) as usize)
    } else if (0..playlist_len as isize).contains(&target) {
        Some(target as usize)
    } else {
        None
    }
}

fn shuffled_target_index(
    state: &mut PlayerState,
    current_index: usize,
    direction: isize,
    wrap: bool,
) -> Option<usize> {
    let playlist_len = state.playlist.len();
    state
        .modes
        .ensure_shuffle_order(playlist_len, current_index);
    let cursor = state.modes.shuffle_cursor.unwrap_or(0);
    let target_cursor =
        ordered_target_index(state.modes.shuffle_order.len(), cursor, direction, wrap)?;
    state.modes.shuffle_cursor = Some(target_cursor);
    state.modes.shuffle_order.get(target_cursor).copied()
}

fn current_playlist_index(state: &PlayerState) -> Option<usize> {
    state.playlist.iter().position(|item| {
        item.is_current(state.current_file.as_deref(), state.current_url.as_deref())
    })
}

fn try_pending_resume(state: &Rc<RefCell<PlayerState>>, duration: f64) {
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }

    let pending = {
        let state = state.borrow();
        state.pending_resume.clone()
    };
    let Some((path, target)) = pending else {
        return;
    };

    let is_current = state
        .borrow()
        .current_file
        .as_ref()
        .is_some_and(|current| current == &path);
    if !is_current {
        state.borrow_mut().pending_resume = None;
        return;
    }

    if target > duration {
        return;
    }

    if target <= duration * 0.05 || target >= history::completion_start(duration) {
        state.borrow_mut().pending_resume = None;
        return;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.seek_absolute(target))
    };
    if matches!(result, Some(Ok(()))) {
        state.borrow_mut().pending_resume = None;
    } else if let Some(Err(error)) = result {
        eprintln!("Failed to resume '{}': {error}", path.display());
    }
}

fn try_pending_playback_preferences(state: &Rc<RefCell<PlayerState>>) {
    let pending = {
        let state = state.borrow();
        state.pending_preferences.clone()
    };
    let Some((path, preferences)) = pending else {
        return;
    };

    let is_current = state
        .borrow()
        .current_file
        .as_ref()
        .is_some_and(|current| current == &path);
    if !is_current {
        state.borrow_mut().pending_preferences = None;
        return;
    }

    let result = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| apply_playback_preferences(mpv, &preferences))
    };

    match result {
        Some(Ok(())) => state.borrow_mut().pending_preferences = None,
        Some(Err(error)) => eprintln!("Failed to restore playback preferences: {error}"),
        None => {}
    }
}

fn apply_playback_preferences(
    mpv: &Mpv,
    preferences: &history::PlaybackPreferences,
) -> Result<(), okp_mpv::MpvError> {
    let tracks = mpv.tracks()?;

    if let Some(enabled) = preferences.audio_enabled {
        if !enabled {
            mpv.select_audio(None)?;
        } else if let Some(track_id) = preferences.audio_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Audio && track.id == track_id)
        {
            mpv.select_audio(Some(track_id))?;
        }
    }

    if let Some(enabled) = preferences.subtitle_enabled {
        if !enabled {
            mpv.select_subtitle(None)?;
        } else if let Some(track_id) = preferences.subtitle_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.id == track_id)
        {
            mpv.select_subtitle(Some(track_id))?;
        }
    }

    if let Some(enabled) = preferences.secondary_subtitle_enabled {
        if !enabled {
            mpv.select_secondary_subtitle(None)?;
        } else if let Some(track_id) = preferences.secondary_subtitle_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.id == track_id)
        {
            mpv.select_secondary_subtitle(Some(track_id))?;
        }
    }

    if let Some(delay) = preferences.subtitle_delay.and_then(finite_option) {
        mpv.set_subtitle_delay(delay)?;
    }
    if let Some(scale) = preferences.subtitle_scale.and_then(finite_option) {
        mpv.set_subtitle_scale(scale)?;
    }
    if let Some(speed) = preferences.speed.and_then(finite_option) {
        mpv.set_speed(speed)?;
    }

    Ok(())
}

fn save_current_preferences(state: &Rc<RefCell<PlayerState>>) {
    let snapshot = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(preferences) = state.mpv.as_ref().map(read_current_playback_preferences) else {
            return;
        };

        (path, preferences)
    };

    let (path, preferences) = snapshot;
    let mut state = state.borrow_mut();
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save playback preferences: {error}");
    }
}

fn read_current_playback_preferences(mpv: &Mpv) -> history::PlaybackPreferences {
    let tracks = mpv.tracks().unwrap_or_else(|error| {
        eprintln!("Failed to read tracks for preferences: {error}");
        Vec::new()
    });
    let selected_audio = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Audio && track.selected);
    let selected_subtitle = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Subtitle && track.selected);
    let secondary_subtitle_id = mpv.secondary_subtitle_id().ok().flatten().filter(|id| {
        tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.id == *id)
    });
    let has_audio_tracks = tracks.iter().any(|track| track.kind == TrackKind::Audio);
    let has_subtitle_tracks = tracks.iter().any(|track| track.kind == TrackKind::Subtitle);

    history::PlaybackPreferences {
        audio_enabled: has_audio_tracks.then_some(selected_audio.is_some()),
        audio_track_id: selected_audio.map(|track| track.id),
        subtitle_enabled: has_subtitle_tracks.then_some(selected_subtitle.is_some()),
        subtitle_track_id: selected_subtitle.map(|track| track.id),
        secondary_subtitle_enabled: has_subtitle_tracks.then_some(secondary_subtitle_id.is_some()),
        secondary_subtitle_track_id: secondary_subtitle_id,
        subtitle_delay: mpv.subtitle_delay().ok().and_then(finite_option),
        subtitle_scale: mpv.subtitle_scale().ok().and_then(finite_option),
        speed: mpv.speed().ok().and_then(finite_option),
    }
}

fn finite_option(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn read_playback_speed(state: &Rc<RefCell<PlayerState>>) -> f64 {
    state
        .borrow()
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.speed().ok())
        .and_then(finite_option)
        .unwrap_or(1.0)
}

fn format_speed(speed: f64) -> String {
    format!("{:.2}x", speed.clamp(0.25, 4.0))
}

fn speed_matches(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.005
}

fn video_aspect_value(value: &str) -> &'static str {
    VIDEO_ASPECT_PRESETS
        .iter()
        .find_map(|(_, preset)| (*preset == value).then_some(*preset))
        .unwrap_or(VIDEO_ASPECT_AUTO)
}

fn save_current_progress(state: &Rc<RefCell<PlayerState>>, finished: bool) {
    let snapshot = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(playback) = state.mpv.as_ref().and_then(|mpv| mpv.playback_state().ok()) else {
            return;
        };
        let preferences = state
            .mpv
            .as_ref()
            .map(read_current_playback_preferences)
            .unwrap_or_default();

        (path, playback, preferences)
    };

    let (path, playback, preferences) = snapshot;
    let Some(duration) = playback.duration else {
        return;
    };
    let position = playback.time_pos.unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 || !position.is_finite() {
        return;
    }

    let mut state = state.borrow_mut();
    state
        .history
        .record(&path, position.clamp(0.0, duration), duration, finished);
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save history: {error}");
    }
}

fn build_folder_playlist(path: &Path) -> Vec<PlaylistItem> {
    let Some(parent) = path.parent() else {
        return vec![PlaylistItem::Local(path.to_path_buf())];
    };

    let files = media_paths_in_directory(parent);
    if files.is_empty() {
        return vec![PlaylistItem::Local(path.to_path_buf())];
    };

    files.into_iter().map(PlaylistItem::Local).collect()
}

fn media_paths_in_directory(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_media_path(path))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        let left = left.file_name().and_then(|name| name.to_str());
        let right = right.file_name().and_then(|name| name.to_str());
        natural_compare::compare(left, right)
    });
    files
}

fn load_subtitle_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) -> bool {
    if !is_subtitle_path(&path) || !has_loaded_media(state) {
        return false;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.add_subtitle_file(&path))
    };

    match result {
        Some(Ok(())) => true,
        Some(Err(error)) => {
            eprintln!(
                "Subtitle queued until media is ready '{}': {error}",
                path.display()
            );
            state.borrow_mut().pending_subtitles.push(path);
            false
        }
        None => false,
    }
}

fn try_pending_subtitles(state: &Rc<RefCell<PlayerState>>) {
    let pending = {
        let mut state = state.borrow_mut();
        if !has_loaded_media_state(&state) || state.pending_subtitles.is_empty() {
            return;
        }

        std::mem::take(&mut state.pending_subtitles)
    };

    let mut retry = Vec::new();
    for path in pending {
        let result = {
            let state = state.borrow();
            state.mpv.as_ref().map(|mpv| mpv.add_subtitle_file(&path))
        };

        if !matches!(result, Some(Ok(()))) {
            retry.push(path);
        }
    }

    if !retry.is_empty() {
        state.borrow_mut().pending_subtitles.extend(retry);
    }
}

fn is_media_path(path: &Path) -> bool {
    media_formats::is_media(path)
}

fn is_media_url(url: &str) -> bool {
    media_formats::is_playable_url(Some(url))
}

fn is_subtitle_path(path: &Path) -> bool {
    media_formats::is_subtitle(path)
}

fn is_playlist_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            extension == "m3u" || extension == "m3u8"
        })
        .unwrap_or(false)
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00".to_owned();
    }

    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn shuffle_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

fn next_shuffle_value(seed: &mut u64) -> u64 {
    let mut value = (*seed).max(1);
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *seed = value;
    value
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .okp-root {
            background: #050507;
        }

        window.okp-player-window {
            background: #050507;
        }

        .okp-window-chrome {
            min-height: 40px;
            background: transparent;
        }

        .okp-window-drag-zone {
            min-height: 40px;
            background: transparent;
        }

        .okp-player-window-controls {
            min-height: 32px;
            border-radius: 12px;
            background: rgba(14, 15, 18, 0.42);
            border: 1px solid rgba(255, 255, 255, 0.07);
        }

        .okp-player-window-controls button,
        button.okp-player-window-control {
            min-width: 42px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.84);
            font-size: 15px;
            font-weight: 400;
        }

        .okp-player-window-controls button:hover,
        button.okp-player-window-control:hover {
            background: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.96);
        }

        label.okp-player-window-control-glyph {
            color: rgba(255, 255, 255, 0.86);
            font-size: 15px;
            font-weight: 400;
        }

        button.okp-player-window-control:hover label.okp-player-window-control-glyph {
            color: rgba(255, 255, 255, 0.98);
        }

        .okp-player-window-controls button:active,
        button.okp-player-window-control:active {
            background: rgba(255, 255, 255, 0.18);
        }

        button.okp-player-window-close:hover {
            background: rgba(219, 59, 59, 0.86);
            color: #ffffff;
        }

        .okp-resize-handle {
            background: transparent;
        }

        .okp-resize-edge-horizontal {
            min-height: 6px;
        }

        .okp-resize-edge-vertical {
            min-width: 6px;
        }

        .okp-resize-corner {
            min-width: 16px;
            min-height: 16px;
        }

        .okp-video-plane {
            background: #050507;
        }

        .okp-empty-surface {
            background: rgba(5, 5, 7, 0.94);
        }

        .okp-empty-panel {
            min-width: 300px;
            padding: 28px;
            border-radius: 8px;
            border: 1px solid rgba(255, 255, 255, 0.12);
            background: rgba(18, 19, 23, 0.84);
        }

        .okp-empty-panel.is-drop-target {
            border-color: rgba(40, 179, 170, 0.82);
            background: rgba(22, 48, 49, 0.92);
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.18);
        }

        .okp-empty-logo {
            color: #28b3aa;
        }

        .okp-empty-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 24px;
            font-weight: 750;
        }

        .okp-empty-primary-button,
        .okp-empty-secondary-button {
            min-height: 36px;
            padding: 6px 14px;
            border-radius: 7px;
        }

        .okp-empty-primary-button {
            background: #28b3aa;
            color: #051011;
        }

        .okp-empty-secondary-button {
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.86);
        }

        .okp-controls {
            padding: 8px 10px;
            border-radius: 18px;
            background: rgba(13, 14, 18, 0.86);
            border: 1px solid rgba(255, 255, 255, 0.11);
            box-shadow: 0 18px 48px rgba(0, 0, 0, 0.48);
        }

        .okp-control-group {
            padding: 3px;
            border-radius: 14px;
            background: rgba(255, 255, 255, 0.045);
            border: 1px solid rgba(255, 255, 255, 0.055);
        }

        .okp-transport-group {
            padding: 0;
            border-radius: 12px;
            background: transparent;
        }

        .okp-timeline-group {
            min-height: 36px;
            padding: 0 2px;
        }

        button.okp-control-button,
        menubutton.okp-control-button > button {
            min-width: 34px;
            min-height: 32px;
            padding: 0 9px;
            border-radius: 9px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.86);
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-control-button:hover,
        menubutton.okp-control-button > button:hover {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.96);
        }

        button.okp-control-button:active,
        menubutton.okp-control-button > button:active,
        button.okp-control-button:checked,
        menubutton.okp-control-button > button:checked {
            background: rgba(40, 179, 170, 0.24);
            border-color: rgba(40, 179, 170, 0.42);
            color: rgba(255, 255, 255, 0.98);
        }

        button.okp-control-button:disabled,
        menubutton.okp-control-button > button:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(255, 255, 255, 0.32);
        }

        button.okp-play-button {
            min-width: 42px;
            border-radius: 11px;
            background: rgba(40, 179, 170, 0.92);
            color: #ffffff;
        }

        button.okp-play-button:hover {
            background: rgba(55, 207, 197, 0.96);
        }

        button.okp-play-button:disabled {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.34);
        }

        button.okp-transport-button {
            min-width: 34px;
        }

        button.okp-chip-button,
        menubutton.okp-chip-button > button {
            min-width: 48px;
            background: rgba(255, 255, 255, 0.055);
        }

        button.okp-icon-button,
        menubutton.okp-icon-button > button {
            min-width: 34px;
            padding: 0;
        }

        menubutton.okp-speed-chip > button {
            min-width: 56px;
            background: rgba(255, 255, 255, 0.08);
            color: rgba(40, 179, 170, 0.98);
            font-feature-settings: 'tnum';
        }

        .okp-control-button.is-selected {
            background: rgba(40, 179, 170, 0.22);
        }

        .okp-time-label {
            min-width: 50px;
            color: rgba(255, 255, 255, 0.84);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-status-toast {
            padding: 8px 12px;
            border-radius: 8px;
            background: rgba(14, 15, 18, 0.9);
            box-shadow: 0 12px 34px rgba(0, 0, 0, 0.38);
            color: rgba(255, 255, 255, 0.9);
            font-size: 13px;
            font-weight: 600;
        }

        .okp-seek {
            min-width: 120px;
        }

        scale.okp-seek trough,
        scale.okp-volume trough {
            min-height: 3px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.23);
            border: none;
        }

        scale.okp-seek highlight,
        scale.okp-volume highlight {
            min-height: 3px;
            border-radius: 999px;
            background: #28b3aa;
        }

        scale.okp-seek slider,
        scale.okp-volume slider {
            min-width: 13px;
            min-height: 13px;
            margin: -5px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.96);
            box-shadow: 0 2px 8px rgba(0, 0, 0, 0.42);
        }

        scale.okp-seek mark indicator {
            min-width: 2px;
            min-height: 8px;
            border-radius: 999px;
            background: rgba(40, 179, 170, 0.84);
        }

        scale.okp-seek mark label {
            color: rgba(40, 179, 170, 0.90);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 700;
        }

        .okp-seek-preview {
            padding: 7px 10px;
            border-radius: 7px;
            background: rgba(14, 15, 18, 0.92);
            box-shadow: 0 10px 28px rgba(0, 0, 0, 0.34);
        }

        .okp-seek-preview-thumb {
            margin-bottom: 6px;
            border-radius: 5px;
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-seek-preview-time {
            color: rgba(255, 255, 255, 0.92);
            font-size: 12px;
            font-weight: 700;
            font-feature-settings: 'tnum';
        }

        .okp-seek-preview-chapter {
            margin-top: 2px;
            color: rgba(255, 255, 255, 0.62);
            font-size: 11px;
        }

        .okp-volume {
            min-width: 92px;
        }

        .okp-up-next-panel {
            padding: 12px;
            border-radius: 12px;
            background: rgba(12, 13, 17, 0.94);
            border: 1px solid rgba(255, 255, 255, 0.10);
            box-shadow: 0 22px 58px rgba(0, 0, 0, 0.48);
        }

        .okp-side-panel-header {
            padding: 2px 2px 4px 2px;
        }

        .okp-up-next-title {
            color: rgba(255, 255, 255, 0.94);
            font-size: 17px;
            font-weight: 760;
        }

        .okp-up-next-summary {
            color: rgba(255, 255, 255, 0.54);
            font-size: 11.5px;
        }

        .okp-side-panel-tabs {
            margin-top: 6px;
            padding: 3px;
            border-radius: 10px;
            background: rgba(255, 255, 255, 0.055);
            border: 1px solid rgba(255, 255, 255, 0.055);
        }

        button.okp-side-panel-tab {
            min-height: 30px;
            padding: 0 10px;
            border-radius: 8px;
            border: none;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.64);
            font-size: 12px;
            font-weight: 650;
        }

        button.okp-side-panel-tab:hover {
            background: rgba(255, 255, 255, 0.07);
            color: rgba(255, 255, 255, 0.86);
        }

        button.okp-side-panel-tab.is-selected {
            background: rgba(40, 179, 170, 0.22);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-up-next-list {
            background: transparent;
        }

        .okp-up-next-list row {
            background: transparent;
        }

        .okp-panel-heading-row {
            padding: 8px 10px 3px 10px;
        }

        .okp-panel-heading {
            color: rgba(255, 255, 255, 0.42);
            font-size: 10.5px;
            font-weight: 720;
        }

        .okp-panel-empty-row {
            min-height: 58px;
            padding: 14px 12px;
            border-radius: 10px;
            background: rgba(255, 255, 255, 0.055);
            border: 1px solid rgba(255, 255, 255, 0.055);
        }

        .okp-panel-empty {
            color: rgba(255, 255, 255, 0.58);
            font-size: 12px;
        }

        .okp-up-next-row {
            min-height: 42px;
            margin: 2px 0;
            padding: 9px 10px;
            border-radius: 9px;
            border: 1px solid transparent;
            background: rgba(255, 255, 255, 0.035);
            color: rgba(255, 255, 255, 0.78);
        }

        .okp-chapter-thumb {
            min-width: 88px;
            min-height: 50px;
            border-radius: 7px;
            background: rgba(255, 255, 255, 0.08);
            border: 1px solid rgba(255, 255, 255, 0.06);
        }

        .okp-up-next-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-up-next-row.is-current {
            background: rgba(40, 179, 170, 0.18);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-up-next-row.is-drop-target {
            background: rgba(40, 179, 170, 0.22);
            border-color: rgba(40, 179, 170, 0.62);
        }

        .okp-up-next-drag-handle {
            min-width: 18px;
            color: rgba(255, 255, 255, 0.34);
        }

        .okp-up-next-drag-handle-icon {
            -gtk-icon-size: 16px;
        }

        .okp-up-next-row:hover .okp-up-next-drag-handle,
        .okp-up-next-row.is-drop-target .okp-up-next-drag-handle {
            color: rgba(255, 255, 255, 0.78);
        }

        .okp-up-next-marker {
            color: rgba(40, 179, 170, 0.98);
            font-size: 11px;
            font-weight: 760;
        }

        .okp-up-next-file {
            color: inherit;
            font-size: 13px;
        }

        .okp-up-next-actions {
            min-width: 104px;
        }

        button.okp-up-next-action-button {
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            border: none;
            border-radius: 5px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.58);
        }

        button.okp-up-next-action-button:hover {
            background: rgba(255, 255, 255, 0.10);
            color: rgba(255, 255, 255, 0.90);
        }

        button.okp-up-next-action-button:disabled {
            color: rgba(255, 255, 255, 0.18);
        }

        .okp-up-next-panel scrolledwindow {
            background: transparent;
        }

        .okp-up-next-panel scrollbar,
        .okp-up-next-panel scrollbar trough {
            background: transparent;
            border: none;
        }

        .okp-up-next-panel scrollbar slider {
            min-width: 4px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.22);
        }

        .okp-track-popover-content {
            padding: 10px;
            background: #121317;
        }

        popover.okp-track-popover {
            padding: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        popover.okp-track-popover > contents,
        popover.okp-track-popover contents {
            padding: 0;
            border-radius: 10px;
            background: #121317;
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 18px 46px rgba(0, 0, 0, 0.46);
        }

        popover.okp-track-popover arrow {
            min-width: 0;
            min-height: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        .okp-track-popover-scroll {
            background: #121317;
        }

        .okp-track-popover-title {
            margin: 0 4px 6px 4px;
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 700;
        }

        button.okp-track-row {
            min-height: 34px;
            padding: 7px 9px;
            border-radius: 7px;
            background: transparent;
            border: none;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.82);
        }

        button.okp-track-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        button.okp-track-row.is-selected {
            background: rgba(40, 179, 170, 0.18);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-track-empty {
            margin: 6px 9px;
            color: rgba(255, 255, 255, 0.55);
            font-size: 13px;
        }

        .okp-track-divider {
            margin: 5px 3px;
        }

        .okp-sub-adjust-row {
            margin: 0 2px;
        }

        .okp-sub-adjust-label {
            color: rgba(255, 255, 255, 0.62);
            font-size: 12px;
        }

        .okp-sub-adjust-value {
            color: rgba(255, 255, 255, 0.9);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        entry.okp-sub-adjust-entry {
            min-width: 74px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            border: 1px solid rgba(255, 255, 255, 0.14);
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.9);
            font-feature-settings: 'tnum';
        }

        entry.okp-sub-adjust-entry:focus {
            border-color: rgba(40, 179, 170, 0.72);
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.16);
        }

        entry.okp-sub-adjust-entry.is-error {
            border-color: rgba(255, 104, 104, 0.88);
            box-shadow: 0 0 0 2px rgba(255, 104, 104, 0.18);
        }

        .okp-sub-adjust-unit {
            color: rgba(255, 255, 255, 0.58);
            font-size: 12px;
        }

        .okp-sub-adjust-button {
            min-width: 44px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.86);
        }

        .okp-sub-adjust-button:hover {
            background: rgba(255, 255, 255, 0.13);
        }

        .okp-info-window {
            background: #eef4f9;
        }

        window.okp-command-dialog {
            background: #101115;
            color: rgba(255, 255, 255, 0.9);
            border-radius: 8px;
        }

        window.okp-command-dialog > contents {
            background: #101115;
        }

        .okp-command-dialog-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 16px;
            font-weight: 700;
        }

        window.okp-command-dialog entry {
            min-height: 34px;
            border-radius: 7px;
            border: 1px solid rgba(40, 179, 170, 0.42);
            background: rgba(255, 255, 255, 0.055);
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        window.okp-command-dialog button {
            min-width: 72px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            border: 1px solid rgba(255, 255, 255, 0.12);
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.9);
            box-shadow: none;
        }

        window.okp-command-dialog button:hover {
            background: rgba(255, 255, 255, 0.13);
            color: rgba(255, 255, 255, 0.98);
        }

        window.okp-command-dialog button:active {
            background: rgba(40, 179, 170, 0.28);
            border-color: rgba(40, 179, 170, 0.48);
        }

        window.okp-command-dialog .okp-info-label {
            color: rgba(255, 255, 255, 0.62);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 500;
        }

        .okp-settings-window {
            background: transparent;
        }

        window.okp-settings-window > contents,
        window.okp-info-window > contents {
            background: transparent;
            box-shadow: none;
            border: none;
        }

        window.okp-settings-window headerbar,
        window.okp-info-window headerbar,
        window.okp-settings-window decoration,
        window.okp-info-window decoration {
            min-height: 0;
            margin: 0;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
        }

        .okp-info-root {
            background: #eef4f9;
            color: #161616;
        }

        .okp-settings-root {
            background: #eef4f9;
            color: #161616;
            border: none;
            border-radius: 0;
        }

        .okp-settings-rail-frame {
            background: #eaf0f5;
        }

        .okp-settings-rail {
            padding: 16px 10px 14px 10px;
            background: #eaf0f5;
            border-right: 1px solid #dde3e7;
        }

        .okp-settings-rail-title {
            margin-left: 5px;
            margin-bottom: 20px;
            color: #3b3f42;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-search {
            min-height: 16px;
            margin-bottom: 11px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #f9fbfc;
            border: 1px solid #d5dce2;
            color: #6c747a;
        }

        .okp-settings-search-label {
            color: #6c747a;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        entry.okp-shortcuts-search {
            min-height: 30px;
            margin-bottom: 2px;
            padding: 6px 10px;
            border-radius: 7px;
            background: #f9fbfc;
            border: 1px solid #d5dce2;
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        entry.okp-shortcuts-search:focus {
            border-color: rgba(0, 103, 192, 0.68);
            box-shadow: 0 0 0 1px rgba(0, 103, 192, 0.18);
        }

        .okp-settings-nav-row {
            min-height: 18px;
            padding: 8px 10px;
            border: none;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
            color: #3f464b;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-nav-row:hover {
            background: rgba(0, 0, 0, 0.035);
        }

        .okp-settings-nav-row.is-selected {
            background: #cfe5e8;
            box-shadow: inset 3px 0 0 #10938a;
            color: #0a655f;
            font-weight: 600;
        }

        .okp-settings-nav-icon {
            min-width: 16px;
            min-height: 16px;
            color: inherit;
        }

        .okp-settings-rail-divider {
            margin: 6px 9px 8px;
            background: #dbe2e7;
        }

        .okp-captionless-window-drag-layer {
            min-height: 32px;
            background: transparent;
        }

        .okp-settings-window-controls {
            min-height: 32px;
        }

        .okp-settings-window-control {
            min-width: 48px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
            color: #161616;
        }

        .okp-settings-window-control:hover {
            background: rgba(0, 0, 0, 0.06);
        }

        .okp-settings-window-control-glyph {
            min-width: 10px;
            min-height: 10px;
            color: #161616;
        }

        button.okp-settings-window-control:hover .okp-settings-window-control-glyph {
            color: #161616;
        }

        button.okp-settings-window-close:hover {
            background: #c42b1c;
        }

        button.okp-settings-window-close:hover .okp-settings-window-control-glyph {
            color: #ffffff;
        }

        .okp-settings-stack {
            background: #eef4f9;
        }

        .okp-settings-scroller {
            background: #eef4f9;
        }

        .okp-settings-page {
            padding: 70px 44px 28px 24px;
        }

        .okp-info-page {
            background: #eef4f9;
        }

        .okp-info-hero {
            min-height: 82px;
        }

        .okp-info-eyebrow {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-title {
            color: #161616;
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 28px;
            font-weight: 650;
        }

        .okp-info-path {
            color: rgba(0, 0, 0, 0.46);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-info-content {
            padding-right: 4px;
        }

        window.okp-info-window scrolledwindow {
            background: #eef4f9;
        }

        window.okp-info-window scrollbar {
            background: transparent;
            border: none;
        }

        window.okp-info-window scrollbar trough {
            background: transparent;
            border: none;
        }

        window.okp-info-window scrollbar slider {
            min-width: 4px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.22);
        }

        .okp-settings-content {
            padding-right: 4px;
        }

        .okp-about-pane {
            padding: 70px 44px 28px 24px;
            background: #eef4f9;
        }

        .okp-about-identity {
            min-height: 112px;
        }

        .okp-about-illustration {
            min-width: 118px;
            min-height: 94px;
        }

        .okp-about-wordmark {
            color: #161616;
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 30px;
            letter-spacing: 0;
        }

        .okp-about-wordmark-ok {
            font-weight: 700;
        }

        .okp-about-wordmark-player {
            font-weight: 300;
        }

        .okp-about-chip-row {
            margin-top: 10px;
        }

        .okp-about-version-chip {
            padding: 3px 9px;
            border-radius: 6px;
            background: #e2e8ec;
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-about-channel-chip {
            padding: 4px 9px;
            border-radius: 6px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
            text-transform: uppercase;
        }

        .okp-about-tagline {
            margin-top: 11px;
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-about-byline {
            margin-top: 3px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-about-identity-divider {
            margin: 22px 0;
            background: rgba(0, 0, 0, 0.07);
        }

        .okp-about-card {
            padding: 14px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-about-card-title {
            margin-bottom: 13px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-row {
            min-height: 14px;
        }

        .okp-about-row-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-about-row-detail {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-about-row-value,
        .okp-about-row-value-mono {
            color: #161616;
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-about-row-value {
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
        }

        .okp-about-row-value-mono {
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-about-host-grid {
            min-width: 0;
        }

        .okp-about-tag {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(0, 0, 0, 0.05);
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-tag.is-accent {
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
        }

        .okp-about-footer {
            margin-top: 8px;
            padding-top: 17px;
            border-top: 1px solid rgba(0, 0, 0, 0.07);
        }

        .okp-about-copy-button {
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #e2e8ec;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-copy-button:hover {
            background: #d9e1e7;
        }

        .okp-about-check-button {
            min-width: 132px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-about-check-button:hover {
            background: #f8fafb;
        }

        button.okp-about-toggle {
            min-width: 39px;
            min-height: 22px;
            padding: 3px;
            border: none;
            border-radius: 999px;
            background: #ccd5dc;
            box-shadow: none;
        }

        button.okp-about-toggle.is-active {
            background: #0067c0;
        }

        .okp-about-toggle-knob {
            min-width: 16px;
            min-height: 16px;
            border-radius: 999px;
            background: #ffffff;
        }

        .okp-about-link-button {
            min-height: 24px;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-arrow {
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-dot {
            min-width: 3px;
            min-height: 24px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8px;
            font-weight: 600;
        }

        .okp-update-status {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-mpv-conf-scroller {
            min-height: 132px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        textview.okp-mpv-conf-editor,
        textview.okp-mpv-conf-editor text {
            padding: 10px;
            background: #ffffff;
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            caret-color: #0067c0;
        }

        textview.okp-mpv-conf-editor selection,
        textview.okp-mpv-conf-editor text selection {
            background: rgba(0, 103, 192, 0.24);
            color: #161616;
        }

        .okp-settings-switch-row {
            min-height: 42px;
            padding: 10px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-settings-state-pill {
            min-width: 34px;
            padding: 3px 8px;
            border-radius: 999px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-integration-state-pill {
            min-width: 74px;
            padding: 4px 8px;
            border-radius: 999px;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-integration-state-pill.is-good {
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
        }

        .okp-integration-state-pill.is-warning {
            background: rgba(176, 118, 0, 0.14);
            color: #6f4b00;
        }

        .okp-integration-state-pill.is-bad {
            background: rgba(196, 43, 28, 0.12);
            color: #9a1f15;
        }

        .okp-info-section {
            padding: 14px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-info-section-title {
            margin-bottom: 10px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-row {
            min-height: 22px;
        }

        .okp-info-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-info-value {
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-info-summary {
            padding: 0;
        }

        .okp-info-chip {
            min-width: 78px;
            padding: 8px 10px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-info-chip-label {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-chip-value {
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-settings-row {
            min-height: 34px;
        }

        .okp-settings-action-row {
            margin-top: 8px;
        }

        .okp-shortcuts-list {
            margin-top: 4px;
        }

        .okp-shortcut-row {
            min-height: 44px;
            padding: 7px 0;
            border-bottom: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-shortcut-row:last-child {
            border-bottom: none;
        }

        .okp-shortcut-row.is-conflict {
            color: #9a1f15;
        }

        .okp-shortcut-action-title {
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-shortcut-action-id {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 10.5px;
            font-feature-settings: 'tnum';
        }

        .okp-shortcut-badge {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
        }

        button.okp-shortcut-chip {
            min-width: 82px;
            min-height: 30px;
            padding: 0 10px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.07);
            box-shadow: none;
            color: #161616;
        }

        button.okp-shortcut-chip:hover {
            background: #f1f5f8;
        }

        button.okp-shortcut-chip.is-secondary {
            min-width: 66px;
        }

        button.okp-shortcut-chip.is-empty {
            background: transparent;
            border-color: rgba(16, 147, 138, 0.18);
            color: #0a655f;
        }

        button.okp-shortcut-chip.is-empty:hover {
            background: rgba(16, 147, 138, 0.08);
        }

        button.okp-shortcut-chip.is-capturing {
            background: rgba(0, 103, 192, 0.12);
            border-color: rgba(0, 103, 192, 0.52);
        }

        button.okp-shortcut-chip.is-conflict {
            background: rgba(196, 43, 28, 0.10);
            border-color: rgba(196, 43, 28, 0.42);
        }

        .okp-shortcut-chip-label {
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        button.okp-shortcut-reset {
            min-width: 52px;
            min-height: 30px;
            padding: 0 10px;
            border-radius: 7px;
            background: transparent;
            border: 1px solid transparent;
            box-shadow: none;
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-shortcut-reset:hover {
            background: rgba(16, 147, 138, 0.08);
        }

        button.okp-shortcut-reset:disabled {
            color: rgba(0, 0, 0, 0.24);
        }

        .okp-settings-scale trough {
            min-height: 6px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.13);
        }

        .okp-settings-scale highlight {
            min-height: 6px;
            border-radius: 999px;
            background: #0067c0;
        }

        .okp-settings-scale slider {
            min-width: 18px;
            min-height: 18px;
            border-radius: 999px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.13);
        }

        .okp-settings-button {
            min-width: 82px;
            min-height: 32px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-settings-button:hover {
            background: #f8fafb;
        }

        button.okp-settings-track-row {
            min-height: 34px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.04);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        button.okp-settings-track-row:hover {
            background: #f1f5f8;
        }

        button.okp-settings-track-row.is-selected {
            background: rgba(16, 147, 138, 0.12);
            border-color: rgba(16, 147, 138, 0.24);
            color: #0a655f;
            font-weight: 600;
        }

        .okp-info-track-row {
            min-height: 44px;
            padding: 8px 9px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.04);
        }

        .okp-info-track-row.is-selected {
            background: rgba(16, 147, 138, 0.10);
            border-color: rgba(16, 147, 138, 0.18);
        }

        .okp-info-track-kind {
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-title {
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-info-track-current {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-detail {
            color: rgba(0, 0, 0, 0.48);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
        }

        .okp-info-footer-button {
            min-width: 82px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #e2e8ec;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-info-footer-button:hover {
            background: #d9e1e7;
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_item(path: &str) -> PlaylistItem {
        PlaylistItem::Local(PathBuf::from(path))
    }

    fn url_item(url: &str) -> PlaylistItem {
        PlaylistItem::Url(url.to_owned())
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "{prefix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ))
    }

    fn write_jpeg_header(path: &Path) {
        fs::write(path, b"\xff\xd8\xffok-player").expect("test jpeg header should be written");
    }

    fn write_png_header(path: &Path) {
        fs::write(path, b"\x89PNG\r\n\x1a\nokp!").expect("test png header should be written");
    }

    fn assert_delay(input: &str, expected: f64) {
        let actual = parse_delay_entry_seconds(input).expect("delay should parse");
        assert!((actual - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn about_display_version_keeps_about_layout_compact() {
        assert_eq!(about_display_version("0.1.0-linux-alpha.77"), "0.1.0");
        assert_eq!(about_display_version("1.0.0"), "1.0.0");
    }

    #[test]
    fn about_channel_separates_linux_release_track_from_version() {
        assert_eq!(about_display_channel("0.1.0-linux-alpha.77"), "Linux alpha");
        assert_eq!(about_hero_channel("Linux alpha"), "ALPHA");
        assert_eq!(about_display_channel("1.0.0-linux"), "Linux");
        assert_eq!(about_hero_channel("Linux"), "LINUX");
    }

    #[test]
    fn settings_shell_matches_windows_reference_geometry() {
        assert_eq!(SETTINGS_REFERENCE_WIDTH, 744);
        assert_eq!(SETTINGS_REFERENCE_HEIGHT, 1030);
        assert_eq!(SETTINGS_RAIL_WIDTH, 192);
        assert_eq!(SETTINGS_CONTENT_WIDTH, 552);
        assert_eq!(
            SETTINGS_RAIL_WIDTH + SETTINGS_CONTENT_WIDTH,
            SETTINGS_REFERENCE_WIDTH
        );
        assert_eq!(CAPTIONLESS_DRAG_HEIGHT, 32);
    }

    #[test]
    fn settings_initial_page_env_accepts_known_pages_only() {
        assert_eq!(normalized_settings_page(" Shortcuts "), Some("shortcuts"));
        assert_eq!(normalized_settings_page("about"), Some("about"));
        assert_eq!(normalized_settings_page("native-caption"), None);
    }

    #[test]
    fn mpris_snapshot_reports_stopped_without_media() {
        let snapshot = MprisSnapshot::default();

        assert_eq!(snapshot.playback_status(), "Stopped");
        assert!(!snapshot.has_media);
        assert_eq!(snapshot.position_us, 0);
    }

    #[test]
    fn mpris_metadata_contains_core_track_fields() {
        let snapshot = MprisSnapshot {
            has_media: true,
            paused: false,
            position_us: 1_000_000,
            duration_us: Some(30_000_000),
            volume: 1.0,
            can_go_next: true,
            can_go_previous: true,
            title: "subtest.mkv".to_owned(),
            uri: Some("file:///tmp/subtest.mkv".to_owned()),
            art_url: Some("file:///tmp/subtest.jpg".to_owned()),
            ..MprisSnapshot::default()
        };

        let metadata = mpris_metadata(&snapshot);

        assert_eq!(snapshot.playback_status(), "Playing");
        assert!(metadata.contains_key("mpris:trackid"));
        assert!(metadata.contains_key("mpris:length"));
        assert!(metadata.contains_key("xesam:title"));
        assert!(metadata.contains_key("xesam:url"));
        assert!(metadata.contains_key("mpris:artUrl"));
    }

    #[test]
    fn mpris_invalidations_cover_shell_state_without_position_spam() {
        let previous = MprisSnapshot::default();
        let next = MprisSnapshot {
            has_media: true,
            paused: false,
            position_us: 1_000_000,
            duration_us: Some(30_000_000),
            volume: 0.75,
            can_go_next: true,
            can_go_previous: true,
            title: "subtest.mkv".to_owned(),
            uri: Some("file:///tmp/subtest.mkv".to_owned()),
            ..MprisSnapshot::default()
        };

        let invalidated = mpris_invalidated_properties(&previous, &next);

        assert!(invalidated.contains(&"PlaybackStatus"));
        assert!(invalidated.contains(&"Metadata"));
        assert!(invalidated.contains(&"CanPlay"));
        assert!(invalidated.contains(&"CanPause"));
        assert!(invalidated.contains(&"CanSeek"));
        assert!(invalidated.contains(&"CanGoNext"));
        assert!(invalidated.contains(&"CanGoPrevious"));
        assert!(invalidated.contains(&"Volume"));
        assert!(!invalidated.contains(&"Position"));

        let moved = MprisSnapshot {
            position_us: 2_000_000,
            ..next.clone()
        };

        assert!(mpris_invalidated_properties(&next, &moved).is_empty());
    }

    #[test]
    fn mpris_metadata_invalidates_when_art_url_changes() {
        let previous = MprisSnapshot {
            has_media: true,
            paused: false,
            position_us: 1_000_000,
            duration_us: Some(30_000_000),
            title: "song.flac".to_owned(),
            uri: Some("file:///tmp/song.flac".to_owned()),
            art_url: Some("file:///tmp/old-cover.jpg".to_owned()),
            ..MprisSnapshot::default()
        };
        let next = MprisSnapshot {
            art_url: Some("file:///tmp/new-cover.jpg".to_owned()),
            ..previous.clone()
        };

        assert_eq!(
            mpris_invalidated_properties(&previous, &next),
            vec!["Metadata"]
        );
    }

    #[test]
    fn mpris_sidecar_art_prefers_same_named_image() {
        let root = unique_temp_dir("okp-mpris-art-same-name");
        fs::create_dir_all(&root).expect("test folder should be created");
        let media = root.join("Track 01.flac");
        let folder_cover = root.join("cover.jpg");
        let same_named = root.join("Track 01.png");
        fs::write(&media, []).expect("test media should be written");
        write_jpeg_header(&folder_cover);
        write_png_header(&same_named);

        assert_eq!(mpris_sidecar_art_path(&media), Some(same_named.clone()));
        assert_eq!(
            mpris_sidecar_art_url(&media),
            Some(local_file_uri(&same_named))
        );

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn mpris_sidecar_art_uses_folder_priority_and_skips_junk() {
        let root = unique_temp_dir("okp-mpris-art-folder");
        fs::create_dir_all(&root).expect("test folder should be created");
        let media = root.join("Episode 1.mkv");
        let bad_cover = root.join("cover.jpg");
        let folder_cover = root.join("folder.jpg");
        let poster = root.join("poster.png");
        fs::write(&media, []).expect("test media should be written");
        fs::write(&bad_cover, []).expect("junk cover should be written");
        write_jpeg_header(&folder_cover);
        write_png_header(&poster);

        assert_eq!(mpris_sidecar_art_path(&media), Some(folder_cover));

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn mpris_local_art_prefers_sidecar_before_embedded_art() {
        let root = unique_temp_dir("okp-mpris-art-sidecar-first");
        fs::create_dir_all(&root).expect("test folder should be created");
        let media = root.join("Song.flac");
        let sidecar = root.join("Song.jpg");
        fs::write(&media, b"not a real flac").expect("test media should be written");
        write_jpeg_header(&sidecar);

        assert_eq!(mpris_local_art_url(&media), Some(local_file_uri(&sidecar)));

        let key = mpris_embedded_art_cache_key(&media).expect("media key should be available");
        let cache = MPRIS_EMBEDDED_ART_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        assert!(
            !cache
                .lock()
                .expect("embedded art cache should lock")
                .contains_key(&key)
        );

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn mpris_embedded_art_is_audio_only() {
        let root = unique_temp_dir("okp-mpris-art-video-skip");
        fs::create_dir_all(&root).expect("test folder should be created");
        let media = root.join("Movie.mkv");
        fs::write(&media, b"not a real video").expect("test media should be written");

        assert_eq!(mpris_embedded_art_url(&media), None);

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn mpris_embedded_art_cache_path_changes_when_media_changes() {
        let root = unique_temp_dir("okp-mpris-art-cache-key");
        fs::create_dir_all(&root).expect("test folder should be created");
        let media = root.join("Song.flac");
        let cache_dir = root.join("cache");
        fs::write(&media, [1_u8]).expect("test media should be written");
        let before = mpris_embedded_art_cache_key(&media).expect("cache key should resolve");

        fs::write(&media, [1_u8, 2_u8]).expect("test media should be updated");
        let after = mpris_embedded_art_cache_key(&media).expect("updated cache key should resolve");

        assert_ne!(before.len, after.len);
        assert_ne!(
            mpris_embedded_art_cache_path_in_dir(&before, &cache_dir),
            mpris_embedded_art_cache_path_in_dir(&after, &cache_dir)
        );

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn mpris_app_icon_art_fallback_is_available_in_dev_tree() {
        let path =
            mpris_app_icon_art_path().expect("app icon should resolve in dev or installed tree");

        assert!(path.is_file());
        assert!(mpris_app_icon_art_url().is_some());
    }

    #[test]
    fn mpris_tracklist_window_limits_context_around_current_track() {
        assert_eq!(mpris_tracklist_window(3, 1), (0, 3));
        assert_eq!(
            mpris_tracklist_window(30, 0),
            (0, MPRIS_TRACKLIST_CONTEXT_LIMIT)
        );
        assert_eq!(
            mpris_tracklist_window(30, 25),
            (9, 9 + MPRIS_TRACKLIST_CONTEXT_LIMIT)
        );
    }

    #[test]
    fn mpris_tracklist_metadata_uses_current_track_id() {
        let root = unique_temp_dir("okp-mpris-tracklist");
        fs::create_dir_all(&root).expect("test folder should be created");
        let first = root.join("Episode 1.mkv");
        let second = root.join("Episode 2.mkv");
        let third = root.join("Episode 3.mkv");
        fs::write(&first, []).expect("test media should be written");
        fs::write(&second, []).expect("test media should be written");
        fs::write(&third, []).expect("test media should be written");

        let mut state = PlayerState {
            current_file: Some(second.clone()),
            playlist: vec![
                PlaylistItem::Local(first.clone()),
                PlaylistItem::Local(second.clone()),
                PlaylistItem::Local(third.clone()),
            ],
            ..PlayerState::default()
        };

        let tracks = mpris_tracklist_from_state(&state, Some(42_000_000));

        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[1].title, "Episode 2.mkv");
        assert_eq!(tracks[1].duration_us, Some(42_000_000));
        assert_eq!(
            mpris_tracklist_target_for_id(&state, tracks[1].id.as_str()),
            Some((1, PlaylistItem::Local(second.clone())))
        );

        let snapshot = mpris_snapshot_from_state(&state, None);
        assert_eq!(snapshot.current_track_id, Some(snapshot.track_id.clone()));
        assert!(snapshot.tracklist_track_ids().contains(&snapshot.track_id));
        assert!(mpris_metadata(&snapshot).contains_key("mpris:trackid"));
        assert!(mpris_track_metadata(&tracks[1]).contains_key("mpris:trackid"));

        state.current_file = Some(third);
        let moved = mpris_snapshot_from_state(&state, None);
        assert_ne!(snapshot.current_track_id, moved.current_track_id);
        assert!(mpris_tracklist_replaced_signal(&snapshot, &moved).is_some());
        assert!(mpris_tracklist_invalidated_properties(&snapshot, &moved).is_empty());

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn mpris_tracklist_replaced_invalidates_tracks_when_playlist_changes() {
        let first = PlaylistItem::Url("https://example.test/one.mp3".to_owned());
        let second = PlaylistItem::Url("https://example.test/two.mp3".to_owned());
        let previous = MprisSnapshot {
            tracklist: vec![MprisTrack {
                id: mpris_tracklist_id_for_item(0, &first),
                title: first.display_name(),
                uri: mpris_playlist_item_uri(&first),
                duration_us: None,
                art_url: None,
            }],
            current_track_id: Some(mpris_tracklist_id_for_item(0, &first)),
            ..MprisSnapshot::default()
        };
        let next = MprisSnapshot {
            tracklist: vec![
                previous.tracklist[0].clone(),
                MprisTrack {
                    id: mpris_tracklist_id_for_item(1, &second),
                    title: second.display_name(),
                    uri: mpris_playlist_item_uri(&second),
                    duration_us: None,
                    art_url: None,
                },
            ],
            ..previous.clone()
        };

        assert_eq!(
            mpris_tracklist_invalidated_properties(&previous, &next),
            vec!["Tracks"]
        );
        let (tracks, current_track) =
            mpris_tracklist_replaced_signal(&previous, &next).expect("playlist should change");
        assert_eq!(tracks.len(), 2);
        assert_eq!(Some(current_track), previous.current_track_id);
    }

    #[test]
    fn mpris_seeked_signal_tracks_large_position_jumps_only() {
        let previous = MprisSnapshot {
            has_media: true,
            paused: false,
            position_us: 1_000_000,
            duration_us: Some(30_000_000),
            volume: 1.0,
            can_go_next: false,
            can_go_previous: false,
            title: "subtest.mkv".to_owned(),
            uri: Some("file:///tmp/subtest.mkv".to_owned()),
            ..MprisSnapshot::default()
        };

        let normal_tick = MprisSnapshot {
            position_us: previous.position_us + 200_000,
            ..previous.clone()
        };
        assert_eq!(mpris_seeked_position(&previous, &normal_tick), None);

        let seek_jump = MprisSnapshot {
            position_us: previous.position_us + 5_000_000,
            ..previous.clone()
        };
        assert_eq!(
            mpris_seeked_position(&previous, &seek_jump),
            Some(6_000_000)
        );

        let different_media = MprisSnapshot {
            position_us: 0,
            title: "other.mkv".to_owned(),
            uri: Some("file:///tmp/other.mkv".to_owned()),
            ..previous.clone()
        };
        assert_eq!(mpris_seeked_position(&previous, &different_media), None);
    }

    #[test]
    fn mpris_volume_setter_maps_to_mpv_percent_range() {
        assert_eq!(mpris_volume_to_mpv_percent(0.0), Some(0.0));
        assert_eq!(mpris_volume_to_mpv_percent(0.42), Some(42.0));
        assert_eq!(mpris_volume_to_mpv_percent(1.0), Some(100.0));
        assert_eq!(mpris_volume_to_mpv_percent(2.0), Some(130.0));
        assert_eq!(mpris_volume_to_mpv_percent(-1.0), Some(0.0));
        assert_eq!(mpris_volume_to_mpv_percent(f64::NAN), None);
        assert_eq!(mpris_volume_to_mpv_percent(f64::INFINITY), None);
    }

    #[test]
    fn mpris_play_mode_properties_follow_player_state() {
        assert_eq!(mpris_loop_status(RepeatMode::Off), "None");
        assert_eq!(mpris_loop_status(RepeatMode::One), "Track");
        assert_eq!(mpris_loop_status(RepeatMode::All), "Playlist");
        assert_eq!(mpris_repeat_mode("None"), Some(RepeatMode::Off));
        assert_eq!(mpris_repeat_mode("Track"), Some(RepeatMode::One));
        assert_eq!(mpris_repeat_mode("Playlist"), Some(RepeatMode::All));
        assert_eq!(mpris_repeat_mode("bad"), None);

        let previous = MprisSnapshot::default();
        let next = MprisSnapshot {
            rate: 1.25,
            repeat_mode: RepeatMode::All,
            shuffle: true,
            ..MprisSnapshot::default()
        };
        let invalidated = mpris_invalidated_properties(&previous, &next);

        assert!(invalidated.contains(&"Rate"));
        assert!(invalidated.contains(&"LoopStatus"));
        assert!(invalidated.contains(&"Shuffle"));
    }

    #[test]
    fn mpris_rate_setter_maps_to_supported_speed_range() {
        assert_eq!(mpris_rate_to_mpv_speed(0.1), Some(0.25));
        assert_eq!(mpris_rate_to_mpv_speed(1.25), Some(1.25));
        assert_eq!(mpris_rate_to_mpv_speed(9.0), Some(4.0));
        assert_eq!(mpris_rate_to_mpv_speed(f64::NAN), None);
        assert_eq!(mpris_rate_to_mpv_speed(f64::INFINITY), None);
    }

    #[test]
    fn raw_mpv_config_parser_accepts_key_value_lines() {
        assert_eq!(
            parse_raw_mpv_config(
                "\
# comment
scale=ewa_lanczossharp
--profile=gpu-hq
script-opts=osc-layout=bottombar
"
            ),
            Ok(vec![
                ("scale".to_owned(), "ewa_lanczossharp".to_owned()),
                ("profile".to_owned(), "gpu-hq".to_owned()),
                ("script-opts".to_owned(), "osc-layout=bottombar".to_owned()),
            ])
        );
    }

    #[test]
    fn raw_mpv_config_parser_reports_missing_value_separator_line() {
        let error = parse_raw_mpv_config("scale=ewa\nbad line\nprofile=gpu-hq")
            .expect_err("line should fail");

        assert_eq!(error.line, 2);
        assert!(error.message.contains("key=value"));
    }

    #[test]
    fn raw_mpv_config_parser_rejects_invalid_names() {
        let error = parse_raw_mpv_config("script/opts=value").expect_err("name should fail");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("Option names"));
    }

    #[test]
    fn raw_mpv_config_parser_rejects_protected_options() {
        let error = parse_raw_mpv_config("--vo=gpu").expect_err("vo should be managed");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("managed by OK Player"));

        assert!(parse_raw_mpv_config("VO=gpu").is_err());
    }

    #[test]
    fn raw_mpv_config_parser_rejects_nul_values() {
        let error = parse_raw_mpv_config("profile=gpu-hq\0").expect_err("nul should fail");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("NUL"));
    }

    #[test]
    fn shortcut_parser_accepts_action_overrides() {
        let bindings = resolved_shortcut_bindings_from_text(
            "\
play-pause=P
copy-frame=Ctrl+Shift+C
",
        )
        .expect("shortcuts should parse");

        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("p").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("space").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            None
        );
        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("C").expect("key exists"),
                gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK,
            ),
            Some(ShortcutAction::CopyFrame)
        );
    }

    #[test]
    fn shortcut_parser_accepts_secondary_action_binding() {
        let bindings = resolved_shortcut_bindings_from_text(
            "\
play-pause=Space
play-pause=P
",
        )
        .expect("secondary shortcut should parse");

        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("space").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("p").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            Some(ShortcutAction::PlayPause)
        );
    }

    #[test]
    fn shortcut_parser_rejects_unknown_action() {
        let error = resolved_shortcut_bindings_from_text("dance=Space")
            .expect_err("unknown action should fail");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("Unknown action"));
    }

    #[test]
    fn shortcut_parser_rejects_unknown_key() {
        let error = resolved_shortcut_bindings_from_text("play-pause=HyperDrive")
            .expect_err("unknown key should fail");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("Unknown key"));
    }

    #[test]
    fn shortcut_parser_rejects_conflicting_bindings() {
        let error =
            resolved_shortcut_bindings_from_text("play-pause=C").expect_err("conflict should fail");

        assert_eq!(error.line, 0);
        assert!(error.message.contains("conflicts"));
    }

    #[test]
    fn shortcut_parser_rejects_third_action_binding() {
        let error = resolved_shortcut_bindings_from_text(
            "\
play-pause=Space
play-pause=P
play-pause=Ctrl+P
",
        )
        .expect_err("third shortcut should fail");

        assert_eq!(error.line, 3);
        assert!(error.message.contains("at most two"));
    }

    #[test]
    fn shortcut_defaults_keep_shift_copy_frame_distinct() {
        let bindings = default_shortcut_bindings();

        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("c").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            Some(ShortcutAction::SaveScreenshot)
        );
        assert_eq!(
            shortcut_action_for_bindings(
                &bindings,
                gdk::Key::from_name("C").expect("key exists"),
                gdk::ModifierType::SHIFT_MASK,
            ),
            Some(ShortcutAction::CopyFrame)
        );
    }

    #[test]
    fn shortcut_config_text_serializes_only_custom_bindings() {
        let mut bindings = default_shortcut_bindings();
        let custom = parse_shortcut_chord("P", 0).expect("custom shortcut should parse");
        bindings
            .iter_mut()
            .find(|binding| binding.action == ShortcutAction::PlayPause)
            .expect("play-pause binding should exist")
            .chord = custom;

        assert_eq!(
            shortcut_config_text_from_bindings(&bindings),
            "play-pause=P"
        );
        assert!(
            resolved_shortcut_bindings_from_text(&shortcut_config_text_from_bindings(&bindings))
                .is_ok()
        );
    }

    #[test]
    fn shortcut_config_text_serializes_secondary_bindings() {
        let mut bindings = default_shortcut_bindings();
        bindings.push(ShortcutBinding {
            action: ShortcutAction::PlayPause,
            chord: parse_shortcut_chord("P", 0).expect("custom shortcut should parse"),
        });

        assert_eq!(
            shortcut_config_text_from_bindings(&bindings),
            "play-pause=Space\nplay-pause=P"
        );
        let resolved =
            resolved_shortcut_bindings_from_text(&shortcut_config_text_from_bindings(&bindings))
                .expect("serialized secondary shortcut should parse");
        assert_eq!(
            shortcut_action_for_bindings(
                &resolved,
                gdk::Key::from_name("space").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(
            shortcut_action_for_bindings(
                &resolved,
                gdk::Key::from_name("p").expect("key exists"),
                gdk::ModifierType::empty(),
            ),
            Some(ShortcutAction::PlayPause)
        );
    }

    #[test]
    fn shortcut_config_text_returns_blank_for_defaults() {
        assert_eq!(
            shortcut_config_text_from_bindings(&default_shortcut_bindings()),
            ""
        );
    }

    #[test]
    fn shortcut_capture_rejects_modifier_only_keys() {
        let shift = gdk::Key::from_name("Shift_L").expect("key exists");
        assert_eq!(
            shortcut_chord_from_event(shift, gdk::ModifierType::SHIFT_MASK),
            Err("Press a non-modifier key.")
        );

        let comma = gdk::Key::from_name("comma").expect("key exists");
        assert_eq!(
            shortcut_chord_from_event(comma, gdk::ModifierType::CONTROL_MASK)
                .map(shortcut_chord_label),
            Ok("Ctrl+,".to_owned())
        );
    }

    #[test]
    fn shortcut_labels_keep_letter_o_distinct_from_zero() {
        assert_eq!(
            shortcut_chord_label(parse_shortcut_chord("O", 0).expect("O should parse")),
            "O"
        );
    }

    #[test]
    fn desktop_mime_parser_keeps_registered_types() {
        let desktop_entry = "\
[Desktop Entry]
Name=OK Player
MimeType=video/mp4;video/x-matroska;audio/flac;
";

        assert_eq!(
            parse_desktop_mime_types(desktop_entry),
            vec![
                "video/mp4".to_owned(),
                "video/x-matroska".to_owned(),
                "audio/flac".to_owned(),
            ]
        );
        assert_eq!(count_registered_key_media_mimes(desktop_entry), 3);
    }

    #[test]
    fn default_app_match_is_exact_desktop_id() {
        assert!(default_app_matches_ok_player(
            "com.befeast.okplayer.desktop"
        ));
        assert!(!default_app_matches_ok_player("vlc.desktop"));
        assert!(!default_app_matches_ok_player("com.befeast.okplayer"));
    }

    #[test]
    fn timeline_marks_include_ab_loop_points() {
        let chapters = vec![
            Chapter {
                index: 0,
                time: 0.0,
                title: Some("Start".to_owned()),
            },
            Chapter {
                index: 1,
                time: 42.0,
                title: Some("Scene".to_owned()),
            },
            Chapter {
                index: 2,
                time: f64::NAN,
                title: None,
            },
        ];

        assert_eq!(
            timeline_marks(
                &chapters,
                AbLoopState {
                    a: Some(0.0),
                    b: Some(120.0),
                },
            ),
            vec![
                TimelineMark {
                    time: 42.0,
                    kind: TimelineMarkKind::Chapter,
                },
                TimelineMark {
                    time: 0.0,
                    kind: TimelineMarkKind::AbStart,
                },
                TimelineMark {
                    time: 120.0,
                    kind: TimelineMarkKind::AbEnd,
                },
            ]
        );
    }

    #[test]
    fn timeline_marks_combine_degenerate_ab_loop_points() {
        assert_eq!(
            timeline_marks(
                &[],
                AbLoopState {
                    a: Some(12.0),
                    b: Some(12.25),
                },
            ),
            vec![TimelineMark {
                time: 12.125,
                kind: TimelineMarkKind::AbLoop,
            }]
        );
        assert!(should_combine_ab_loop_marks(12.0, 12.5));
        assert!(!should_combine_ab_loop_marks(12.0, 12.501));
    }

    #[test]
    fn parses_subtitle_delay_entry_as_milliseconds_by_default() {
        assert_delay("250", 0.25);
        assert_delay("-125", -0.125);
        assert_delay("+500ms", 0.5);
    }

    #[test]
    fn parses_subtitle_delay_entry_seconds_suffix() {
        assert_delay("1.5s", 1.5);
        assert_delay("-0.25s", -0.25);
    }

    #[test]
    fn rejects_invalid_subtitle_delay_entry() {
        assert!(parse_delay_entry_seconds("").is_none());
        assert!(parse_delay_entry_seconds("soon").is_none());
        assert!(parse_delay_entry_seconds("nan").is_none());
    }

    #[test]
    fn clamps_subtitle_delay_entry_to_ten_minutes() {
        assert_delay("999999999", 600.0);
        assert_delay("-999999999", -600.0);
    }

    #[test]
    fn ab_loop_message_describes_cycle_state() {
        assert_eq!(
            ab_loop_message(
                AbLoopState {
                    a: Some(12.0),
                    b: None,
                },
                false,
            ),
            Some("A-B loop: start at 00:12".to_owned())
        );
        assert_eq!(
            ab_loop_message(
                AbLoopState {
                    a: Some(12.0),
                    b: Some(42.0),
                },
                true,
            ),
            Some("A-B loop: 00:12 - 00:42".to_owned())
        );
        assert_eq!(
            ab_loop_message(AbLoopState::default(), true),
            Some("A-B loop cleared".to_owned())
        );
        assert_eq!(ab_loop_message(AbLoopState::default(), false), None);
    }

    #[test]
    fn audio_device_restore_skips_auto_or_blank_devices() {
        assert!(!should_restore_audio_device(""));
        assert!(!should_restore_audio_device("  "));
        assert!(!should_restore_audio_device("auto"));
        assert!(should_restore_audio_device("pulse/alsa_output"));
    }

    #[test]
    fn audio_device_restore_retry_is_bounded() {
        let pending = PendingAudioDeviceRestore::new("pulse/device".to_owned());

        let pending = next_audio_device_restore_retry(pending, 3).expect("first miss should retry");
        assert_eq!(pending.attempts, 1);

        let pending =
            next_audio_device_restore_retry(pending, 3).expect("second miss should retry");
        assert_eq!(pending.attempts, 2);

        assert_eq!(next_audio_device_restore_retry(pending, 3), None);
    }

    #[test]
    fn playlist_save_path_adds_default_extension_only_when_missing() {
        assert_eq!(
            playlist_save_path(PathBuf::from("/tmp/OK Player Playlist")).as_path(),
            Path::new("/tmp/OK Player Playlist.m3u")
        );
        assert_eq!(
            playlist_save_path(PathBuf::from("/tmp/list.m3u8")).as_path(),
            Path::new("/tmp/list.m3u8")
        );
    }

    #[test]
    fn playlist_path_detects_m3u_variants() {
        assert!(is_playlist_path(Path::new("/tmp/list.m3u")));
        assert!(is_playlist_path(Path::new("/tmp/list.M3U8")));
        assert!(!is_playlist_path(Path::new("/tmp/movie.mkv")));
    }

    #[test]
    fn m3u_playlist_items_keep_urls_and_skip_subtitles_unknown_entries() {
        let entries = vec![
            "/media/ep2.mkv".to_owned(),
            "https://example.test/ep3.mp4".to_owned(),
            "/media/captions.srt".to_owned(),
            "/media/readme.txt".to_owned(),
            "/media/ep1.mp4".to_owned(),
        ];

        let items = playlist_items_from_m3u_entries(&entries);

        assert_eq!(
            items,
            vec![
                local_item("/media/ep2.mkv"),
                url_item("https://example.test/ep3.mp4"),
                local_item("/media/ep1.mp4")
            ]
        );
    }

    #[test]
    fn launch_args_keep_ordered_media_urls_and_subtitles() {
        let launch = parse_launch_args_from(
            [
                "/media/b.mkv",
                "https://example.test/a.mp4",
                "/media/b.mkv",
                "/media/captions.srt",
                "--sub",
                "/media/forced.ass",
                "/media/readme.txt",
            ]
            .into_iter()
            .map(Into::into),
        );

        assert_eq!(
            launch.items,
            vec![
                local_item("/media/b.mkv"),
                url_item("https://example.test/a.mp4")
            ]
        );
        assert_eq!(
            launch.subtitles,
            vec![
                PathBuf::from("/media/captions.srt"),
                PathBuf::from("/media/forced.ass")
            ]
        );
        assert!(launch.playlists.is_empty());
    }

    #[test]
    fn launch_args_decode_file_uris_and_detect_playlists() {
        let launch = parse_launch_args_from(
            [
                "file:///tmp/OK%20Player/movie.mkv",
                "file:///tmp/OK%20Player/list.m3u8",
                "file:///tmp/OK%20Player/subs.vtt",
            ]
            .into_iter()
            .map(Into::into),
        );

        assert_eq!(launch.items, vec![local_item("/tmp/OK Player/movie.mkv")]);
        assert_eq!(
            launch.playlists,
            vec![PathBuf::from("/tmp/OK Player/list.m3u8")]
        );
        assert_eq!(
            launch.subtitles,
            vec![PathBuf::from("/tmp/OK Player/subs.vtt")]
        );
    }

    #[test]
    fn launch_args_resolve_relative_paths_against_command_line_cwd() {
        let launch = parse_launch_args_from_cwd(
            ["movie.mkv", "--sub", "subs.srt"]
                .into_iter()
                .map(Into::into),
            Some(Path::new("/tmp/OK Player")),
        );

        assert_eq!(launch.items, vec![local_item("/tmp/OK Player/movie.mkv")]);
        assert_eq!(
            launch.subtitles,
            vec![PathBuf::from("/tmp/OK Player/subs.srt")]
        );
    }

    #[test]
    fn load_launch_args_uses_explicit_playlist_for_multiple_items() {
        let state = Rc::new(RefCell::new(PlayerState::default()));
        let launch = LaunchArgs {
            items: vec![
                local_item("/media/a.mkv"),
                url_item("https://example.test/b.mp4"),
            ],
            playlists: Vec::new(),
            subtitles: Vec::new(),
        };

        assert!(load_launch_args(&state, &launch));

        let state = state.borrow();
        assert_eq!(state.current_file, Some(PathBuf::from("/media/a.mkv")));
        assert_eq!(state.current_url, None);
        assert_eq!(state.playlist, launch.items);
    }

    #[test]
    fn unique_media_paths_keeps_order_and_skips_non_media_duplicates() {
        let paths = vec![
            PathBuf::from("/media/a.mkv"),
            PathBuf::from("/media/a.mkv"),
            PathBuf::from("/media/subs.srt"),
            PathBuf::from("/media/b.flac"),
            PathBuf::from("/media/readme.txt"),
        ];

        assert_eq!(
            unique_media_paths(paths),
            vec![
                PathBuf::from("/media/a.mkv"),
                PathBuf::from("/media/b.flac")
            ]
        );
    }

    #[test]
    fn selected_media_paths_keep_selection_order_and_skip_non_media() {
        let paths = vec![
            PathBuf::from("/media/b.mkv"),
            PathBuf::from("/media/subs.srt"),
            PathBuf::from("/media/a.mp4"),
            PathBuf::from("/media/b.mkv"),
            PathBuf::from("/media/list.m3u"),
        ];

        assert_eq!(
            selected_media_paths(&paths),
            vec![PathBuf::from("/media/b.mkv"), PathBuf::from("/media/a.mp4")]
        );
    }

    #[test]
    fn selected_subtitle_paths_keep_order_and_deduplicate() {
        let paths = vec![
            PathBuf::from("/media/a.en.srt"),
            PathBuf::from("/media/movie.mkv"),
            PathBuf::from("/media/a.en.srt"),
            PathBuf::from("/media/a.signs.ass"),
        ];

        assert_eq!(
            selected_subtitle_paths(&paths),
            vec![
                PathBuf::from("/media/a.en.srt"),
                PathBuf::from("/media/a.signs.ass")
            ]
        );
    }

    #[test]
    fn selected_playlist_path_picks_first_m3u_variant() {
        let paths = vec![
            PathBuf::from("/media/movie.mkv"),
            PathBuf::from("/media/queue.m3u8"),
            PathBuf::from("/media/other.m3u"),
        ];

        assert_eq!(
            selected_playlist_path(&paths),
            Some(PathBuf::from("/media/queue.m3u8"))
        );
    }

    #[test]
    fn selected_media_paths_expands_folders_in_natural_order() {
        let root = unique_temp_dir("okp-folder-selection");
        fs::create_dir_all(&root).expect("test folder should be created");
        let first = root.join("Episode 1.mp4");
        let second = root.join("Episode 2.mkv");
        let tenth = root.join("Episode 10.mkv");
        fs::write(&tenth, []).expect("test media should be created");
        fs::write(&first, []).expect("test media should be created");
        fs::write(root.join("Episode 2.srt"), []).expect("test subtitle should be created");
        fs::write(&second, []).expect("test media should be created");
        fs::write(root.join("cover.jpg"), []).expect("test ignored file should be created");

        assert_eq!(
            selected_media_paths(std::slice::from_ref(&root)),
            vec![first, second, tenth]
        );

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn selected_media_paths_expands_multiple_folders_in_selection_order() {
        let root = unique_temp_dir("okp-folder-multi-selection");
        let season_one = root.join("Season 1");
        let season_two = root.join("Season 2");
        fs::create_dir_all(&season_one).expect("first test folder should be created");
        fs::create_dir_all(&season_two).expect("second test folder should be created");
        let s1e1 = season_one.join("Episode 1.mkv");
        let s1e2 = season_one.join("Episode 2.mkv");
        let s2e1 = season_two.join("Episode 1.mkv");
        let s2e10 = season_two.join("Episode 10.mkv");
        fs::write(&s1e2, []).expect("test media should be created");
        fs::write(&s1e1, []).expect("test media should be created");
        fs::write(&s2e10, []).expect("test media should be created");
        fs::write(&s2e1, []).expect("test media should be created");

        assert_eq!(
            selected_media_paths(&[season_two, season_one]),
            vec![s2e1, s2e10, s1e1, s1e2]
        );

        fs::remove_dir_all(root).expect("test folders should be removed");
    }

    #[test]
    fn load_selected_local_paths_uses_explicit_playlist_for_multiple_media() {
        let state = Rc::new(RefCell::new(PlayerState::default()));
        let paths = vec![
            PathBuf::from("/media/b.mkv"),
            PathBuf::from("/media/subs.srt"),
            PathBuf::from("/media/a.mp4"),
            PathBuf::from("/media/b.mkv"),
        ];

        assert!(load_selected_local_paths(&state, paths));

        let state = state.borrow();
        assert_eq!(state.current_file, Some(PathBuf::from("/media/b.mkv")));
        assert_eq!(
            state.playlist,
            vec![local_item("/media/b.mkv"), local_item("/media/a.mp4")]
        );
    }

    #[test]
    fn load_selected_local_paths_preserves_folder_playlist_for_single_media() {
        let root = unique_temp_dir("okp-selection");
        fs::create_dir_all(&root).expect("test folder should be created");
        let first = root.join("Episode 1.mkv");
        let second = root.join("Episode 2.mkv");
        let subtitle = root.join("Episode 2.srt");
        fs::write(&first, []).expect("test media should be created");
        fs::write(&second, []).expect("test media should be created");
        fs::write(&subtitle, []).expect("test subtitle should be created");

        let state = Rc::new(RefCell::new(PlayerState::default()));

        assert!(load_selected_local_paths(&state, vec![second.clone()]));

        let state_ref = state.borrow();
        assert_eq!(state_ref.current_file, Some(second.clone()));
        assert_eq!(
            state_ref.playlist,
            vec![
                PlaylistItem::Local(first.clone()),
                PlaylistItem::Local(second.clone())
            ]
        );
        drop(state_ref);

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn load_selected_local_paths_opens_folder_as_playlist() {
        let root = unique_temp_dir("okp-folder-load");
        fs::create_dir_all(&root).expect("test folder should be created");
        let first = root.join("Episode 1.mkv");
        let second = root.join("Episode 2.mkv");
        fs::write(&second, []).expect("test media should be created");
        fs::write(&first, []).expect("test media should be created");

        let state = Rc::new(RefCell::new(PlayerState::default()));

        assert!(load_selected_local_paths(&state, vec![root.clone()]));

        let state_ref = state.borrow();
        assert_eq!(state_ref.current_file, Some(first.clone()));
        assert_eq!(
            state_ref.playlist,
            vec![
                PlaylistItem::Local(first.clone()),
                PlaylistItem::Local(second.clone())
            ]
        );
        drop(state_ref);

        fs::remove_dir_all(root).expect("test folder should be removed");
    }

    #[test]
    fn queue_playlist_append_adds_new_media_to_the_end() {
        let playlist = vec![
            local_item("/media/current.mkv"),
            url_item("https://example.test/stream"),
            local_item("/media/queued.mkv"),
        ];
        let additions = vec![
            PathBuf::from("/media/current.mkv"),
            PathBuf::from("/media/queued.mkv"),
            PathBuf::from("/media/new.mp4"),
            PathBuf::from("/media/album.flac"),
        ];

        let (playlist, count) = queue_playlist_insert(
            playlist,
            Path::new("/media/current.mkv"),
            additions,
            QueueInsertMode::Append,
        )
        .expect("new media should append");

        assert_eq!(count, 2);
        assert_eq!(
            playlist,
            vec![
                local_item("/media/current.mkv"),
                url_item("https://example.test/stream"),
                local_item("/media/queued.mkv"),
                local_item("/media/new.mp4"),
                local_item("/media/album.flac"),
            ]
        );
    }

    #[test]
    fn queue_playlist_play_next_inserts_after_current_and_moves_existing_items() {
        let playlist = vec![
            local_item("/media/previous.mkv"),
            local_item("/media/current.mkv"),
            url_item("https://example.test/stream"),
            local_item("/media/later.mkv"),
            local_item("/media/final.mkv"),
        ];
        let additions = vec![
            PathBuf::from("/media/later.mkv"),
            PathBuf::from("/media/new.mp4"),
        ];

        let (playlist, count) = queue_playlist_insert(
            playlist,
            Path::new("/media/current.mkv"),
            additions,
            QueueInsertMode::PlayNext,
        )
        .expect("play next should insert");

        assert_eq!(count, 2);
        assert_eq!(
            playlist,
            vec![
                local_item("/media/previous.mkv"),
                local_item("/media/current.mkv"),
                local_item("/media/later.mkv"),
                local_item("/media/new.mp4"),
                url_item("https://example.test/stream"),
                local_item("/media/final.mkv"),
            ]
        );
    }

    #[test]
    fn queue_playlist_rejects_current_only_selection() {
        assert!(
            queue_playlist_insert(
                vec![local_item("/media/current.mkv")],
                Path::new("/media/current.mkv"),
                vec![PathBuf::from("/media/current.mkv")],
                QueueInsertMode::Append,
            )
            .is_none()
        );
    }

    #[test]
    fn reorder_playlist_moves_item_to_target_slot_after_removal() {
        let playlist = vec![
            local_item("/media/a.mkv"),
            local_item("/media/b.mkv"),
            url_item("https://example.test/c.mp4"),
            local_item("/media/d.mkv"),
        ];

        let reordered = reorder_playlist(playlist.clone(), 0, 2).expect("move should work");
        assert_eq!(
            reordered,
            vec![
                local_item("/media/b.mkv"),
                url_item("https://example.test/c.mp4"),
                local_item("/media/a.mkv"),
                local_item("/media/d.mkv"),
            ]
        );

        let reordered = reorder_playlist(playlist, 3, 1).expect("move should work");
        assert_eq!(
            reordered,
            vec![
                local_item("/media/a.mkv"),
                local_item("/media/d.mkv"),
                local_item("/media/b.mkv"),
                url_item("https://example.test/c.mp4"),
            ]
        );
    }

    #[test]
    fn reorder_playlist_rejects_noop_or_out_of_range_moves() {
        let playlist = vec![local_item("/media/a.mkv"), local_item("/media/b.mkv")];

        assert!(reorder_playlist(playlist.clone(), 0, 0).is_none());
        assert!(reorder_playlist(playlist, 3, 0).is_none());
    }

    #[test]
    fn playlist_drop_target_index_maps_before_after_slots() {
        assert_eq!(playlist_drop_target_index(0, 2, false), Some(1));
        assert_eq!(playlist_drop_target_index(0, 2, true), Some(2));
        assert_eq!(playlist_drop_target_index(3, 1, false), Some(1));
        assert_eq!(playlist_drop_target_index(3, 1, true), Some(2));
    }

    #[test]
    fn playlist_drop_target_index_rejects_self_or_existing_slot() {
        assert_eq!(playlist_drop_target_index(2, 2, false), None);
        assert_eq!(playlist_drop_target_index(2, 2, true), None);
        assert_eq!(playlist_drop_target_index(1, 2, false), None);
        assert_eq!(playlist_drop_target_index(2, 1, true), None);
    }

    #[test]
    fn remove_playlist_index_keeps_at_least_one_item() {
        let playlist = vec![
            local_item("/media/a.mkv"),
            url_item("https://example.test/b.mp4"),
            local_item("/media/c.mkv"),
        ];

        let without_middle = remove_playlist_index(playlist, 1).expect("remove should work");
        assert_eq!(
            without_middle,
            vec![local_item("/media/a.mkv"), local_item("/media/c.mkv")]
        );

        assert!(remove_playlist_index(vec![local_item("/media/a.mkv")], 0).is_none());
    }

    fn github_asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: Some(42),
        }
    }

    fn github_release(
        tag_name: &str,
        draft: bool,
        prerelease: bool,
        assets: Vec<GitHubAsset>,
    ) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_owned(),
            draft,
            prerelease,
            assets,
        }
    }

    #[test]
    fn linux_version_compare_orders_alpha_numbers_naturally() {
        assert_eq!(
            compare_linux_versions("0.1.0-linux-alpha.10", "0.1.0-linux-alpha.9"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_linux_versions("0.1.0-linux-alpha.45", "0.1.0-linux-alpha.45"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_linux_versions("0.1.0-linux-alpha.44", "0.1.0-linux-alpha.45"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn selects_latest_linux_deb_prerelease_newer_than_current() {
        let update = select_latest_linux_deb_update(
            vec![
                github_release(
                    "linux-v0.1.0-linux-alpha.46",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.46_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.47",
                    true,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.47_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.48",
                    false,
                    false,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.48_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.49",
                    false,
                    true,
                    vec![github_asset("com.befeast.okplayer.AppImage")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.45",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.45_amd64.deb")],
                ),
            ],
            "0.1.0-linux-alpha.45",
        )
        .expect("alpha46 .deb should be selected");

        assert_eq!(update.version, "0.1.0-linux-alpha.46");
        assert_eq!(update.name, "ok-player_0.1.0-linux-alpha.46_amd64.deb");
        assert_eq!(update.size, Some(42));
    }

    #[test]
    fn deb_update_action_requests_install() {
        let update = PendingLinuxUpdate {
            manager: None,
            target: LinuxUpdateTarget::Deb(ManualDebUpdate {
                version: "0.1.0-linux-alpha.46".to_owned(),
                name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
                url: "https://example.invalid/update.deb".to_owned(),
                size: Some(42),
            }),
        };

        assert_eq!(update.action_label(), "Install .deb");
        assert_eq!(update.available_status(), "Available: 0.1.0-linux-alpha.46");
    }

    #[test]
    fn linux_update_status_reflects_last_check_result() {
        let up_to_date = LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::UpToDate);
        assert_eq!(up_to_date.about_status_text(), "Up to date");
        assert_eq!(
            up_to_date.settings_status_text(true),
            "OK Player is up to date"
        );
        assert_eq!(up_to_date.action_label(), "Check for updates");
        assert!(up_to_date.pending_update().is_none());

        let update = PendingLinuxUpdate {
            manager: None,
            target: LinuxUpdateTarget::Deb(ManualDebUpdate {
                version: "0.1.0-linux-alpha.46".to_owned(),
                name: "ok-player_0.1.0-linux-alpha.46_amd64.deb".to_owned(),
                url: "https://example.invalid/update.deb".to_owned(),
                size: Some(42),
            }),
        };
        let available =
            LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::Available(update));
        assert_eq!(
            available.about_status_text(),
            "Available: 0.1.0-linux-alpha.46"
        );
        assert_eq!(
            available.settings_status_text(true),
            "Available: 0.1.0-linux-alpha.46"
        );
        assert_eq!(available.action_label(), "Install .deb");
        assert!(available.pending_update().is_some());

        let failed =
            LinuxUpdateStatus::from_check_result(&LinuxUpdateCheckResult::Failed("no feed".into()));
        assert_eq!(failed.about_status_text(), "Update check failed");
        assert_eq!(
            failed.settings_status_text(true),
            "Update check failed: no feed"
        );
    }

    #[test]
    fn deb_self_install_timeout_uses_positive_override_only() {
        assert_eq!(
            parse_deb_self_install_timeout(Some("5")),
            Duration::from_secs(5)
        );
        assert_eq!(
            parse_deb_self_install_timeout(Some("0")),
            DEB_SELF_INSTALL_TIMEOUT
        );
        assert_eq!(
            parse_deb_self_install_timeout(Some("soon")),
            DEB_SELF_INSTALL_TIMEOUT
        );
        assert_eq!(
            parse_deb_self_install_timeout(None),
            DEB_SELF_INSTALL_TIMEOUT
        );
    }

    #[test]
    fn deb_update_selection_returns_none_when_only_current_or_older_exist() {
        let update = select_latest_linux_deb_update(
            vec![
                github_release(
                    "linux-v0.1.0-linux-alpha.44",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.44_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.45",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.45_amd64.deb")],
                ),
            ],
            "0.1.0-linux-alpha.45",
        );

        assert!(update.is_none());
    }
}
