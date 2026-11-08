//! JSON session C ABI over `okp-session`.
//!
//! Every function exchanges UTF-8, nul-terminated JSON documents (see
//! `docs/session-abi.md` for the envelope, `schema_version`, threading and
//! ownership rules). Strings returned here are owned by the caller and must be
//! released with [`okp_string_free`]. The render calls draw video frames into the
//! shell's OpenGL framebuffer; frames never travel as JSON.

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;

use okp_session::Session;
use okp_session::protocol::{
    RejectReason, dispatch_rejection_json, empty_event_batch_json, query_rejection_json,
};

use crate::c_text::copy_text;

/// Opaque session handle. Create with [`okp_session_new`], release with
/// [`okp_session_free`].
pub struct OkpSession {
    inner: Session,
}

/// Called from the session's event thread when a batch of events is ready to
/// drain. `ctx` is the pointer given to [`okp_session_set_wakeup`].
pub type OkpSessionWakeup = Option<unsafe extern "C" fn(ctx: *mut c_void)>;

/// Create a session from a config document. On failure returns null and, when
/// `error_buffer` is non-null and `error_capacity` nonzero, copies a nul-terminated
/// message into it.
///
/// # Safety
/// `config_json` must be null or a valid nul-terminated string. `error_buffer`, when
/// non-null, must identify writable storage of at least `error_capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_new(
    config_json: *const c_char,
    error_buffer: *mut c_char,
    error_capacity: usize,
) -> *mut OkpSession {
    let result = match unsafe { utf8(config_json) } {
        Some(text) => Session::from_config_json(text),
        None => Err("the session config must be non-null UTF-8 text".to_owned()),
    };
    match result {
        Ok(inner) => Box::into_raw(Box::new(OkpSession { inner })),
        Err(message) => {
            unsafe { copy_text(&message, error_buffer, error_capacity) };
            ptr::null_mut()
        }
    }
}

/// Stop the session and release it. Undrained events are discarded. When this
/// returns, the wakeup is not running and will not be called again. Must not be
/// called concurrently with any other call on the same session.
///
/// # Safety
/// `session` must be null or a pointer returned by [`okp_session_new`] that has not
/// been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_free(session: *mut OkpSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Dispatch a command document; returns a reply document. Never returns null.
///
/// # Safety
/// `session` must be null or live. `command_json` must be null or a valid
/// nul-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_dispatch(
    session: *const OkpSession,
    command_json: *const c_char,
) -> *mut c_char {
    let reply = match (unsafe { session.as_ref() }, unsafe { utf8(command_json) }) {
        (Some(session), Some(text)) => session.inner.dispatch_json(text),
        _ => dispatch_rejection_json(RejectReason::InvalidArgument),
    };
    into_c_string(reply)
}

/// Answer a query document; returns a reply document. Never returns null.
///
/// # Safety
/// `session` must be null or live. `query_json` must be null or a valid
/// nul-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_query(
    session: *const OkpSession,
    query_json: *const c_char,
) -> *mut c_char {
    let reply = match (unsafe { session.as_ref() }, unsafe { utf8(query_json) }) {
        (Some(session), Some(text)) => session.inner.query_json(text),
        _ => query_rejection_json(RejectReason::InvalidArgument),
    };
    into_c_string(reply)
}

/// Install (or, with a null `callback`, remove) the wakeup. It is called from the
/// session's event thread once per batch: after it fires, it fires again only
/// after [`okp_session_drain_events`] has taken the batch. When this returns, a
/// replaced wakeup is not running and will not be called again. The wakeup must
/// not call this function or [`okp_session_free`].
///
/// # Safety
/// `session` must be null or live. `ctx` must stay valid for use from another
/// thread until the wakeup is replaced or the session is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_set_wakeup(
    session: *const OkpSession,
    callback: OkpSessionWakeup,
    ctx: *mut c_void,
) {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return;
    };
    let wakeup = callback.map(|callback| CCallback { callback, ctx });
    session.inner.set_wakeup(
        wakeup.map(|wakeup| Box::new(move || wakeup.call()) as Box<dyn Fn() + Send + Sync>),
    );
}

