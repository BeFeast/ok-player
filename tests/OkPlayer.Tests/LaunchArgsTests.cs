using OkPlayer.Core;

namespace OkPlayer.Tests;

public class LaunchArgsTests
{
    private const string Movie = @"C:\media\movie.mkv";

    [Fact]
    public void Parse_Null_ReturnsEmpty()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(null);
        Assert.Empty(files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_Empty_ReturnsEmpty()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new string[0]);
        Assert.Empty(files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_FileOnly_NoResume()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_ResumeBeforeFile()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { "--resume", "90", Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(90, resume);
    }

    [Fact]
    public void Parse_ResumeAfterFile()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { Movie, "--resume", "90" });
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
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { Movie, token });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(expected, resume);
    }

    [Fact]
    public void Parse_TimecodeAsSeparateValue()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { Movie, "--resume", "1:23:45" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(5025, resume);
    }

    [Fact]
    public void Parse_ResumeZero_IsKept_NotTreatedAsAbsent()
    {
        var (_, resume, _, _) = LaunchArgs.Parse(new[] { Movie, "--resume", "0" });
        Assert.Equal(0, resume);
    }

    [Fact]
    public void Parse_MalformedResumeValue_IsIgnored_AndNextTokenStaysPositional()
    {
        // "--resume" with a non-timecode following it: the value parses to null and must NOT swallow the path.
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { "--resume", Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_InlineMalformedResume_IsNull()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { Movie, "--resume=abc" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_BareResumeAtEnd_IsNull()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { Movie, "--resume" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_UnknownSwitches_AreIgnored()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { "--fullscreen", Movie, "/foo", "-x" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(resume);
    }

    [Fact]
    public void Parse_MultiplePositionals_KeptInOrder()
    {
        var (files, _, _, _) = LaunchArgs.Parse(new[] { "garbage", Movie });
        Assert.Equal(new[] { "garbage", Movie }, files); // caller picks the first that is a URL / existing file
    }

    [Fact]
    public void Parse_Url_IsPositional()
    {
        var (files, resume, _, _) = LaunchArgs.Parse(new[] { "https://example.com/a.mp4", "--resume", "12" });
        Assert.Equal(new[] { "https://example.com/a.mp4" }, files);
        Assert.Equal(12, resume);
    }

    [Fact]
    public void Parse_SubAndAudioTrackIds()
    {
        var (files, _, sub, audio) = LaunchArgs.Parse(new[] { Movie, "--sub", "2", "--audio=1" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(2, sub);
        Assert.Equal(1, audio);
    }

    [Theory]
    [InlineData("no")]
    [InlineData("off")]
    [InlineData("OFF")]
    public void Parse_SubOff_IsMinusOne(string token)
    {
        var (_, _, sub, _) = LaunchArgs.Parse(new[] { Movie, "--sub", token });
        Assert.Equal(-1, sub);
    }

    [Fact]
    public void Parse_AudioOff_IsMinusOne()
    {
        var (_, _, _, audio) = LaunchArgs.Parse(new[] { Movie, "--audio=no" });
        Assert.Equal(-1, audio);
    }

    [Fact]
    public void Parse_NoTrackFlags_AreNull()
    {
        var (_, _, sub, audio) = LaunchArgs.Parse(new[] { Movie });
        Assert.Null(sub);
        Assert.Null(audio);
    }

    [Theory]
    [InlineData("abc")]   // not a number
    [InlineData("2.5")]   // not an integer
    public void Parse_MalformedTrackId_IsNull_AndDoesNotSwallowNextToken(string bad)
    {
        var (files, _, sub, _) = LaunchArgs.Parse(new[] { "--sub", bad, Movie });
        Assert.Equal(new[] { bad, Movie }, files); // bad value isn't a track id -> stays positional, path preserved
        Assert.Null(sub);
    }

    [Fact]
    public void Parse_NegativeLiteralTrackId_IsRejected()
    {
        // "-1" as a literal is rejected (only no/off yield -1) and, leading-dash, is treated as a switch.
        var (files, _, sub, _) = LaunchArgs.Parse(new[] { "--sub", "-1", Movie });
        Assert.Equal(new[] { Movie }, files);
        Assert.Null(sub);
    }

    [Fact]
    public void Parse_AllFlagsTogether()
    {
        var (files, resume, sub, audio) = LaunchArgs.Parse(
            new[] { "--resume", "1:30", Movie, "--sub", "3", "--audio", "2" });
        Assert.Equal(new[] { Movie }, files);
        Assert.Equal(90, resume);
        Assert.Equal(3, sub);
        Assert.Equal(2, audio);
    }

    [Fact]
    public void Parse_TrackIdZero_IsRejected_BecauseMpvIdsAre1Based()
    {
        // mpv reads aid/sid 0 as "auto", not track 0 — so 0 is ignored rather than silently selecting auto.
        var (_, _, sub, audio) = LaunchArgs.Parse(new[] { Movie, "--sub=0", "--audio=0" });
        Assert.Null(sub);
        Assert.Null(audio);
    }

    [Fact]
    public void Parse_LaterMalformedRepeat_KeepsEarlierValidValue()
    {
        var (_, resume, sub, _) = LaunchArgs.Parse(
            new[] { Movie, "--resume=90", "--resume=bad", "--sub=2", "--sub=bad" });
        Assert.Equal(90, resume); // the malformed repeat must not wipe the valid 90
        Assert.Equal(2, sub);     // nor the valid 2
    }

    [Fact]
    public void Parse_LaterValidRepeat_Wins()
    {
        var (_, resume, _, _) = LaunchArgs.Parse(new[] { Movie, "--resume=90", "--resume=120" });
        Assert.Equal(120, resume);
    }
}
