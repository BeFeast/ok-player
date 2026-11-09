//! Tracks and subtitles over the real libmpv: subtitle files named after the
//! media load with it, an added file becomes the subtitle track, and the delay
//! and size keys reach the engine, with the core's labels and readouts in the
//! `tracks` query.

mod common;

use std::fs;
use std::path::Path;

use common::{engine_lock, headless_config, wait_for, write_media_fixture};
use okp_session::Session;
use okp_session::protocol::{
    Command, DispatchResult, Event, MediaSource, Query, QueryResult, SourceKind, Status, TrackKind,
    TracksView,
};
use okp_test_fixtures::unique_temp_dir;

const CUE: &str = "1\n00:00:00,000 --> 00:00:02,000\nA line\n\n";

fn tracks(session: &Session) -> TracksView {
    match session.query(&Query::Tracks {}) {
        QueryResult::Tracks { tracks } => tracks,
        other => panic!("not a tracks reply: {other:?}"),
    }
}

fn subtitle_count(session: &Session) -> usize {
    session
        .snapshot()
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .count()
}

fn accepted(reply: DispatchResult) {
    assert!(
        matches!(reply, DispatchResult::Accepted { .. }),
        "the session refused: {reply:?}"
    );
}

fn open(session: &Session, media: &Path) {
    accepted(session.dispatch(&Command::Open {
        source: MediaSource {
            kind: SourceKind::Local,
            location: media.to_string_lossy().into_owned(),
        },
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    }));
    assert!(
        wait_for(|| matches!(session.snapshot().status, Status::Playing | Status::Paused)),
        "the fixture never loaded"
    );
}

#[test]
fn subtitle_files_named_after_the_media_load_with_it_and_an_added_one_is_selected() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-tracks");
    let media = write_media_fixture(root.path());
    // `movie.edl` is the media: the same stem, with and without a language,
    // loads with it; another name does not.
    for name in ["movie.srt", "movie.en.srt", "other.srt"] {
        fs::write(root.path().join(name), CUE).expect("the subtitle fixture is written");
    }
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    open(&session, &media);
    assert!(
        wait_for(|| subtitle_count(&session) == 2),
        "the two same-stem files did not load: {:?}",
        session.snapshot().tracks
    );
    let view = tracks(&session);
    // The labels are the core's for whatever libmpv reports: its titles for
    // external files differ between versions, the codec and source do not.
    for row in &view.subtitles[1..] {
        assert!(
            row.label.ends_with(" · SRT · EXT"),
            "{:?} is not marked as an external SRT",
            row.label
        );
    }
    let languages: Vec<_> = session
        .snapshot()
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .map(|track| track.lang.clone())
        .collect();
    assert!(
        languages.contains(&Some("en".to_owned())),
        "the language of movie.en.srt comes from its name: {languages:?}"
    );
    let stem_ids: Vec<_> = view.subtitles[1..]
        .iter()
        .filter_map(|row| row.id)
        .collect();
    assert!(
        view.audio.iter().any(|row| row.selected),
        "{:?}",
        view.audio
    );

    let added = unique_temp_dir("okp-session-tracks-added");
    let extra = added.path().join("Commentary.srt");
    fs::write(&extra, CUE).expect("the added subtitle is written");
    accepted(session.dispatch(&Command::AddSubtitle {
        path: extra.to_string_lossy().into_owned(),
    }));
    assert!(
        wait_for(|| subtitle_count(&session) == 3),
        "the added file never became a track: {:?}",
        session.snapshot().tracks
    );
    assert!(
        wait_for(|| {
            let view = tracks(&session);
            let selected: Vec<_> = view.subtitles.iter().filter(|row| row.selected).collect();
            selected.len() == 1
                && selected[0].id.is_some_and(|id| !stem_ids.contains(&id))
                && selected[0].label.ends_with(" · SRT · EXT")
        }),
        "the added file is not the selected subtitle: {:?}",
        tracks(&session).subtitles
    );

    accepted(session.dispatch(&Command::SelectTrack {
        kind: TrackKind::Subtitle,
        id: None,
    }));
    assert!(
        wait_for(|| tracks(&session).subtitles[0].selected),
        "subtitles Off is not selected after turning them off"
    );
    let errors: Vec<_> = session
        .drain_events()
        .into_iter()
        .filter(|event| matches!(event, Event::Error { .. }))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    drop(session);
    added.close().expect("fixture directory is removed");
    root.close().expect("fixture directory is removed");
}

#[test]
fn the_delay_and_size_keys_reach_libmpv_and_the_readouts_follow() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-subtitle-keys");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    open(&session, &media);
    let invoke = |id: &str| session.dispatch(&Command::InvokeCommand { id: id.to_owned() });

    accepted(invoke("subtitle-delay-forward"));
    assert!(
        wait_for(|| tracks(&session).subtitle_delay.label == "+50 ms"),
        "libmpv never reported the delay: {:?}",
        tracks(&session).subtitle_delay
    );
    accepted(invoke("subtitle-delay-back"));
    accepted(invoke("subtitle-delay-back"));
    assert!(wait_for(
        || tracks(&session).subtitle_delay.label == "-50 ms"
    ));

    accepted(invoke("subtitle-size-up"));
    assert_eq!(tracks(&session).subtitle_scale.label, "110%");
    for _ in 0..40 {
        accepted(invoke("subtitle-size-up"));
    }
    assert_eq!(
        tracks(&session).subtitle_scale.label,
        "400%",
        "the size stops at the core's maximum"
    );
    accepted(session.dispatch(&Command::SetSubtitleScale { scale: 1.0 }));
    accepted(invoke("subtitle-size-down"));
    assert_eq!(tracks(&session).subtitle_scale.label, "90%");

    let errors: Vec<_> = session
        .drain_events()
        .into_iter()
        .filter(|event| matches!(event, Event::Error { .. }))
        .collect();
    assert!(errors.is_empty(), "libmpv refused: {errors:?}");
    drop(session);
    root.close().expect("fixture directory is removed");
}

#[test]
fn a_subtitle_file_libmpv_cannot_read_is_reported_against_its_command() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-missing-subtitle");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    open(&session, &media);
    let missing = root.path().join("missing.srt");
    let DispatchResult::Accepted { request_id } = session.dispatch(&Command::AddSubtitle {
        path: missing.to_string_lossy().into_owned(),
    }) else {
        panic!("the session refused a subtitle path")
    };
    let mut reported = false;
    assert!(
        wait_for(|| {
            reported |= session.drain_events().into_iter().any(|event| {
                matches!(event, Event::Error { request_id: Some(id), .. } if id == request_id)
            });
            reported
        }),
        "no error answered the add of a missing file"
    );
    assert_eq!(subtitle_count(&session), 0);
    drop(session);
    root.close().expect("fixture directory is removed");
}
