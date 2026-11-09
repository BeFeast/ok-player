//! The JSON documents a shell exchanges with a [`crate::Session`].
//!
//! Every document is an envelope with a `schema_version`. Enums are externally
//! tagged with `snake_case` names, and every variant is a struct variant (an
//! empty one serialises as `{"close":{}}`), so the shape maps one-to-one onto
//! Swift's synthesised `Codable` for enums with labelled associated values.
//! Optional fields are written as `null` and may be omitted on input.
//!
//! The golden fixtures under `fixtures/` pin every shape; see `docs/` for the
//! threading and ownership rules of the C ABI that carries these documents.

use std::path::PathBuf;

use okp_core::app::AppIdentity;
use okp_core::player as core;
use okp_core::playlist::PlaylistItem;
use okp_core::shortcuts::ShortcutAction;
use serde::{Deserialize, Serialize};

pub use crate::commands::{CommandGroup, CommandMenu, RegistryCommand, ShortcutRow};
pub use crate::history::{
    HistoryAction, HistoryActionResult, HistoryBucket, HistoryEntry, HistoryEvent, HistoryQuery,
    HistoryQueryResult, HistorySection, HistoryState,
};
pub use crate::playlist::{PlaylistAction, PlaylistEntry, PlaylistState, QueueMode, RepeatMode};
pub use crate::tracks::{SubtitleValue, TrackChoice, TracksView};
pub use crate::transport::{OscSurface, OscView, SpeedChoice, TransportView, VolumeView};
pub use crate::window::{VideoClickIntent, WindowRect, WindowSize};

/// Version of every envelope below. Bump on any change a shell built against
/// the previous version could not decode or would misread. A new command,
/// query or event, or a reply variant that answers only a new command, keeps
/// the version: shells skip events they do not know (`docs/session-abi.md`,
/// "Compatibility within a version").
pub const SCHEMA_VERSION: u32 = 1;

/// What the shell passes when it creates a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    pub schema_version: u32,
    pub app: AppIdentity,
    /// Where the session keeps per-user data. Must be absolute.
    pub data_dir: PathBuf,
    /// Where the session keeps settings. Must be absolute.
    pub config_dir: PathBuf,
    /// Where the session keeps disposable caches. Must be absolute.
    pub cache_dir: PathBuf,
    /// libmpv `hwdec` value; `null` keeps the session default.
    #[serde(default)]
    pub hwdec: Option<String>,
    /// Extra libmpv options, applied in order after the session defaults.
    #[serde(default)]
    pub mpv_options: Vec<MpvOption>,
    /// The registry commands this shell carries out itself (dialogs, window
    /// and clipboard actions). A command neither the session nor the shell
    /// carries out is reported unavailable.
    #[serde(default)]
    pub shell_commands: Vec<String>,
}

/// One libmpv option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MpvOption {
    pub name: String,
    pub value: String,
}

/// A media source: a local path or a URL. URLs are handed to the engine as they
/// are; nothing resolves them yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaSource {
    pub kind: SourceKind,
    pub location: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Local,
    Url,
}

/// `{"schema_version":1,"command":{...}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub schema_version: u32,
    pub command: Command,
}

