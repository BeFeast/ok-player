//! The open documents' wire format, pinned by `fixtures/open/`. Each reply fixture
//! is what the core rules answer for its query, so the fixtures also pin which
//! paths open, which load as subtitles and which do nothing.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use okp_session::SCHEMA_VERSION;
use okp_session::open_paths::{self, OpenFormats, OpenPlan};
use okp_session::protocol::{Query, QueryEnvelope, QueryReply, QueryResult, Status};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/open")
}

const CHECKED: &[&str] = &[
    "query-open-formats.json",
    "query-reply-open-formats.json",
    "query-open-paths.json",
    "query-reply-open-paths-open.json",
    "query-reply-open-paths-add-subtitles.json",
    "query-reply-open-paths-nothing.json",
];

/// The paths of `query-open-paths.json`: a folder, a film, its subtitle, an M3U
/// file and a document.
const DROPPED: &[&str] = &[
    "/Users/example/Movies/Show",
    "/Users/example/Movies/film.mkv",
    "/Users/example/Movies/film.en.srt",
    "/Users/example/Music/queue.m3u8",
    "/Users/example/Documents/notes.pdf",
];

/// Those of [`DROPPED`] the shell knows are folders.
const FOLDERS: &[&str] = &["/Users/example/Movies/Show"];

const SUBTITLE: &str = "/Users/example/Movies/film.en.srt";

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

fn reply(result: QueryResult) -> QueryReply {
    QueryReply {
        schema_version: SCHEMA_VERSION,
        result,
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn the_open_formats_query_and_its_reply_match_their_fixtures() {
    golden("query-open-formats.json", query(Query::OpenFormats {}));
    golden(
        "query-reply-open-formats.json",
        reply(QueryResult::OpenFormats {
            formats: OpenFormats::current(),
        }),
    );
}

#[test]
fn the_open_paths_query_matches_its_fixture() {
    golden(
        "query-open-paths.json",
        query(Query::OpenPaths {
            paths: strings(DROPPED),
            folders: strings(FOLDERS),
        }),
    );
}

#[test]
fn the_plan_replies_are_what_the_core_rules_answer() {
    // Media, a folder and an M3U file open; the subtitle and the document beside
    // them are left out.
    golden(
        "query-reply-open-paths-open.json",
        reply(QueryResult::OpenPaths {
            plan: open_paths::plan(&strings(DROPPED), &strings(FOLDERS), Status::Playing),
        }),
    );
    // A subtitle alone loads onto the media that plays.
    let subtitle = strings(&[SUBTITLE]);
    golden(
        "query-reply-open-paths-add-subtitles.json",
        reply(QueryResult::OpenPaths {
            plan: open_paths::plan(&subtitle, &[], Status::Playing),
        }),
    );
    // Without media it does nothing.
    golden(
        "query-reply-open-paths-nothing.json",
        reply(QueryResult::OpenPaths {
            plan: open_paths::plan(&subtitle, &[], Status::Idle),
        }),
    );
    assert_eq!(
        open_paths::plan(&subtitle, &[], Status::Idle),
        OpenPlan::Nothing {}
    );
}

#[test]
fn every_open_fixture_is_checked() {
    let on_disk: BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("the open fixtures directory exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    let checked: BTreeSet<String> = CHECKED.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(on_disk, checked);
}
