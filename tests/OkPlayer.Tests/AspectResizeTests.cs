using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

public class AspectResizeTests
{
    private const double Wide = 16.0 / 9.0;
    private const int FrameW = 16, FrameH = 8; // typical resize-border insets

    [Fact]
    public void HorizontalDrag_WidthLeads_DerivesHeight()
    {
        // Drag the right edge to a 1600-wide client at a stale 500 height; height should snap to 900.
        var (l, t, r, b) = AspectResize.Constrain(100, 100, 100 + 1600 + FrameW, 100 + 500 + FrameH,
            AspectResize.Right, Wide, FrameW, FrameH);
        Assert.Equal(100, l);
        Assert.Equal(100, t); // top edge stays
        Assert.Equal(1600, (r - l) - FrameW);
        Assert.Equal(900, (b - t) - FrameH);
    }

    [Fact]
    public void VerticalDrag_HeightLeads_DerivesWidth()
    {
        // Drag the bottom edge to a 900-tall client at a stale 500 width; width should snap to 1600.
        var (l, t, r, b) = AspectResize.Constrain(100, 100, 100 + 500 + FrameW, 100 + 900 + FrameH,
            AspectResize.Bottom, Wide, FrameW, FrameH);
        Assert.Equal(100, l); // left edge stays
        Assert.Equal(1600, (r - l) - FrameW);
        Assert.Equal(900, (b - t) - FrameH);
    }

    [Fact]
    public void BottomRightCorner_MovesBottom_KeepsTopAndLeft()
    {
        var (l, t, r, b) = AspectResize.Constrain(100, 100, 100 + 1600 + FrameW, 100 + 500 + FrameH,
            AspectResize.BottomRight, Wide, FrameW, FrameH);
        Assert.Equal(100, l);
        Assert.Equal(100, t);
        Assert.Equal(1600, (r - l) - FrameW);
        Assert.Equal(900, (b - t) - FrameH);
    }

    [Fact]
    public void TopLeftCorner_MovesTop_KeepsBottomAndRight()
    {
        int bottom = 100 + 500 + FrameH; // this edge must be preserved
        int rightStart = 100 + 1600 + FrameW;
        var (l, t, r, b) = AspectResize.Constrain(100, 100, rightStart, bottom,
            AspectResize.TopLeft, Wide, FrameW, FrameH);
        Assert.Equal(rightStart, r);  // right edge stays
        Assert.Equal(bottom, b);      // bottom edge stays
        Assert.Equal(1600, (r - l) - FrameW);
        Assert.Equal(900, (b - t) - FrameH); // height grew upward to satisfy the aspect
    }

    [Theory]
    [InlineData(0.0)]   // no aspect known
    [InlineData(-2.0)]  // nonsense
    public void NonPositiveAspect_LeavesRectUntouched(double aspect)
    {
        var rect = AspectResize.Constrain(10, 20, 410, 320, AspectResize.Right, aspect, FrameW, FrameH);
        Assert.Equal((10, 20, 410, 320), rect);
    }

    [Fact]
    public void DegenerateClient_LeavesRectUntouched()
    {
        // Proposed box smaller than the frame insets → no valid client; return as-is rather than invert.
        var rect = AspectResize.Constrain(0, 0, FrameW - 2, FrameH - 2, AspectResize.Right, Wide, FrameW, FrameH);
        Assert.Equal((0, 0, FrameW - 2, FrameH - 2), rect);
    }
}
