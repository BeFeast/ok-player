//! The transport documents' wire format, pinned by `fixtures/transport/`. The
//! reply fixture is the projection of a known snapshot, so it also pins the core
//! rules behind the time readout, the scrubber and the volume and speed controls.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{
    Chapter, Command, CommandEnvelope, MediaSource, Query, QueryEnvelope, QueryReply, QueryResult,
    SessionSnapshot, SourceKind, Status, TransportView,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/transport")
}

const CHECKED: &[&str] = &[
    "query-transport.json",
    "query-transport-at.json",
    "query-reply-transport.json",
    "command-nudge-volume.json",
    "command-toggle-mute.json",
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

fn query(query: Query) -> QueryEnvelope {
    QueryEnvelope {
        schema_version: SCHEMA_VERSION,
        query,
    }
}

#[test]
fn the_transport_queries_match_their_fixtures() {
    golden("query-transport.json", query(Query::Transport { at: None }));
    golden(
        "query-transport-at.json",
        query(Query::Transport { at: Some(95.5) }),
    );
    // Swift leaves a nil `at` out; that reads as no position.
    let bare: QueryEnvelope =
        serde_json::from_str(r#"{"schema_version":1,"query":{"transport":{}}}"#).unwrap();
    assert_eq!(bare.query, Query::Transport { at: None });
}

#[test]
fn the_nudge_command_matches_its_fixture() {
    golden(
        "command-nudge-volume.json",
        CommandEnvelope {
            schema_version: SCHEMA_VERSION,
            command: Command::NudgeVolume { delta: -2.0 },
        },
    );
}

#[test]
fn the_mute_toggle_matches_its_fixture() {
    golden(
        "command-toggle-mute.json",
        CommandEnvelope {
            schema_version: SCHEMA_VERSION,
            command: Command::ToggleMute {},
        },
    );
}

#[test]
fn the_reply_fixture_is_the_projection_of_a_playing_snapshot() {
    let chapter = |index, time| Chapter {
        index,
        time,
        title: None,
    };
    let snapshot = SessionSnapshot {
        status: Status::Playing,
        source: Some(MediaSource {
            kind: SourceKind::Local,
            location: "/Users/example/Movies/film.mkv".to_owned(),
        }),
        time_pos: Some(1000.0),
        duration: Some(4000.0),
        volume: Some(117.0),
        speed: Some(1.25),
        chapters: vec![chapter(0, 0.0), chapter(1, 2000.0), chapter(2, 3000.0)],
        cache_ahead: Some(600.0),
        ..SessionSnapshot::default()
    };
    golden(
        "query-reply-transport.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Transport {
                transport: TransportView::project(&snapshot, None),
            },
        },
    );
}

#[test]
fn every_transport_fixture_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the transport fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}
