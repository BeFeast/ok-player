using OkPlayer.Core;

namespace OkPlayer.Tests;

public class LaunchArgsTests
{
    private const string Movie = @"C:\media\movie.mkv";

    [Fact]
    public void Parse_Null_ReturnsEmpty()
    {
        var (files, resume) = LaunchArgs.Parse(null);
        Assert.Empty(files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_Empty_ReturnsEmpty()
    {
        var (files, resume) = LaunchArgs.Parse(new string[0]);
        Assert.Empty(files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_FileOnly_NoResume()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_ResumeBeforeFile()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { "--resume", "90", Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(90, resume);
    }

    [Fact]
    public void Parse_ResumeAfterFile()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { Movie, "--resume", "90" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(90, resume);
    }

    [Theory]
    [InlineData("--resume=90", 90)]
    [InlineData("--resume:90", 90)]
    [InlineData("-resume=90", 90)]
    [InlineData("/resume=90", 90)]
    [InlineData("--resume=1:23:45", 5025)]
    [InlineData("--resume=83.5", 83.5)]
    public void Parse_InlineResumeValue(string token, double expected)
    {
        var (files, resume) = LaunchArgs.Parse(new[] { Movie, token });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(expected, resume);
    }

    [Fact]
    public void Parse_TimecodeAsSeparateValue()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { Movie, "--resume", "1:23:45" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(5025, resume);
    }

    [Fact]
    public void Parse_ResumeZero_IsKept_NotTreatedAsAbsent()
    {
        var (_, resume) = LaunchArgs.Parse(new[] { Movie, "--resume", "0" });
        Assert.Equal(0, resume);
    }

    [Fact]
    public void Parse_MalformedResumeValue_IsIgnored_AndNextTokenStaysPositional()
    {
        // "--resume" with a non-timecode following it: the value parses to null and must NOT swallow the path.
        var (files, resume) = LaunchArgs.Parse(new[] { "--resume", Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_InlineMalformedResume_IsNull()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { Movie, "--resume=abc" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_BareResumeAtEnd_IsNull()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { Movie, "--resume" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_UnknownSwitches_AreIgnored()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { "--fullscreen", Movie, "/foo", "-x" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_MultiplePositionals_KeptInOrder()
    {
        var (files, _) = LaunchArgs.Parse(new[] { "garbage", Movie });
        Assert.Equal(new[] { "garbage", Movie }, files); // caller picks the first that is a URL / existing file
    }

    [Fact]
    public void Parse_Url_IsPositional()
    {
        var (files, resume) = LaunchArgs.Parse(new[] { "https://example.com/a.mp4", "--resume", "12" });
        Assert.Equal(new[] { "https://example.com/a.mp4" }, files);
        Assert.Equal(12, resume);
    }
}
