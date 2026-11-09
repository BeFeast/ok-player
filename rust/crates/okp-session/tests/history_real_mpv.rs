//! Watch history over the real libmpv: a reopened file starts where it was left
//! and gets its tracks and subtitle delay back, through the JSON documents a
//! shell exchanges with the session.

mod common;

use std::fs;
use std::path::Path;

use common::{engine_lock, headless_config, wait_for, write_two_audio_fixture};
use okp_core::history::History;
use okp_session::Session;
use okp_session::history::HISTORY_FILE_NAME;
use okp_session::protocol::{
    Command, DispatchResult, Event, HistoryEvent, MediaSource, SeekMode, SeekPrecision,
    SessionSnapshot, SourceKind, Status, TrackKind,
};
use okp_test_fixtures::unique_temp_dir;

/// The state a shell builds from drained batches, and the history events seen.
struct Store {
    state: SessionSnapshot,
    history: Vec<HistoryEvent>,
}

impl Store {
    fn drain(&mut self, session: &Session) {
        for event in session.drain_events() {
            match event {
                Event::SnapshotChanged { changes } => {
                    for change in &changes {
                        self.state.apply(change);
                    }
                }
                Event::History { event } => self.history.push(event),
                Event::Error { .. } | Event::Shutdown {} | Event::PlaylistChanged { .. } => {}
            }
        }
    }

    fn selected(&self, kind: TrackKind) -> Option<i64> {
        self.state
            .tracks
            .iter()
            .find(|track| track.kind == kind && track.selected)
            .map(|track| track.id)
    }

    fn loaded(&self, location: &str) -> bool {
        self.state
            .source
            .as_ref()
            .is_some_and(|source| source.location == location)
            && self.state.status == Status::Paused
            && self.selected(TrackKind::Audio).is_some()
    }
}

fn accepted(reply: DispatchResult) {
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "the command was not accepted: {reply:?}"
    );
}

fn open(location: &str) -> Command {
    Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: location.to_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    }
}

#[test]
fn a_reopened_file_resumes_and_gets_its_tracks_and_delay_back() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-history-real-mpv");
    let media = write_two_audio_fixture(root.path());
    let location = media.to_string_lossy().into_owned();
    let config = headless_config(root.path());
    let session = Session::new(&config).expect("libmpv starts");
    let mut store = Store {
        state: SessionSnapshot::default(),
        history: Vec::new(),
    };

    accepted(session.dispatch(&open(&location)));
    assert!(
        wait_for(|| {
            store.drain(&session);
            store.loaded(&location)
        }),
        "the file never loaded: {:?}",
        store.state
    );
    assert_eq!(store.selected(TrackKind::Audio), Some(1));
    for command in [
        Command::SelectTrack {
            kind: TrackKind::Audio,
            id: Some(2),
        },
        Command::SetSubtitleDelay { seconds: 0.5 },
        Command::Seek {
            seconds: 1.0,
            mode: SeekMode::Absolute,
            precision: SeekPrecision::Exact,
        },
    ] {
        accepted(session.dispatch(&command));
    }
    assert!(
        wait_for(|| {
            store.drain(&session);
            store.selected(TrackKind::Audio) == Some(2)
                && store.state.subtitle_delay == 0.5
                && store
                    .state
                    .time_pos
                    .is_some_and(|at| (at - 1.0).abs() < 0.05)
        }),
        "the engine never reported the choices: {:?}",
        store.state
    );
    accepted(session.dispatch(&Command::Close {}));
    assert!(wait_for(|| {
        store.drain(&session);
        store.state.status == Status::Idle
    }));
    store.history.clear();

    // libmpv starts the next load from its own defaults; the session puts the
    // remembered position, track and delay back.
    let reply = session.dispatch_json(&format!(
        r#"{{"schema_version":1,"command":{{"open":{{"source":{{"kind":"local","location":{}}}}}}}}}"#,
        serde_json::to_string(&location).expect("paths serialise")
    ));
    assert!(reply.contains(r#""accepted""#), "{reply}");
    assert!(
        wait_for(|| {
            store.drain(&session);
            store.loaded(&location)
                && store.selected(TrackKind::Audio) == Some(2)
                && store.state.subtitle_delay == 0.5
        }),
        "the remembered choices were not restored: {:?}",
        store.state
    );
    let at = store.state.time_pos.expect("a loaded file has a position");
    assert!((at - 1.0).abs() < 0.05, "playback resumed at {at}, not 1.0");
    let resumed: Vec<_> = store
        .history
        .iter()
        .filter_map(|event| match event {
            HistoryEvent::Resumed { position } => Some(*position),
            _ => None,
        })
        .collect();
    assert_eq!(resumed.len(), 1, "{:?}", store.history);
    assert!((resumed[0] - 1.0).abs() < 0.05);

    let page = session.query_json(
        r#"{"schema_version":1,"query":{"history":{"query":{"page":{"utc_offset_seconds":0}}}}}"#,
    );
    let page: serde_json::Value = serde_json::from_str(&page).expect("the page is JSON");
    let entry = &page["result"]["history"]["result"]["page"]["sections"][0]["entries"][0];
    assert_eq!(entry["source"]["location"], location.as_str(), "{page}");
    assert_eq!(entry["title"], "two-audio");
    drop(session);

    let written = fs::read_to_string(Path::new(&config.data_dir).join(HISTORY_FILE_NAME))
        .expect("the session wrote its history file");
    let history = History::load(&written).expect("the history file loads");
    let preferences = &history.files[&location].preferences;
    assert_eq!(preferences.audio_track_id, Some(2));
    assert_eq!(preferences.subtitle_delay, Some(0.5));
    root.close().expect("fixture directory is removed");
}
