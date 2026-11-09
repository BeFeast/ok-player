//! The protocol's wire format, pinned by the hand-written documents in
//! `fixtures/`. Each Rust value must serialise to exactly its fixture and decode
//! back from it.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_core::app::{AppIdentity, Platform};
use okp_session::protocol::{
    Chapter, Command, CommandEnvelope, CommandGroup, CommandMenu, DispatchReply, DispatchResult,
    EndReason, ErrorKind, Event, EventBatch, MediaSource, MpvOption, Query, QueryEnvelope,
    QueryReply, QueryResult, RejectReason, SeekMode, SeekPrecision, SessionConfig, SessionSnapshot,
    SnapshotChange, SourceKind, Status, Track, TrackKind, VideoClickIntent, VideoGeometry,
    WindowRect, WindowSize,
};
use okp_session::{CommandRegistry, SCHEMA_VERSION, WindowState};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture(name: &str) -> Value {
    let path = fixtures_dir().join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{name} is not JSON: {error}"))
}

/// The fixtures every test below checks, so that a new file cannot sit unchecked.
const CHECKED: &[&str] = &[
    "config.json",
    "command-open-local.json",
    "command-open-url.json",
    "command-close.json",
    "command-set-paused.json",
    "command-toggle-pause.json",
    "command-seek.json",
    "command-seek-exact.json",
    "command-set-volume.json",
    "command-set-mute.json",
    "command-set-speed.json",
    "command-select-track.json",
    "command-select-track-off.json",
    "command-set-subtitle-delay.json",
    "command-invoke-command.json",
    "reply-accepted.json",
    "reply-no-op.json",
    "reply-rejected.json",
    "reply-failed.json",
    "reply-perform.json",
    "query-snapshot.json",
    "query-reply-snapshot.json",
    "query-reply-rejected.json",
    "query-window-fit.json",
    "query-initial-window-fit.json",
    "query-reply-window-fit.json",
    "query-video-click.json",
    "query-reply-video-click.json",
    "query-command-registry.json",
    "query-reply-command-registry.json",
    "query-shortcut-table.json",
    "query-reply-shortcut-table.json",
    "query-repeating-shortcuts.json",
    "query-reply-repeating-shortcuts.json",
    "events-batch.json",
    "events-empty.json",
    "enums.json",
];

fn golden<T>(name: &str, expected: T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    assert!(CHECKED.contains(&name), "{name} is missing from CHECKED");
    let document = fixture(name);
    assert_eq!(
        serde_json::to_value(&expected).expect("protocol values serialise"),
        document,
        "{name}: the Rust value no longer serialises to the fixture"
    );
    let decoded: T = serde_json::from_value(document)
        .unwrap_or_else(|error| panic!("{name} no longer decodes: {error}"));
    assert_eq!(decoded, expected, "{name}: decoding changed the value");
}

fn command(command: Command) -> CommandEnvelope {
    CommandEnvelope {
        schema_version: SCHEMA_VERSION,
        command,
    }
}

fn reply(result: DispatchResult) -> DispatchReply {
    DispatchReply {
        schema_version: SCHEMA_VERSION,
        result,
    }
}

fn local(location: &str) -> MediaSource {
    MediaSource {
        kind: SourceKind::Local,
        location: location.to_owned(),
    }
}

#[test]
fn the_config_matches_its_fixture() {
    golden(
        "config.json",
        SessionConfig {
            schema_version: 1,
            app: AppIdentity {
                name: "OK Player".to_owned(),
                platform: okp_core::app::Platform::MacOs,
            },
            data_dir: "/Users/example/Library/Application Support/OK Player".into(),
            config_dir: "/Users/example/Library/Preferences/OK Player".into(),
            cache_dir: "/Users/example/Library/Caches/OK Player".into(),
            hwdec: Some("videotoolbox".to_owned()),
            mpv_options: vec![MpvOption {
                name: "ao".to_owned(),
                value: "coreaudio".to_owned(),
            }],
            shell_commands: vec!["open-file".to_owned(), "fullscreen".to_owned()],
        },
    );
}

