use super::*;
use crate::protocol::QueryResult;
use crate::tracks::TracksView;

fn tracks(session: &Session) -> TracksView {
    match session.query(&Query::Tracks {}) {
        QueryResult::Tracks { tracks } => tracks,
        other => panic!("the tracks query answered {other:?}"),
    }
}

fn audio(id: i64, codec: &str, channels: &str, selected: bool) -> okp_mpv::Track {
    okp_mpv::Track {
        id,
        kind: okp_mpv::TrackKind::Audio,
        selected,
        external: false,
        external_filename: None,
        default: id == 1,
        title: None,
        lang: Some("jpn".to_owned()),
        codec: Some(codec.to_owned()),
        audio_channels: Some(channels.to_owned()),
    }
}

fn sidecar(id: i64, file: &str) -> okp_mpv::Track {
    okp_mpv::Track {
        id,
        kind: okp_mpv::TrackKind::Subtitle,
        selected: true,
        external: true,
        external_filename: Some(file.to_owned()),
        default: false,
        title: None,
        lang: Some("en".to_owned()),
        codec: Some("subrip".to_owned()),
        audio_channels: None,
    }
}

/// A session playing a loaded file whose engine reports `tracks`.
fn playing_with(tracks: Vec<okp_mpv::Track>) -> (Session, Arc<FakeState>) {
    let (session, engine) = fake_session();
    assert!(matches!(
        session.dispatch(&open("/media/film.mkv")),
        DispatchResult::Accepted { .. }
    ));
    engine.publish(
        |observation| {
            observation.tracks = tracks;
            observation.playback.duration = Some(60.0);
            observation.subtitle_scale = Some(1.0);
        },
        vec![MpvEvent::FileLoaded {
            video_dimensions: None,
        }],
    );
    assert!(wait_for(|| session.snapshot().status == Status::Playing));
    (session, engine)
}

#[test]
fn the_tracks_query_labels_the_loaded_files_tracks_with_the_core_rules() {
    let (session, _engine) = playing_with(vec![
        audio(1, "aac", "2.0", true),
        audio(2, "eac3", "5.1", false),
        sidecar(1, "/media/film.en.srt"),
    ]);
    let view = tracks(&session);
    let rows = |rows: &[crate::tracks::TrackChoice]| {
        rows.iter()
            .map(|row| (row.id, row.label.clone(), row.selected))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&view.audio),
        [
            (Some(1), "jpn · 2.0 · AAC".to_owned(), true),
            (Some(2), "jpn · 5.1 · EAC3".to_owned(), false),
        ]
    );
    assert_eq!(
        rows(&view.subtitles),
        [
            (None, "Off".to_owned(), false),
            (Some(1), "en · SRT · EXT".to_owned(), true),
        ]
    );
    assert_eq!(view.subtitle_delay.label, "0 ms");
    assert_eq!(view.subtitle_scale.label, "100%");
    assert!(view.adjustable);
}

#[test]
fn an_added_subtitle_file_reaches_the_engine_only_with_media_and_a_subtitle_extension() {
    let add = |path: &str| Command::AddSubtitle {
        path: path.to_owned(),
    };
    let (idle, idle_engine) = fake_session();
    assert_eq!(
        idle.dispatch(&add("/media/film.en.srt")),
        rejected(RejectReason::NoActiveMedia),
        "nothing is loaded to add it to"
    );
    assert!(idle_engine.forwarded().is_empty());

    let (session, engine_playing) = playing_with(Vec::new());
    let DispatchResult::Rejected { reason, message } = session.dispatch(&add("/media/film.mkv"))
    else {
        panic!("a video is not a subtitle file")
    };
    assert_eq!(reason, RejectReason::InvalidArgument);
    assert_eq!(message, "`/media/film.mkv` is not a subtitle file");
    assert!(matches!(
        session.dispatch(&add("/media/Film.EN.SRT")),
        DispatchResult::Accepted { .. }
    ));
    assert_eq!(
        engine_playing.forwarded().last(),
        Some(&add("/media/Film.EN.SRT")),
        "the file goes to the engine as given"
    );
    assert!(
        !engine_playing.forwarded().contains(&add("/media/film.mkv")),
        "a refused file never reaches the engine"
    );
}

#[test]
fn the_size_keys_step_from_the_size_asked_for_and_the_readout_follows_at_once() {
    let (session, engine) = playing_with(Vec::new());
    for _ in 0..2 {
        assert!(matches!(
            session.dispatch(&Command::InvokeCommand {
                id: "subtitle-size-up".to_owned(),
            }),
            DispatchResult::Accepted { .. }
        ));
    }
    let sizes: Vec<_> = engine
        .forwarded()
        .into_iter()
        .filter_map(|command| match command {
            Command::SetSubtitleScale { scale } => Some(scale),
            _ => None,
        })
        .collect();
    assert_eq!(
        sizes,
        [1.1, 1.2],
        "the second press builds on the first before the engine reports it"
    );
    assert_eq!(tracks(&session).subtitle_scale.label, "120%");

    assert!(matches!(
        session.dispatch(&Command::SetSubtitleScale { scale: 0.0 }),
        DispatchResult::Accepted { .. }
    ));
    assert_eq!(
        engine.forwarded().last(),
        Some(&Command::SetSubtitleScale { scale: 0.25 }),
        "a size below the core's range is its minimum"
    );
    assert_eq!(
        session.dispatch(&Command::SetSubtitleScale { scale: f64::NAN }),
        rejected(RejectReason::NotFinite)
    );
}

#[test]
fn the_size_and_the_size_keys_need_loaded_media() {
    let (session, engine) = fake_session();
    assert_eq!(
        session.dispatch(&Command::NudgeSubtitleScale { delta: 0.1 }),
        rejected(RejectReason::NoActiveMedia)
    );
    let up = session.query(&Query::CommandRegistry {
        fullscreen: false,
        mini_player: false,
    });
    let QueryResult::CommandRegistry { commands } = up else {
        panic!("the registry query answered {up:?}")
    };
    let row = commands
        .iter()
        .find(|command| command.id == "subtitle-size-up")
        .expect("the size key is a registry command");
    assert!(row.available, "the session runs it itself");
    assert!(!row.enabled, "without media it does nothing");
    assert!(
        !engine
            .forwarded()
            .iter()
            .any(|command| matches!(command, Command::SetSubtitleScale { .. })),
        "nothing reached the engine"
    );
    assert_eq!(tracks(&session).subtitle_scale.label, "100%");
}

#[test]
fn delay_keys_pressed_before_libmpv_reports_the_delay_all_count() {
    let (session, engine) = playing_with(Vec::new());
    for _ in 0..3 {
        assert!(matches!(
            session.dispatch(&Command::InvokeCommand {
                id: "subtitle-delay-forward".to_owned(),
            }),
            DispatchResult::Accepted { .. }
        ));
    }
    let delays: Vec<_> = engine
        .forwarded()
        .into_iter()
        .filter_map(|command| match command {
            Command::SetSubtitleDelay { seconds } => Some((seconds * 1000.0).round()),
            _ => None,
        })
        .collect();
    assert_eq!(delays, [50.0, 100.0, 150.0]);
    assert_eq!(
        tracks(&session).subtitle_delay.label,
        "+150 ms",
        "the readout shows the delay asked for before libmpv reports it"
    );
}
