//! The session playlist over the real libmpv: a local open lists its folder,
//! and the end of a file plays the next one.

mod common;

use std::path::Path;

use common::{engine_lock, headless_config, wait_for, write_two_audio_fixture};
use okp_session::Session;
use okp_session::protocol::{
    Command, DispatchResult, MediaSource, PlaylistState, Query, QueryResult, SessionSnapshot,
    SourceKind, Status,
};
use okp_test_fixtures::unique_temp_dir;

fn open(path: &Path) -> Command {
    Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: path.to_string_lossy().into_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    }
}

fn playlist(session: &Session) -> PlaylistState {
    match session.query(&Query::Playlist {}) {
        QueryResult::Playlist { playlist } => playlist,
        other => panic!("expected the playlist, got {other:?}"),
    }
}

fn location(snapshot: &SessionSnapshot) -> Option<&str> {
    snapshot
        .source
        .as_ref()
        .map(|source| source.location.as_str())
}

#[test]
fn the_next_file_of_the_folder_plays_when_the_first_one_ends() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-auto-advance");
    // Two 2-second tones, first.wav and second.wav, beside an EDL that is not
    // media and stays out of the playlist.
    write_two_audio_fixture(root.path());
    let first = root.path().join("first.wav");
    let second = root.path().join("second.wav");
    let first_location = first.to_string_lossy().into_owned();
    let second_location = second.to_string_lossy().into_owned();

    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let reply = session.dispatch(&open(&first));
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "{reply:?}"
    );

    let mut state = playlist(&session);
    assert!(
        wait_for(|| {
            state = playlist(&session);
            !state.scanning && state.items.len() == 2
        }),
        "the folder never filled in: {state:?}"
    );
    let items: Vec<_> = state
        .items
        .iter()
        .map(|entry| entry.source.location.as_str())
        .collect();
    assert_eq!(items, [first_location.as_str(), second_location.as_str()]);
    assert_eq!(state.current_index, Some(0));

    // The headless config pauses on load; play the first tone at four times
    // speed so it ends quickly.
    assert!(
        wait_for(|| session.snapshot().status == Status::Paused),
        "{:?}",
        session.snapshot()
    );
    assert!(matches!(
        session.dispatch(&Command::SetSpeed { speed: 4.0 }),
        DispatchResult::Accepted { .. }
    ));
    assert!(matches!(
        session.dispatch(&Command::SetPaused { paused: false }),
        DispatchResult::Accepted { .. }
    ));

    let mut snapshot = session.snapshot();
    assert!(
        wait_for(|| {
            snapshot = session.snapshot();
            location(&snapshot) == Some(second_location.as_str())
                && matches!(snapshot.status, Status::Playing | Status::Ended)
        }),
        "the second file never played: {snapshot:?}"
    );
    assert_eq!(playlist(&session).current_index, Some(1));

    // The last file ends and, with repeat off, nothing follows.
    assert!(
        wait_for(|| {
            snapshot = session.snapshot();
            snapshot.status == Status::Ended
        }),
        "the second file never ended: {snapshot:?}"
    );
    assert_eq!(location(&snapshot), Some(second_location.as_str()));
    assert_eq!(playlist(&session).current_index, Some(1));
}
