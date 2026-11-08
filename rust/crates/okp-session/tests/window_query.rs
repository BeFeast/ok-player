//! The window queries through the `okp_session_*` C ABI: the golden query
//! documents a shell sends are answered with exactly the golden replies, the fit
//! and click answers need no loaded media, and no initial fit is owed before a
//! source is open.

mod common;

use std::ffi::{CStr, CString, c_char};
use std::fs;
use std::path::PathBuf;
use std::ptr;

use common::{engine_lock, headless_config};
use okp_ffi::{okp_session_free, okp_session_new, okp_session_query, okp_string_free};
use okp_session::protocol::{QueryReply, QueryResult, VideoClickIntent, WindowRect};
use okp_test_fixtures::unique_temp_dir;
use serde_json::{Value, json};

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()
}

/// Send one query document and return the reply document.
fn query(session: *const okp_ffi::OkpSession, document: &Value) -> Value {
    let text = CString::new(document.to_string()).unwrap();
    let reply: *mut c_char = unsafe { okp_session_query(session, text.as_ptr()) };
    assert!(!reply.is_null());
    let owned = unsafe { CStr::from_ptr(reply) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { okp_string_free(reply) };
    serde_json::from_str(&owned).unwrap()
}

fn result(reply: Value) -> QueryResult {
    serde_json::from_value::<QueryReply>(reply).unwrap().result
}

#[test]
fn the_window_queries_are_answered_by_core_without_media() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-window-query");
    let config =
        CString::new(serde_json::to_string(&headless_config(root.path())).unwrap()).unwrap();
    let session = unsafe { okp_session_new(config.as_ptr(), ptr::null_mut(), 0) };
    assert!(!session.is_null());

    assert_eq!(
        query(session, &fixture("query-window-fit.json")),
        fixture("query-reply-window-fit.json"),
        "the golden fit query gets the golden reply"
    );
    assert_eq!(
        query(session, &fixture("query-video-click.json")),
        fixture("query-reply-video-click.json"),
    );

    // One click waits for a possible second one; three are nothing.
    for (count, intent) in [
        (1, VideoClickIntent::SchedulePlayPause),
        (3, VideoClickIntent::Ignore),
    ] {
        let reply = query(
            session,
            &json!({"schema_version": 1, "query": {"video_click": {"press_count": count}}}),
        );
        assert_eq!(result(reply), QueryResult::VideoClick { intent });
    }

    // A shell's encoder leaves out a missing minimum size, and a video with no
    // size has no fit.
    let reply = query(
        session,
        &json!({"schema_version": 1, "query": {"window_fit": {
            "video": {"width": 1280, "height": 720},
            "backing_scale": 2.0,
            "visible_frame": {"x": 0, "y": 0, "width": 1512, "height": 945}
        }}}),
    );
    assert_eq!(
        result(reply),
        QueryResult::WindowFit {
            frame: Some(WindowRect {
                x: 436,
                y: 292,
                width: 640,
                height: 360,
            })
        }
    );
    let reply = query(
        session,
        &json!({"schema_version": 1, "query": {"window_fit": {
            "video": {"width": 0, "height": 0},
            "backing_scale": 2.0,
            "visible_frame": {"x": 0, "y": 0, "width": 1512, "height": 945}
        }}}),
    );
    assert_eq!(result(reply), QueryResult::WindowFit { frame: None });

    // The fit an opened source is owed: none before anything is open.
    assert_eq!(
        result(query(session, &fixture("query-initial-window-fit.json"))),
        QueryResult::WindowFit { frame: None }
    );

    unsafe { okp_session_free(session) };
    root.close().expect("fixture directory is removed");
}
