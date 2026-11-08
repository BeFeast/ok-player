using System.Globalization;
using System.Text.Json;
using OkPlayer.App.Services;

namespace OkPlayer.Tests;

/// <summary>The Windows shell's colours come from <c>brand/palette.json</c> at the repository root: these tests
/// read that file directly (independently of <see cref="BrandPalette.Parse"/>) and check that the palette
/// compiled into Core, the accent slots and the themed brush table all carry its values.</summary>
public class BrandPaletteTests
{
    private static readonly Lazy<JsonDocument> Json = new(() => JsonDocument.Parse(File.ReadAllText(FindPaletteFile())));

    private static string FindPaletteFile()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            string candidate = Path.Combine(dir.FullName, "brand", "palette.json");
            if (File.Exists(candidate))
                return candidate;
        }
        throw new FileNotFoundException("brand/palette.json not found above the test output directory");
    }

    /// <summary>The colour the shared file gives <paramref name="role"/> in <paramref name="theme"/>.</summary>
    private static AccentRgb FromFile(string theme, string role)
    {
        string hex = Json.Value.RootElement.GetProperty(theme).GetProperty(role).GetString()!;
        uint rgb = uint.Parse(hex.AsSpan(1), NumberStyles.HexNumber, CultureInfo.InvariantCulture);
        return new AccentRgb(0xFF, (byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb);
    }

    private static readonly Dictionary<string, Func<BrandThemeColors, AccentRgb>> Roles = new()
    {
        ["canvas"] = t => t.Canvas, ["surface"] = t => t.Surface, ["surfaceRaised"] = t => t.SurfaceRaised,
        ["text"] = t => t.Text, ["textMuted"] = t => t.TextMuted, ["border"] = t => t.Border,
        ["borderSubtle"] = t => t.BorderSubtle, ["accent"] = t => t.Accent, ["onAccent"] = t => t.OnAccent,
        ["selected"] = t => t.Selected, ["focus"] = t => t.Focus, ["link"] = t => t.Link,
        ["success"] = t => t.Success, ["warning"] = t => t.Warning, ["danger"] = t => t.Danger, ["info"] = t => t.Info,
    };

    [Theory]
    [InlineData("light")]
    [InlineData("dark")]
    public void EmbeddedPalette_CarriesEveryRoleOfTheSharedFile(string theme)
    {
        var colors = theme == "dark" ? BrandPalette.Default.Dark : BrandPalette.Default.Light;
        var fileRoles = Json.Value.RootElement.GetProperty(theme).EnumerateObject().Select(p => p.Name).ToList();

        Assert.Equal(Roles.Keys.OrderBy(k => k), fileRoles.OrderBy(k => k));
        foreach (string role in fileRoles)
            Assert.True(FromFile(theme, role) == Roles[role](colors), $"{theme}.{role}");
        Assert.Equal(Json.Value.RootElement.GetProperty("name").GetString(), BrandPalette.Default.Name);
    }

    [Fact]
    public void BrandAccent_TakesFillLinkAndFocusFromTheSharedFile()
    {
        var p = AccentPalette.Brand;

        Assert.Equal(FromFile("light", "accent"), p.AccentLight);
        Assert.Equal(FromFile("light", "link"), p.TextLight);
        Assert.Equal(FromFile("light", "accent"), p.SecondaryLight);
        Assert.Equal(FromFile("light", "onAccent"), p.OnAccentLight);
        Assert.Equal(FromFile("dark", "accent"), p.AccentDark);
        Assert.Equal(FromFile("dark", "link"), p.TextDark);
        Assert.Equal(FromFile("dark", "link"), p.SecondaryDark);
        Assert.Equal(FromFile("dark", "onAccent"), p.OnAccentDark);
        Assert.Equal(FromFile("dark", "focus"), p.OverVideo);
    }

    [Fact]
    public void BrandAccent_TranslucentBrushes_TintOffTheSharedAccent()
    {
        var p = AccentPalette.Brand;

        Assert.Equal(FromFile("light", "accent").WithAlpha(0x1F), p.SelectionFillLight);
        Assert.Equal(FromFile("light", "accent").WithAlpha(0x1A), p.TintLight);
        Assert.Equal(FromFile("dark", "accent").WithAlpha(0x26), p.SelectionFillDark);
        Assert.Equal(FromFile("dark", "accent").WithAlpha(0x1F), p.TintDark);
        Assert.Equal(FromFile("light", "accent").WithAlpha(0x1F), p.ListSelected);
        Assert.Equal(FromFile("light", "accent").WithAlpha(0x29), p.ListSelectedHover);
        Assert.Equal(FromFile("dark", "focus").WithAlpha(0x29), p.AbRegion);
        Assert.Equal(FromFile("dark", "focus").WithAlpha(0x1F), p.OverVideoTint);
    }

    [Theory]
    [InlineData("light")]
    [InlineData("dark")]
    public void ThemedBrushes_MapSurfacesTextStrokesAndErrorToTheSharedFile(string theme)
    {
        var brushes = BrandPalette.Default.ThemedBrushes(dark: theme == "dark");
        AccentRgb C(string role) => FromFile(theme, role);

        Assert.Equal(C("canvas"), brushes["OkSurfaceBaseBrush"]);
        Assert.Equal(C("surface"), brushes["OkSurfaceLayer1Brush"]);
        Assert.Equal(C("surface"), brushes["OkSurfaceCardBrush"]);
        Assert.Equal(C("canvas"), brushes["OkSettingsWindowBrush"]);
        Assert.Equal(C("surface").WithAlpha(theme == "dark" ? (byte)0xF7 : (byte)0xE6), brushes["OkPanelOverVideoBrush"]);
        Assert.Equal(C("surface").WithAlpha(0xF7), brushes["OkPopoverBrush"]);
        Assert.Equal(C("text"), brushes["OkTextPrimaryBrush"]);
        Assert.Equal(C("text"), brushes["OkTextBodyBrush"]);
        Assert.Equal(C("textMuted"), brushes["OkTextSecondaryBrush"]);
        Assert.Equal(C("borderSubtle"), brushes["OkStrokeBrush"]);
        Assert.Equal(C("borderSubtle"), brushes["OkCardStrokeBrush"]);
        Assert.Equal(C("danger"), brushes["OkErrorBrush"]);
        Assert.Equal(12, brushes.Count);
    }

    [Theory]
    [InlineData("#2563EB", 0x25, 0x63, 0xEB)]
    [InlineData("#ffffff", 0xFF, 0xFF, 0xFF)]
    public void ParseHex_ReadsRrggbbAsOpaque(string hex, byte r, byte g, byte b)
        => Assert.Equal(new AccentRgb(0xFF, r, g, b), BrandPalette.ParseHex(hex));

    [Theory]
    [InlineData("2563EB")]
    [InlineData("#2563E")]
    [InlineData("#FF2563EB")]
    [InlineData("#GG63EB")]
    public void ParseHex_RejectsAnythingButRrggbb(string hex)
        => Assert.Throws<FormatException>(() => BrandPalette.ParseHex(hex));

    [Fact]
    public void Parse_MissingRole_Throws()
    {
        const string json = """{"name":"x","light":{"canvas":"#000000"},"dark":{"canvas":"#000000"}}""";
        Assert.Throws<KeyNotFoundException>(() => BrandPalette.Parse(json));
    }
}
