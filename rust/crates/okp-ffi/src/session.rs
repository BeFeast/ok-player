//! JSON session C ABI over `okp-session`.
//!
//! Every function exchanges UTF-8, nul-terminated JSON documents (see
//! `docs/session-abi.md` for the envelope, `schema_version`, threading and
//! ownership rules). Strings returned here are owned by the caller and must be
//! released with [`okp_string_free`]. Render calls are not part of this surface yet.

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
    let wakeup = callback.map(|callback| CWakeup { callback, ctx });
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

struct CWakeup {
    callback: unsafe extern "C" fn(*mut c_void),
    ctx: *mut c_void,
}

// SAFETY: the caller of `okp_session_set_wakeup` guarantees that `ctx` may be used
// from the session's event thread for as long as the wakeup is installed.
unsafe impl Send for CWakeup {}
unsafe impl Sync for CWakeup {}

impl CWakeup {
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
