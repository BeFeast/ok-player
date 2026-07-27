//! Intent-based fullscreen toggling for the double-click contract, plus the
//! acknowledgement boundary the native video plane hangs its geometry on.
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
//!
//! Requests are counted rather than compared. A rapid Enter → Leave reversal
//! leaves the intent back where it started, so an `intended != acknowledged`
//! comparison would report "settled" while two round-trips are still in flight;
//! counting keeps the transition outstanding until every request has been
//! answered. The same counter lets [`FullscreenToggle::observe`] tell an
//! acknowledgement of an older request from the final one, so a late ack can no
//! longer overwrite a newer intent.
//!
//! Geometry the shell would otherwise apply mid-transition is *stashed* rather
//! than dropped ([`FullscreenToggle::offer_geometry`]) and replayed the moment
//! the transition settles, so no resize is silently lost. Because a compositor
//! may reject or ignore a fullscreen request outright — in which case the
//! acknowledgement never arrives — the shell arms a bounded timer per request
//! and force-releases the hold through
//! [`FullscreenToggle::settle_timed_out_request`], which is what keeps the
//! native plane from freezing at a stale size for the lifetime of the window.

/// The window operation a toggle resolves to, derived from the intended state
/// rather than the compositor's lagging report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullscreenAction {
    Enter,
    Leave,
}

/// Size and surface scale of the native video plane, as the shell last measured
/// it from the video widget's allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoPlaneGeometry {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

/// What the shell should do with geometry handed to
/// [`FullscreenToggle::offer_geometry`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeometryDisposition {
    /// No fullscreen transition is outstanding — apply this geometry now.
    Apply(VideoPlaneGeometry),
    /// A transition is outstanding. The geometry has been stashed and will be
    /// handed back by [`FullscreenToggle::release_held_geometry`] or by
    /// [`FullscreenToggle::settle_timed_out_request`], so it is deferred rather
    /// than lost.
    Held,
}

/// Outcome of the bounded acknowledgement timer firing for one request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AckTimeout {
    /// The compositor answered every outstanding request in time.
    AlreadySettled,
    /// A newer request was issued after the one this timer belongs to; that
    /// request's own timer owns the hold from here.
    Superseded,
    /// Nothing acknowledged the request within the deadline, so the hold was
    /// force-released and the intent realigned with the platform's own state.
    /// Replay `geometry` if the shell stashed any while the hold was up.
    ForceReleased {
        geometry: Option<VideoPlaneGeometry>,
    },
}

/// Tracks the fullscreen state the user has asked for.
///
/// The shell owns the actual `GtkWindow`; this only decides what the next
/// toggle should do and stays aligned with reality through [`Self::observe`],
/// which the shell calls from the window's `fullscreened` notify so changes
/// driven by other paths (keyboard shortcut, `Escape`, the window manager) keep
/// the intent honest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FullscreenToggle {
    intended: bool,
    /// Requests issued since construction. The running total doubles as the
    /// generation id of the most recent request.
    issued: u64,
    /// How many of those requests the compositor has answered.
    acknowledged: u64,
    held_geometry: Option<VideoPlaneGeometry>,
}

