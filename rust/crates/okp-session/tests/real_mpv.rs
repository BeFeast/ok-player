//! A session over the real libmpv: commands reach the engine and what the engine
//! reports reaches the drained events.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{CHAPTER_TITLES, engine_lock, headless_config, wait_for, write_media_fixture};
use okp_session::Session;
use okp_session::protocol::{
    Command, DispatchResult, Event, MediaSource, SessionSnapshot, SourceKind, Status, TrackKind,
};
use okp_test_fixtures::unique_temp_dir;

/// Applies drained batches the way a shell's store does.
struct Store {
    state: SessionSnapshot,
}

impl Store {
    fn new() -> Self {
        Self {
            state: SessionSnapshot::default(),
        }
    }

    fn drain(&mut self, session: &Session) {
        for event in session.drain_events() {
            if let Event::SnapshotChanged { changes } = event {
                for change in &changes {
                    self.state.apply(change);
                }
            }
        }
    }
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
                Event::Shutdown {} => {}
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
