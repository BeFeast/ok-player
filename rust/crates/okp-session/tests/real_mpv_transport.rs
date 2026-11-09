//! The transport controls over the real libmpv: volume nudges arriving faster
//! than libmpv reports them all count, and the transport query reads the loaded
//! file.

mod common;

use common::{engine_lock, headless_config, wait_for, write_media_fixture};
use okp_session::Session;
use okp_session::protocol::{
    Command, DispatchResult, MediaSource, Query, QueryResult, SourceKind, Status, TransportView,
};
use okp_test_fixtures::unique_temp_dir;

fn transport(session: &Session) -> TransportView {
    match session.query(&Query::Transport { at: None }) {
        QueryResult::Transport { transport } => transport,
        other => panic!("not a transport reply: {other:?}"),
    }
}

#[test]
fn rapid_volume_nudges_all_reach_libmpv() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-nudge");
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    assert!(wait_for(|| session.snapshot().volume.is_some()));
    let start = session
        .snapshot()
        .volume
        .expect("libmpv reports its volume");
    accepted(session.dispatch(&Command::SetVolume { volume: 40.0 }));

    // Back to back, with no chance for libmpv's reports to arrive in between:
    // a nudge that built on the last report would lose the steps before it.
    for _ in 0..12 {
        accepted(session.dispatch(&Command::NudgeVolume { delta: 2.5 }));
    }
    let mut level = None;
    let settled = wait_for(|| {
        level = session.snapshot().volume;
        level == Some(70.0)
    });
    assert!(settled, "libmpv went to {level:?} from {start}, not 70");
    assert_eq!(transport(&session).volume.readout, "70%");

    // The core's ceiling holds against libmpv as well.
    for _ in 0..30 {
        accepted(session.dispatch(&Command::NudgeVolume { delta: 5.0 }));
    }
    assert!(wait_for(|| session.snapshot().volume == Some(130.0)));
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn the_transport_query_reads_the_loaded_file() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-transport");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    accepted(session.dispatch(&Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: media.to_string_lossy().into_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    }));
    assert!(wait_for(|| {
        let snapshot = session.snapshot();
        matches!(snapshot.status, Status::Playing | Status::Paused) && snapshot.duration.is_some()
    }));
    let view = transport(&session);
    assert!(view.seekable);
    assert_eq!(view.total, "00:02", "the fixture is two seconds long");
    assert!(view.osc.revealed.visible);
    assert_eq!(view.chapter_ticks.len(), 2, "the fixture has two chapters");
    drop(session);
    root.close().expect("fixture directory is removed");
}

fn accepted(reply: DispatchResult) {
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "the session refused: {reply:?}"
    );
}
