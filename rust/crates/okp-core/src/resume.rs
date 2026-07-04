//! Resume-point resolution shared by both shells (PRD §12.1 / §13.1).
//!
//! Two kinds of resume target compete when media loads:
//!
//! - an **explicit launch resume** the companion library hands over on the command line
//!   (`--resume <time>`, PRD §13.1) — the library, not the player, decides where to start,
//!   so it is honoured verbatim and **overrides** the remembered position; and
//! - the player's own **remembered position** from watch history (PRD §12.1) — applied only
//!   when it clears the "barely started / near the end" heuristic (skip `< 5%` watched, skip
//!   the final stretch).
//!
//! [`resolve_resume`] folds both into a single [`ResumeDecision`] so the precedence and the
//! thresholds live in one pure, testable place instead of being re-derived per shell. The
//! near-end boundary ([`completion_start`]) is the same signal the report-back seam
//! ([`crate::progress_report`]) uses to raise the "watched" flag, so resume and watched-state
//! always agree.

/// Positions at or below this fraction of the duration count as "barely started" — the
/// remembered position is ignored (PRD §12.1). An explicit launch resume is not subject to it.
pub const RESUME_MIN_FRACTION: f64 = 0.05;

/// An explicit launch resume is clamped to at most `duration - this` so a target at (or past)
/// the very end can never land on EOF and latch the file "finished".
pub const RESUME_END_GUARD_SECONDS: f64 = 0.5;

/// The near-end / "finished" window starts at whichever is earlier: this fraction of the
/// duration, or [`NEAR_END_TAIL_SECONDS`] before the end.
pub const NEAR_END_FRACTION: f64 = 0.95;

/// The near-end window is never longer than this many seconds, so long films still resume
/// close to the credits rather than being treated as finished minutes early.
pub const NEAR_END_TAIL_SECONDS: f64 = 30.0;

/// The position at which a media counts as being in its final stretch: `max(duration * 0.95,
/// duration - 30s)`. A remembered position at or after this is treated as "finished" (no
/// resume); crossing it during playback is what raises the report-back "watched" flag.
#[must_use]
pub fn completion_start(duration: f64) -> f64 {
    (duration * NEAR_END_FRACTION).max(duration - NEAR_END_TAIL_SECONDS)
}

/// What to do with a freshly loaded media given the competing resume targets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResumeDecision {
    /// Seek to this absolute position (seconds), then play.
    Seek(f64),
    /// Start from the beginning; no seek.
    Start,
    /// A target exists but the currently known duration does not cover it yet — keep the
    /// target queued and re-resolve when a larger duration is reported. Engines can report a
    /// small provisional duration before the final one for progressive / network media, so
    /// seeking now would land at the wrong early spot and then skip the real seek.
    Wait,
}

