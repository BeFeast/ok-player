//! Commit policy for single-click play/pause versus double-click fullscreen.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    Ignore,
    SchedulePlayPause,
    CancelPlayPauseAndToggleFullscreen,
}

/// Classify a primary-button release count. The shell owns the platform timer,
/// while this pure policy guarantees a double-click cancels the pending single
/// click instead of briefly pausing before fullscreen.
pub fn release_intent(press_count: i32) -> Intent {
    match press_count {
        1 => Intent::SchedulePlayPause,
        2 => Intent::CancelPlayPauseAndToggleFullscreen,
        _ => Intent::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_double_clicks_have_distinct_commit_intents() {
        assert_eq!(release_intent(1), Intent::SchedulePlayPause);
        assert_eq!(
            release_intent(2),
            Intent::CancelPlayPauseAndToggleFullscreen
        );
        assert_eq!(release_intent(3), Intent::Ignore);
    }
}
