using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using OkPlayer.App.Services;
using Windows.UI;
using Windows.UI.ViewManagement;

namespace OkPlayer.App;

/// <summary>Applies the chosen accent to the design-system brushes at runtime. When
/// <c>AccentSource == "System"</c> the app follows the Windows personalization accent (and live-updates
/// when the user changes it); otherwise it uses the shipped teal. Works by mutating the shared
/// <see cref="SolidColorBrush"/> instances in place — the <c>Ok*</c> accent brushes live in
/// <c>ThemeDictionaries</c>, so changing each instance's <c>.Color</c> propagates to every
/// <c>{ThemeResource}</c> consumer without rebinding. The colour derivation is the pure, unit-tested
/// <see cref="AccentPalette"/>; this layer is just the WinUI glue.</summary>
internal static class AccentManager
{
    private static readonly UISettings _ui = new();
    private static DispatcherQueue? _dispatcher;

    /// <summary>Wire the live-update path and apply the current accent once. Call on the UI thread at
    /// startup, after the app resources exist.</summary>
    public static void Initialize()
    {
        _dispatcher = DispatcherQueue.GetForCurrentThread();
        // The OS accent can change while we run (Settings → Personalization). This fires off-thread.
        _ui.ColorValuesChanged += OnSystemColorsChanged;
        Apply();
    }

    private static void OnSystemColorsChanged(UISettings sender, object args)
        => _dispatcher?.TryEnqueue(() => { if (App.Settings.Current.AccentSource == "System") Apply(); });

    /// <summary>Push the accent for the current <c>AccentSource</c> into the brush instances. Idempotent;
    /// safe to call on every <c>Settings.Changed</c>.</summary>
    public static void Apply()
    {
        AccentPalette p = App.Settings.Current.AccentSource == "System" ? FromSystem() : AccentPalette.Teal;

        SetThemed("OkAccentBrush", p.AccentLight, p.AccentDark);
        SetThemed("OkAccentTextBrush", p.TextLight, p.TextDark);
        SetThemed("OkAccentSecondaryTextBrush", p.SecondaryLight, p.SecondaryDark);
        SetThemed("OkAccentRailBrush", p.AccentLight, p.AccentDark);
        SetThemed("OkAccentSelectionFillBrush", p.SelectionFillLight, p.SelectionFillDark);
        SetThemed("OkAccentTintBrush", p.TintLight, p.TintDark);
        SetThemed("OkOnAccentBrush", p.OnAccentLight, p.OnAccentDark);

        // Over-video accents are theme-invariant (they sit on the dark video plane) — one bright shade.
        SetFlat("OkSeekFillBrush", p.OverVideo);
        SetFlat("OkOverVideoAccentBrush", p.OverVideo);
        SetFlat("OkAbRegionBrush", p.AbRegion);
    }

    private static AccentPalette FromSystem()
    {
        AccentRgb C(UIColorType t) { var c = _ui.GetColorValue(t); return new AccentRgb(c.A, c.R, c.G, c.B); }
        return AccentPalette.FromSystem(
            C(UIColorType.Accent),
            C(UIColorType.AccentDark1), C(UIColorType.AccentDark2), C(UIColorType.AccentDark3),
            C(UIColorType.AccentLight1), C(UIColorType.AccentLight2), C(UIColorType.AccentLight3));
    }

    private static void SetThemed(string key, AccentRgb light, AccentRgb dark)
    {
        if (FindThemed(key, "Light") is { } l) l.Color = ToColor(light);
        if (FindThemed(key, "Dark") is { } d) d.Color = ToColor(dark);
    }

    private static void SetFlat(string key, AccentRgb rgb)
    {
        if (FindFlat(Application.Current.Resources, key) is { } b) b.Color = ToColor(rgb);
    }

    private static Color ToColor(AccentRgb c) => Color.FromArgb(c.A, c.R, c.G, c.B);

    // ---- brush lookup (the Ok* accent brushes live in ThemeDictionaries; over-video ones are flat) ----

    private static SolidColorBrush? FindThemed(string key, string themeKey)
    {
        foreach (var md in Application.Current.Resources.MergedDictionaries)
            if (FindThemedIn(md, key, themeKey) is { } b)
                return b;
        return null;
    }

    private static SolidColorBrush? FindThemedIn(ResourceDictionary dict, string key, string themeKey)
    {
        if (dict.ThemeDictionaries.TryGetValue(themeKey, out var themeObj) && themeObj is ResourceDictionary themed
            && themed.TryGetValue(key, out var v) && v is SolidColorBrush b)
            return b;
        foreach (var md in dict.MergedDictionaries)
            if (FindThemedIn(md, key, themeKey) is { } found)
                return found;
        return null;
    }

    private static SolidColorBrush? FindFlat(ResourceDictionary dict, string key)
    {
        if (dict.TryGetValue(key, out var v) && v is SolidColorBrush b)
            return b;
        foreach (var md in dict.MergedDictionaries)
            if (FindFlat(md, key) is { } found)
                return found;
        return null;
    }
}
