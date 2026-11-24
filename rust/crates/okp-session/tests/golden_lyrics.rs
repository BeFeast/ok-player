//! Golden protocol fixtures for local sidecar lyrics queries and events.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{
    Event, EventBatch, LyricsLineView, LyricsState, LyricsView, MediaSource, Query, QueryEnvelope,
    QueryReply, QueryResult, SourceKind,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const CHECKED: &[&str] = &[
    "enums.json",
    "events.json",
    "query-lyrics.json",
    "query-lyrics-at.json",
    "query-reply-synced.json",
    "query-reply-plain.json",
    "query-reply-missing.json",
    "query-reply-invalid.json",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("lyrics")
}

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
        serde_json::to_value(&expected).expect("lyrics protocol serialises"),
        document,
        "{name}: Rust value no longer serialises to the fixture"
    );
    let decoded: T = serde_json::from_value(document)
        .unwrap_or_else(|error| panic!("{name} no longer decodes: {error}"));
    assert_eq!(decoded, expected, "{name}: decoding changed the value");
}

fn local(location: &str) -> MediaSource {
    MediaSource {
        kind: SourceKind::Local,
        location: location.to_owned(),
    }
}

#[test]
fn lyrics_queries_replies_and_events_match_fixtures() {
    golden(
        "query-lyrics.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::Lyrics { at: None },
        },
    );
    golden(
        "query-lyrics-at.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::Lyrics { at: Some(17.5) },
        },
    );
    golden(
        "query-reply-synced.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Lyrics {
                lyrics: LyricsView {
                    source: Some(local("/Users/example/Music/Neon Skyline.mp3")),
                    generation: 2,
                    header: "LYRICS".to_owned(),
                    state: LyricsState::Synced,
                    message: None,
                    lines: vec![
                        LyricsLineView {
                            time_seconds: 0.0,
                            text: "City lights blur into the rain".to_owned(),
                            is_gap: false,
                        },
                        LyricsLineView {
                            time_seconds: 5.4,
                            text: "♪".to_owned(),
                            is_gap: true,
                        },
                        LyricsLineView {
                            time_seconds: 16.8,
                            text: "A neon skyline out of sight".to_owned(),
                            is_gap: false,
                        },
                    ],
                    active_index: Some(2),
                },
            },
        },
    );
    golden(
        "query-reply-plain.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Lyrics {
                lyrics: LyricsView {
                    source: Some(local("/Users/example/Music/Plain Song.flac")),
                    generation: 3,
                    header: "LYRICS · NOT SYNCED".to_owned(),
                    state: LyricsState::Plain,
                    message: None,
                    lines: vec![
                        LyricsLineView {
                            time_seconds: 0.0,
                            text: "Wander down the boulevard".to_owned(),
                            is_gap: false,
                        },
                        LyricsLineView {
                            time_seconds: 0.0,
                            text: "Counting every falling star".to_owned(),
                            is_gap: false,
                        },
                    ],
                    active_index: None,
                },
            },
        },
    );
    golden(
        "query-reply-missing.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Lyrics {
                lyrics: LyricsView {
                    source: Some(local("/Users/example/Music/Instrumental.mp3")),
                    generation: 4,
                    header: "LYRICS".to_owned(),
                    state: LyricsState::Missing,
                    message: Some("No lyrics found for this track".to_owned()),
                    lines: vec![],
                    active_index: None,
                },
            },
        },
    );
    golden(
        "query-reply-invalid.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Lyrics {
                lyrics: LyricsView {
                    source: Some(local("/Users/example/Music/Corrupt.mp3")),
                    generation: 5,
                    header: "LYRICS".to_owned(),
                    state: LyricsState::Invalid,
                    message: Some("Local lyrics file could not be read".to_owned()),
                    lines: vec![],
                    active_index: None,
                },
            },
        },
    );
    golden(
        "events.json",
        EventBatch {
            schema_version: SCHEMA_VERSION,
            events: vec![Event::LyricsChanged {}],
        },
    );
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
struct Enums {
    lyrics_state: Vec<LyricsState>,
}

#[test]
fn every_lyrics_enum_value_matches_the_fixture() {
    let states = vec![
        LyricsState::Unavailable,
        LyricsState::Loading,
        LyricsState::Synced,
        LyricsState::Plain,
        LyricsState::Missing,
        LyricsState::Invalid,
    ];
    for state in &states {
        match state {
            LyricsState::Unavailable
            | LyricsState::Loading
            | LyricsState::Synced
            | LyricsState::Plain
            | LyricsState::Missing
            | LyricsState::Invalid => {}
        }
    }
    golden(
        "enums.json",
        Enums {
            lyrics_state: states,
        },
    );
}

#[test]
fn every_lyrics_fixture_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("lyrics fixtures dir exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}
