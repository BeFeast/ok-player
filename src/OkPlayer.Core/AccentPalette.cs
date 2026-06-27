namespace OkPlayer.App.Services;

/// <summary>An ARGB colour as plain bytes, so the accent math stays in the engine-agnostic Core (no
/// WinUI <c>Windows.UI.Color</c> dependency). The App layer converts to/from <c>Color</c>.</summary>
public readonly record struct AccentRgb(byte A, byte R, byte G, byte B)
{
    public AccentRgb WithAlpha(byte a) => this with { A = a };

    // Rec.601 luma; "light" accents need a dark glyph on top, dark ones a white glyph.
    public bool IsLight => 0.299 * R + 0.587 * G + 0.114 * B >= 140;
}

/// <summary>The full set of accent colours the app's brushes need, per theme — derived once here so the
/// (visual, untestable) brush mutation in the App layer is a thin mapping. Mirrors the hand-tuned teal:
/// the light theme leans on the base/darker shades for text contrast, the dark theme on a lighter shade
/// that pops on dark surfaces, and a single "over video" accent for the seek bar / A–B region.</summary>
public sealed record AccentPalette(
    AccentRgb AccentLight, AccentRgb TextLight, AccentRgb SecondaryLight, AccentRgb OnAccentLight,
    AccentRgb AccentDark, AccentRgb TextDark, AccentRgb SecondaryDark, AccentRgb OnAccentDark,
    AccentRgb OverVideo)
{
    // Alpha bytes match Brushes.xaml: selection fill ~12–15%, tint ~10–12%, A–B region ~16%.
    public AccentRgb SelectionFillLight => AccentLight.WithAlpha(0x1F);
    public AccentRgb TintLight => AccentLight.WithAlpha(0x1A);
    public AccentRgb SelectionFillDark => AccentDark.WithAlpha(0x26);
    public AccentRgb TintDark => AccentDark.WithAlpha(0x1F);
    public AccentRgb AbRegion => OverVideo.WithAlpha(0x29);

    private static readonly AccentRgb White = new(0xFF, 0xFF, 0xFF, 0xFF);
    private static readonly AccentRgb NearBlack = new(0xFF, 0x1A, 0x1A, 0x1A);

    /// <summary>The shipped teal palette (the defaults baked into Brushes.xaml), used whenever the
    /// accent source is not "System".</summary>
    public static AccentPalette Teal { get; } = new(
        AccentLight: new(0xFF, 0x10, 0x93, 0x8A), TextLight: new(0xFF, 0x0A, 0x65, 0x5F),
        SecondaryLight: new(0xFF, 0x0C, 0x7C, 0x75), OnAccentLight: White,
        AccentDark: new(0xFF, 0x28, 0xB3, 0xAA), TextDark: new(0xFF, 0x28, 0xB3, 0xAA),
        SecondaryDark: new(0xFF, 0x28, 0xB3, 0xAA), OnAccentDark: new(0xFF, 0x04, 0x20, 0x1E),
        OverVideo: new(0xFF, 0x28, 0xB3, 0xAA));

    /// <summary>Build a palette from the Windows system accent and its shade ramp (UISettings
    /// Accent + AccentDark1–3 / AccentLight1–3). Light theme: base accent, darker shades for readable
    /// text, a glyph colour chosen by the accent's luminance. Dark theme + over-video: a lighter shade.</summary>
    public static AccentPalette FromSystem(
        AccentRgb baseAccent, AccentRgb dark1, AccentRgb dark2, AccentRgb dark3,
        AccentRgb light1, AccentRgb light2, AccentRgb light3)
        => new(
            AccentLight: baseAccent,
            TextLight: dark2,
            SecondaryLight: dark1,
            OnAccentLight: baseAccent.IsLight ? NearBlack : White,
            AccentDark: light2,
            TextDark: light2,
            SecondaryDark: light2,
            OnAccentDark: dark3,
            OverVideo: light2);
}