#[test]
fn every_command_matches_its_fixture() {
    golden(
        "command-open-local.json",
        command(Command::Open {
            source: local("/Users/example/Movies/film.mkv"),
            resume_from: Some(754.5),
            audio_track: Some(2),
            subtitle_track: Some(3),
        }),
    );
    golden(
        "command-open-url.json",
        command(Command::Open {
            source: MediaSource {
                kind: SourceKind::Url,
                location: "https://example.com/stream.m3u8".to_owned(),
            },
            resume_from: None,
            audio_track: None,
            subtitle_track: None,
        }),
    );
    golden("command-close.json", command(Command::Close {}));
    golden(
        "command-set-paused.json",
        command(Command::SetPaused { paused: true }),
    );
    golden(
        "command-toggle-pause.json",
        command(Command::TogglePause {}),
    );
    golden(
        "command-seek.json",
        command(Command::Seek {
            seconds: -5.0,
            mode: SeekMode::Relative,
            precision: SeekPrecision::Keyframe,
        }),
    );
    golden(
        "command-seek-exact.json",
        command(Command::Seek {
            seconds: 90.25,
            mode: SeekMode::Absolute,
            precision: SeekPrecision::Exact,
        }),
    );
    golden(
        "command-set-volume.json",
        command(Command::SetVolume { volume: 85.0 }),
    );
    golden(
        "command-set-mute.json",
        command(Command::SetMute { muted: true }),
    );
    golden(
        "command-set-speed.json",
        command(Command::SetSpeed { speed: 1.5 }),
    );
    golden(
        "command-select-track.json",
        command(Command::SelectTrack {
            kind: TrackKind::Subtitle,
            id: Some(2),
        }),
    );
    golden(
        "command-select-track-off.json",
        command(Command::SelectTrack {
            kind: TrackKind::Audio,
            id: None,
        }),
    );
    golden(
        "command-set-subtitle-delay.json",
        command(Command::SetSubtitleDelay { seconds: -0.25 }),
    );
    golden(
        "command-invoke-command.json",
        command(Command::InvokeCommand {
            id: "mute".to_owned(),
        }),
    );
}

#[test]
fn every_reply_matches_its_fixture() {
    golden(
        "reply-accepted.json",
        reply(DispatchResult::Accepted { request_id: 7 }),
    );
    golden("reply-no-op.json", reply(DispatchResult::NoOp {}));
    golden(
        "reply-rejected.json",
        reply(DispatchResult::Rejected {
            reason: RejectReason::NoActiveMedia,
            message: "the command needs loaded media".to_owned(),
        }),
    );
    golden(
        "reply-failed.json",
        reply(DispatchResult::Failed {
            request_id: 8,
            message: "error running command".to_owned(),
        }),
    );
    golden("reply-perform.json", reply(DispatchResult::Perform {}));
}

#[test]
fn the_snapshot_query_and_its_replies_match_their_fixtures() {
    golden(
        "query-snapshot.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::Snapshot {},
        },
    );
    golden(
        "query-reply-snapshot.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Snapshot {
                snapshot: SessionSnapshot {
                    status: Status::Playing,
                    source: Some(local("/Users/example/Movies/film.mkv")),
                    time_pos: Some(12.5),
                    duration: Some(5400.0),
                    volume: Some(100.0),
                    mute: false,
                    speed: Some(1.0),
                    subtitle_delay: 0.0,
                    tracks: vec![
                        Track {
                            id: 1,
                            kind: TrackKind::Audio,
                            selected: true,
                            external: false,
                            default: true,
                            title: Some("Stereo".to_owned()),
                            lang: Some("eng".to_owned()),
                        },
                        Track {
                            id: 1,
                            kind: TrackKind::Subtitle,
                            selected: false,
                            external: true,
                            default: false,
                            title: None,
                            lang: None,
                        },
                    ],
                    chapters: vec![
                        Chapter {
                            index: 0,
                            time: 0.0,
                            title: Some("Opening".to_owned()),
                        },
                        Chapter {
                            index: 1,
                            time: 300.0,
                            title: None,
                        },
                    ],
                    end_reason: None,
                    title: Some("film.mkv".to_owned()),
                    video: Some(VideoGeometry {
                        width: 1920,
                        height: 800,
                        display_aspect: 2.4,
                    }),
                    buffering: false,
                    cache_ahead: Some(20.0),
                },
            },
        },
    );
    golden(
        "query-reply-rejected.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Rejected {
                reason: RejectReason::UnsupportedSchemaVersion,
                message: "schema_version 2 is not supported; this session speaks 1".to_owned(),
            },
        },
    );
}

