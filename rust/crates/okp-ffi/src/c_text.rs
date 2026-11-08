//! Copying Rust text into caller-owned C buffers.

use std::ffi::c_char;

/// Copy bytes and always terminate when capacity allows. Interior nuls from foreign
/// diagnostic text are replaced so C consumers never observe a truncated message.
pub(crate) unsafe fn copy_text(text: &str, buffer: *mut c_char, capacity: usize) -> usize {
    let required = text.len().saturating_add(1);
    if buffer.is_null() || capacity == 0 {
        return required;
    }
    let count = text.len().min(capacity.saturating_sub(1));
    for (index, byte) in text.as_bytes()[..count].iter().copied().enumerate() {
        unsafe {
            buffer
                .add(index)
                .write(if byte == 0 { b'?' } else { byte } as c_char)
        };
    }
    unsafe { buffer.add(count).write(0) };
    required
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_copy_reports_required_size_and_nul_terminates_truncation() {
        let mut buffer = [b'x' as c_char; 5];
        let required = unsafe { copy_text("abcdef", buffer.as_mut_ptr(), buffer.len()) };

        assert_eq!(required, 7);
        assert_eq!(
            &buffer,
            &[
                b'a' as c_char,
                b'b' as c_char,
                b'c' as c_char,
                b'd' as c_char,
                0
            ]
        );
    }
}
