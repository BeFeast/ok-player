using System.Collections.Generic;
using System.Globalization;
using System.Text.Json;

namespace OkPlayer.App.Services;

/// <summary>One theme's semantic colours, as named in <c>brand/palette.json</c>.</summary>
public sealed record BrandThemeColors(
    AccentRgb Canvas, AccentRgb Surface, AccentRgb SurfaceRaised,
    AccentRgb Text, AccentRgb TextMuted, AccentRgb Border, AccentRgb BorderSubtle,
    AccentRgb Accent, AccentRgb OnAccent, AccentRgb Selected,
    AccentRgb Focus, AccentRgb Link,
    AccentRgb Success, AccentRgb Warning, AccentRgb Danger, AccentRgb Info);

/// <summary>The app's own colour family, shared by every shell through <c>brand/palette.json</c> at the
/// repository root. The file is embedded into this assembly at build time, so the Windows shell carries no
/// hex copy of it: the themed <c>Ok*</c> brushes are filled from here at startup (see the App's
/// <c>AccentManager</c>), and <see cref="AccentPalette.Brand"/> derives the accent slots from it.</summary>
public sealed record BrandPalette(string Name, BrandThemeColors Light, BrandThemeColors Dark)
{
    internal const string ResourceName = "OkPlayer.Core.Brand.palette.json";

    /// <summary>The palette compiled into this build.</summary>
    public static BrandPalette Default { get; } = LoadEmbedded();

    /// <summary>Parse the <c>palette.json</c> format: <c>name</c> plus <c>light</c>/<c>dark</c> objects
    /// mapping each semantic role to a <c>#RRGGBB</c> colour. A missing role throws, so a renamed key
    /// fails the unit tests instead of shipping a transparent brush.</summary>
    public static BrandPalette Parse(string json)
    {
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        return new BrandPalette(
            root.GetProperty("name").GetString() ?? string.Empty,
            ParseTheme(root.GetProperty("light")),
            ParseTheme(root.GetProperty("dark")));
    }

    /// <summary><c>#RRGGBB</c> to an opaque colour.</summary>
    public static AccentRgb ParseHex(string hex)
    {
        if (hex.Length != 7 || hex[0] != '#'
            || !uint.TryParse(hex.AsSpan(1), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out uint rgb))
            throw new FormatException($"palette colour '{hex}' is not #RRGGBB");
        return new AccentRgb(0xFF, (byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb);
    }

    /// <summary>The non-accent themed brushes of <c>Brushes.xaml</c> and the palette colour each one takes,
    /// for the Light (<paramref name="dark"/> false) or Dark dictionary. The accent brushes come from
    /// <see cref="AccentPalette"/> instead, because the "System" accent source replaces only those; the
    /// HighContrast dictionary keeps the Windows system colours and is never touched.</summary>
    public IReadOnlyDictionary<string, AccentRgb> ThemedBrushes(bool dark)
    {
        var t = dark ? Dark : Light;
        return new Dictionary<string, AccentRgb>
        {
            // Navy canvas/surfaces in dark, pale canvas + white surfaces in light. Mica stays the window
            // backdrop; these paint only the surfaces that were already opaque.
            ["OkSurfaceBaseBrush"] = t.Canvas,
            ["OkSurfaceLayer1Brush"] = t.Surface,
            ["OkSurfaceCardBrush"] = t.Surface,
            ["OkSettingsWindowBrush"] = t.Canvas,
            // Panels and popovers over video keep their near-opaque alpha.
            ["OkPanelOverVideoBrush"] = t.Surface.WithAlpha(dark ? (byte)0xF7 : (byte)0xE6),
            ["OkPopoverBrush"] = t.Surface.WithAlpha(0xF7),
            ["OkTextPrimaryBrush"] = t.Text,
            ["OkTextBodyBrush"] = t.Text,
            ["OkTextSecondaryBrush"] = t.TextMuted,
            // Card outlines and separators are decorative, so they take the subtle border.
            ["OkStrokeBrush"] = t.BorderSubtle,
            ["OkCardStrokeBrush"] = t.BorderSubtle,
            ["OkErrorBrush"] = t.Danger,
        };
    }

    private static BrandThemeColors ParseTheme(JsonElement theme)
    {
        AccentRgb C(string role) => ParseHex(theme.GetProperty(role).GetString() ?? string.Empty);
        return new BrandThemeColors(
            Canvas: C("canvas"), Surface: C("surface"), SurfaceRaised: C("surfaceRaised"),
            Text: C("text"), TextMuted: C("textMuted"), Border: C("border"), BorderSubtle: C("borderSubtle"),
            Accent: C("accent"), OnAccent: C("onAccent"), Selected: C("selected"),
            Focus: C("focus"), Link: C("link"),
            Success: C("success"), Warning: C("warning"), Danger: C("danger"), Info: C("info"));
    }

    private static BrandPalette LoadEmbedded()
    {
        using var stream = typeof(BrandPalette).Assembly.GetManifestResourceStream(ResourceName)
            ?? throw new InvalidOperationException($"embedded resource {ResourceName} is missing");
        using var reader = new StreamReader(stream);
        return Parse(reader.ReadToEnd());
    }
}
