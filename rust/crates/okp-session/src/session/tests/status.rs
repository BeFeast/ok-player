use super::*;
use crate::protocol::{SeekMode, SeekPrecision, StatusSurface};
use crate::status::FailureKind;

fn surface(session: &Session) -> StatusSurface {
    match session.query(&Query::StatusSurface {}) {
        QueryResult::StatusSurface { surface } => surface,
        other => panic!("not a status surface: {other:?}"),
    }
}

fn notices(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Notice { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn local(path: &str) -> MediaSource {
    MediaSource {
        kind: SourceKind::Local,
        location: path.to_owned(),
    }
}

#[test]
fn an_opening_source_is_loading_until_the_engine_has_loaded_it() {
    let (session, engine) = fake_session();
    assert_eq!(surface(&session), StatusSurface::Clear {});
    let _ = session.dispatch(&open("/media/Show/episode.mkv"));
    assert_eq!(
        surface(&session),
        StatusSurface::Loading {
            label: "Opening episode.mkv".to_owned()
        }
    );
    loaded(&engine, 10.0);
    assert!(wait_for(|| surface(&session) == StatusSurface::Clear {}));
}

#[test]
fn a_cache_stall_shows_buffering_with_the_engines_progress() {
    let (session, engine) = fake_session();
    let _ = session.dispatch(&open("/media/a.mkv"));
    loaded(&engine, 10.0);
    engine.publish(
        |observation| {
            observation.buffering = true;
            observation.buffering_percent = Some(38);
        },
        vec![],
    );
    assert_eq!(
        surface(&session),
        StatusSurface::Buffering {
            percent: Some(38),
            label: "Buffering 38%".to_owned()
        }
    );
    engine.publish(|observation| observation.buffering = false, vec![]);
    assert_eq!(surface(&session), StatusSurface::Clear {});
}

#[test]
fn a_load_that_fails_shows_the_cores_card_with_its_source_until_the_next_open() {
    let (session, engine) = fake_session();
    let _ = session.dispatch(&open("/media/missing.mkv"));
    engine.publish(
        |_| {},
        vec![MpvEvent::EndFile {
            reason: EndFileReason::Error(-13),
            path: None,
            diagnostic_messages: vec!["Failed to open /media/missing.mkv.".to_owned()],
            last_duration: None,
        }],
    );
    let StatusSurface::Failed { card } = surface(&session) else {
        panic!("no failure card after a failed load");
    };
    assert_eq!(card.kind, FailureKind::Application);
    assert_eq!(card.title, "Playback failed");
    assert_eq!(card.source, local("/media/missing.mkv"));
    assert!(card.detail.contains("Path: /media/missing.mkv"));

    // Retry opens the card's source again: the failure is gone while it loads.
    let _ = session.dispatch(&Command::Open {
        source: card.source.clone(),
        resume_from: None,
        audio_track: None,
        subtitle_track: None,
    });
    assert!(matches!(surface(&session), StatusSurface::Loading { .. }));
}

#[test]
fn an_open_the_engine_refuses_at_once_shows_a_card_and_a_close_clears_it() {
    let (session, engine) = fake_session();
    *lock(&engine.refuse_with) = Some("loadfile refused".to_owned());
    let _ = session.dispatch(&open("/media/a.mkv"));
    let StatusSurface::Failed { card } = surface(&session) else {
        panic!("no failure card after a refused open");
    };
    assert_eq!(card.source, local("/media/a.mkv"));
    assert!(card.detail.ends_with("Reason: loadfile refused"));
    let _ = session.dispatch(&Command::Close {});
    assert_eq!(surface(&session), StatusSurface::Clear {});
}

#[test]
fn a_failure_of_a_replaced_source_shows_no_card() {
    let (session, engine) = fake_session();
    let _ = session.dispatch(&open("/media/new.mkv"));
    engine.publish(
        |_| {},
        vec![MpvEvent::EndFile {
            reason: EndFileReason::Error(-13),
            path: Some("/media/old.mkv".to_owned()),
            diagnostic_messages: vec![],
            last_duration: None,
        }],
    );
    assert!(matches!(surface(&session), StatusSurface::Loading { .. }));
}

#[test]
fn accepted_commands_queue_one_notice_each_and_refused_ones_none() {
    let (session, engine) = fake_session();
    // Nothing is loaded: a seek is refused, a volume change is not.
    let _ = session.dispatch(&Command::Seek {
        seconds: 5.0,
        mode: SeekMode::Relative,
        precision: SeekPrecision::Keyframe,
    });
    let _ = session.dispatch(&Command::NudgeVolume { delta: -5.0 });
    assert_eq!(notices(&session.drain_events()), vec!["Volume 95%"]);

    let _ = session.dispatch(&open("/media/a.mkv"));
    loaded(&engine, 100.0);
    engine.publish(
        |observation| observation.playback.time_pos = Some(42.0),
        vec![],
    );
    assert!(wait_for(|| session.snapshot().time_pos == Some(42.0)));
    let _ = session.drain_events();
    for command in [
        Command::SetSpeed { speed: 1.5 },
        Command::SetSubtitleDelay { seconds: -0.125 },
        Command::SelectTrack {
            kind: TrackKind::Subtitle,
            id: None,
        },
        Command::ToggleMute {},
        Command::Seek {
            seconds: 10.0,
            mode: SeekMode::Relative,
            precision: SeekPrecision::Exact,
        },
        Command::TogglePause {},
    ] {
        let _ = session.dispatch(&command);
    }
    assert_eq!(
        notices(&session.drain_events()),
        vec![
            "Speed 1.50×",
            "Subtitle delay: -125 ms",
            "Subtitles off",
            "Muted",
            "00:52"
        ]
    );
}
