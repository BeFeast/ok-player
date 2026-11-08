//! The playlist documents' wire format, pinned by the hand-written documents in
//! `fixtures/playlist/`. Each Rust value must serialise to exactly its fixture
//! and decode back from it.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{
    Command, CommandEnvelope, DispatchReply, DispatchResult, Event, EventBatch, MediaSource,
    PlaylistAction, PlaylistEntry, PlaylistState, Query, QueryEnvelope, QueryReply, QueryResult,
    QueueMode, RepeatMode, SnapshotChange, SourceKind, Status,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/playlist")
}

/// The fixtures the tests below check, so that a new file cannot sit unchecked.
const CHECKED: &[&str] = &[
    "command-open.json",
    "command-next.json",
    "command-previous.json",
    "command-jump.json",
    "command-queue.json",
    "command-play-next.json",
    "command-move.json",
    "command-remove.json",
    "command-clear-queue.json",
    "command-set-repeat.json",
    "command-set-shuffle.json",
    "command-set-shuffle-unseeded.json",
    "reply-applied.json",
    "query-playlist.json",
    "query-reply-playlist.json",
    "query-reply-playlist-empty.json",
    "events-playlist.json",
    "enums.json",
];

fn golden<T>(name: &str, expected: T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    assert!(CHECKED.contains(&name), "{name} is missing from CHECKED");
    let path = fixtures_dir().join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
    let document: Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{name} is not JSON: {error}"));
    assert_eq!(
        serde_json::to_value(&expected).expect("protocol values serialise"),
        document,
        "{name}: the Rust value no longer serialises to the fixture"
    );
    let decoded: T = serde_json::from_value(document)
        .unwrap_or_else(|error| panic!("{name} no longer decodes: {error}"));
    assert_eq!(decoded, expected, "{name}: decoding changed the value");
}

fn playlist(action: PlaylistAction) -> CommandEnvelope {
    CommandEnvelope {
        schema_version: SCHEMA_VERSION,
        command: Command::Playlist { action },
    }
}

fn local(location: &str) -> MediaSource {
    MediaSource {
        kind: SourceKind::Local,
        location: location.to_owned(),
    }
}

fn entry(location: &str, title: &str) -> PlaylistEntry {
    PlaylistEntry {
        source: local(location),
        title: title.to_owned(),
    }
}

#[test]
fn every_playlist_command_matches_its_fixture() {
    golden(
        "command-open.json",
        playlist(PlaylistAction::Open {
            sources: vec![
                local("/Users/example/Movies/Show/Episode 1.mkv"),
                local("/Users/example/Movies/Extras"),
                local("/Users/example/Music/queue.m3u8"),
                MediaSource {
                    kind: SourceKind::Url,
                    location: "https://example.com/clip.mp4".to_owned(),
                },
            ],
        }),
    );
    golden("command-next.json", playlist(PlaylistAction::Next {}));
    golden(
        "command-previous.json",
        playlist(PlaylistAction::Previous {}),
    );
    golden(
        "command-jump.json",
        playlist(PlaylistAction::Jump { index: 3 }),
    );
    golden(
        "command-queue.json",
        playlist(PlaylistAction::Queue {
            sources: vec![local("/Users/example/Movies/next.mkv")],
            mode: QueueMode::PlayNext,
        }),
    );
    golden(
        "command-play-next.json",
        playlist(PlaylistAction::PlayNext { index: 4 }),
    );
    golden(
        "command-move.json",
        playlist(PlaylistAction::Move { from: 2, to: 0 }),
    );
    golden(
        "command-remove.json",
        playlist(PlaylistAction::Remove { index: 1 }),
    );
    golden(
        "command-clear-queue.json",
        playlist(PlaylistAction::ClearQueue {}),
    );
    golden(
        "command-set-repeat.json",
        playlist(PlaylistAction::SetRepeat {
            mode: RepeatMode::One,
        }),
    );
    golden(
        "command-set-shuffle.json",
        playlist(PlaylistAction::SetShuffle {
            enabled: true,
            seed: Some(42),
        }),
    );
    golden(
        "command-set-shuffle-unseeded.json",
        playlist(PlaylistAction::SetShuffle {
            enabled: false,
            seed: None,
        }),
    );
}