/// Take every queued event as one batch document. Never returns null.
///
/// # Safety
/// `session` must be null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_drain_events(session: *const OkpSession) -> *mut c_char {
    let batch = match unsafe { session.as_ref() } {
        Some(session) => session.inner.drain_events_json(),
        None => empty_event_batch_json(),
    };
    into_c_string(batch)
}

/// Called from an engine thread when the render context has work for the
/// shell's render thread. `ctx` is the pointer given to
/// [`okp_session_render_create`]. It must only wake the render thread, which then
/// calls [`okp_session_render_update`]; it must not call any `okp_session_*`
/// function itself.
pub type OkpSessionRenderUpdate = Option<unsafe extern "C" fn(ctx: *mut c_void)>;

/// Create the session's OpenGL render context, with libmpv's advanced control,
/// and install `update` before returning. Call it on the shell's render thread
/// with the shell's OpenGL context current; a second call while the context
/// exists does nothing. Returns false on failure (a null `update` is refused) and,
/// when `error_buffer` is non-null and `error_capacity` nonzero, copies a
/// nul-terminated message into it. Free the context with
/// [`okp_session_render_destroy`] before [`okp_session_free`].
///
/// Advanced control binds the render thread: it must call
/// [`okp_session_render_update`] promptly whenever `update` fires, and it must
/// never wait for a thread that is inside another `okp_session_*` call.
///
/// # Safety
/// `session` must be null or live. `update_ctx` must stay valid for use from
/// another thread until [`okp_session_render_destroy`] returns. `error_buffer`, when non-null, must identify writable storage of at
/// least `error_capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_render_create(
    session: *const OkpSession,
    update: OkpSessionRenderUpdate,
    update_ctx: *mut c_void,
    error_buffer: *mut c_char,
    error_capacity: usize,
) -> bool {
    let result = match (unsafe { session.as_ref() }, update) {
        (None, _) => Err("the session is null".to_owned()),
        (Some(_), None) => Err("the render update callback is required".to_owned()),
        (Some(session), Some(callback)) => {
            let update = CCallback {
                callback,
                ctx: update_ctx,
            };
            session.inner.render_create(Box::new(move || update.call()))
        }
    };
    match result {
        Ok(()) => true,
        Err(message) => {
            unsafe { copy_text(&message, error_buffer, error_capacity) };
            false
        }
    }
}

/// Serve libmpv's pending render-thread work after the update callback fired,
/// and return true when a new frame is due. Call it every time the callback
/// fires, even when nothing will be drawn. False without a render context.
///
/// # Safety
/// `session` must be null or live. The OpenGL context the render context was
/// created under must be current on the calling thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_render_update(session: *const OkpSession) -> bool {
    unsafe { session.as_ref() }.is_some_and(|session| session.inner.render_update())
}

/// Draw the due video frame, or redraw the current one, into the bound
/// framebuffer of `width` by `height` pixels. It does not report the swap: call
/// [`okp_session_render_report_swap`] after the shell has flushed the frame.
/// Returns false if there is no render context or libmpv failed to draw.
///
/// # Safety
/// `session` must be null or live. The OpenGL context the render context was
/// created under must be current on the calling thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_render(
    session: *const OkpSession,
    width: i32,
    height: i32,
) -> bool {
    unsafe { session.as_ref() }.is_some_and(|session| session.inner.render(width, height).is_ok())
}

/// Tell libmpv that the frame drawn last has been presented, after the shell's
/// flush, so it can time the next one. Returns the number of swaps reported to
/// the current render context so far, this one included, so the shell can check
/// that every frame it presented reached libmpv. Does nothing and returns 0
/// without a render context.
///
/// # Safety
/// `session` must be null or live. The OpenGL context the render context was
/// created under must be current on the calling thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_render_report_swap(session: *const OkpSession) -> u64 {
    unsafe { session.as_ref() }.map_or(0, |session| session.inner.render_report_swap())
}

