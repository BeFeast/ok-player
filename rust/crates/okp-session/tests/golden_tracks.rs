//! The track documents' wire format, pinned by `fixtures/tracks/`. The labels in
//! the reply fixture are the core's for the tracks described here.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_core::{subtitle_delay, track_label};
use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{
    Command, CommandEnvelope, Query, QueryEnvelope, QueryReply, QueryResult, SubtitleValue,
    TrackChoice, TracksView,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/tracks")
}

const CHECKED: &[&str] = &[
    "command-add-subtitle.json",
    "command-set-subtitle-scale.json",
    "command-nudge-subtitle-scale.json",
    "query-tracks.json",
    "query-reply-tracks.json",
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

fn command(command: Command) -> CommandEnvelope {
    CommandEnvelope {
        schema_version: SCHEMA_VERSION,
        command,
    }
}

#[test]
fn the_track_commands_match_their_fixtures() {
    golden(
        "command-add-subtitle.json",
        command(Command::AddSubtitle {
            path: "/Users/example/Movies/film.en.srt".to_owned(),
        }),
    );
    golden(
        "command-set-subtitle-scale.json",
        command(Command::SetSubtitleScale { scale: 1.2 }),
    );
    golden(
        "command-nudge-subtitle-scale.json",
        command(Command::NudgeSubtitleScale { delta: -0.1 }),
    );
}

#[test]
fn the_tracks_query_matches_its_fixture() {
    golden(
        "query-tracks.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::Tracks {},
        },
    );
}

#[test]
fn the_reply_fixture_carries_the_core_labels() {
    let choice = |id, label: String, selected| TrackChoice {
        id,
        label,
        selected,
    };
    let expected = TracksView {
        audio: vec![
            choice(
                Some(1),
                track_label::audio_track_label(
                    1,
                    Some("English"),
                    Some("eng"),
                    Some("5.1"),
                    Some("eac3"),
                ),
                true,
            ),
            choice(
                Some(2),
                track_label::audio_track_label(
                    2,
                    Some("Director's Commentary"),
                    Some("eng"),
                    Some("2.0"),
                    Some("ac3"),
                ),
                false,
            ),
        ],
        subtitles: vec![
            choice(None, "Off".to_owned(), false),
            choice(
                Some(1),
                track_label::subtitle_track_label(
                    1,
                    Some("English SDH"),
                    Some("eng"),
                    Some("hdmv_pgs_subtitle"),
                    None,
                    false,
                    true,
                ),
                false,
            ),
            choice(
                Some(2),
                track_label::subtitle_track_label(
                    2,
                    None,
                    Some("en"),
                    Some("subrip"),
                    Some("/Users/example/Movies/film.en.srt"),
                    true,
                    false,
                ),
                true,
            ),
        ],
        subtitle_delay: SubtitleValue {
            value: 0.25,
            label: subtitle_delay::format_label(0.25),
            reported: Some(0.25),
        },
        subtitle_scale: SubtitleValue {
            value: 1.1,
            label: "110%".to_owned(),
            // The size asked for is on its way: libmpv still reports 100 %.
            reported: Some(1.0),
        },
        adjustable: true,
    };
    golden(
        "query-reply-tracks.json",
        QueryReply {
            schema_version: SCHEMA_VERSION,
            result: QueryResult::Tracks { tracks: expected },
        },
    );
}

#[test]
fn every_track_fixture_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the track fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}
