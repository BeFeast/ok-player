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

/// Device-independent pixels a primary-button drag must travel before it stops
/// being a click and becomes a compositor-native window move. Small enough to
/// feel responsive, large enough that a jittery single click or a stationary
/// double click never crosses it and steals the play/pause and fullscreen
/// gestures.
pub const WINDOW_MOVE_THRESHOLD: f64 = 6.0;

/// Snapshot the shell samples on each primary-drag update over the player
/// surface. Keeping the arbitration pure lets the gesture tests exercise the
/// click/drag threshold, the maximized/fullscreen guard, the interactive-surface
/// exclusion, and cancellation without a live compositor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowDragContext {
    /// The window is presenting fullscreen; a move must not begin.
    pub fullscreen: bool,
    /// The window is maximized; a move must not begin.
    pub maximized: bool,
    /// The press landed on an interactive surface (OSC, seek/volume sliders,
    /// buttons, popovers, panels, text inputs, or resize handles) whose input
    /// must be preserved instead of consumed by a window move.
    pub over_interactive: bool,
    /// A move has already begun for this drag sequence, so the compositor now
    /// owns the pointer and the shell must not start a second move.
    pub already_moving: bool,
}

/// Whether a primary drag over the player surface should convert into a window
/// move or stay a click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowDragAction {
    /// Leave the click/gesture intact: the threshold is not crossed yet, a move
    /// is already running, or a move is not permitted here.
    Hold,
    /// Convert the drag into a compositor-native window move.
    BeginMove,
}

/// Decide whether an in-progress primary drag should start a window move.
///
/// `offset_x`/`offset_y` are the pointer's travel from the press point in the
/// gesture's own coordinate space. A move begins only once the pointer clears
/// [`WINDOW_MOVE_THRESHOLD`] on a movable, non-interactive surface; every guard
/// resolves to [`WindowDragAction::Hold`] so the underlying click keeps working.
pub fn window_drag_action(
    context: WindowDragContext,
    offset_x: f64,
    offset_y: f64,
) -> WindowDragAction {
    if context.already_moving
        || context.fullscreen
        || context.maximized
        || context.over_interactive
        || offset_x.hypot(offset_y) < WINDOW_MOVE_THRESHOLD
    {
        return WindowDragAction::Hold;
    }
    WindowDragAction::BeginMove
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

    fn movable() -> WindowDragContext {
        WindowDragContext {
            fullscreen: false,
            maximized: false,
            over_interactive: false,
            already_moving: false,
        }
    }

    #[test]
    fn a_short_drag_stays_a_click_until_it_clears_the_threshold() {
        // Just under the threshold keeps the single/double-click gestures intact.
        assert_eq!(
            window_drag_action(movable(), WINDOW_MOVE_THRESHOLD - 0.01, 0.0),
            WindowDragAction::Hold
        );
        // Combined travel is measured as the Euclidean distance, not per-axis.
        assert_eq!(
            window_drag_action(movable(), 3.0, 3.0),
            WindowDragAction::Hold
        );
        // A stationary double-click never moves, so it reaches fullscreen.
        assert_eq!(
            window_drag_action(movable(), 0.0, 0.0),
            WindowDragAction::Hold
        );
    }

    #[test]
    fn crossing_the_threshold_on_a_free_surface_begins_the_move() {
        assert_eq!(
            window_drag_action(movable(), WINDOW_MOVE_THRESHOLD, 0.0),
            WindowDragAction::BeginMove
        );
        assert_eq!(
            window_drag_action(movable(), -5.0, 5.0),
            WindowDragAction::BeginMove
        );
    }

    #[test]
    fn fullscreen_and_maximized_windows_never_start_a_move() {
        let far = WINDOW_MOVE_THRESHOLD + 20.0;
        assert_eq!(
            window_drag_action(
                WindowDragContext {
                    fullscreen: true,
                    ..movable()
                },
                far,
                far
            ),
            WindowDragAction::Hold
        );
        assert_eq!(
            window_drag_action(
                WindowDragContext {
                    maximized: true,
                    ..movable()
                },
                far,
                far
            ),
            WindowDragAction::Hold
        );
    }

    #[test]
    fn interactive_surfaces_keep_their_own_input() {
        let far = WINDOW_MOVE_THRESHOLD + 20.0;
        assert_eq!(
            window_drag_action(
                WindowDragContext {
                    over_interactive: true,
                    ..movable()
                },
                far,
                far
            ),
            WindowDragAction::Hold
        );
    }

    #[test]
    fn a_running_move_is_not_started_again() {
        let far = WINDOW_MOVE_THRESHOLD + 20.0;
        assert_eq!(
            window_drag_action(
                WindowDragContext {
                    already_moving: true,
                    ..movable()
                },
                far,
                far
            ),
            WindowDragAction::Hold
        );
    }
}
