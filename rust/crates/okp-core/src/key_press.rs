//! Physical key press latching for commands that must ignore auto-repeat.
//!
//! Desktop shells often receive repeated key-pressed events while a key remains
//! held, followed by a single release. This policy turns that stream into one
//! command dispatch per physical press without depending on toolkit-specific
//! repeat flags.

/// Tracks whether a command key is currently held.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyPressLatch {
    pressed: bool,
}

impl KeyPressLatch {
    /// Records a press and returns `true` only for the first press before release.
    pub fn press(&mut self) -> bool {
        !std::mem::replace(&mut self.pressed, true)
    }

    /// Records the physical key release so the next press can dispatch again.
    pub fn release(&mut self) {
        self.pressed = false;
    }

    /// Whether the key is currently latched as held.
    pub fn is_pressed(self) -> bool {
        self.pressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_press_dispatches_and_repeats_do_not() {
        let mut latch = KeyPressLatch::default();

        assert!(latch.press());
        assert!(latch.is_pressed());
        for _ in 0..20 {
            assert!(!latch.press());
        }
    }

    #[test]
    fn release_allows_exactly_one_new_dispatch() {
        let mut latch = KeyPressLatch::default();

        assert!(latch.press());
        assert!(!latch.press());
        latch.release();
        assert!(!latch.is_pressed());
        assert!(latch.press());
        assert!(!latch.press());
    }

    #[test]
    fn release_without_a_press_is_idempotent() {
        let mut latch = KeyPressLatch::default();

        latch.release();
        latch.release();
        assert!(!latch.is_pressed());
        assert!(latch.press());
    }
}
