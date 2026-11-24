//! JSON bridge for the shared feedback outbox.
use okp_core::feedback::{Outbox, Submission};
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char},
    path::Path,
};

/// Read the feedback form, including a pending report, as an owned JSON string.
///
/// # Safety
/// Directory is a valid NUL-terminated UTF-8 string. Release the result with
/// `okp_feedback_free`. A nonzero port selects an isolated loopback test queue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_feedback_form(
    directory: *const c_char,
    test_port: u16,
) -> *mut c_char {
    let result = unsafe { input(directory) }
        .and_then(|dir| outbox(dir, test_port).form())
        .map(|form| json!(form));
    output(result)
}

/// Submit a JSON-encoded report. Blocks during the network request.
///
/// # Safety
/// Both inputs are valid NUL-terminated UTF-8 strings. Release the result with
/// `okp_feedback_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_feedback_submit(
    directory: *const c_char,
    test_port: u16,
    submission: *const c_char,
) -> *mut c_char {
    let result = (|| {
        let directory = unsafe { input(directory) }?;
        let data: Submission = serde_json::from_str(unsafe { input(submission) }?)
            .map_err(|_| "Could not read the feedback form.")?;
        outbox(directory, test_port)
            .submit(data.token, data.report)
            .map(|receipt| json!(receipt))
    })();
    output(result)
}

/// Discard the pending report identified by the form's token.
///
/// # Safety
/// Both inputs are valid NUL-terminated UTF-8 strings. Release the result with
/// `okp_feedback_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_feedback_discard(
    directory: *const c_char,
    test_port: u16,
    token: *const c_char,
) -> *mut c_char {
    let result = (|| {
        let directory = unsafe { input(directory) }?;
        let token = unsafe { input(token) }?
            .parse()
            .map_err(|_| "Invalid draft token.")?;
        outbox(directory, test_port)
            .discard(token)
            .map(|()| json!(true))
    })();
    output(result)
}

/// Release a feedback JSON string.
///
/// # Safety
/// Pass a live allocation returned by a feedback function, exactly once, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_feedback_free(response: *mut c_char) {
    if !response.is_null() {
        drop(unsafe { CString::from_raw(response) });
    }
}

unsafe fn input<'a>(value: *const c_char) -> Result<&'a str, &'static str> {
    if value.is_null() {
        return Err("Missing feedback input.");
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| "Invalid feedback input.")
}

fn outbox(directory: &str, test_port: u16) -> Outbox {
    if test_port == 0 {
        Outbox::new(Path::new(directory))
    } else {
        Outbox::loopback(Path::new(directory), test_port)
    }
}

fn output(value: Result<Value, &'static str>) -> *mut c_char {
    let response = match value {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": error }),
    };
    CString::new(response.to_string())
        .expect("JSON escapes interior NULs")
        .into_raw()
}
