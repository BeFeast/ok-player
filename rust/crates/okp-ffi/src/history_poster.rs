use std::ffi::{CStr, c_char};
use std::ptr;

use okp_mpv::{POSTER_HEIGHT, POSTER_WIDTH, decode_history_poster, history_poster_key};

const POSTER_KEY_BUFFER_LEN: usize = 17;

/// Write the local video's current 16-digit ASCII fingerprint, including a terminating NUL.
/// Returns false when the source is ineligible, unavailable, or the buffer is not 17 bytes.
///
/// # Safety
/// `path` must be a valid NUL-terminated C string and `output` must point to
/// `output_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn okp_history_poster_key(
    path: *const c_char,
    output: *mut c_char,
    output_len: usize,
) -> bool {
    if path.is_null() || output.is_null() || output_len != POSTER_KEY_BUFFER_LEN {
        return false;
    }
    let Some(key) = (unsafe { CStr::from_ptr(path) })
        .to_str()
        .ok()
        .and_then(history_poster_key)
    else {
        return false;
    };
    if key.len() != output_len - 1 {
        return false;
    }
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
    if frame.len() != output_len {
        return false;
    }
    unsafe { ptr::copy_nonoverlapping(frame.as_ptr(), output, frame.len()) };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poster_key_leaves_a_short_output_buffer_untouched() {
        let path = std::env::current_exe().expect("test binary path");
        let path = std::ffi::CString::new(path.to_str().expect("UTF-8 test binary path")).unwrap();
        let mut bytes = [1 as c_char; POSTER_KEY_BUFFER_LEN];
        let short_len = bytes.len() - 1;
        assert!(!unsafe { okp_history_poster_key(path.as_ptr(), bytes.as_mut_ptr(), short_len) });
        assert_eq!(bytes, [1 as c_char; POSTER_KEY_BUFFER_LEN]);
    }
}
