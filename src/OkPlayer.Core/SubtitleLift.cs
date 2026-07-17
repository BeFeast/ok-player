namespace OkPlayer.Core;

/// <summary>
/// Pure math for the OSC subtitle lift (PRD P1-D9: the on-screen controls must never overlap captions). The
/// lift is applied through mpv's <c>sub-pos</c>, a <em>percentage</em> of the rendered video height. The OSC
/// pill, however, is a fixed device-independent height anchored to the bottom of the surface — so a constant
/// percentage clears it on a large window but shrinks to too few pixels on a small one (a 240p mini-player,
/// a tiny resized window). This converts the OSC's fixed DIP clearance into the percentage needed for the
/// current surface height, never dropping below a floor so large windows keep their tuned, tested lift.
///
/// The ratio is DPI-independent: clearance and surface height are both in DIPs, so it equals the pixel ratio
/// mpv applies. It assumes the worst case of video filling the surface height; letterboxing only lifts the
/// caption further off the bottom, so the result stays conservative. Engine- and UI-free for headless tests.
/// </summary>
public static class SubtitleLift
{
    /// <summary>Lift (in <c>sub-pos</c> percentage points) to raise subtitles by while the OSC chrome is up.
    /// <paramref name="surfaceHeightDip"/> is the player surface height; <paramref name="oscClearanceDip"/>
    /// is how far above the bottom the controls reach (their top + a gap); <paramref name="floorPercent"/> is
    /// the minimum lift for large surfaces. Falls back to the floor when the surface height isn't known yet
    /// (≤ 0, e.g. before first layout). Clamped to a sane ≤ 100 so it can never invert sub-pos.</summary>
    public static double ForSurface(double surfaceHeightDip, double oscClearanceDip, double floorPercent)
    {
        if (surfaceHeightDip <= 0 || oscClearanceDip <= 0)
            return floorPercent;
        double needed = oscClearanceDip / surfaceHeightDip * 100.0;
        double lift = needed > floorPercent ? needed : floorPercent;
        return lift < 100.0 ? lift : 100.0;
    }

    /// <summary>Combine the user's configured <c>sub-pos</c> baseline with the transient OSC lift. Both are
    /// percentage points; the result is clamped to mpv's valid range.</summary>
    public static double ApplyToPosition(double basePosition, double lift)
        => System.Math.Clamp(basePosition - lift, 0, 100);
}