#[test]
fn the_applied_reply_matches_its_fixture() {
    golden(
        "reply-applied.json",
        DispatchReply {
            schema_version: SCHEMA_VERSION,
            result: DispatchResult::Applied {},
        },
    );
}

#[test]
fn the_playlist_query_and_its_replies_match_their_fixtures() {
    golden(
        "query-playlist.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::Playlist {},
        },
    );
    golden(
        "query-reply-playlist.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Playlist {
                playlist: PlaylistState {
                    items: vec![
                        entry("/Users/example/Movies/Show/Episode 1.mkv", "Episode 1.mkv"),
                        entry("/Users/example/Movies/Show/Episode 2.mkv", "Episode 2.mkv"),
                        PlaylistEntry {
                            source: MediaSource {
                                kind: SourceKind::Url,
                                location: "https://example.com/clip.mp4?token=a".to_owned(),
                            },
                            title: "clip.mp4".to_owned(),
                        },
                    ],
                    current_index: Some(1),
                    repeat: RepeatMode::All,
                    shuffle: true,
                    scanning: false,
                },
            },
        },
    );
    golden(
        "query-reply-playlist-empty.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Playlist {
                playlist: PlaylistState {
                    items: vec![],
                    current_index: None,
                    repeat: RepeatMode::Off,
                    shuffle: false,
                    scanning: false,
                },
            },
        },
    );
}

#[test]
fn the_playlist_event_follows_the_snapshot_change_in_its_fixture() {
    golden(
        "events-playlist.json",
        EventBatch {
            schema_version: SCHEMA_VERSION,
            events: vec![
                Event::SnapshotChanged {
                    changes: vec![SnapshotChange::Status {
                        value: Status::Opening,
                    }],
                },
                Event::PlaylistChanged {
                    playlist: PlaylistState {
                        items: vec![entry(
                            "/Users/example/Movies/Show/Episode 2.mkv",
                            "Episode 2.mkv",
                        )],
                        current_index: Some(0),
                        repeat: RepeatMode::Off,
                        shuffle: false,
                        scanning: true,
                    },
                },
            ],
        },
    );
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
struct Enums {
    repeat_mode: Vec<RepeatMode>,
    queue_mode: Vec<QueueMode>,
}

#[test]
fn every_playlist_enum_value_matches_the_enums_fixture() {
    // Exhaustive matches: a new value fails to compile until the fixture names it.
    let repeat_mode = vec![RepeatMode::Off, RepeatMode::One, RepeatMode::All];
    for value in &repeat_mode {
        match value {
            RepeatMode::Off | RepeatMode::One | RepeatMode::All => {}
        }
    }
    let queue_mode = vec![QueueMode::Append, QueueMode::PlayNext];
    for value in &queue_mode {
        match value {
            QueueMode::Append | QueueMode::PlayNext => {}
        }
    }
    golden(
        "enums.json",
        Enums {
            repeat_mode,
            queue_mode,
        },
    );
}

#[test]
fn every_playlist_fixture_file_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the playlist fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}

#[test]
fn playlist_documents_that_omit_optional_fields_or_carry_bad_indexes_are_judged() {
    let unseeded: CommandEnvelope = serde_json::from_str(
        r#"{"schema_version":1,"command":{"playlist":{"action":{"set_shuffle":{"enabled":true}}}}}"#,
    )
    .unwrap();
    assert_eq!(
        unseeded.command,
        Command::Playlist {
            action: PlaylistAction::SetShuffle {
                enabled: true,
                seed: None,
            },
        }
    );
    for bad in [
        r#"{"schema_version":1,"command":{"playlist":{"action":{"jump":{"index":-1}}}}}"#,
        r#"{"schema_version":1,"command":{"playlist":{"action":{"next":{"wrap":true}}}}}"#,
        r#"{"schema_version":1,"command":{"playlist":{"action":{"set_repeat":{"mode":"twice"}}}}}"#,
    ] {
        assert!(
            serde_json::from_str::<CommandEnvelope>(bad).is_err(),
            "{bad} must be refused"
        );
    }
}
