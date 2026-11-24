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
    Command, DispatchResult, Event, MediaSource, MpvOption, Query, QueryResult, SourceKind, Status,
    TrackKind, TracksView,
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
    assert_eq!(view.subtitles[1].label, "Subtitles · SRT · External");
    assert_eq!(view.subtitles[2].label, "English · SRT · External");
    for row in &view.subtitles[1..] {
        assert!(
            row.label.ends_with(" · SRT · External"),
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
                && selected[0].label.ends_with(" · SRT · External")
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
    // Selecting a discovered file reuses that track.
    accepted(session.dispatch(&Command::AddSubtitle {
        path: root.path().join("movie.srt").to_string_lossy().into_owned(),
    }));
    assert!(
        wait_for(|| {
            subtitle_count(&session) == 3
                && tracks(&session)
                    .subtitles
                    .iter()
                    .any(|row| row.selected && row.id.is_some_and(|id| stem_ids.contains(&id)))
        }),
        "an existing same-stem subtitle was duplicated instead of selected: {:?}",
        session.snapshot().tracks
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
fn tagged_sidecars_supply_real_mpv_titles_languages_and_preferred_selection() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-tagged-sidecars");
    let stem = "A Serious Man (2009) {tmdb-12573}";
    let movie = root.path().join(format!("{stem}.mp4"));
    fs::rename(write_media_fixture(root.path()), &movie).unwrap();
    for suffix in ["en.hi.srt", "he.srt", "ru.hi.srt", "extended.en.srt"] {
        fs::write(root.path().join(format!("{stem}.{suffix}")), CUE).unwrap();
    }
    fs::write(root.path().join("movie.nfo"), "<movie/>").unwrap();
    let mut config = headless_config(root.path());
    config.mpv_options.push(MpvOption {
        name: "slang".to_owned(),
        value: "rus,en".to_owned(),
    });
    let session = Session::new(&config).expect("libmpv starts");
    open(&session, &movie);
    let snapshot = session.snapshot();
    let subtitles: Vec<_> = snapshot
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect();
    let reported: Vec<_> = subtitles
        .iter()
        .map(|track| {
            (
                track.title.as_deref(),
                track.lang.as_deref(),
                track.external,
                track.selected,
            )
        })
        .collect();
    assert_eq!(
        reported,
        [
            (Some("English (SDH)"), Some("en"), true, false),
            (Some("Hebrew"), Some("he"), true, false),
            (Some("Russian (SDH)"), Some("rus"), true, true),
        ]
    );
    let view = tracks(&session);
    let labels: Vec<_> = view
        .subtitles
        .iter()
        .map(|row| row.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Off",
            "English (SDH) · SRT · External",
            "Hebrew · SRT · External",
            "Russian (SDH) · SRT · External",
        ]
    );
    assert!(view.subtitles[3].selected);

    open(&session, &common::tone_fixture(root.path()));
    assert_eq!(subtitle_count(&session), 0);
}

#[test]
fn the_delay_and_size_keys_reach_libmpv_and_the_readouts_follow() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-subtitle-keys");
    let media = write_media_fixture(root.path());
    let session = Session::new(&headless_config(root.path())).expect("libmpv starts");
    open(&session, &media);
    let invoke = |id: &str| session.dispatch(&Command::InvokeCommand { id: id.to_owned() });
    // What libmpv itself reports, not the value the session asked for: the
    // readouts lead with that one until the report arrives.
    let reported_delay =
        |seconds: f64| wait_for(|| (session.snapshot().subtitle_delay - seconds).abs() < 0.000_5);
    let reported_scale = |scale: f64| {
        wait_for(|| {
            tracks(&session)
                .subtitle_scale
                .reported
                .is_some_and(|reported| (reported - scale).abs() < 0.000_5)
        })
    };

    accepted(invoke("subtitle-delay-forward"));
    assert_eq!(tracks(&session).subtitle_delay.label, "+50 ms");
    assert!(
        reported_delay(0.05),
        "libmpv never reported the delay: {}",
        session.snapshot().subtitle_delay
    );
    accepted(invoke("subtitle-delay-back"));
    accepted(invoke("subtitle-delay-back"));
    assert_eq!(tracks(&session).subtitle_delay.label, "-50 ms");
    assert!(
        reported_delay(-0.05),
        "libmpv never reported the delay: {}",
        session.snapshot().subtitle_delay
    );

    accepted(invoke("subtitle-size-up"));
    assert_eq!(tracks(&session).subtitle_scale.label, "110%");
    assert!(
        reported_scale(1.1),
        "libmpv never reported the size: {:?}",
        tracks(&session).subtitle_scale
    );
    for _ in 0..40 {
        accepted(invoke("subtitle-size-up"));
    }
    assert_eq!(
        tracks(&session).subtitle_scale.label,
        "400%",
        "the size stops at the core's maximum"
    );
    assert!(
        reported_scale(4.0),
        "libmpv never reported the maximum size: {:?}",
        tracks(&session).subtitle_scale
    );
    accepted(session.dispatch(&Command::SetSubtitleScale { scale: 1.0 }));
    accepted(invoke("subtitle-size-down"));
    assert_eq!(tracks(&session).subtitle_scale.label, "90%");
    assert!(
        reported_scale(0.9),
        "libmpv never reported the smaller size: {:?}",
        tracks(&session).subtitle_scale
    );
    let shown = tracks(&session).subtitle_scale;
    assert_eq!(
        (shown.label.as_str(), shown.reported),
        ("90%", Some(shown.value)),
        "once reported, libmpv's value is the one shown"
    );

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
