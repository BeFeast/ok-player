//! Intent-based fullscreen toggling for the double-click contract.
//!
//! On Wayland `GtkWindow::is_fullscreen` only flips once the compositor
//! acknowledges the `xdg_toplevel` state change, several milliseconds after the
//! request is sent. A toggle that reads that lagging value to decide between
//! entering and leaving fullscreen can misfire when a second toggle arrives
//! before the round-trip completes: both reads observe the same stale state and
//! repeat the same request, so the toggle is "missed". This pure policy owns the
//! *intended* state instead — it is flipped eagerly on every toggle and
//! reconciled with the compositor's authoritative notify — so repeated
//! double-clicks alternate deterministically regardless of acknowledgement lag.

/// The window operation a toggle resolves to, derived from the intended state
/// rather than the compositor's lagging report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullscreenAction {
    Enter,
    Leave,
}

/// Tracks the fullscreen state the user has asked for.
///
/// The shell owns the actual `GtkWindow`; this only decides what the next
/// toggle should do and stays aligned with reality through [`Self::observe`],
/// which the shell calls from the window's `fullscreened` notify so changes
/// driven by other paths (keyboard shortcut, `Escape`, the window manager) keep
/// the intent honest.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FullscreenToggle {
    intended: bool,
    acknowledged: bool,
}

impl FullscreenToggle {
    /// Seed the policy with the window's current fullscreen state.
    pub fn new(is_fullscreen: bool) -> Self {
        Self {
            intended: is_fullscreen,
            acknowledged: is_fullscreen,
        }
    }

    /// Flip the intended state and report the operation to perform. The decision
    /// never consults a possibly-stale platform read, so two toggles issued
    /// faster than a compositor round-trip still alternate Enter/Leave.
    pub fn toggle(&mut self) -> FullscreenAction {
        self.request(!self.intended);
        if self.intended {
            FullscreenAction::Enter
        } else {
            FullscreenAction::Leave
        }
    }

    /// Record an explicit platform request without treating it as settled.
    ///
    /// Shell paths that enter or leave fullscreen without calling [`Self::toggle`]
    /// use this before invoking the platform window operation. Native child
    /// surfaces can then hold transition-time allocations until the compositor
    /// acknowledges the requested state.
    pub fn request(&mut self, is_fullscreen: bool) {
        self.intended = is_fullscreen;
    }

    /// Reconcile with the compositor's authoritative fullscreen state. Called
    /// when the window reports a settled change so a fullscreen transition made
    /// outside [`Self::toggle`] (an `Escape` unfullscreen, a window-manager
    /// shortcut) leaves the next toggle pointing the right way.
    pub fn observe(&mut self, is_fullscreen: bool) {
        self.intended = is_fullscreen;
        self.acknowledged = is_fullscreen;
    }

    /// The fullscreen state the user most recently asked for.
    pub fn intended(&self) -> bool {
        self.intended
    }

    /// Whether the compositor has acknowledged the latest requested state.
    pub fn transition_pending(&self) -> bool {
        self.intended != self.acknowledged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowed_toggle_enters_and_fullscreen_toggle_leaves() {
        let mut windowed = FullscreenToggle::new(false);
        assert_eq!(windowed.toggle(), FullscreenAction::Enter);
        assert!(windowed.intended());

        let mut fullscreen = FullscreenToggle::new(true);
        assert_eq!(fullscreen.toggle(), FullscreenAction::Leave);
        assert!(!fullscreen.intended());
    }

    #[test]
    fn default_starts_windowed() {
        assert_eq!(FullscreenToggle::default(), FullscreenToggle::new(false));
        assert!(!FullscreenToggle::default().intended());
    }

    #[test]
    fn back_to_back_toggles_alternate_without_a_settled_observe() {
        // Reproduces the regression: on Wayland the platform fullscreen flag has
        // not updated yet when the second toggle fires. Deciding from intent
        // keeps the two toggles a distinct Enter then Leave instead of two
        // identical Enter requests that would drop the second toggle.
        let mut toggle = FullscreenToggle::new(false);
        assert_eq!(toggle.toggle(), FullscreenAction::Enter);
        assert!(toggle.transition_pending());
        assert_eq!(toggle.toggle(), FullscreenAction::Leave);
        assert!(!toggle.intended());
        assert!(!toggle.transition_pending());
    }

    #[test]
    fn twenty_double_click_toggles_never_miss() {
        // Mirrors the installed GNOME/Wayland QA: 20 double-clicks alternate the
        // window in and out of fullscreen with no repeated or dropped request,
        // even though `observe` (the compositor ack) only lands between clicks.
        let mut toggle = FullscreenToggle::new(false);
        for iteration in 0..20 {
            let entering = iteration % 2 == 0;
            let expected = if entering {
                FullscreenAction::Enter
            } else {
                FullscreenAction::Leave
            };
            assert_eq!(toggle.toggle(), expected, "iteration {iteration}");
            // The compositor acknowledges the settled state before the next
            // double-click; the reconciliation must not perturb the intent.
            toggle.observe(entering);
            assert_eq!(toggle.intended(), entering);
        }
    }

    #[test]
    fn observe_realigns_intent_after_an_external_change() {
        // Entered via double-click, then left with the Escape key (a direct
        // unfullscreen the shell reports through `observe`). The next toggle must
        // re-enter rather than issue a redundant leave.
        let mut toggle = FullscreenToggle::new(false);
        assert_eq!(toggle.toggle(), FullscreenAction::Enter);
        assert!(toggle.transition_pending());
        toggle.observe(false);
        assert!(!toggle.intended());
        assert!(!toggle.transition_pending());
        assert_eq!(toggle.toggle(), FullscreenAction::Enter);
    }

    #[test]
    fn explicit_leave_stays_pending_until_the_compositor_acknowledges_it() {
        let mut toggle = FullscreenToggle::new(true);

        // Screenshot completion and its transient toast do not touch this
        // policy. The only geometry boundary is the explicit fullscreen exit
        // followed by the compositor acknowledgement.
        toggle.request(false);
        assert!(!toggle.intended());
        assert!(toggle.transition_pending());

        toggle.observe(false);
        assert!(!toggle.transition_pending());
    }

    #[test]
    fn observe_is_idempotent_when_already_aligned() {
        let mut toggle = FullscreenToggle::new(true);
        toggle.observe(true);
        assert!(toggle.intended());
        assert_eq!(toggle.toggle(), FullscreenAction::Leave);
    }
}
