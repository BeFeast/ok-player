//! The `okp_session_*` C ABI, driven the way a native shell drives it: JSON in,
//! JSON out, wakeups from the session's event thread.

mod common;

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{CHAPTER_TITLES, engine_lock, headless_config, wait_for, write_media_fixture};
use okp_ffi::{
    okp_session_dispatch, okp_session_drain_events, okp_session_free, okp_session_new,
    okp_session_query, okp_session_render, okp_session_render_create, okp_session_render_destroy,
    okp_session_render_report_swap, okp_session_render_update, okp_session_set_wakeup,
    okp_string_free,
};
use okp_session::protocol::{
    DispatchReply, DispatchResult, Event, EventBatch, QueryReply, QueryResult, RejectReason,
    SessionSnapshot, TrackKind,
};
use okp_test_fixtures::unique_temp_dir;
use serde_json::json;

unsafe extern "C" fn count_wakeup(ctx: *mut c_void) {
    let counter = unsafe { &*(ctx as *const AtomicUsize) };
    counter.fetch_add(1, Ordering::SeqCst);
}

fn c_json(value: &serde_json::Value) -> CString {
    CString::new(value.to_string()).expect("JSON has no nul bytes")
}

/// Copy a returned document and release it with `okp_string_free`.
fn take(text: *mut c_char) -> String {
    assert!(!text.is_null(), "the ABI never returns a null document");
    let owned = unsafe { CStr::from_ptr(text) }
        .to_str()
        .expect("documents are UTF-8")
        .to_owned();
    unsafe { okp_string_free(text) };
    owned
}

fn open_command(path: &std::path::Path) -> serde_json::Value {
    json!({
        "schema_version": 1,
        "command": {"open": {"source": {"kind": "local", "location": path}}}
    })
}

#[test]
fn a_shell_opens_a_file_and_drains_tracks_and_chapters_after_wakeups() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-c-abi");
    let media = write_media_fixture(root.path());
    let config = c_json(&serde_json::to_value(headless_config(root.path())).unwrap());
    let mut error = [0 as c_char; 256];
    let session = unsafe { okp_session_new(config.as_ptr(), error.as_mut_ptr(), error.len()) };
    assert!(!session.is_null(), "session failed: {:?}", unsafe {
        CStr::from_ptr(error.as_ptr())
    });

    let wakeups = Box::new(AtomicUsize::new(0));
    let ctx = &*wakeups as *const AtomicUsize as *mut c_void;
    unsafe { okp_session_set_wakeup(session, Some(count_wakeup), ctx) };

    // Validate-then-forward: a command the core refuses never reaches libmpv.
    let toggle = c_json(&json!({"schema_version": 1, "command": {"toggle_pause": {}}}));
    let reply: DispatchReply = serde_json::from_str(&take(unsafe {
        okp_session_dispatch(session, toggle.as_ptr())
    }))
    .unwrap();
    assert!(matches!(
        reply.result,
        DispatchResult::Rejected {
            reason: RejectReason::NoActiveMedia,
            ..
        }
    ));

    let open = c_json(&open_command(&media));
    let reply: DispatchReply = serde_json::from_str(&take(unsafe {
        okp_session_dispatch(session, open.as_ptr())
    }))
    .unwrap();
    assert!(
        matches!(reply.result, DispatchResult::Accepted { .. }),
        "{reply:?}"
    );

    let mut state = SessionSnapshot::default();
    let loaded = wait_for(|| {
        if wakeups.load(Ordering::SeqCst) == 0 {
            return false;
        }
        let batch: EventBatch =
            serde_json::from_str(&take(unsafe { okp_session_drain_events(session) })).unwrap();
        assert_eq!(batch.schema_version, 1);
        for event in batch.events {
            if let Event::SnapshotChanged { changes } = event {
                for change in &changes {
                    state.apply(change);
                }
            }
        }
        state.chapters.len() == CHAPTER_TITLES.len()
            && state
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Audio)
    });
    assert!(loaded, "tracks and chapters never arrived: {state:?}");

    let query = c_json(&json!({"schema_version": 1, "query": {"snapshot": {}}}));
    let reply: QueryReply =
        serde_json::from_str(&take(unsafe { okp_session_query(session, query.as_ptr()) })).unwrap();
    let QueryResult::Snapshot { snapshot } = reply.result else {
        panic!("snapshot query was refused: {reply:?}")
    };
    assert_eq!(snapshot.chapters, state.chapters);
    assert_eq!(snapshot.tracks, state.tracks);

    unsafe { okp_session_free(session) };
    root.close().expect("fixture directory is removed");
}

