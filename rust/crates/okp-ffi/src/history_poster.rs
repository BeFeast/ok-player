use std::ffi::{CStr, c_char};
use std::ptr;

use okp_mpv::{POSTER_HEIGHT, POSTER_WIDTH, decode_history_poster, history_poster_key};

/// Write the local video's current 16-digit ASCII fingerprint, including a terminating NUL.
/// Returns false when the source is ineligible or unavailable.
///
/// # Safety
/// `path` must be a valid NUL-terminated C string and `output` must have 17 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_history_poster_key(path: *const c_char, output: *mut c_char) -> bool {
    if path.is_null() || output.is_null() {
        return false;
    }
    let Some(key) = (unsafe { CStr::from_ptr(path) })
        .to_str()
        .ok()
        .and_then(history_poster_key)
    else {
        return false;
    };
    unsafe {
        ptr::copy_nonoverlapping(key.as_ptr().cast(), output, key.len());
        *output.add(key.len()) = 0;
    }
    true
}

/// Sample the local video off the UI thread. On success, write one 384×216 BGRx image
/// on little-endian targets. The FFI never keeps the caller's buffer.
///
/// # Safety
/// `path` must be a valid NUL-terminated C string and `output` must point to
/// `output_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_history_poster_decode(
    path: *const c_char,
    duration: f64,
    output: *mut u8,
    output_len: usize,
) -> bool {
    if path.is_null() || output.is_null() || output_len != POSTER_WIDTH * POSTER_HEIGHT * 4 {
        return false;
    }
    let Some(frame) = (unsafe { CStr::from_ptr(path) })
        .to_str()
        .ok()
        .and_then(|path| decode_history_poster(path, duration))
    else {
        return false;
    };
    unsafe { ptr::copy_nonoverlapping(frame.as_ptr(), output, frame.len()) };
    true
}
