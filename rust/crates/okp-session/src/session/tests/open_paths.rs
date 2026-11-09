use super::*;
use crate::open_paths::OpenPlan;

/// The plan for `paths`, of which `folders` are folders.
fn open_paths(session: &Session, paths: &[&str], folders: &[&str]) -> OpenPlan {
    let reply = session.query_json(
        &serde_json::json!({
            "schema_version": 1,
            "query": {"open_paths": {"paths": paths, "folders": folders}}
        })
        .to_string(),
    );
    match serde_json::from_str::<QueryReply>(&reply).map(|reply| reply.result) {
        Ok(QueryResult::OpenPaths { plan }) => plan,
        other => panic!("open_paths answered {other:?}"),
    }
}

#[test]
fn a_dropped_subtitle_is_planned_onto_media_only_once_it_has_loaded() {
    let (session, engine) = fake_session();
    assert_eq!(
        open_paths(&session, &["/media/a.en.srt"], &[]),
        OpenPlan::Nothing {},
        "no media to load it onto"
    );

    let _ = session.dispatch(&open("/media/a.mkv"));
    assert_eq!(
        open_paths(&session, &["/media/a.en.srt"], &[]),
        OpenPlan::Nothing {},
        "the media is still opening"
    );
    loaded(&engine, 10.0);
    assert!(wait_for(|| session.snapshot().status == Status::Playing));
    assert_eq!(
        open_paths(&session, &["/media/a.en.srt"], &[]),
        OpenPlan::AddSubtitles {
            paths: vec!["/media/a.en.srt".to_owned()]
        }
    );

    // Media and folders open whatever plays, and the query forwards nothing.
    let forwarded = engine.forwarded().len();
    assert_eq!(
        open_paths(&session, &["/media/Show", "/media/b.mkv"], &["/media/Show"]),
        OpenPlan::Open {
            sources: vec![
                MediaSource {
                    kind: SourceKind::Local,
                    location: "/media/Show".to_owned()
                },
                MediaSource {
                    kind: SourceKind::Local,
                    location: "/media/b.mkv".to_owned()
                },
            ]
        }
    );
    assert_eq!(engine.forwarded().len(), forwarded);
    // A document dropped onto the playing media is no open at all.
    assert_eq!(
        open_paths(&session, &["/media/report.pdf"], &[]),
        OpenPlan::Nothing {}
    );
}

#[test]
fn a_query_without_folders_takes_none_for_folders() {
    let (session, _engine) = fake_session();
    let reply = session.query_json(
        r#"{"schema_version":1,"query":{"open_paths":{"paths":["/media/Show","/media/a.mkv"]}}}"#,
    );
    assert_eq!(
        serde_json::from_str::<QueryReply>(&reply)
            .map(|reply| reply.result)
            .ok(),
        Some(QueryResult::OpenPaths {
            plan: OpenPlan::Open {
                sources: vec![MediaSource {
                    kind: SourceKind::Local,
                    location: "/media/a.mkv".to_owned()
                }]
            }
        })
    );
}

#[test]
fn the_open_formats_query_answers_the_core_types() {
    let (session, _engine) = fake_session();
    let reply = session.query_json(r#"{"schema_version":1,"query":{"open_formats":{}}}"#);
    let Ok(QueryResult::OpenFormats { formats }) =
        serde_json::from_str::<QueryReply>(&reply).map(|reply| reply.result)
    else {
        panic!("open_formats answered {reply}");
    };
    assert_eq!(formats, crate::open_paths::OpenFormats::current());
}
