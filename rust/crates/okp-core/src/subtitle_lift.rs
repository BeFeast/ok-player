//! Pure math for the OSC subtitle lift — port of `src/OkPlayer.Core/SubtitleLift.cs`; the C#
//! suite in `tests/OkPlayer.Tests/SubtitleLiftTests.cs` is the executable spec (PRD P1-D9: the
//! on-screen controls must never overlap captions). The lift is applied through mpv's `sub-pos`,
//! a *percentage* of the rendered video height. The OSC pill, however, is a fixed
//! device-independent height anchored to the bottom of the surface — so a constant percentage
//! clears it on a large window but shrinks to too few pixels on a small one (a 240p mini-player,
//! a tiny resized window). This converts the OSC's fixed DIP clearance into the percentage
//! needed for the current surface height, never dropping below a floor so large windows keep
//! their tuned, tested lift.
//!
//! The ratio is DPI-independent: clearance and surface height are both in DIPs, so it equals the
//! pixel ratio mpv applies. It assumes the worst case of video filling the surface height;
//! letterboxing only lifts the caption further off the bottom, so the result stays conservative.
//! Engine- and UI-free for headless tests.

/// Lift (in `sub-pos` percentage points) to raise subtitles by while the OSC chrome is up.
/// `surface_height_dip` is the player surface height; `osc_clearance_dip` is how far above the
/// bottom the controls reach (their top + a gap); `floor_percent` is the minimum lift for large
/// surfaces. Falls back to the floor when the surface height isn't known yet (≤ 0, e.g. before
/// first layout). Clamped to a sane ≤ 100 so it can never invert sub-pos.
pub fn for_surface(surface_height_dip: f64, osc_clearance_dip: f64, floor_percent: f64) -> f64 {
    if surface_height_dip <= 0.0 || osc_clearance_dip <= 0.0 {
        return floor_percent;
    }
    let needed = osc_clearance_dip / surface_height_dip * 100.0;
    let lift = if needed > floor_percent {
        needed
    } else {
        floor_percent
    };
    if lift < 100.0 { lift } else { 100.0 }
}

/// Combine the user's configured `sub-pos` baseline with the transient OSC lift. Both inputs are
/// percentage points; the result is clamped to mpv's valid range so an extreme small-window lift
/// can never move captions past the top edge.
pub fn apply_to_position(base_position: f64, lift: f64) -> f64 {
    (base_position - lift).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PlayerViewModel.OscSubtitleLift` on the Windows side.
    const FLOOR: f64 = 16.0;
    /// `PlayerView.OscClearanceDip` on the Windows side.
    const OSC: f64 = 88.0;

    #[test]
    fn large_surfaces_use_the_tuned_floor() {
        // 88/1080 ≈ 8% and 88/720 ≈ 12% are below the floor; 88/550 = 16% sits exactly on it.
        for height in [1080.0, 720.0, 550.0] {
            assert_eq!(FLOOR, for_surface(height, OSC, FLOOR), "height {height}");
        }
    }

    #[test]
    fn small_surfaces_lift_more_than_the_floor() {
        // Mini-player-ish sizes: 88/360 ≈ 24.4%, 88/240 ≈ 36.7% — must exceed the floor.
        for height in [360.0, 240.0] {
            let lift = for_surface(height, OSC, FLOOR);
            assert!(
                lift > FLOOR,
                "expected a small surface to lift more than the {FLOOR}% floor, got {lift}%"
            );
            // It must equal exactly the percentage that maps the fixed OSC clearance onto this
            // surface height.
            let expected = OSC / height * 100.0;
            okp_test_fixtures::assert_close(lift, expected, 1e-6);
        }
    }

    #[test]
    fn lift_keeps_the_caption_clear_of_the_osc_in_pixels() {
        // The whole point: lift% of the surface height must cover the OSC's fixed pixel clearance
        // on a small surface — which a flat 16% would not (16% of 240 = 38px < 88px).
        let height = 240.0;
        let lift = for_surface(height, OSC, FLOOR);
        let lifted_px = lift / 100.0 * height;
        assert!(
            lifted_px >= OSC,
            "lifted {lifted_px}px must clear the {OSC}px OSC band"
        );
        assert!(
            FLOOR / 100.0 * height < OSC,
            "sanity: the flat floor would NOT have cleared it"
        );
    }

    #[test]
    fn unknown_surface_height_falls_back_to_floor() {
        // Before first layout the surface height can be 0; never produce a NaN/divide-by-zero
        // lift.
        assert_eq!(FLOOR, for_surface(0.0, OSC, FLOOR));
        assert_eq!(FLOOR, for_surface(-1.0, OSC, FLOOR));
    }

    #[test]
    fn unknown_clearance_falls_back_to_floor() {
        // Same guard on the other operand (spec'd by the C# implementation, not its suite).
        assert_eq!(FLOOR, for_surface(720.0, 0.0, FLOOR));
        assert_eq!(FLOOR, for_surface(720.0, -1.0, FLOOR));
    }

    #[test]
    fn lift_is_clamped_below_100_so_it_never_inverts_sub_pos() {
        // A pathological surface shorter than the OSC clearance must not push sub-pos negative.
        assert_eq!(100.0, for_surface(40.0, OSC, FLOOR)); // 88/40 = 220%
    }

    #[test]
    fn configured_position_and_osc_lift_compose_without_leaving_mpv_range() {
        assert_eq!(100.0, apply_to_position(100.0, 0.0));
        assert_eq!(84.0, apply_to_position(100.0, 16.0));
        assert_eq!(74.0, apply_to_position(90.0, 16.0));
        assert_eq!(0.0, apply_to_position(20.0, 36.7));
        assert_eq!(100.0, apply_to_position(120.0, 0.0));
    }
}
