using OkPlayer.Core;

namespace OkPlayer.Tests;

public class VideoAdjustmentsTests
{
    [Theory]
    [InlineData(VideoAdjustmentKind.Brightness, "brightness")]
    [InlineData(VideoAdjustmentKind.Contrast, "contrast")]
    [InlineData(VideoAdjustmentKind.Saturation, "saturation")]
    [InlineData(VideoAdjustmentKind.Gamma, "gamma")]
    public void MpvProperty_MapsEverySharedAdjustment(VideoAdjustmentKind kind, string property)
        => Assert.Equal(property, VideoAdjustments.MpvProperty(kind));

    [Theory]
    [InlineData(-125.0, -100.0)]
    [InlineData(-25.0, -25.0)]
    [InlineData(0.0, 0.0)]
    [InlineData(25.0, 25.0)]
    [InlineData(125.0, 100.0)]
    public void Normalize_ClampsToTheLibmpvRange(double input, double expected)
        => Assert.Equal(expected, VideoAdjustments.Normalize(input));

    [Fact]
    public void Normalize_NonFiniteValuesReturnNeutral()
    {
        Assert.Equal(0.0, VideoAdjustments.Normalize(double.NaN));
        Assert.Equal(0.0, VideoAdjustments.Normalize(double.PositiveInfinity));
        Assert.Equal(0.0, VideoAdjustments.Normalize(double.NegativeInfinity));
    }
}