impl FullscreenToggle {
    /// Seed the policy with the window's current fullscreen state.
    pub fn new(is_fullscreen: bool) -> Self {
        Self {
            intended: is_fullscreen,
            issued: 0,
            acknowledged: 0,
            held_geometry: None,
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

    /// Record an explicit platform request without treating it as settled, and
    /// return its generation id for the shell's acknowledgement timer.
    ///
    /// Shell paths that enter or leave fullscreen without calling [`Self::toggle`]
    /// use this before invoking the platform window operation. Native child
    /// surfaces can then hold transition-time allocations until the compositor
    /// acknowledges the requested state.
    pub fn request(&mut self, is_fullscreen: bool) -> u64 {
        self.intended = is_fullscreen;
        self.issued = self.issued.saturating_add(1);
        self.issued
    }

    /// Generation id of the most recent request. `0` before any request.
    pub fn generation(&self) -> u64 {
        self.issued
    }

    /// Reconcile with the compositor's authoritative fullscreen state. Called
    /// when the window reports a settled change so a fullscreen transition made
    /// outside [`Self::toggle`] (an `Escape` unfullscreen, a window-manager
    /// shortcut) leaves the next toggle pointing the right way.
    ///
    /// Each call answers at most one outstanding request. While a newer request
    /// is still in flight the reported state describes an intermediate step, so
    /// the intent is left alone rather than being rolled back to it.
    pub fn observe(&mut self, is_fullscreen: bool) {
        if self.acknowledged < self.issued {
            self.acknowledged += 1;
        }
        if self.acknowledged == self.issued {
            self.intended = is_fullscreen;
        }
    }

    /// Force-release the acknowledgement hold for `generation` after the
    /// shell's bounded timer expired without a `fullscreened` notify, realigning
    /// the intent with the platform's own `is_fullscreen`.
    ///
    /// A compositor is free to ignore or refuse a fullscreen request; without
    /// this ceiling the hold — and with it the native plane's geometry — would
    /// last for the lifetime of the window.
    pub fn settle_timed_out_request(&mut self, generation: u64, is_fullscreen: bool) -> AckTimeout {
        if self.issued != generation {
            return AckTimeout::Superseded;
        }
        if !self.transition_pending() {
            return AckTimeout::AlreadySettled;
        }
        self.acknowledged = self.issued;
        self.intended = is_fullscreen;
        AckTimeout::ForceReleased {
            geometry: self.held_geometry.take(),
        }
    }

    /// Offer the video plane's latest measured geometry.
    ///
    /// Outside a transition the shell applies it immediately. During one the
    /// geometry describes a transient allocation the compositor has not settled
    /// on yet, so it is stashed instead of applied — and, unlike simply
    /// discarding it, replayed once the transition resolves.
    pub fn offer_geometry(&mut self, geometry: VideoPlaneGeometry) -> GeometryDisposition {
        if self.transition_pending() {
            self.held_geometry = Some(geometry);
            GeometryDisposition::Held
        } else {
            self.held_geometry = None;
            GeometryDisposition::Apply(geometry)
        }
    }

    /// Take the geometry stashed while the hold was up, if any.
    pub fn release_held_geometry(&mut self) -> Option<VideoPlaneGeometry> {
        self.held_geometry.take()
    }

    /// The fullscreen state the user most recently asked for.
    pub fn intended(&self) -> bool {
        self.intended
    }

    /// Whether any issued request is still waiting on the compositor.
    pub fn transition_pending(&self) -> bool {
        self.issued > self.acknowledged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWED: VideoPlaneGeometry = VideoPlaneGeometry {
        width: 1280,
        height: 720,
        scale: 1.0,
    };
    const FULLSCREEN: VideoPlaneGeometry = VideoPlaneGeometry {
        width: 3840,
        height: 2160,
        scale: 2.0,
    };

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
        // The reversal returns the intent to its starting value, but two
        // round-trips are still outstanding: the transition is emphatically not
        // settled, and the native geometry hold must stay up.
        assert!(toggle.transition_pending());
    }

    #[test]
    fn a_rapid_reversal_stays_pending_until_every_request_is_acknowledged() {
        let mut toggle = FullscreenToggle::new(false);
        toggle.request(true);
        toggle.request(false);
        assert!(toggle.transition_pending());

        // The compositor answers the enter first. One round-trip is still owed.
        toggle.observe(true);
        assert!(toggle.transition_pending());

        toggle.observe(false);
        assert!(!toggle.transition_pending());
    }

    #[test]
    fn an_acknowledgement_of_an_older_request_does_not_clobber_a_newer_intent() {
        let mut toggle = FullscreenToggle::new(false);
        toggle.request(true);
        toggle.request(false);
        assert!(!toggle.intended());

        // Ack of the *enter*, arriving while the leave is still in flight. The
        // user asked to be windowed; the intent must not be rolled forward to
        // fullscreen, or the next toggle would leave a window that is already
        // leaving fullscreen.
        toggle.observe(true);
        assert!(!toggle.intended());
        assert_eq!(toggle.toggle(), FullscreenAction::Enter);
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
            assert!(!toggle.transition_pending(), "iteration {iteration}");
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

    #[test]
    fn geometry_offered_outside_a_transition_applies_immediately() {
        let mut toggle = FullscreenToggle::new(false);
        assert_eq!(
            toggle.offer_geometry(WINDOWED),
            GeometryDisposition::Apply(WINDOWED)
        );
        assert_eq!(toggle.release_held_geometry(), None);
    }

    #[test]
    fn geometry_offered_during_a_transition_is_stashed_and_replayed_on_acknowledgement() {
        // The issue-#628 sequence: a screenshot taken fullscreen, then an exit.
        // Every allocation GTK reports mid-transition used to be discarded
        // outright, so the last one before the compositor settled was lost.
        let mut toggle = FullscreenToggle::new(true);
        toggle.request(false);

        assert_eq!(toggle.offer_geometry(FULLSCREEN), GeometryDisposition::Held);
        assert_eq!(toggle.offer_geometry(WINDOWED), GeometryDisposition::Held);

        toggle.observe(false);
        assert!(!toggle.transition_pending());
        assert_eq!(toggle.release_held_geometry(), Some(WINDOWED));
        assert_eq!(toggle.release_held_geometry(), None);
    }

    #[test]
    fn a_stashed_geometry_survives_an_intermediate_acknowledgement() {
        let mut toggle = FullscreenToggle::new(false);
        toggle.request(true);
        toggle.request(false);
        assert_eq!(toggle.offer_geometry(FULLSCREEN), GeometryDisposition::Held);

        // Ack of the enter only. The hold stays up and the stash is untouched.
        toggle.observe(true);
        assert!(toggle.transition_pending());
        assert_eq!(toggle.offer_geometry(WINDOWED), GeometryDisposition::Held);

        toggle.observe(false);
        assert_eq!(toggle.release_held_geometry(), Some(WINDOWED));
    }

    #[test]
    fn an_unacknowledged_request_force_releases_on_timeout_and_returns_the_stash() {
        // A compositor that refuses or ignores the request never emits the
        // `fullscreened` notify. Without the bounded timer the hold — and the
        // native plane's geometry — would never be released.
        let mut toggle = FullscreenToggle::new(true);
        let generation = toggle.request(false);
        assert_eq!(toggle.offer_geometry(WINDOWED), GeometryDisposition::Held);

        assert_eq!(
            toggle.settle_timed_out_request(generation, true),
            AckTimeout::ForceReleased {
                geometry: Some(WINDOWED)
            }
        );
        assert!(!toggle.transition_pending());
        // The intent is realigned with what the platform actually reports, so
        // the next toggle leaves the still-fullscreen window.
        assert!(toggle.intended());
        assert_eq!(toggle.toggle(), FullscreenAction::Leave);
    }

    #[test]
    fn a_timeout_releases_the_hold_even_with_nothing_stashed() {
        let mut toggle = FullscreenToggle::new(false);
        let generation = toggle.request(true);
        assert_eq!(
            toggle.settle_timed_out_request(generation, false),
            AckTimeout::ForceReleased { geometry: None }
        );
        assert!(!toggle.transition_pending());
        assert_eq!(
            toggle.offer_geometry(WINDOWED),
            GeometryDisposition::Apply(WINDOWED)
        );
    }

    #[test]
    fn a_timeout_for_an_acknowledged_request_changes_nothing() {
        let mut toggle = FullscreenToggle::new(false);
        let generation = toggle.request(true);
        toggle.observe(true);
        assert_eq!(
            toggle.settle_timed_out_request(generation, false),
            AckTimeout::AlreadySettled
        );
        // The stale platform read passed to the timer must not rewrite an intent
        // the compositor already confirmed.
        assert!(toggle.intended());
    }

    #[test]
    fn a_timeout_superseded_by_a_newer_request_leaves_the_hold_alone() {
        let mut toggle = FullscreenToggle::new(false);
        let first = toggle.request(true);
        toggle.request(false);
        assert_eq!(toggle.offer_geometry(FULLSCREEN), GeometryDisposition::Held);

        assert_eq!(
            toggle.settle_timed_out_request(first, true),
            AckTimeout::Superseded
        );
        // The newer request's own timer owns the hold, so neither the stash nor
        // the pending state is disturbed.
        assert!(toggle.transition_pending());
        assert!(!toggle.intended());
        assert_eq!(
            toggle.settle_timed_out_request(toggle.generation(), false),
            AckTimeout::ForceReleased {
                geometry: Some(FULLSCREEN)
            }
        );
    }
}