/// An intent from the shell. Every command is validated by the core player
/// state machine before anything reaches the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Load a source, replacing the current one.
    Open {
        source: MediaSource,
        /// Start playing here (seconds from the start; a negative value means
        /// the start).
        #[serde(default)]
        resume_from: Option<f64>,
        /// Audio track id to select once loaded, if the file has that track.
        #[serde(default)]
        audio_track: Option<i64>,
        /// Subtitle track id to select once loaded, if the file has that track.
        #[serde(default)]
        subtitle_track: Option<i64>,
    },
    /// Stop and unload the current media.
    Close {},
    SetPaused {
        paused: bool,
    },
    TogglePause {},
    Seek {
        seconds: f64,
        mode: SeekMode,
        precision: SeekPrecision,
    },
    /// Output volume in percent.
    SetVolume {
        volume: f64,
    },
    SetMute {
        muted: bool,
    },
    /// Playback speed multiplier.
    SetSpeed {
        speed: f64,
    },
    /// Select a track by id; `null` (or no `id`) turns that kind off.
    SelectTrack {
        kind: TrackKind,
        #[serde(default)]
        id: Option<i64>,
    },
    SetSubtitleDelay {
        seconds: f64,
    },
    /// Run a command of the registry (`command_registry`) by its id.
    InvokeCommand {
        id: String,
    },
    /// A watch-history action; see [`HistoryAction`].
    History {
        action: HistoryAction,
    },
    /// Open, step through and edit the playlist; see [`PlaylistAction`].
    Playlist {
        action: PlaylistAction,
    },
    /// Change the volume by `delta` percent from the level asked for last, or
    /// the reported one, clamped to 0-130 %. Forwarded as a `set_volume`.
    NudgeVolume {
        delta: f64,
    },
    /// Mute as the core does: a level of 0 that remembers the last level above
    /// it, which the next toggle restores. Forwarded as a `set_volume`, or as
    /// `set_mute(false)` while libmpv's own mute flag is on.
    ToggleMute {},
    /// Load a subtitle file for the current media and select it.
    AddSubtitle {
        path: String,
    },
    /// Subtitle size as a factor (1 = 100 %), kept within 0.25-4.
    SetSubtitleScale {
        scale: f64,
    },
    /// Change the subtitle size by `delta` from the size asked for last, or the
    /// reported one. Forwarded as a `set_subtitle_scale`.
    NudgeSubtitleScale {
        delta: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeekMode {
    Absolute,
    Relative,
}

/// `exact` decodes to the requested frame; `keyframe` lands on the nearest
/// keyframe and is faster for scrubbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeekPrecision {
    Exact,
    Keyframe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Audio,
    Subtitle,
}

/// `{"schema_version":1,"result":{...}}`, the answer to a dispatched command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchReply {
    pub schema_version: u32,
    pub result: DispatchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DispatchResult {
    /// Validated and handed to the engine. A later `error` event with the same
    /// `request_id` reports an asynchronous engine failure.
    Accepted { request_id: u64 },
    /// Valid, but it would change nothing; nothing reached the engine.
    NoOp {},
    /// Refused; nothing reached the engine.
    Rejected {
        reason: RejectReason,
        message: String,
    },
    /// Validated, but the engine refused it immediately. The session state has
    /// already absorbed the failure.
    Failed { request_id: u64, message: String },
    /// An invoked registry command the shell carries out itself. The session
    /// found it available; nothing reached the engine.
    Perform {},
    /// The answer to a `history` command.
    History { result: HistoryActionResult },
    /// Carried out by the session itself (a playlist edit, or an open whose
    /// folders are being read); nothing reached the engine yet. Events report
    /// the effect.
    Applied {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The command needs loaded media and there is none.
    NoActiveMedia,
    /// A number in the command is NaN or infinite.
    NotFinite,
    /// The document is not valid JSON or not a known command or query.
    Malformed,
    /// The envelope carries a `schema_version` this session does not speak.
    UnsupportedSchemaVersion,
    /// The C caller passed a null pointer or text that is not UTF-8, or
    /// `add_subtitle` named a file that is not a subtitle.
    InvalidArgument,
    /// `invoke_command` named an id that is not in the registry.
    UnknownCommand,
    /// `invoke_command` named a command this shell cannot run.
    Unavailable,
}

/// `{"schema_version":1,"query":{...}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryEnvelope {
    pub schema_version: u32,
    pub query: Query,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    /// The full session state, as the event stream has reported it so far.
    Snapshot {},
    /// The frame of a window fitted to a video on one screen
    /// ([`crate::window::fit_window`]). Reads no session state.
    WindowFit {
        /// Display size of the video in pixels, as the snapshot's `video`.
        video: WindowSize,
        /// Device pixels per logical unit on that screen.
        backing_scale: f64,
        /// The part of the screen windows may use, in logical units.
        visible_frame: WindowRect,
        /// The window's minimum size, if it has one.
        #[serde(default)]
        min_size: Option<WindowSize>,
        /// The room the window's own controls take outside the video area, if
        /// any: the frame grows by it so the video area keeps the video's aspect.
        #[serde(default)]
        chrome: Option<WindowSize>,
    },
    /// The fit the current source is owed: once per opened source, the frame of
    /// a window fitted to the first video size reported while it is loaded (as
    /// `window_fit` with that size); `null` before that size is known, after the
    /// fit was taken, and for a source without video. Answering takes the fit.
    InitialWindowFit {
        backing_scale: f64,
        visible_frame: WindowRect,
        #[serde(default)]
        min_size: Option<WindowSize>,
        #[serde(default)]
        chrome: Option<WindowSize>,
    },
    /// What a primary-button release with `press_count` clicks on the video
    /// means ([`crate::window::video_click_intent`]). Reads no session state.
    VideoClick { press_count: i32 },
    /// Every registry command, resolved against the current state. The shell
    /// passes the window state only it knows.
    CommandRegistry {
        #[serde(default)]
        fullscreen: bool,
        #[serde(default)]
        mini_player: bool,
    },
    /// Every bindable action with its chords on this platform.
    ShortcutTable {},
    /// The actions whose key repeats while held; every other action runs once
    /// per physical press.
    RepeatingShortcuts {},
    /// A watch-history query; see [`HistoryQuery`].
    History { query: HistoryQuery },
    /// The playlist as it is now.
    Playlist {},
    /// What the transport controls show for that state; with `at`, the time
    /// readout at that position instead of the playhead (a scrubber drag).
    Transport {
        #[serde(default)]
        at: Option<f64>,
    },
    /// What the audio and subtitle menus show: the tracks with their labels,
    /// and the subtitle delay and size.
    Tracks {},
}