#[test]
fn the_window_queries_and_their_replies_match_their_fixtures() {
    golden(
        "query-window-fit.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::WindowFit {
                video: WindowSize {
                    width: 3840,
                    height: 2160,
                },
                backing_scale: 2.0,
                visible_frame: WindowRect {
                    x: 0,
                    y: 0,
                    width: 1512,
                    height: 945,
                },
                min_size: Some(WindowSize {
                    width: 420,
                    height: 280,
                }),
                chrome: Some(WindowSize {
                    width: 0,
                    height: 48,
                }),
            },
        },
    );
    golden(
        "query-initial-window-fit.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::InitialWindowFit {
                backing_scale: 2.0,
                visible_frame: WindowRect {
                    x: 0,
                    y: 0,
                    width: 1512,
                    height: 945,
                },
                min_size: Some(WindowSize {
                    width: 420,
                    height: 280,
                }),
                chrome: Some(WindowSize {
                    width: 0,
                    height: 48,
                }),
            },
        },
    );
    golden(
        "query-reply-window-fit.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::WindowFit {
                frame: Some(WindowRect {
                    x: 45,
                    y: 49,
                    width: 1421,
                    height: 847,
                }),
            },
        },
    );
    golden(
        "query-video-click.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::VideoClick { press_count: 2 },
        },
    );
    golden(
        "query-reply-video-click.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::VideoClick {
                intent: VideoClickIntent::CancelPlayPauseAndToggleFullscreen,
            },
        },
    );
}

/// The macOS shell of the registry fixtures: it opens files and folders and owns
/// the window and the track and speed menus.
fn macos_registry() -> CommandRegistry {
    let shell_commands = [
        "open-file",
        "open-folder",
        "fullscreen",
        "escape-fullscreen",
        "fit-window-to-media",
        "subtitles",
        "audio-track",
        "playback-speed",
    ]
    .map(str::to_owned);
    CommandRegistry::new(Platform::MacOs, &shell_commands).expect("known command ids")
}

#[test]
fn the_command_registry_query_and_its_reply_match_their_fixtures() {
    golden(
        "query-command-registry.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::CommandRegistry {
                fullscreen: true,
                mini_player: false,
            },
        },
    );
    // A muted local film playing fullscreen: the whole registry as the session
    // resolves it, with the macOS chords.
    let snapshot = SessionSnapshot {
        status: Status::Playing,
        source: Some(local("/Users/example/Movies/film.mkv")),
        time_pos: Some(12.5),
        duration: Some(5400.0),
        volume: Some(100.0),
        mute: true,
        speed: Some(1.0),
        video: Some(VideoGeometry {
            width: 1920,
            height: 800,
            display_aspect: 2.4,
        }),
        ..SessionSnapshot::default()
    };
    golden(
        "query-reply-command-registry.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::CommandRegistry {
                commands: macos_registry().resolve(
                    &snapshot,
                    WindowState {
                        fullscreen: true,
                        mini_player: false,
                    },
                    1,
                ),
            },
        },
    );
}

#[test]
fn the_shortcut_table_query_and_its_reply_match_their_fixtures() {
    golden(
        "query-shortcut-table.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::ShortcutTable {},
        },
    );
    golden(
        "query-reply-shortcut-table.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::ShortcutTable {
                shortcuts: macos_registry().shortcut_table(),
            },
        },
    );
}

#[test]
fn the_repeating_shortcuts_query_and_its_reply_match_their_fixtures() {
    golden(
        "query-repeating-shortcuts.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::RepeatingShortcuts {},
        },
    );
    golden(
        "query-reply-repeating-shortcuts.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::RepeatingShortcuts {
                actions: macos_registry().repeating_shortcuts(),
            },
        },
    );
}

