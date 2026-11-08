//! Turning libmpv end-of-file and failure reports into core player events.
//! Shared by the session and the older live C ABI in `okp-ffi`.

use okp_core::player::EndReason;
use okp_core::playlist::PlaylistItem;
use okp_mpv::{EndFileReason, error_description};

/// An end-of-file is about the current media only if the engine names the same
/// source (or none). A stale failure of a source the user already replaced must
/// not end the new one.
pub fn event_matches_source(current: Option<&PlaylistItem>, ended_path: Option<&str>) -> bool {
    let Some(current) = current else { return false };
    ended_path.is_none_or(|ended| match current {
        PlaylistItem::Local(path) => path.to_string_lossy() == ended,
        PlaylistItem::Url(url) => url == ended,
    })
}

/// The failure an end-of-file reports, if any: an engine error, or a decoder
/// failure that libmpv surfaced as a premature end of file.
pub fn endfile_error(reason: EndFileReason, messages: &[String]) -> Option<String> {
    match reason {
        EndFileReason::Error(code) | EndFileReason::Unknown(code) => {
            let mut message = error_description(code);
            if let Some(diagnostic) = messages.last() {
                message.push_str(": ");
                message.push_str(diagnostic.trim());
            }
            Some(message)
        }
        EndFileReason::Eof => okp_core::playback_failure::diagnose_mpv_eof(
            messages,
            okp_core::playback_failure::CodecEnvironment::System,
        )
        .map(|diagnostic| diagnostic.detail),
        _ => None,
    }
}

/// The core meaning of a libmpv end-of-file reason.
pub fn core_end_reason(reason: EndFileReason) -> EndReason {
    match reason {
        EndFileReason::Eof => EndReason::Eof,
        EndFileReason::Stop => EndReason::Stopped,
        EndFileReason::Quit => EndReason::Quit,
        EndFileReason::Error(_) | EndFileReason::Unknown(_) => EndReason::Error,
        EndFileReason::Redirect => EndReason::Redirect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superseded_source_errors_do_not_match_the_current_video() {
        let current = PlaylistItem::Local("/media/b.mp4".into());
        assert!(!event_matches_source(Some(&current), Some("/media/a.mp4")));
        assert!(event_matches_source(Some(&current), Some("/media/b.mp4")));
        assert!(event_matches_source(Some(&current), None));
        assert!(!event_matches_source(None, None));
    }

    #[test]
    fn decoder_failure_at_eof_is_reported_but_normal_eof_is_not() {
        let messages = vec!["Failed to open codec".to_owned()];
        assert!(endfile_error(EndFileReason::Eof, &messages).is_some());
        assert!(endfile_error(EndFileReason::Eof, &[]).is_none());
        assert!(endfile_error(EndFileReason::Stop, &messages).is_none());
    }

    #[test]
    fn end_file_reasons_keep_core_meaning() {
        assert_eq!(core_end_reason(EndFileReason::Eof), EndReason::Eof);
        assert_eq!(core_end_reason(EndFileReason::Stop), EndReason::Stopped);
        assert_eq!(core_end_reason(EndFileReason::Error(-12)), EndReason::Error);
        assert_eq!(
            core_end_reason(EndFileReason::Redirect),
            EndReason::Redirect
        );
    }
}