/// `{"schema_version":1,"result":{...}}`, the answer to a query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryReply {
    pub schema_version: u32,
    pub result: QueryResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryResult {
    Snapshot {
        snapshot: SessionSnapshot,
    },
    /// The answer to `window_fit` and `initial_window_fit`; `null` when there
    /// is nothing to fit.
    WindowFit {
        frame: Option<WindowRect>,
    },
    VideoClick {
        intent: VideoClickIntent,
    },
    CommandRegistry {
        commands: Vec<RegistryCommand>,
    },
    ShortcutTable {
        shortcuts: Vec<ShortcutRow>,
    },
    RepeatingShortcuts {
        actions: Vec<ShortcutAction>,
    },
    /// The answer to a `history` query.
    History {
        result: HistoryQueryResult,
    },
    Playlist {
        playlist: PlaylistState,
    },
    Transport {
        transport: TransportView,
    },
    Tracks {
        tracks: TracksView,
    },
    Rejected {
        reason: RejectReason,
        message: String,
    },
}

/// `{"schema_version":1,"events":[...]}`, one drained batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventBatch {
    pub schema_version: u32,
    pub events: Vec<Event>,
}

/// One entry of a drained batch. A batch holds at most one `snapshot_changed`,
/// first, carrying the latest value of every field that changed since the
/// previous batch; then at most one `playlist_changed` with the latest
/// playlist; `error` events follow in the order they happened; `shutdown` comes
/// last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    SnapshotChanged {
        changes: Vec<SnapshotChange>,
    },
    /// The playlist changed: its items, current item, play modes or scanning.
    PlaylistChanged {
        playlist: PlaylistState,
    },
    Error {
        kind: ErrorKind,
        message: String,
        /// The command this error answers, when it answers one.
        request_id: Option<u64>,
    },
    /// The engine has quit; the session accepts no further playback.
    Shutdown {},
    /// A watch-history event; see [`HistoryEvent`].
    History {
        event: HistoryEvent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    LoadFailed,
    CommandFailed,
    Engine,
}

/// The full state a shell renders from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSnapshot {
    pub status: Status,
    pub source: Option<MediaSource>,
    pub time_pos: Option<f64>,
    pub duration: Option<f64>,
    pub volume: Option<f64>,
    pub mute: bool,
    pub speed: Option<f64>,
    pub subtitle_delay: f64,
    pub tracks: Vec<Track>,
    pub chapters: Vec<Chapter>,
    /// Why playback ended; set while `status` is `ended`.
    pub end_reason: Option<EndReason>,
    /// A display title for the current media.
    pub title: Option<String>,
    /// Display size of the video after aspect correction; `null` without video.
    pub video: Option<VideoGeometry>,
    /// Playback is held while the network cache refills.
    pub buffering: bool,
    /// Seconds of media cached ahead of the playhead.
    pub cache_ahead: Option<f64>,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            status: Status::Idle,
            source: None,
            time_pos: None,
            duration: None,
            volume: None,
            mute: false,
            speed: None,
            subtitle_delay: 0.0,
            tracks: Vec::new(),
            chapters: Vec::new(),
            end_reason: None,
            title: None,
            video: None,
            buffering: false,
            cache_ahead: None,
        }
    }
}

