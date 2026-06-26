using OkPlayer.Core;

namespace OkPlayer.Tests;

public class TimeCodeTests
{
    [Theory]
    [InlineData("90", 90)]
    [InlineData("0", 0)]
    [InlineData("1:30", 90)]
    [InlineData("0:05", 5)]
    [InlineData("1:23:45", 5025)]
    [InlineData("2:00:00", 7200)]
    [InlineData("83.5", 83.5)]
    [InlineData("2:05.5", 125.5)]
    [InlineData("  1:30  ", 90)] // trimmed
    public void Parse_ValidTimecodes(string text, double expected)
        => Assert.Equal(expected, TimeCode.Parse(text));

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData(null)]
    [InlineData("abc")]
    [InlineData("1:2:3:4")]   // too many fields
    [InlineData("1::3")]      // empty field
    [InlineData("-5")]        // negative
    [InlineData("1:-3")]      // negative field
    [InlineData("1.5:30")]    // fractional minutes not allowed
    public void Parse_InvalidTimecodes_ReturnNull(string? text)
        => Assert.Null(TimeCode.Parse(text));

    [Theory]
    [InlineData(90, "1:30")]
    [InlineData(5, "0:05")]
    [InlineData(5025, "1:23:45")]
    [InlineData(0, "0:00")]
    [InlineData(-3, "0:00")]   // clamps negatives
    [InlineData(83.7, "1:23")] // truncates, not rounds — matches the on-screen clock
    [InlineData(59.9, "0:59")]
    public void Format_RendersTimecode(double seconds, string expected)
        => Assert.Equal(expected, TimeCode.Format(seconds));

    [Fact]
    public void Parse_Then_Format_RoundTripsWholeSeconds()
    {
        double? secs = TimeCode.Parse("1:23:45");
        Assert.NotNull(secs);
        Assert.Equal("1:23:45", TimeCode.Format(secs!.Value));
    }
}