#[test]
fn a_registry_query_without_window_state_decodes() {
    let query: QueryEnvelope =
        serde_json::from_str(r#"{"schema_version":1,"query":{"command_registry":{}}}"#).unwrap();
    assert_eq!(
        query.query,
        Query::CommandRegistry {
            fullscreen: false,
            mini_player: false,
        }
    );
}

/// Exhaustive on purpose: a new change variant does not compile until the batch
/// fixture covers it.
fn change_name(change: &SnapshotChange) -> &'static str {
    match change {
        SnapshotChange::Status { .. } => "status",
        SnapshotChange::Source { .. } => "source",
        SnapshotChange::TimePos { .. } => "time_pos",
        SnapshotChange::Duration { .. } => "duration",
        SnapshotChange::Volume { .. } => "volume",
        SnapshotChange::Mute { .. } => "mute",
        SnapshotChange::Speed { .. } => "speed",
        SnapshotChange::SubtitleDelay { .. } => "subtitle_delay",
        SnapshotChange::Tracks { .. } => "tracks",
        SnapshotChange::Chapters { .. } => "chapters",
        SnapshotChange::EndReason { .. } => "end_reason",
        SnapshotChange::Title { .. } => "title",
        SnapshotChange::Video { .. } => "video",
        SnapshotChange::Buffering { .. } => "buffering",
        SnapshotChange::CacheAhead { .. } => "cache_ahead",
    }
}

