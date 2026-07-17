using System.Linq;
using System.Text.RegularExpressions;
using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

/// <summary>Engine-agnostic guards for the subtitle appearance presets. The render-level proof that the
/// options actually take effect lives in OkPlayer.IntegrationTests.SubtitleStyleTests (real libmpv, pixel
/// colour); these pin the pure-data invariants the UI and the apply-path rely on.</summary>
public class SubtitleStyleTests
{
    [Fact]
    public void All_ListsTheFourPresets_WithUniqueKeys()
    {
        Assert.Equal(new[] { "Default", "Bold", "Classic", "Contrast" },
            SubtitleStyle.All.Select(s => s.Key).ToArray());
        Assert.Equal(SubtitleStyle.All.Count, SubtitleStyle.All.Select(s => s.Key).Distinct().Count());
    }

    [Fact]
    public void EveryPreset_WritesTheSameOptionNames_SoSwitchingFullyOverrides()
    {
        // The whole design rests on this: because every preset sets the identical set of options, switching
        // from any preset to any other repaints every field and leaves no residual state (e.g. Classic's
        // yellow can't linger after picking Default). If a preset adds or drops an option, that breaks.
        var expected = SubtitleStyle.Default.Options.Select(o => o.Key).OrderBy(k => k).ToArray();
        Assert.Equal(7, expected.Length);
        foreach (var style in SubtitleStyle.All)
        {
            var names = style.Options.Select(o => o.Key).OrderBy(k => k).ToArray();
            Assert.Equal(expected, names);
            // No option set twice within a preset (a later value would silently win).
            Assert.Equal(names.Length, names.Distinct().Count());
        }
    }

    [Theory]
    [InlineData("Default", "Default")]
    [InlineData("Bold", "Bold")]
    [InlineData("Classic", "Classic")]
    [InlineData("Contrast", "Contrast")]
    [InlineData("classic", "Classic")]   // case-insensitive
    [InlineData("CONTRAST", "Contrast")]
    public void FromKey_ResolvesKnownKeys(string key, string expectedKey)
        => Assert.Equal(expectedKey, SubtitleStyle.FromKey(key).Key);

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("nonsense")]
    public void FromKey_FallsBackToDefault_ForUnknownOrEmpty(string? key)
        => Assert.Same(SubtitleStyle.Default, SubtitleStyle.FromKey(key));

    [Fact]
    public void DefaultPreset_IsTheWhiteUnboldedBaseline()
    {
        var opts = SubtitleStyle.Default.Options.ToDictionary(o => o.Key, o => o.Value);
        Assert.Equal("#FFFFFF", opts["sub-color"]);
        Assert.Equal("no", opts["sub-bold"]);
    }

    [Fact]
    public void ClassicPreset_IsYellow()
    {
        // Mirrors the integration test's pixel assertion at the data level: Classic must request yellow text.
        var opts = SubtitleStyle.Classic.Options.ToDictionary(o => o.Key, o => o.Value);
        Assert.Equal("#FFFF00", opts["sub-color"]);
    }

    [Fact]
    public void ContrastPreset_UsesSemiTransparentBackgroundBox()
    {
        var opts = SubtitleStyle.Contrast.Options.ToDictionary(o => o.Key, o => o.Value);
        Assert.Equal("background-box", opts["sub-border-style"]);
        Assert.Equal("0.0/0.0/0.0/0.72", opts["sub-back-color"]);
    }

    [Fact]
    public void NonBoxedPresets_RestoreOutlineAndShadow()
    {
        foreach (var style in new[] { SubtitleStyle.Default, SubtitleStyle.Bold, SubtitleStyle.Classic })
        {
            var opts = style.Options.ToDictionary(o => o.Key, o => o.Value);
            Assert.Equal("outline-and-shadow", opts["sub-border-style"]);
            Assert.Equal("#000000", opts["sub-back-color"]);
        }
    }

    [Fact]
    public void OpaqueColors_UseSixDigitRrggbb()
    {
        var colorKeys = new[] { "sub-color", "sub-border-color" };
        var rrggbb = new Regex("^#[0-9A-Fa-f]{6}$");
        foreach (var style in SubtitleStyle.All)
            foreach (var (name, value) in style.Options)
                if (colorKeys.Contains(name))
                    Assert.Matches(rrggbb, value);
    }
}
