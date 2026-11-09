//! The status surface and notices over the real libmpv: a missing file ends
//! with the core's failure card, a loaded file clears it, and a volume change
//! is confirmed with a notice.

mod common;

use common::{engine_lock, headless_config, wait_for, write_media_fixture};
use okp_session::Session;
use okp_session::protocol::{
    Command, Event, FailureAction, FailureKind, MediaSource, Query, QueryResult, SourceKind,
    StatusSurface,
};
use okp_test_fixtures::unique_temp_dir;

fn surface(session: &Session) -> StatusSurface {
    match session.query(&Query::StatusSurface {}) {
        QueryResult::StatusSurface { surface } => surface,
        other => panic!("not a status surface: {other:?}"),
    }
}

fn open(session: &Session, location: &str) {
    let _ = session.dispatch(&Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: location.to_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    });
}

#[test]
fn a_missing_file_shows_the_cores_failure_card_and_a_playable_one_clears_it() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-status");
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let missing = root.path().join("absent").join("missing.mkv");
    let missing = missing.to_string_lossy().into_owned();

    open(&session, &missing);
    let mut shown = StatusSurface::Clear {};
    assert!(
        wait_for(|| {
            shown = surface(&session);
            matches!(shown, StatusSurface::Failed { .. })
        }),
        "a missing file never showed a failure card: {shown:?}"
    );
    let StatusSurface::Failed { card } = shown else {
        unreachable!()
    };
    assert_eq!(card.kind, FailureKind::Application);
    assert_eq!(card.title, "Playback failed");
    assert_eq!(
        card.source,
        MediaSource {
            kind: SourceKind::Local,
            location: missing.clone()
        }
    );
    assert!(
        card.detail.contains(&format!("Path: {missing}")),
        "{}",
        card.detail
    );
    assert_eq!(
        card.actions
            .iter()
            .map(|view| view.action)
            .collect::<Vec<_>>(),
        vec![
            FailureAction::Retry,
            FailureAction::OpenAnother,
            FailureAction::CopyDetails
        ]
    );

    let media = write_media_fixture(root.path());
    open(&session, &media.to_string_lossy());
    assert!(
        wait_for(|| surface(&session) == StatusSurface::Clear {}
            && session.snapshot().duration.is_some()),
        "the playable file kept {:?}",
        surface(&session)
    );
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn a_volume_change_is_confirmed_with_a_notice() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-notice");
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    let _ = session.dispatch(&Command::SetVolume { volume: 40.0 });
    let mut notices = Vec::new();
    assert!(
        wait_for(|| {
            notices.extend(
                session
                    .drain_events()
                    .into_iter()
                    .filter_map(|event| match event {
                        Event::Notice { text } => Some(text),
                        _ => None,
                    }),
            );
            !notices.is_empty()
        }),
        "no notice for a volume change"
    );
    assert_eq!(notices, vec!["Volume 40%".to_owned()]);
    drop(session);
    root.close().expect("fixture directory is removed");
}