impl SessionSnapshot {
    /// Apply one change from a `snapshot_changed` event. Applying every drained
    /// change in order to the snapshot a query returned yields the session's
    /// state as of the last drain.
    pub fn apply(&mut self, change: &SnapshotChange) {
        match change {
            SnapshotChange::Status { value } => self.status = *value,
            SnapshotChange::Source { value } => self.source.clone_from(value),
            SnapshotChange::TimePos { value } => self.time_pos = *value,
            SnapshotChange::Duration { value } => self.duration = *value,
            SnapshotChange::Volume { value } => self.volume = *value,
            SnapshotChange::Mute { value } => self.mute = *value,
            SnapshotChange::Speed { value } => self.speed = *value,
            SnapshotChange::SubtitleDelay { value } => self.subtitle_delay = *value,
            SnapshotChange::Tracks { value } => self.tracks.clone_from(value),
            SnapshotChange::Chapters { value } => self.chapters.clone_from(value),
            SnapshotChange::EndReason { value } => self.end_reason = *value,
            SnapshotChange::Title { value } => self.title.clone_from(value),
            SnapshotChange::Video { value } => self.video.clone_from(value),
            SnapshotChange::Buffering { value } => self.buffering = *value,
            SnapshotChange::CacheAhead { value } => self.cache_ahead = *value,
        }
    }
}