#[test]
fn freeing_a_session_with_undrained_events_is_safe_and_silences_the_wakeup() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-free");
    let media = write_media_fixture(root.path());
    let config = c_json(&serde_json::to_value(headless_config(root.path())).unwrap());
    let session = unsafe { okp_session_new(config.as_ptr(), ptr::null_mut(), 0) };
    assert!(!session.is_null());

    let wakeups = Box::new(AtomicUsize::new(0));
    let ctx = &*wakeups as *const AtomicUsize as *mut c_void;
    unsafe { okp_session_set_wakeup(session, Some(count_wakeup), ctx) };
    let open = c_json(&open_command(&media));
    take(unsafe { okp_session_dispatch(session, open.as_ptr()) });
    assert!(
        wait_for(|| wakeups.load(Ordering::SeqCst) > 0),
        "a batch was announced"
    );

    // The announced batch is still undrained.
    unsafe { okp_session_free(session) };
    let after_free = wakeups.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        wakeups.load(Ordering::SeqCst),
        after_free,
        "the wakeup ran after okp_session_free returned"
    );
    // The context is released only now; the session no longer holds it.
    drop(wakeups);
    root.close().expect("fixture directory is removed");
}

#[test]
fn malformed_and_future_documents_are_refused() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-malformed");
    let config = c_json(&serde_json::to_value(headless_config(root.path())).unwrap());
    let session = unsafe { okp_session_new(config.as_ptr(), ptr::null_mut(), 0) };
    assert!(!session.is_null());

    for (document, expected) in [
        ("not json", RejectReason::Malformed),
        (r#"{"command":{"close":{}}}"#, RejectReason::Malformed),
        (
            r#"{"schema_version":1,"command":{"rewind":{}}}"#,
            RejectReason::Malformed,
        ),
        (
            r#"{"schema_version":2,"command":{"close":{}}}"#,
            RejectReason::UnsupportedSchemaVersion,
        ),
    ] {
        let text = CString::new(document).unwrap();
        let reply: DispatchReply = serde_json::from_str(&take(unsafe {
            okp_session_dispatch(session, text.as_ptr())
        }))
        .unwrap();
        match reply.result {
            DispatchResult::Rejected { reason, .. } => {
                assert_eq!(reason, expected, "{document}")
            }
            other => panic!("{document} was not refused: {other:?}"),
        }
    }

    let query = CString::new(r#"{"schema_version":1,"query":{"history":{}}}"#).unwrap();
    let reply: QueryReply =
        serde_json::from_str(&take(unsafe { okp_session_query(session, query.as_ptr()) })).unwrap();
    assert!(matches!(
        reply.result,
        QueryResult::Rejected {
            reason: RejectReason::Malformed,
            ..
        }
    ));
    unsafe { okp_session_free(session) };
    root.close().expect("fixture directory is removed");
}

#[test]
fn a_render_context_is_refused_without_an_update_callback() {
    // Advanced control deadlocks libmpv unless the render thread is woken by the
    // update callback, so the session refuses to create a context without one.
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-c-abi-render");
    let config = c_json(&serde_json::to_value(headless_config(root.path())).unwrap());
    let session = unsafe { okp_session_new(config.as_ptr(), ptr::null_mut(), 0) };
    assert!(!session.is_null());

    let mut error = [0 as c_char; 128];
    let created = unsafe {
        okp_session_render_create(
            session,
            None,
            ptr::null_mut(),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    assert!(!created);
    let message = unsafe { CStr::from_ptr(error.as_ptr()) }.to_str().unwrap();
    assert_eq!(message, "the render update callback is required");

    // Without a render context the render calls do nothing and report so.
    assert!(!unsafe { okp_session_render_update(session) });
    assert!(!unsafe { okp_session_render(session, 64, 36) });
    unsafe {
        okp_session_render_report_swap(session);
        okp_session_render_destroy(session);
        okp_session_free(session);
    }
    root.close()
        .expect("the temporary directory should be removed");
}
