using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

/// <summary>Poster-frame brightness scoring: a black/fade frame must score near 0 and a lit frame high, so the
/// poster picker rejects the dark grabs that used to ship as black "Continue watching" thumbnails.</summary>
public class ImageLumaTests
{
    static byte[] Fill(int pixels, byte b, byte g, byte r, byte a = 255)
    {
        var buf = new byte[pixels * 4];
        for (int i = 0; i < buf.Length; i += 4)
        {
            buf[i] = b; buf[i + 1] = g; buf[i + 2] = r; buf[i + 3] = a;
        }
        return buf;
    }

    [Fact]
    public void AllBlack_ScoresZero()
        => Assert.Equal(0, ImageLuma.MeanBgra(Fill(64, 0, 0, 0)), 3);

    [Fact]
    public void AllWhite_ScoresFull()
        => Assert.Equal(255, ImageLuma.MeanBgra(Fill(64, 255, 255, 255)), 3); // 0.114+0.587+0.299 = 1.0

    [Fact]
    public void Green_DominatesLuma()
        => Assert.Equal(0.587 * 255, ImageLuma.MeanBgra(Fill(64, 0, 255, 0)), 1); // ≈ 149.7

    [Fact]
    public void LitFrame_BeatsBlackFrame()
    {
        double black = ImageLuma.MeanBgra(Fill(64, 8, 8, 8));      // a near-black fade
        double lit = ImageLuma.MeanBgra(Fill(64, 120, 140, 130)); // an ordinary lit scene
        Assert.True(lit > black + 60, $"lit {lit:0.0} should clearly beat black {black:0.0}");
    }

    [Fact]
    public void Empty_ScoresZero()
        => Assert.Equal(0d, ImageLuma.MeanBgra(System.ReadOnlySpan<byte>.Empty), 3);

    [Fact]
    public void Stride_IsFlooredToWholePixels_AndStillSamples()
    {
        // An odd stride must not throw or drift off pixel boundaries; a uniform buffer still scores its color.
        double mid = ImageLuma.MeanBgra(Fill(64, 100, 100, 100), stride: 7);
        Assert.Equal(100, mid, 3);
    }
}
