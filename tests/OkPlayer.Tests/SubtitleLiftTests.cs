using OkPlayer.Core;

namespace OkPlayer.Tests;

/// <summary>Unit tests for the OSC subtitle-lift math (PRD P1-D9). The lift is a sub-pos percentage but the
/// OSC pill is a fixed device-independent height, so the lift must grow on small surfaces to keep clearing
/// the controls in pixels — the gap a fixed 16% would leave on a mini-player. These pin that behaviour.</summary>
public class SubtitleLiftTests
{
    const double Floor = 16;   // PlayerViewModel.OscSubtitleLift
    const double Osc = 88;     // PlayerView.OscClearanceDip

    [Theory]
    [InlineData(1080)] // a large window: 88/1080 ≈ 8% < floor
    [InlineData(720)]  // the size the render regression test renders at: 88/720 ≈ 12% < floor
    [InlineData(550)]  // 88/550 = 16% == floor (boundary)
    public void LargeSurfaces_UseTheTunedFloor(double height)
    {
        Assert.Equal(Floor, SubtitleLift.ForSurface(height, Osc, Floor));
    }

    [Theory]
    [InlineData(360)] // mini-player-ish: 88/360 ≈ 24.4% — must exceed the floor
    [InlineData(240)] // 88/240 ≈ 36.7%
    public void SmallSurfaces_LiftMoreThanTheFloor(double height)
    {
        double lift = SubtitleLift.ForSurface(height, Osc, Floor);
        Assert.True(lift > Floor, $"expected a small surface to lift more than the {Floor}% floor, got {lift}%");
        // It must equal exactly the percentage that maps the fixed OSC clearance onto this surface height.
        Assert.Equal(Osc / height * 100.0, lift, precision: 6);
    }

    [Fact]
    public void Lift_KeepsTheCaptionClearOfTheOsc_InPixels()
    {
        // The whole point: lift% of the surface height must cover the OSC's fixed pixel clearance on a small
        // surface — which a flat 16% would not (16% of 240 = 38px < 88px).
        const double height = 240;
        double lift = SubtitleLift.ForSurface(height, Osc, Floor);
        double liftedPx = lift / 100.0 * height;
        Assert.True(liftedPx >= Osc, $"lifted {liftedPx}px must clear the {Osc}px OSC band");
        Assert.True(Floor / 100.0 * height < Osc, "sanity: the flat floor would NOT have cleared it");
    }

    [Fact]
    public void UnknownSurfaceHeight_FallsBackToFloor()
    {
        // Before first layout ActualHeight can be 0; never produce a NaN/divide-by-zero lift.
        Assert.Equal(Floor, SubtitleLift.ForSurface(0, Osc, Floor));
        Assert.Equal(Floor, SubtitleLift.ForSurface(-1, Osc, Floor));
    }

    [Fact]
    public void Lift_IsClampedBelow100_SoItNeverInvertsSubPos()
    {
        // A pathological surface shorter than the OSC clearance must not push sub-pos negative.
        double lift = SubtitleLift.ForSurface(40, Osc, Floor); // 88/40 = 220%
        Assert.Equal(100.0, lift);
    }
}