/// One changed snapshot field and its new value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SnapshotChange {
    Status { value: Status },
    Source { value: Option<MediaSource> },
    TimePos { value: Option<f64> },
    Duration { value: Option<f64> },
    Volume { value: Option<f64> },
    Mute { value: bool },
    Speed { value: Option<f64> },
    SubtitleDelay { value: f64 },
    Tracks { value: Vec<Track> },
    Chapters { value: Vec<Chapter> },
    EndReason { value: Option<EndReason> },
    Title { value: Option<String> },
    Video { value: Option<VideoGeometry> },
    Buffering { value: bool },
    CacheAhead { value: Option<f64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Opening,
    Playing,
    Paused,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Eof,
    Stopped,
    Quit,
    Error,
    Redirect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub selected: bool,
    pub external: bool,
    pub default: bool,
    pub title: Option<String>,
    pub lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
    pub index: i64,
    /// Start of the chapter (seconds).
    pub time: f64,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoGeometry {
    pub width: i32,
    pub height: i32,
    /// `width / height`.
    pub display_aspect: f64,
}

impl MediaSource {
    pub(crate) fn from_item(item: &PlaylistItem) -> Self {
        match item {
            PlaylistItem::Local(path) => Self {
                kind: SourceKind::Local,
                location: path.to_string_lossy().into_owned(),
            },
            PlaylistItem::Url(url) => Self {
                kind: SourceKind::Url,
                location: url.clone(),
            },
        }
    }

    pub(crate) fn to_item(&self) -> PlaylistItem {
        match self.kind {
            SourceKind::Local => PlaylistItem::Local(PathBuf::from(&self.location)),
            SourceKind::Url => PlaylistItem::Url(self.location.clone()),
        }
    }
}

/// Where the session sends a command.
pub(crate) enum Route<'a> {
    /// The core command `PlayerMachine` validates before the engine sees it.
    Player(core::PlayerCommand),
    /// A history action, which the session carries out itself.
    History(&'a HistoryAction),
    /// `invoke_command`, which names a registry command instead of carrying
    /// one; the session resolves it before routing.
    Registry,
    /// A playlist command: the session's playlist decides it and plays an item
    /// with an `open`, so the command itself is never forwarded.
    Playlist(&'a PlaylistAction),
}

impl Command {
    /// Where this command goes: a player command becomes the core command the
    /// state machine validates.
    pub(crate) fn route(&self) -> Route<'_> {
        Route::Player(match self {
            Self::Open {
                source,
                resume_from,
                audio_track,
                subtitle_track,
            } => core::PlayerCommand::Open(core::OpenRequest {
                source: source.to_item(),
                resume_from: resume_from.map(media_position),
                initial_audio: *audio_track,
                initial_subtitle: *subtitle_track,
            }),
            Self::Close {} => core::PlayerCommand::Close,
            Self::SetPaused { paused } => core::PlayerCommand::SetPaused(*paused),
            Self::TogglePause {} => core::PlayerCommand::TogglePause,
            Self::Seek { seconds, mode, .. } => core::PlayerCommand::Seek(core::SeekRequest {
                mode: match mode {
                    SeekMode::Absolute => core::SeekMode::Absolute,
                    SeekMode::Relative => core::SeekMode::Relative,
                },
                seconds: seek_seconds(*seconds, *mode),
            }),
            Self::SetVolume { volume } => core::PlayerCommand::SetVolume(*volume),
            // The session turns a nudge into a `set_volume` before validation
            // (`transport::VolumeTarget`); validated on its own, it is a level.
            Self::NudgeVolume { delta } => core::PlayerCommand::SetVolume(*delta),
            // Resolved by the session as well; on its own it is the mute level.
            Self::ToggleMute {} => core::PlayerCommand::SetVolume(okp_core::volume::MIN_VOLUME),
            Self::SetMute { muted } => core::PlayerCommand::SetMute(*muted),
            Self::SetSpeed { speed } => core::PlayerCommand::SetSpeed(*speed),
            Self::SelectTrack { kind, id } => core::PlayerCommand::SelectTrack {
                kind: match kind {
                    TrackKind::Audio => core::TrackKind::Audio,
                    TrackKind::Subtitle => core::TrackKind::Subtitle,
                },
                id: *id,
            },
            Self::SetSubtitleDelay { seconds } => core::PlayerCommand::SetSubtitleDelay(*seconds),
            // `sub-add … select` selects the track it adds, so the core validates
            // an added subtitle file as a subtitle selection: it needs loaded media.
            Self::AddSubtitle { .. } => core::PlayerCommand::SelectTrack {
                kind: core::TrackKind::Subtitle,
                id: None,
            },
            // The size is subtitle presentation like the delay, and the core gates
            // it the same way: loaded media and a finite value. A nudge is turned
            // into a size by the session first (`tracks::TrackState`).
            Self::SetSubtitleScale { scale } => core::PlayerCommand::SetSubtitleDelay(*scale),
            Self::NudgeSubtitleScale { delta } => core::PlayerCommand::SetSubtitleDelay(*delta),
            Self::History { action } => return Route::History(action),
            Self::InvokeCommand { .. } => return Route::Registry,
            Self::Playlist { action } => return Route::Playlist(action),
        })
    }
}

/// A position in the media. libmpv counts a negative position from the end, so
/// one before the start is the start. Non-finite values pass through for the core
/// state machine to reject.
pub(crate) fn media_position(seconds: f64) -> f64 {
    if seconds.is_finite() && seconds <= 0.0 {
        0.0
    } else {
        seconds
    }
}

/// The seek amount libmpv is given: an absolute target is a media position.
pub(crate) fn seek_seconds(seconds: f64, mode: SeekMode) -> f64 {
    match mode {
        SeekMode::Absolute => media_position(seconds),
        SeekMode::Relative => seconds,
    }
}

impl From<core::PlaybackStatus> for Status {
    fn from(status: core::PlaybackStatus) -> Self {
        match status {
            core::PlaybackStatus::Idle => Self::Idle,
            core::PlaybackStatus::Opening => Self::Opening,
            core::PlaybackStatus::Playing => Self::Playing,
            core::PlaybackStatus::Paused => Self::Paused,
            core::PlaybackStatus::Ended => Self::Ended,
        }
    }
}

impl From<core::EndReason> for EndReason {
    fn from(reason: core::EndReason) -> Self {
        match reason {
            core::EndReason::Eof => Self::Eof,
            core::EndReason::Stopped => Self::Stopped,
            core::EndReason::Quit => Self::Quit,
            core::EndReason::Error => Self::Error,
            core::EndReason::Redirect => Self::Redirect,
        }
    }
}

impl From<core::PlayerErrorKind> for ErrorKind {
    fn from(kind: core::PlayerErrorKind) -> Self {
        match kind {
            core::PlayerErrorKind::LoadFailed => Self::LoadFailed,
            core::PlayerErrorKind::CommandFailed => Self::CommandFailed,
            core::PlayerErrorKind::Engine => Self::Engine,
        }
    }
}

impl From<&core::TrackInfo> for Track {
    fn from(track: &core::TrackInfo) -> Self {
        Self {
            id: track.id,
            kind: match track.kind {
                core::TrackKind::Audio => TrackKind::Audio,
                core::TrackKind::Subtitle => TrackKind::Subtitle,
            },
            selected: track.selected,
            external: track.external,
            default: track.default,
            title: track.title.clone(),
            lang: track.lang.clone(),
        }
    }
}

impl From<&core::ChapterInfo> for Chapter {
    fn from(chapter: &core::ChapterInfo) -> Self {
        Self {
            index: chapter.index,
            time: chapter.time,
            title: chapter.title.clone(),
        }
    }
}

impl From<core::RejectReason> for RejectReason {
    fn from(reason: core::RejectReason) -> Self {
        match reason {
            core::RejectReason::NoActiveMedia => Self::NoActiveMedia,
            core::RejectReason::NotFinite => Self::NotFinite,
        }
    }
}

impl RejectReason {
    /// A short human-readable explanation.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::NoActiveMedia => "the command needs loaded media",
            Self::NotFinite => "a number in the command is not finite",
            Self::Malformed => "the document is not a known command or query",
            Self::UnsupportedSchemaVersion => "unsupported schema_version",
            Self::InvalidArgument => "a null pointer or non-UTF-8 text was passed",
            Self::UnknownCommand => "not a registry command",
            Self::Unavailable => "the command is not available in this shell",
        }
    }
}

