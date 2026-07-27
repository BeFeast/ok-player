use std::path::Path;

/// Character budget for the generic status-toast message.
///
/// The toast is deliberately excluded from overlay measurement, so nothing else stops a long
/// dynamic message from being clipped rather than ellipsized. The label also carries a matching
/// Pango bound, but the text is capped here so the guarantee does not depend on layout.
pub(crate) const TOAST_MESSAGE_MAX_CHARS: usize = 72;

/// Collapses everything Pango would turn into a second line into a single space.
///
/// Local paths may legally contain newlines and other control characters, and a multi-line label
/// grows the toast vertically over the video.
fn to_single_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() || character == '\u{2028}' || character == '\u{2029}' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// Shortens `text` to `max_chars` characters, keeping both ends and eliding the middle.
///
/// Operates on characters so non-ASCII text is never split mid-codepoint.
fn middle_ellipsize(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars || max_chars == 0 {
        return text.to_owned();
    }

    let kept = max_chars - 1;
    let head = kept.div_ceil(2);
    let tail = kept - head;
    let mut shortened: String = text.chars().take(head).collect();
    shortened.push('…');
    shortened.extend(text.chars().skip(total - tail));
    shortened
}

/// Text for the generic status-toast message: one line, bounded length.
pub(crate) fn bounded_toast_message(message: &str) -> String {
    middle_ellipsize(&to_single_line(message), TOAST_MESSAGE_MAX_CHARS)
}

/// Full displayable form of a saved path for the toast link, tooltip, and accessible label.
///
/// Kept complete on purpose — the visible label is ellipsized by Pango, while the tooltip and
/// accessible text are expected to carry the whole path.
pub(crate) fn toast_display_path(path: &Path) -> String {
    to_single_line(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    #[test]
    fn short_messages_are_passed_through_unchanged() {
        assert_eq!(bounded_toast_message("Volume 72%"), "Volume 72%");
    }

    #[test]
    fn long_messages_are_bounded_to_the_toast_budget() {
        let message = format!("Copied {}", "ok-player://reserved/".repeat(40));

        let bounded = bounded_toast_message(&message);

        assert_eq!(bounded.chars().count(), TOAST_MESSAGE_MAX_CHARS);
        assert!(bounded.starts_with("Copied ok-player://"));
        assert!(bounded.ends_with("reserved/"));
        assert!(bounded.contains('…'));
    }

    #[test]
    fn bounding_never_splits_a_multibyte_character() {
        let message = "Плейлист открыт: ".to_owned() + &"кадр".repeat(60);

        let bounded = bounded_toast_message(&message);

        assert_eq!(bounded.chars().count(), TOAST_MESSAGE_MAX_CHARS);
        assert!(bounded.starts_with("Плейлист открыт: "));
        assert!(bounded.ends_with("кадр"));
    }

    #[test]
    fn messages_are_flattened_to_one_line() {
        let bounded = bounded_toast_message("Couldn't open\n/screens/one\ntwo.png\t(2)");

        assert!(!bounded.contains('\n'), "unexpected line break: {bounded}");
        assert!(!bounded.contains('\t'), "unexpected tab: {bounded}");
        assert_eq!(bounded, "Couldn't open /screens/one two.png (2)");
    }

    #[test]
    fn displayed_paths_keep_spaces_and_non_ascii_intact() {
        let path = PathBuf::from("/screens/OK Player/кадр 01.png");

        assert_eq!(toast_display_path(&path), "/screens/OK Player/кадр 01.png");
    }

    #[test]
    fn displayed_paths_cannot_add_a_second_toast_line() {
        let path = PathBuf::from("/screens/one\ntwo/frame\r\n01.png");

        let displayed = toast_display_path(&path);

        assert!(
            !displayed.contains('\n'),
            "unexpected line break: {displayed}"
        );
        assert!(
            !displayed.contains('\r'),
            "unexpected carriage return: {displayed}"
        );
        assert_eq!(displayed, "/screens/one two/frame  01.png");
    }

    #[test]
    fn displayed_paths_survive_non_utf8_bytes() {
        let path = PathBuf::from(OsString::from_vec(b"/screens/frame-\xff.png".to_vec()));

        let displayed = toast_display_path(&path);

        assert!(displayed.starts_with("/screens/frame-"));
        assert!(displayed.ends_with(".png"));
    }
}
