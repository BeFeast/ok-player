//! The wire format of the `history` command, query and event groups, pinned by
//! the hand-written documents in `fixtures/history/`. Each Rust value must
//! serialise to exactly its fixture and decode back from it.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_session::SCHEMA_VERSION;
use okp_session::protocol::{
    Command, CommandEnvelope, DispatchReply, DispatchResult, Event, EventBatch, HistoryAction,
    HistoryActionResult, HistoryBucket, HistoryEntry, HistoryEvent, HistoryQuery,
    HistoryQueryResult, HistorySection, HistoryState, MediaSource, Query, QueryEnvelope,
    QueryReply, QueryResult, SourceKind,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/history")
}

/// The fixtures every test below checks, so that a new file cannot sit unchecked.
const CHECKED: &[&str] = &[
    "command-set-private.json",
    "command-remove.json",
    "reply-done.json",
    "reply-unchanged.json",
    "reply-failed.json",
    "query-welcome-shelf.json",
    "query-page.json",
    "query-reply-welcome-shelf.json",
    "query-reply-page.json",
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

fn command(action: HistoryAction) -> CommandEnvelope {
    CommandEnvelope {
        schema_version: SCHEMA_VERSION,
        command: Command::History { action },
    }
}

fn reply(result: HistoryActionResult) -> DispatchReply {
    DispatchReply {
        schema_version: SCHEMA_VERSION,
        result: DispatchResult::History { result },
    }
}

fn query(query: HistoryQuery) -> QueryEnvelope {
    QueryEnvelope {
        schema_version: SCHEMA_VERSION,
        query: Query::History { query },
    }
}

fn query_reply(result: HistoryQueryResult) -> QueryReply {
    QueryReply {
        schema_version: SCHEMA_VERSION,
        result: QueryResult::History { result },
    }
}

#[test]
fn every_history_command_and_reply_matches_its_fixture() {
    golden(
        "command-set-private.json",
        command(HistoryAction::SetPrivate { enabled: true }),
    );
    golden(
        "command-remove.json",
        command(HistoryAction::Remove {
            source: MediaSource {
                kind: SourceKind::Local,
                location: "/Users/example/Movies/film.mkv".to_owned(),
            },
        }),
    );
    golden("reply-done.json", reply(HistoryActionResult::Done {}));
    golden(
        "reply-unchanged.json",
        reply(HistoryActionResult::Unchanged {}),
    );
    golden(
        "reply-failed.json",
        reply(HistoryActionResult::Failed {
            message: "the history could not be saved: Permission denied".to_owned(),
        }),
    );
}

#[test]
fn every_history_query_and_its_answer_match_their_fixtures() {
    golden(
        "query-welcome-shelf.json",
        query(HistoryQuery::WelcomeShelf {
            limit: 8,
            utc_offset_seconds: 7_200,
        }),
    );
    golden(
        "query-page.json",
        query(HistoryQuery::Page {
            search: "severance".to_owned(),
            utc_offset_seconds: -18_000,
        }),
    );
    golden(
        "query-reply-welcome-shelf.json",
        query_reply(HistoryQueryResult::WelcomeShelf {
            private_session: false,
            entries: vec![HistoryEntry {
                source: MediaSource {
                    kind: SourceKind::Local,
                    location: "/Users/example/Movies/Severance/Season 02/Severance S02E03.mkv"
                        .to_owned(),
                },
                title: "Severance S02E03".to_owned(),
                location: "Severance › Season 02".to_owned(),
                position: 1325.5,
                duration: 3120.0,
                progress: 0.42,
                state: HistoryState::Progress,
                state_label: "30m left".to_owned(),
                when_label: "Today 21:14".to_owned(),
                updated_at_unix: 1_699_990_000,
                resume_from: Some(1325.5),
            }],
        }),
    );
    golden(
        "query-reply-page.json",
        query_reply(HistoryQueryResult::Page {
            private_session: true,
            read_failed: false,
            unreadable_copy: None,
            summary: "Everything you’ve opened · kept forever".to_owned(),
            end_cap: "End of history · kept forever".to_owned(),
            sections: vec![HistorySection {
                bucket: HistoryBucket::Earlier,
                header: "EARLIER".to_owned(),
                entries: vec![HistoryEntry {
                    source: MediaSource {
                        kind: SourceKind::Url,
                        location: "https://example.com/watch?v=abc123".to_owned(),
                    },
                    title: "Example talk".to_owned(),
                    location: "https://example.com/watch?v=abc123".to_owned(),
                    position: 0.0,
                    duration: 900.0,
                    progress: 0.0,
                    state: HistoryState::Finished,
                    state_label: "Finished".to_owned(),
                    when_label: "12 Jun".to_owned(),
                    updated_at_unix: 1_686_528_000,
                    resume_from: None,
                }],
            }],
        }),
    );
}

/// Exhaustive on purpose: a new history event does not compile until the
/// events fixture covers it.
fn event_name(event: &HistoryEvent) -> &'static str {
    match event {
        HistoryEvent::Resumed { .. } => "resumed",
        HistoryEvent::Changed {} => "changed",
        HistoryEvent::PrivateSession { .. } => "private_session",
        HistoryEvent::SaveFailed { .. } => "save_failed",
    }
}

#[test]
fn every_history_event_matches_the_events_fixture() {
    let events = vec![
        HistoryEvent::Resumed { position: 754.5 },
        HistoryEvent::Changed {},
        HistoryEvent::PrivateSession { enabled: true },
        HistoryEvent::SaveFailed {
            message: "the history could not be saved: No space left on device".to_owned(),
        },
    ];
    let names: BTreeSet<_> = events.iter().map(event_name).collect();
    assert_eq!(names.len(), 4, "the fixture lists every history event once");
    golden(
        "events.json",
        EventBatch {
            schema_version: SCHEMA_VERSION,
            events: events
                .into_iter()
                .map(|event| Event::History { event })
                .collect(),
        },
    );
}

#[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
struct Enums {
    history_state: Vec<HistoryState>,
    history_bucket: Vec<HistoryBucket>,
}

#[test]
fn every_history_enum_value_matches_the_enums_fixture() {
    let history_state = vec![
        HistoryState::Finished,
        HistoryState::Progress,
        HistoryState::Barely,
    ];
    for value in &history_state {
        match value {
            HistoryState::Finished | HistoryState::Progress | HistoryState::Barely => {}
        }
    }
    let history_bucket = vec![
        HistoryBucket::Today,
        HistoryBucket::Yesterday,
        HistoryBucket::EarlierThisWeek,
        HistoryBucket::Earlier,
    ];
    for value in &history_bucket {
        match value {
            HistoryBucket::Today
            | HistoryBucket::Yesterday
            | HistoryBucket::EarlierThisWeek
            | HistoryBucket::Earlier => {}
        }
    }
    golden(
        "enums.json",
        Enums {
            history_state,
            history_bucket,
        },
    );
}

#[test]
fn every_history_fixture_file_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the history fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}

#[test]
fn a_page_query_without_a_search_decodes_as_all_entries() {
    // Swift's synthesised encoder leaves out an empty optional, and a shell may
    // leave out an empty search.
    let decoded: QueryEnvelope = serde_json::from_str(
        r#"{"schema_version":1,"query":{"history":{"query":{"page":{"utc_offset_seconds":0}}}}}"#,
    )
    .expect("a page query without a search decodes");
    assert_eq!(
        decoded.query,
        Query::History {
            query: HistoryQuery::Page {
                search: String::new(),
                utc_offset_seconds: 0,
            },
        }
    );
}