/// Only the version, read before the full document so that a newer shell gets
/// `unsupported_schema_version` rather than a confusing shape error.
#[derive(Deserialize)]
struct VersionProbe {
    schema_version: Option<u32>,
}

/// Parse an envelope after checking its `schema_version`.
pub(crate) fn parse_envelope<T: serde::de::DeserializeOwned>(
    text: &str,
) -> Result<T, (RejectReason, String)> {
    let probe: VersionProbe = serde_json::from_str(text)
        .map_err(|error| (RejectReason::Malformed, format!("invalid JSON: {error}")))?;
    match probe.schema_version {
        None => {
            return Err((
                RejectReason::Malformed,
                "the envelope has no schema_version".to_owned(),
            ));
        }
        Some(SCHEMA_VERSION) => {}
        Some(other) => {
            return Err((
                RejectReason::UnsupportedSchemaVersion,
                format!(
                    "schema_version {other} is not supported; this session speaks {SCHEMA_VERSION}"
                ),
            ));
        }
    }
    serde_json::from_str(text).map_err(|error| (RejectReason::Malformed, error.to_string()))
}

/// A dispatch reply refusing a command for `reason`, for a caller that cannot
/// reach a session (a C caller passing a null handle, for instance).
#[must_use]
pub fn dispatch_rejection_json(reason: RejectReason) -> String {
    to_json(&DispatchReply {
        schema_version: SCHEMA_VERSION,
        result: DispatchResult::Rejected {
            reason,
            message: reason.describe().to_owned(),
        },
    })
}

/// A query reply refusing a query for `reason`; see [`dispatch_rejection_json`].
#[must_use]
pub fn query_rejection_json(reason: RejectReason) -> String {
    to_json(&QueryReply {
        schema_version: SCHEMA_VERSION,
        result: QueryResult::Rejected {
            reason,
            message: reason.describe().to_owned(),
        },
    })
}

/// An event batch with no events.
#[must_use]
pub fn empty_event_batch_json() -> String {
    to_json(&EventBatch {
        schema_version: SCHEMA_VERSION,
        events: Vec::new(),
    })
}

/// Serialise an outbound document. Every outbound type is plain data, so this
/// cannot fail; a failure would be a bug in these definitions.
pub(crate) fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("session documents always serialise")
}
