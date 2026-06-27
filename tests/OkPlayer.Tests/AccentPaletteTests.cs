using OkPlayer.App.Services;

namespace OkPlayer.Tests;

public class AccentPaletteTests
{
    private static readonly AccentRgb White = new(0xFF, 0xFF, 0xFF, 0xFF);
    private static readonly AccentRgb NearBlack = new(0xFF, 0x1A, 0x1A, 0x1A);

    [Fact]
    public void WithAlpha_KeepsRgb_SetsAlpha()
    {
        var c = new AccentRgb(0xFF, 0x10, 0x93, 0x8A).WithAlpha(0x1F);
        Assert.Equal(new AccentRgb(0x1F, 0x10, 0x93, 0x8A), c);
    }

    [Theory]
    [InlineData(0x10, 0x93, 0x8A, false)] // teal 500 — dark enough for a white glyph
    [InlineData(0x00, 0x00, 0x00, false)] // black
    [InlineData(0xFF, 0xFF, 0xFF, true)]  // white
    [InlineData(0xF2, 0xE0, 0x00, true)]  // bright yellow accent — needs a dark glyph
    public void IsLight_FollowsLuma(byte r, byte g, byte b, bool expected)
        => Assert.Equal(expected, new AccentRgb(0xFF, r, g, b).IsLight);

    [Fact]
    public void Teal_MatchesShippedBrushDefaults()
    {
        var p = AccentPalette.Teal;
        Assert.Equal(new AccentRgb(0xFF, 0x10, 0x93, 0x8A), p.AccentLight);
        Assert.Equal(new AccentRgb(0xFF, 0x0A, 0x65, 0x5F), p.TextLight);
        Assert.Equal(new AccentRgb(0xFF, 0x28, 0xB3, 0xAA), p.AccentDark);
        Assert.Equal(new AccentRgb(0xFF, 0x04, 0x20, 0x1E), p.OnAccentDark);
        Assert.Equal(White, p.OnAccentLight);
    }

    [Fact]
    public void Teal_AlphaDerivations_MatchBrushesXaml()
    {
        var p = AccentPalette.Teal;
        Assert.Equal(new AccentRgb(0x1F, 0x10, 0x93, 0x8A), p.SelectionFillLight);
        Assert.Equal(new AccentRgb(0x1A, 0x10, 0x93, 0x8A), p.TintLight);
        Assert.Equal(new AccentRgb(0x26, 0x28, 0xB3, 0xAA), p.SelectionFillDark);
        Assert.Equal(new AccentRgb(0x1F, 0x28, 0xB3, 0xAA), p.TintDark);
        Assert.Equal(new AccentRgb(0x29, 0x28, 0xB3, 0xAA), p.AbRegion);
    }

    [Fact]
    public void FromSystem_MapsShadesToTheRightSlots()
    {
        // Distinct sentinel shades so each slot's source is unambiguous.
        var baseA = new AccentRgb(0xFF, 0x10, 0x20, 0x30);
        var d1 = new AccentRgb(0xFF, 0x11, 0x11, 0x11);
        var d2 = new AccentRgb(0xFF, 0x22, 0x22, 0x22);
        var d3 = new AccentRgb(0xFF, 0x33, 0x33, 0x33);
        var l1 = new AccentRgb(0xFF, 0xA1, 0xA1, 0xA1);
        var l2 = new AccentRgb(0xFF, 0xB2, 0xB2, 0xB2);
        var l3 = new AccentRgb(0xFF, 0xC3, 0xC3, 0xC3);

        var p = AccentPalette.FromSystem(baseA, d1, d2, d3, l1, l2, l3);

        Assert.Equal(baseA, p.AccentLight);
        Assert.Equal(d2, p.TextLight);
        Assert.Equal(d1, p.SecondaryLight);
        Assert.Equal(l2, p.AccentDark);
        Assert.Equal(l2, p.TextDark);
        Assert.Equal(l2, p.SecondaryDark);
        Assert.Equal(d3, p.OnAccentDark);
        Assert.Equal(l2, p.OverVideo);
    }

    [Fact]
    public void FromSystem_OnAccentLight_ChosenByLuma()
    {
        var dark = new AccentRgb(0xFF, 0x10, 0x20, 0x30);  // dark base -> white glyph
        var light = new AccentRgb(0xFF, 0xF2, 0xE0, 0x00); // bright base -> dark glyph
        var any = new AccentRgb(0xFF, 0x55, 0x55, 0x55);

        Assert.Equal(White, AccentPalette.FromSystem(dark, any, any, any, any, any, any).OnAccentLight);
        Assert.Equal(NearBlack, AccentPalette.FromSystem(light, any, any, any, any, any, any).OnAccentLight);
    }

    [Fact]
    public void FromSystem_AlphaTints_UseTheMappedAccent()
    {
        var baseA = new AccentRgb(0xFF, 0x40, 0x50, 0x60);
        var l2 = new AccentRgb(0xFF, 0xB2, 0xB2, 0xB2);
        var any = new AccentRgb(0xFF, 0x77, 0x77, 0x77);

        var p = AccentPalette.FromSystem(baseA, any, any, any, any, l2, any);
        Assert.Equal(baseA.WithAlpha(0x1F), p.SelectionFillLight); // light tint off the base accent
        Assert.Equal(l2.WithAlpha(0x1F), p.TintDark);              // dark tint off the lighter shade
        Assert.Equal(l2.WithAlpha(0x29), p.AbRegion);              // over-video region off the lighter shade
    }
}