#[test]
fn every_event_and_snapshot_change_matches_the_batch_fixture() {
    let changes = vec![
        SnapshotChange::Status {
            value: Status::Ended,
        },
        SnapshotChange::Source {
            value: Some(MediaSource {
                kind: SourceKind::Url,
                location: "https://example.com/a.mp4".to_owned(),
            }),
        },
        SnapshotChange::TimePos { value: None },
        SnapshotChange::Duration { value: Some(61.0) },
        SnapshotChange::Volume { value: Some(70.0) },
        SnapshotChange::Mute { value: true },
        SnapshotChange::Speed { value: Some(2.0) },
        SnapshotChange::SubtitleDelay { value: 0.5 },
        SnapshotChange::Tracks {
            value: vec![Track {
                id: 2,
                kind: TrackKind::Audio,
                selected: true,
                external: false,
                default: false,
                title: None,
                lang: Some("deu".to_owned()),
            }],
        },
        SnapshotChange::Chapters { value: vec![] },
        SnapshotChange::EndReason {
            value: Some(EndReason::Eof),
        },
        SnapshotChange::Title {
            value: Some("a.mp4".to_owned()),
        },
        SnapshotChange::Video { value: None },
        SnapshotChange::Buffering { value: true },
        SnapshotChange::CacheAhead { value: Some(3.5) },
    ];
    let names: BTreeSet<_> = changes.iter().map(change_name).collect();
    assert_eq!(names.len(), 15, "the batch fixture lists every change once");

    golden(
        "events-batch.json",
        EventBatch {
            schema_version: SCHEMA_VERSION,
            events: vec![
                Event::SnapshotChanged { changes },
                Event::Error {
                    kind: ErrorKind::LoadFailed,
                    message: "loading failed".to_owned(),
                    request_id: None,
                },
                Event::Error {
                    kind: ErrorKind::CommandFailed,
                    message: "property unavailable".to_owned(),
                    request_id: Some(4),
                },
                Event::Shutdown {},
            ],
        },
    );
    golden(
        "events-empty.json",
        EventBatch {
            schema_version: SCHEMA_VERSION,
            events: vec![],
        },
    );
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
struct Enums {
    status: Vec<Status>,
    end_reason: Vec<EndReason>,
    reject_reason: Vec<RejectReason>,
    error_kind: Vec<ErrorKind>,
    track_kind: Vec<TrackKind>,
    source_kind: Vec<SourceKind>,
    seek_mode: Vec<SeekMode>,
    seek_precision: Vec<SeekPrecision>,
    video_click_intent: Vec<VideoClickIntent>,
    command_group: Vec<CommandGroup>,
    command_menu: Vec<CommandMenu>,
}

#[test]
fn every_enum_value_matches_the_enums_fixture() {
    // Each list is checked exhaustive by a match, so a new value fails to compile
    // until the fixture names it.
    let status = vec![
        Status::Idle,
        Status::Opening,
        Status::Playing,
        Status::Paused,
        Status::Ended,
    ];
    for value in &status {
        match value {
            Status::Idle | Status::Opening | Status::Playing | Status::Paused | Status::Ended => {}
        }
    }
    let end_reason = vec![
        EndReason::Eof,
        EndReason::Stopped,
        EndReason::Quit,
        EndReason::Error,
        EndReason::Redirect,
    ];
    for value in &end_reason {
        match value {
            EndReason::Eof
            | EndReason::Stopped
            | EndReason::Quit
            | EndReason::Error
            | EndReason::Redirect => {}
        }
    }
    let reject_reason = vec![
        RejectReason::NoActiveMedia,
        RejectReason::NotFinite,
        RejectReason::Malformed,
        RejectReason::UnsupportedSchemaVersion,
        RejectReason::InvalidArgument,
        RejectReason::UnknownCommand,
        RejectReason::Unavailable,
    ];
    for value in &reject_reason {
        match value {
            RejectReason::NoActiveMedia
            | RejectReason::NotFinite
            | RejectReason::Malformed
            | RejectReason::UnsupportedSchemaVersion
            | RejectReason::InvalidArgument
            | RejectReason::UnknownCommand
            | RejectReason::Unavailable => {}
        }
    }
    let command_group = vec![
        CommandGroup::PlaybackNavigation,
        CommandGroup::TracksSubtitles,
        CommandGroup::ViewWindow,
        CommandGroup::MediaFile,
        CommandGroup::Tools,
        CommandGroup::Settings,
    ];
    for value in &command_group {
        match value {
            CommandGroup::PlaybackNavigation
            | CommandGroup::TracksSubtitles
            | CommandGroup::ViewWindow
            | CommandGroup::MediaFile
            | CommandGroup::Tools
            | CommandGroup::Settings => {}
        }
    }
    let command_menu = vec![
        CommandMenu::TopLevel,
        CommandMenu::Video,
        CommandMenu::Playback,
        CommandMenu::Window,
        CommandMenu::ToolsAdvanced,
    ];
    for value in &command_menu {
        match value {
            CommandMenu::TopLevel
            | CommandMenu::Video
            | CommandMenu::Playback
            | CommandMenu::Window
            | CommandMenu::ToolsAdvanced => {}
        }
    }
    let error_kind = vec![
        ErrorKind::LoadFailed,
        ErrorKind::CommandFailed,
        ErrorKind::Engine,
    ];
    for value in &error_kind {
        match value {
            ErrorKind::LoadFailed | ErrorKind::CommandFailed | ErrorKind::Engine => {}
        }
    }
    let video_click_intent = vec![
        VideoClickIntent::Ignore,
        VideoClickIntent::SchedulePlayPause,
        VideoClickIntent::CancelPlayPauseAndToggleFullscreen,
    ];
    for value in &video_click_intent {
        match value {
            VideoClickIntent::Ignore
            | VideoClickIntent::SchedulePlayPause
            | VideoClickIntent::CancelPlayPauseAndToggleFullscreen => {}
        }
    }
    golden(
        "enums.json",
        Enums {
            status,
            end_reason,
            reject_reason,
            error_kind,
            track_kind: vec![TrackKind::Audio, TrackKind::Subtitle],
            source_kind: vec![SourceKind::Local, SourceKind::Url],
            seek_mode: vec![SeekMode::Absolute, SeekMode::Relative],
            seek_precision: vec![SeekPrecision::Exact, SeekPrecision::Keyframe],
            video_click_intent,
            command_group,
            command_menu,
        },
    );
}

#[test]
fn every_fixture_file_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        // The playlist's documents are checked by `golden_playlist.rs`.
        .filter(|name| name.ends_with(".json") && !name.starts_with("playlist-"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}

#[test]
fn documents_that_omit_optional_fields_decode() {
    // Swift's synthesised encoder leaves out `nil` values instead of writing null.
    let open: CommandEnvelope = serde_json::from_str(
        r#"{"schema_version":1,"command":{"open":{"source":{"kind":"local","location":"/a.mkv"}}}}"#,
    )
    .unwrap();
    assert_eq!(
        open.command,
        Command::Open {
            source: local("/a.mkv"),
            resume_from: None,
            audio_track: None,
            subtitle_track: None,
        }
    );
    let off: CommandEnvelope = serde_json::from_str(
        r#"{"schema_version":1,"command":{"select_track":{"kind":"subtitle"}}}"#,
    )
    .unwrap();
    assert_eq!(
        off.command,
        Command::SelectTrack {
            kind: TrackKind::Subtitle,
            id: None,
        }
    );
    // Unknown fields are a bug in the sender, not something to ignore silently.
    assert!(
        serde_json::from_str::<CommandEnvelope>(
            r#"{"schema_version":1,"command":{"close":{"now":true}}}"#
        )
        .is_err()
    );
}
