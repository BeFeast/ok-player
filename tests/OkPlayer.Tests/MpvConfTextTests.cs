using System.Linq;
using OkPlayer.App.Services;

namespace OkPlayer.Tests;

public class MpvConfTextTests
{
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    public void Parse_EmptyOrNull_ReturnsEmpty(string? text)
        => Assert.Empty(MpvConfText.Parse(text));

    [Fact]
    public void Parse_BasicKeyValue()
    {
        var options = MpvConfText.Parse("hwdec=no");
        Assert.Equal(new MpvOption("hwdec", "no"), Assert.Single(options));
    }

    [Fact]
    public void Parse_TrimsKeyAndValue()
    {
        var option = Assert.Single(MpvConfText.Parse("  sub-font  =  Arial Bold  "));
        Assert.Equal(new MpvOption("sub-font", "Arial Bold"), option);
    }

    [Fact]
    public void Parse_SkipsCommentsAndBlankLines()
    {
        var options = MpvConfText.Parse("# a comment\n\n   \n  # indented comment\ncache=yes");
        Assert.Equal(new MpvOption("cache", "yes"), Assert.Single(options));
    }

    [Fact]
    public void Parse_BareKey_BecomesYes()
    {
        var option = Assert.Single(MpvConfText.Parse("fs"));
        Assert.Equal(new MpvOption("fs", "yes"), option);
    }

    [Fact]
    public void Parse_SkipsProfileSectionHeaders()
    {
        // A "[fast]" header must not become a bare option (which would serialize to the bogus "[fast]=yes");
        // it's dropped like a comment, and the options keep parsing.
        var options = MpvConfText.Parse("[fast]\nhwdec=auto\n[slow]\ncache=yes");
        Assert.Equal(new[] { "hwdec", "cache" }, options.Select(o => o.Key));
        Assert.DoesNotContain(options, o => o.Key.StartsWith('['));
    }

    [Fact]
    public void RoundTrip_DoesNotCorruptProfileHeaderIntoOption()
    {
        // Regression: the round-trip used to rewrite "[fast]" as "[fast]=yes", corrupting the profile boundary.
        string serialized = MpvConfText.Serialize(MpvConfText.Parse("[fast]\nhwdec=auto"));
        Assert.DoesNotContain("[fast]", serialized);
        Assert.Equal("hwdec=auto\n", serialized);
    }

    [Fact]
    public void Parse_ValueWithEquals_SplitsOnFirstOnly()
    {
        // glsl-shaders paths and option=sub-option=value forms keep everything after the first '='.
        var option = Assert.Single(MpvConfText.Parse("glsl-shaders=~~/a.glsl=b"));
        Assert.Equal(new MpvOption("glsl-shaders", "~~/a.glsl=b"), option);
    }

    [Fact]
    public void Parse_HashInsideValue_IsNotAComment()
    {
        var option = Assert.Single(MpvConfText.Parse("sub-color=#FFFFFF"));
        Assert.Equal(new MpvOption("sub-color", "#FFFFFF"), option);
    }

    [Fact]
    public void Parse_HandlesCrlfAndPreservesOrder()
    {
        var options = MpvConfText.Parse("a=1\r\nb=2\r\nc=3");
        Assert.Equal(new[] { "a", "b", "c" }, options.Select(o => o.Key));
        Assert.Equal(new[] { "1", "2", "3" }, options.Select(o => o.Value));
    }

    [Fact]
    public void Serialize_WritesKeyValueLinesWithTrailingNewline()
        => Assert.Equal("hwdec=no\ncache=yes\n",
            MpvConfText.Serialize(new[] { new MpvOption("hwdec", "no"), new MpvOption("cache", "yes") }));

    [Fact]
    public void Serialize_SkipsBlankKeysAndTrims()
        => Assert.Equal("hwdec=no\n",
            MpvConfText.Serialize(new[]
            {
                new MpvOption("  hwdec ", "  no "),
                new MpvOption("   ", "orphan"),
                new MpvOption("", "alsoDropped"),
            }));

    [Fact]
    public void Serialize_Empty_ReturnsEmptyString()
        => Assert.Equal(string.Empty, MpvConfText.Serialize(System.Array.Empty<MpvOption>()));

    [Fact]
    public void RoundTrip_ParseSerializeParse_IsStable()
    {
        const string canonical = "hwdec=no\nsub-font=Arial\nglsl-shaders=~~/a.glsl=b\nfs=yes\n";
        var once = MpvConfText.Parse(canonical);
        string serialized = MpvConfText.Serialize(once);
        Assert.Equal(canonical, serialized);
        Assert.Equal(once, MpvConfText.Parse(serialized));
    }
}
