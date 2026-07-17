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

/// Whether a pointer drag has travelled far enough from its press origin to
/// become a window move rather than a click. A stationary press — the first half
/// of a double-click — stays under the threshold and must never begin a move, so
/// the second click still reaches the fullscreen toggle. The shell owns the live
/// offset from its drag gesture; this keeps the threshold decision pure and
/// testable across the compact and canonical drag surfaces.
pub fn drag_exceeds_move_threshold(offset_x: f64, offset_y: f64, threshold_px: f64) -> bool {
    offset_x.hypot(offset_y) >= threshold_px
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

    #[test]
    fn a_stationary_double_click_press_never_starts_a_move() {
        assert!(!drag_exceeds_move_threshold(0.0, 0.0, 6.0));
        // Sub-threshold jitter from a heavy double-click stays a click.
        assert!(!drag_exceeds_move_threshold(3.0, 4.0, 6.0));
    }

    #[test]
    fn crossing_the_threshold_promotes_the_gesture_to_a_move() {
        // 3-4-5 triangle: exactly at the threshold and beyond both move.
        assert!(drag_exceeds_move_threshold(3.0, 4.0, 5.0));
        assert!(drag_exceeds_move_threshold(-10.0, 0.0, 6.0));
    }
}
