using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

public class WindowFitTests
{
    [Fact]
    public void ExactAspect_NoCorrection()
    {
        // Client already matches the video aspect (4:3) → no resize.
        Assert.Null(WindowFit.FillClient(640, 480, 640, 480));
    }

    [Fact]
    public void SubPixelMismatch_NoCorrection()
    {
        // A 4K-ish downscale lands ~0.1px off the exact aspect — below the 1px bar threshold, so leave it.
        Assert.Null(WindowFit.FillClient(3840, 1606, 1805, 755));
    }

    [Fact]
    public void WidthClampedUp_GrowsHeightToFill()
    {
        // The #110 case: a 640x480 video on a small display gets its window clamped to the 720px minimum width
        // (≈704px client), so the client (704x480 = 1.467) is wider than the video (1.333) → side bars. The fix
        // keeps the clamped width and grows the height to 704/(4/3) = 528 so the video fills the window.
        var fill = WindowFit.FillClient(640, 480, 704, 480);
        Assert.NotNull(fill);
        Assert.Equal(704, fill!.Value.Width);  // clamped width preserved (can't go below the OS minimum)
        Assert.Equal(528, fill.Value.Height);  // grown to the video aspect
    }

    [Fact]
    public void HeightClampedUp_GrowsWidthToFill()
    {
        // The transpose: a tall video whose window hit the minimum HEIGHT → client taller than the video aspect
        // (top/bottom bars). Keep the clamped height, grow the width.
        var fill = WindowFit.FillClient(480, 640, 480, 704);
        Assert.NotNull(fill);
        Assert.Equal(528, fill!.Value.Width);  // grown to the video aspect
        Assert.Equal(704, fill.Value.Height);  // clamped height preserved
    }

    [Fact]
    public void FilledResult_IsStable_NoSecondCorrection()
    {
        // Feeding a corrected client back in must not trigger another resize (no oscillation).
        var fill = WindowFit.FillClient(640, 480, 704, 480);
        Assert.NotNull(fill);
        Assert.Null(WindowFit.FillClient(640, 480, fill!.Value.Width, fill.Value.Height));
    }

    [Theory]
    [InlineData(0, 480, 704, 480)]
    [InlineData(640, 0, 704, 480)]
    [InlineData(640, 480, 0, 480)]
    [InlineData(640, 480, 704, 0)]
    public void NonPositiveInputs_ReturnNull(int vw, int vh, int cw, int ch)
        => Assert.Null(WindowFit.FillClient(vw, vh, cw, ch));
}