/// Free the render context, if there is one. When this returns, the update
/// callback is not running and is never called again.
///
/// # Safety
/// `session` must be null or live. The OpenGL context the render context was
/// created under must be current on the calling thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_session_render_destroy(session: *const OkpSession) {
    if let Some(session) = unsafe { session.as_ref() } {
        session.inner.render_destroy();
    }
}

/// Release a string returned by an `okp_session_*` function. Null is ignored.
///
/// # Safety
/// `text` must be null or a pointer returned by this library that has not been
/// freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

/// A C callback and its context: the wakeup or the render update callback.
struct CCallback {
    callback: unsafe extern "C" fn(*mut c_void),
    ctx: *mut c_void,
}

// SAFETY: the callers of `okp_session_set_wakeup` and `okp_session_render_create`
// guarantee that `ctx` may be used from the session's threads for as long as the
// callback is installed.
unsafe impl Send for CCallback {}
unsafe impl Sync for CCallback {}

impl CCallback {
    fn call(&self) {
        unsafe { (self.callback)(self.ctx) }
    }
}

/// # Safety
/// `text` must be null or a valid nul-terminated string.
unsafe fn utf8<'a>(text: *const c_char) -> Option<&'a str> {
    if text.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(text) }.to_str().ok()
}

fn into_c_string(text: String) -> *mut c_char {
    // JSON escapes control characters, so a document never contains a nul byte.
    CString::new(text)
        .unwrap_or_else(|error| {
            let bytes = error.into_vec().into_iter().filter(|byte| *byte != 0);
            CString::new(bytes.collect::<Vec<u8>>()).expect("nul bytes were removed")
        })
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn ignore_update(_ctx: *mut c_void) {}

    fn take(text: *mut c_char) -> String {
        assert!(!text.is_null());
        let owned = unsafe { CStr::from_ptr(text) }
            .to_str()
            .expect("replies are UTF-8")
            .to_owned();
        unsafe { okp_string_free(text) };
        owned
    }

    #[test]
    fn calls_without_a_session_answer_with_documents_not_null() {
        let command = CString::new(r#"{"schema_version":1,"command":{"close":{}}}"#).unwrap();
        let reply = take(unsafe { okp_session_dispatch(ptr::null(), command.as_ptr()) });
        assert!(reply.contains(r#""reason":"invalid_argument""#), "{reply}");

        let query = CString::new(r#"{"schema_version":1,"query":{"snapshot":{}}}"#).unwrap();
        let reply = take(unsafe { okp_session_query(ptr::null(), query.as_ptr()) });
        assert!(reply.contains(r#""reason":"invalid_argument""#), "{reply}");

        let batch = take(unsafe { okp_session_drain_events(ptr::null()) });
        assert_eq!(batch, r#"{"schema_version":1,"events":[]}"#);

        unsafe {
            okp_session_set_wakeup(ptr::null(), None, ptr::null_mut());
            okp_string_free(ptr::null_mut());
            okp_session_free(ptr::null_mut());
        }

        let mut buffer = [0 as c_char; 64];
        assert!(!unsafe {
            okp_session_render_create(
                ptr::null(),
                Some(ignore_update),
                ptr::null_mut(),
                buffer.as_mut_ptr(),
                64,
            )
        });
        let message = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().unwrap();
        assert_eq!(message, "the session is null");
        assert!(!unsafe { okp_session_render_update(ptr::null()) });
        assert!(!unsafe { okp_session_render(ptr::null(), 16, 16) });
        assert_eq!(unsafe { okp_session_render_report_swap(ptr::null()) }, 0);
        unsafe { okp_session_render_destroy(ptr::null()) };
    }

    #[test]
    fn a_rejected_config_returns_null_and_explains_why() {
        let config = CString::new(r#"{"schema_version":2}"#).unwrap();
        let mut buffer = [0 as c_char; 160];
        let session =
            unsafe { okp_session_new(config.as_ptr(), buffer.as_mut_ptr(), buffer.len()) };
        assert!(session.is_null());
        let message = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().unwrap();
        assert!(message.contains("schema_version 2"), "{message}");

        let session = unsafe { okp_session_new(ptr::null(), ptr::null_mut(), 0) };
        assert!(session.is_null());
    }
}
