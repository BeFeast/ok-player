//! Commit policy for single-click play/pause versus double-click fullscreen.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    Ignore,
    SchedulePlayPause,
    CancelPlayPauseAndToggleFullscreen,
}

/// Match the existing Windows backdrop gesture: movement begins only after the
/// pointer travels more than four logical pixels in total across both axes.
pub const WINDOW_MOVE_DRAG_THRESHOLD: f64 = 4.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMoveTarget {
    Background,
    InteractiveControl,
    ResizeHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMode {
    Restored,
    Fullscreen,
    Maximized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMoveIntent {
    None,
    StartNativeMove,
    SuppressClick,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowMoveOutcome {
    Click,
    Dragged,
    Ignored,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum WindowMoveState {
    #[default]
    Idle,
    ArmedMove,
    ArmedSuppress,
    Moving,
    Blocked,
}

/// Portable click-versus-drag policy for captionless player backgrounds. The
/// shell supplies GTK widget classification and invokes the compositor-native
/// move API only when this state machine promotes the gesture to a real drag.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowMoveGesture {
    state: WindowMoveState,
}

impl WindowMoveGesture {
    pub fn begin(&mut self, target: WindowMoveTarget, mode: WindowMode) {
        self.state = match (target, mode) {
            (WindowMoveTarget::Background, WindowMode::Restored) => WindowMoveState::ArmedMove,
            (WindowMoveTarget::Background, WindowMode::Fullscreen | WindowMode::Maximized) => {
                WindowMoveState::ArmedSuppress
            }
            (WindowMoveTarget::InteractiveControl | WindowMoveTarget::ResizeHandle, _) => {
                WindowMoveState::Blocked
            }
        };
    }

    pub fn update(&mut self, offset_x: f64, offset_y: f64) -> WindowMoveIntent {
        if offset_x.abs() + offset_y.abs() <= WINDOW_MOVE_DRAG_THRESHOLD {
            return WindowMoveIntent::None;
        }

        match self.state {
            WindowMoveState::ArmedMove => {
                self.state = WindowMoveState::Moving;
                WindowMoveIntent::StartNativeMove
            }
            WindowMoveState::ArmedSuppress => {
                self.state = WindowMoveState::Moving;
                WindowMoveIntent::SuppressClick
            }
            WindowMoveState::Idle | WindowMoveState::Moving | WindowMoveState::Blocked => {
                WindowMoveIntent::None
            }
        }
    }

    pub fn finish(&mut self) -> WindowMoveOutcome {
        let outcome = match self.state {
            WindowMoveState::ArmedMove | WindowMoveState::ArmedSuppress => WindowMoveOutcome::Click,
            WindowMoveState::Moving => WindowMoveOutcome::Dragged,
            WindowMoveState::Idle | WindowMoveState::Blocked => WindowMoveOutcome::Ignored,
        };
        self.state = WindowMoveState::Idle;
        outcome
    }

    pub fn cancel(&mut self) {
        self.state = WindowMoveState::Idle;
    }
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

    #[test]
    fn background_click_stays_a_click_until_the_drag_threshold_is_crossed() {
        let mut gesture = WindowMoveGesture::default();
        gesture.begin(WindowMoveTarget::Background, WindowMode::Restored);

        assert_eq!(gesture.update(2.0, 2.0), WindowMoveIntent::None);
        assert_eq!(gesture.finish(), WindowMoveOutcome::Click);

        gesture.begin(WindowMoveTarget::Background, WindowMode::Restored);
        assert_eq!(gesture.update(3.0, 2.0), WindowMoveIntent::StartNativeMove);
        assert_eq!(gesture.update(12.0, 8.0), WindowMoveIntent::None);
        assert_eq!(gesture.finish(), WindowMoveOutcome::Dragged);
    }

    #[test]
    fn interactive_controls_and_resize_handles_never_arm_window_movement() {
        for target in [
            WindowMoveTarget::InteractiveControl,
            WindowMoveTarget::ResizeHandle,
        ] {
            let mut gesture = WindowMoveGesture::default();
            gesture.begin(target, WindowMode::Restored);
            assert_eq!(
                gesture.update(100.0, 100.0),
                WindowMoveIntent::None,
                "target should remain excluded: {target:?}"
            );
            assert_eq!(gesture.finish(), WindowMoveOutcome::Ignored);
        }
    }

    #[test]
    fn fullscreen_and_maximized_windows_never_arm_window_movement() {
        for mode in [WindowMode::Fullscreen, WindowMode::Maximized] {
            let mut gesture = WindowMoveGesture::default();
            gesture.begin(WindowMoveTarget::Background, mode);
            assert_eq!(
                gesture.update(100.0, 100.0),
                WindowMoveIntent::SuppressClick,
                "window mode should suppress the dragged click without moving: {mode:?}"
            );
            assert_eq!(gesture.finish(), WindowMoveOutcome::Dragged);
        }
    }
}
