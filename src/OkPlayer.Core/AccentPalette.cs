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
/// (visual, untestable) brush mutation in the App layer is a thin mapping. Each theme has a fill accent, a
/// readable accent-text colour, a secondary accent text, the glyph colour on a filled accent, and a single
/// "over video" accent for the seek bar / A–B region.</summary>
public sealed record AccentPalette(
    AccentRgb AccentLight, AccentRgb TextLight, AccentRgb SecondaryLight, AccentRgb OnAccentLight,
    AccentRgb AccentDark, AccentRgb TextDark, AccentRgb SecondaryDark, AccentRgb OnAccentDark,
    AccentRgb OverVideo)
{
    // Alpha bytes for the translucent accent brushes: selection fill ~12–15%, tint ~10–12%, A–B region ~16%.
    public AccentRgb SelectionFillLight => AccentLight.WithAlpha(0x1F);
    public AccentRgb TintLight => AccentLight.WithAlpha(0x1A);
    public AccentRgb SelectionFillDark => AccentDark.WithAlpha(0x26);
    public AccentRgb TintDark => AccentDark.WithAlpha(0x1F);
    public AccentRgb AbRegion => OverVideo.WithAlpha(0x29);
    public AccentRgb OverVideoTint => OverVideo.WithAlpha(0x1F);
    // List selection is theme-invariant (one brush for both themes), so it tints off the light accent.
    public AccentRgb ListSelected => AccentLight.WithAlpha(0x1F);
    public AccentRgb ListSelectedHover => AccentLight.WithAlpha(0x29);

    private static readonly AccentRgb White = new(0xFF, 0xFF, 0xFF, 0xFF);
    private static readonly AccentRgb NearBlack = new(0xFF, 0x1A, 0x1A, 0x1A);

    /// <summary>The app's own accent, used whenever the accent source is not "System". Derived from the
    /// embedded <c>brand/palette.json</c>: royal-blue fills with a white glyph in both themes, the palette's
    /// link colour for accent text (cyan on dark surfaces, where royal-blue text would be too dim), and the
    /// cyan focus colour over the dark video plane.</summary>
    public static AccentPalette Brand { get; } = FromBrand(BrandPalette.Default);

    /// <summary>Map a <see cref="BrandPalette"/> onto the accent slots.</summary>
    public static AccentPalette FromBrand(BrandPalette brand)
        => new(
            AccentLight: brand.Light.Accent,
            TextLight: brand.Light.Link,
            SecondaryLight: brand.Light.Accent,
            OnAccentLight: brand.Light.OnAccent,
            AccentDark: brand.Dark.Accent,
            TextDark: brand.Dark.Link,
            SecondaryDark: brand.Dark.Link,
            OnAccentDark: brand.Dark.OnAccent,
            OverVideo: brand.Dark.Focus);

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
