//! The status documents' wire format, pinned by `fixtures/status/`. The reply
//! fixtures are projections of known states, so `query-reply-failed.json` also
//! pins the core's classification, wording and actions for a failed load.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_core::playback_failure::{CodecEnvironment, diagnose_mpv_failure};
use okp_core::playlist::PlaylistItem;
use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{
    Event, EventBatch, FailureAction, FailureCard, FailureKind, MediaSource, Query, QueryEnvelope,
    QueryReply, QueryResult, SessionSnapshot, SourceKind, Status, StatusSurface,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/status")
}

const CHECKED: &[&str] = &[
    "query-status-surface.json",
    "query-reply-clear.json",
    "query-reply-loading.json",
    "query-reply-buffering.json",
    "query-reply-failed.json",
    "events.json",
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

fn reply(surface: StatusSurface) -> QueryReply {
    QueryReply {
        schema_version: SCHEMA_VERSION,
        result: QueryResult::StatusSurface { surface },
    }
}

const FILM: &str = "/Users/example/Movies/film.mkv";

fn film(status: Status) -> SessionSnapshot {
    SessionSnapshot {
        status,
        source: Some(MediaSource {
            kind: SourceKind::Local,
            location: FILM.to_owned(),
        }),
        ..SessionSnapshot::default()
    }
}

#[test]
fn the_status_query_matches_its_fixture() {
    golden(
        "query-status-surface.json",
        QueryEnvelope {
            schema_version: SCHEMA_VERSION,
            query: Query::StatusSurface {},
        },
    );
}

#[test]
fn the_reply_fixtures_are_the_projections_of_their_states() {
    golden(
        "query-reply-clear.json",
        reply(StatusSurface::project(&film(Status::Playing), None, None)),
    );
    golden(
        "query-reply-loading.json",
        reply(StatusSurface::project(&film(Status::Opening), None, None)),
    );
    let stalled = SessionSnapshot {
        buffering: true,
        ..film(Status::Playing)
    };
    golden(
        "query-reply-buffering.json",
        reply(StatusSurface::project(&stalled, None, Some(45))),
    );
    // A missing file: libmpv's loading error with a log line that names neither
    // a codec nor the renderer.
    let diagnostic = diagnose_mpv_failure(
        -13,
        &[format!("Failed to open {FILM}.")],
        CodecEnvironment::System,
    );
    let card = FailureCard::new(&PlaylistItem::Local(FILM.into()), &diagnostic);
    golden(
        "query-reply-failed.json",
        reply(StatusSurface::project(
            &film(Status::Ended),
            Some(&card),
            None,
        )),
    );
}

#[test]
fn the_notice_event_matches_its_fixture() {
    golden(
        "events.json",
        EventBatch {
            schema_version: SCHEMA_VERSION,
            events: vec![
                Event::Notice {
                    text: "Volume 72%".to_owned(),
                },
                Event::Notice {
                    text: "Resumed at 12:34".to_owned(),
                },
            ],
        },
    );
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
struct Enums {
    failure_kind: Vec<FailureKind>,
    failure_action: Vec<FailureAction>,
}

#[test]
fn every_failure_enum_value_matches_the_enums_fixture() {
    let failure_kind = vec![
        FailureKind::MissingCodec,
        FailureKind::Renderer,
        FailureKind::Application,
    ];
    for value in &failure_kind {
        match value {
            FailureKind::MissingCodec | FailureKind::Renderer | FailureKind::Application => {}
        }
    }
    let failure_action = vec![
        FailureAction::Retry,
        FailureAction::OpenAnother,
        FailureAction::CopyDetails,
    ];
    for value in &failure_action {
        match value {
            FailureAction::Retry | FailureAction::OpenAnother | FailureAction::CopyDetails => {}
        }
    }
    golden(
        "enums.json",
        Enums {
            failure_kind,
            failure_action,
        },
    );
}

#[test]
fn every_status_fixture_file_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the status fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}
