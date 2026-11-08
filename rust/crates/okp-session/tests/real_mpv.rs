//! A session over the real libmpv: commands reach the engine and what the engine
//! reports reaches the drained events.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use std::path::Path;

use common::{
    CHAPTER_TITLES, engine_lock, headless_config, tone_fixture, wait_for, write_media_fixture,
    write_two_audio_fixture,
};
use okp_session::Session;
use okp_session::protocol::{
    Command, DispatchResult, Event, MediaSource, SeekMode, SeekPrecision, SessionSnapshot,
    SnapshotChange, SourceKind, Status, TrackKind,
};
use okp_test_fixtures::unique_temp_dir;

/// Applies drained batches the way a shell's store does, and keeps every status
/// it passed through and every error message.
struct Store {
    state: SessionSnapshot,
    statuses: Vec<Status>,
    errors: Vec<String>,
}

impl Store {
    fn new() -> Self {
        Self {
            state: SessionSnapshot::default(),
            statuses: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn drain(&mut self, session: &Session) {
        for event in session.drain_events() {
            match event {
                Event::SnapshotChanged { changes } => {
                    for change in &changes {
                        if let SnapshotChange::Status { value } = change {
                            self.statuses.push(*value);
                        }
                        self.state.apply(change);
                    }
                }
                Event::Error { message, .. } => self.errors.push(message),
                Event::PlaylistChanged { .. } | Event::Shutdown {} => {}
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
}

fn open(path: &Path, resume_from: Option<f64>, audio_track: Option<i64>) -> Command {
    Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: path.to_string_lossy().into_owned(),
        },
        resume_from,
        audio_track,
        subtitle_track: None,
    }
}

fn accepted(reply: DispatchResult) {
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "the command was not accepted: {reply:?}"
    );
}

/// Drain until the open has loaded paused with an audio track selected.
fn wait_until_loaded(store: &mut Store, session: &Session, location: &Path) -> bool {
    let location = location.to_string_lossy();
    wait_for(|| {
        store.drain(session);
        store
            .state
            .source
            .as_ref()
            .is_some_and(|source| source.location == location)
            && store.state.status == Status::Paused
            && store.selected(TrackKind::Audio).is_some()
    })
}

#[test]
fn tracks_and_chapters_from_a_real_file_appear_in_drained_events() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-real-mpv");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let wakeups = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakeups);
    session.set_wakeup(Some(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })));

    let reply = session.dispatch(&Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: media.to_string_lossy().into_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    });
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "open was not accepted: {reply:?}"
    );

    let mut store = Store::new();
    let loaded = wait_for(|| {
        store.drain(&session);
        store.state.chapters.len() == CHAPTER_TITLES.len()
            && store
                .state
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Audio)
    });
    assert!(
        loaded,
        "tracks and chapters never arrived: {:?}",
        store.state
    );

    let titles: Vec<_> = store
        .state
        .chapters
        .iter()
        .map(|chapter| chapter.title.as_deref())
        .collect();
    assert_eq!(titles, CHAPTER_TITLES.map(Some));
    assert_eq!(store.state.chapters[1].time, 1.0);
    assert_eq!(
        store.state.status,
        Status::Paused,
        "the config pauses on load"
    );
    assert_eq!(store.state.title.as_deref(), Some("movie.edl"));
    assert!(
        store.state.volume.is_some(),
        "the engine reports its volume"
    );
    assert!(store.state.duration.is_some_and(|duration| duration > 1.5));
    assert!(
        wakeups.load(Ordering::SeqCst) >= 1,
        "batches were announced"
    );

    // A command validated by the core reaches the engine, whose report comes back.
    assert!(matches!(
        session.dispatch(&Command::SetVolume { volume: 42.0 }),
        DispatchResult::Accepted { .. }
    ));
    assert!(
        wait_for(|| {
            store.drain(&session);
            store.state.volume == Some(42.0)
        }),
        "the engine never reported the new volume: {:?}",
        store.state.volume
    );

    // The query answers with the same state the event stream has built.
    assert_eq!(session.snapshot().volume, Some(42.0));
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn a_missing_file_ends_with_a_load_error() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-missing");
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let missing = root.path().join("missing.mkv");

    let reply = session.dispatch(&Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: missing.to_string_lossy().into_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    });
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "{reply:?}"
    );

    let mut state = SessionSnapshot::default();
    let mut errors = Vec::new();
    let ended = wait_for(|| {
        for event in session.drain_events() {
            match event {
                Event::SnapshotChanged { changes } => {
                    for change in &changes {
                        state.apply(change);
                    }
                }
                Event::Error { message, .. } => errors.push(message),
                Event::PlaylistChanged { .. } | Event::Shutdown {} => {}
            }
        }
        state.status == Status::Ended
    });
    assert!(ended, "a missing file never ended: {state:?}");
    assert_eq!(
        state.end_reason,
        Some(okp_session::protocol::EndReason::Error)
    );
    assert_eq!(errors.len(), 1, "one load error is reported: {errors:?}");
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn the_next_file_starts_with_its_own_tracks_and_delay_and_no_ended_flash() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-next-file");
    let first = write_media_fixture(root.path());
    let second = tone_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let mut store = Store::new();
    accepted(session.dispatch(&open(&first, None, None)));
    assert!(
        wait_until_loaded(&mut store, &session, &first),
        "the first file never loaded: {:?}",
        store.state
    );

    // Choices made while the first file plays.
    accepted(session.dispatch(&Command::SetSubtitleDelay { seconds: 0.75 }));
    accepted(session.dispatch(&Command::SelectTrack {
        kind: TrackKind::Audio,
        id: None,
    }));
    assert!(
        wait_for(|| {
            store.drain(&session);
            store.state.subtitle_delay == 0.75 && store.selected(TrackKind::Audio).is_none()
        }),
        "the engine never reported the choices: {:?}",
        store.state
    );

    store.statuses.clear();
    accepted(session.dispatch(&open(&second, None, None)));
    let loaded = wait_until_loaded(&mut store, &session, &second)
        && wait_for(|| {
            store.drain(&session);
            store.state.subtitle_delay == 0.0
        });
    assert!(
        loaded,
        "the second file did not start fresh: {:?}, statuses {:?}, errors {:?}",
        store.state, store.statuses, store.errors
    );
    // Batches coalesce, so a fast load may show only its final status; an
    // `ended` in between is what the replaced file's late stop used to cause.
    assert!(
        !store.statuses.contains(&Status::Ended),
        "replacing the file passed through ended: {:?}",
        store.statuses
    );
    assert!(store.errors.is_empty(), "{:?}", store.errors);
    assert_eq!(store.state, session.snapshot(), "events and query agree");
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn an_open_starts_at_its_resume_point_and_ignores_a_track_the_file_lacks() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-resume");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let mut store = Store::new();

    // The file has one audio track; asking for a second must not cost the load.
    accepted(session.dispatch(&open(&media, Some(1.5), Some(2))));
    assert!(
        wait_until_loaded(&mut store, &session, &media),
        "the file never loaded: {:?}, statuses {:?}, errors {:?}",
        store.state,
        store.statuses,
        store.errors
    );
    assert_eq!(store.selected(TrackKind::Audio), Some(1));
    let at = store.state.time_pos.expect("a loaded file has a position");
    assert!((at - 1.5).abs() < 0.05, "playback starts at {at}, not 1.5");
    assert!(store.errors.is_empty(), "{:?}", store.errors);
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn an_opens_audio_track_is_selected_once_the_file_has_loaded() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-open-track");
    let media = write_two_audio_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let mut store = Store::new();

    accepted(session.dispatch(&open(&media, None, Some(2))));
    assert!(
        wait_until_loaded(&mut store, &session, &media)
            && wait_for(|| {
                store.drain(&session);
                store.selected(TrackKind::Audio) == Some(2)
            }),
        "audio 2 was never selected: {:?}",
        store.state.tracks
    );
    let audio = store
        .state
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .count();
    assert_eq!(audio, 2, "{:?}", store.state.tracks);
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn an_absolute_seek_before_the_start_goes_to_the_start() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-seek");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let mut store = Store::new();
    accepted(session.dispatch(&open(&media, None, None)));
    assert!(wait_until_loaded(&mut store, &session, &media));

    // libmpv counts a negative absolute position from the end (1.5 s into this
    // 2 s file). The relative step after it gives the test a position to wait
    // for: 0.25 from the start, against 1.75 from the end.
    for (seconds, mode) in [(-0.5, SeekMode::Absolute), (0.25, SeekMode::Relative)] {
        accepted(session.dispatch(&Command::Seek {
            seconds,
            mode,
            precision: SeekPrecision::Exact,
        }));
    }
    let mut at = None;
    let settled = wait_for(|| {
        at = session.snapshot().time_pos;
        at.is_some_and(|at| (at - 0.25).abs() < 0.05)
    });
    assert!(settled, "the playhead went to {at:?}, not 0.25");
    drop(session);
    root.close().expect("fixture directory is removed");
}