/// Resolve the effective resume target. `explicit` is the launch-provided override (PRD
/// §13.1), `remembered` the position from watch history (PRD §12.1), and `duration` the
/// currently known media length.
///
/// Precedence: an explicit target always wins — it is honoured verbatim (only clamped just
/// shy of the end) and bypasses the barely-started / near-end heuristic, since the companion
/// asked for exactly that position. A resume of `0` is a meaningful explicit target ("start
/// from the beginning"), so it still overrides a remembered position. The remembered position
/// applies only in the absence of an explicit target, and only when it clears the heuristic.
#[must_use]
pub fn resolve_resume(
    explicit: Option<f64>,
    remembered: Option<f64>,
    duration: f64,
) -> ResumeDecision {
    // Without a known, positive duration there is nothing to resolve against yet.
    if !duration.is_finite() || duration <= 0.0 {
        return ResumeDecision::Wait;
    }

    if let Some(target) = explicit {
        if !target.is_finite() {
            return ResumeDecision::Start;
        }
        if target > duration {
            return ResumeDecision::Wait; // provisional duration; the real one may cover it
        }
        let ceiling = (duration - RESUME_END_GUARD_SECONDS).max(0.0);
        return ResumeDecision::Seek(target.clamp(0.0, ceiling));
    }

    if let Some(target) = remembered {
        if !target.is_finite() {
            return ResumeDecision::Start;
        }
        if target > duration {
            return ResumeDecision::Wait;
        }
        // Skip when barely started (< 5%) or already in the final stretch.
        if target > duration * RESUME_MIN_FRACTION && target < completion_start(duration) {
            return ResumeDecision::Seek(target);
        }
    }

    ResumeDecision::Start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_start_takes_the_earlier_of_fraction_and_tail() {
        // Short/medium files: the 5% tail is smaller than 30s, so the fraction wins.
        assert_eq!(completion_start(600.0), 570.0); // max(570, 570)
        assert_eq!(completion_start(100.0), 95.0); // max(95, 70)
        assert_eq!(completion_start(40.0), 38.0); // max(38, 10)
        // Long files cap the window at 30s so resume lands close to the credits.
        assert_eq!(completion_start(3600.0), 3570.0); // max(3420, 3570)
    }

    #[test]
    fn explicit_overrides_a_remembered_position() {
        assert_eq!(
            resolve_resume(Some(45.0), Some(120.0), 600.0),
            ResumeDecision::Seek(45.0)
        );
    }

    #[test]
    fn explicit_zero_still_overrides_remembered_and_starts_from_the_top() {
        // "Resume from 0" is a deliberate instruction — it must beat the remembered 120s.
        assert_eq!(
            resolve_resume(Some(0.0), Some(120.0), 600.0),
            ResumeDecision::Seek(0.0)
        );
    }

    #[test]
    fn explicit_near_the_end_is_honoured_verbatim_not_skipped() {
        // The remembered path would skip a near-end position; an explicit one is honoured,
        // clamped just shy of the end so it cannot latch "finished".
        assert_eq!(
            resolve_resume(Some(599.9), None, 600.0),
            ResumeDecision::Seek(599.5)
        );
    }

    #[test]
    fn explicit_beyond_known_duration_waits_for_a_larger_duration() {
        assert_eq!(
            resolve_resume(Some(700.0), None, 600.0),
            ResumeDecision::Wait
        );
        // Even with a remembered fallback present, the explicit target keeps waiting rather
        // than dropping down to the remembered one.
        assert_eq!(
            resolve_resume(Some(700.0), Some(120.0), 600.0),
            ResumeDecision::Wait
        );
    }

    #[test]
    fn non_finite_explicit_falls_back_to_start() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                resolve_resume(Some(bad), Some(120.0), 600.0),
                ResumeDecision::Start
            );
        }
    }

    #[test]
    fn remembered_applies_only_when_no_explicit_target() {
        assert_eq!(
            resolve_resume(None, Some(120.0), 600.0),
            ResumeDecision::Seek(120.0)
        );
    }

    #[test]
    fn remembered_is_skipped_in_the_first_five_percent() {
        // 30s of 600s is exactly 5% — the boundary is exclusive, so it is skipped.
        assert_eq!(
            resolve_resume(None, Some(30.0), 600.0),
            ResumeDecision::Start
        );
        assert_eq!(
            resolve_resume(None, Some(20.0), 600.0),
            ResumeDecision::Start
        );
    }

    #[test]
    fn remembered_is_skipped_in_the_completion_window() {
        assert_eq!(
            resolve_resume(None, Some(completion_start(600.0)), 600.0),
            ResumeDecision::Start
        );
        assert_eq!(
            resolve_resume(None, Some(590.0), 600.0),
            ResumeDecision::Start
        );
    }

    #[test]
    fn remembered_beyond_provisional_duration_waits() {
        assert_eq!(
            resolve_resume(None, Some(300.0), 100.0),
            ResumeDecision::Wait
        );
    }

    #[test]
    fn unknown_duration_waits_for_both_kinds_of_target() {
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                resolve_resume(Some(50.0), None, duration),
                ResumeDecision::Wait
            );
            assert_eq!(
                resolve_resume(None, Some(50.0), duration),
                ResumeDecision::Wait
            );
        }
    }

    #[test]
    fn no_target_starts_from_the_beginning() {
        assert_eq!(resolve_resume(None, None, 600.0), ResumeDecision::Start);
    }
}
