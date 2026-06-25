using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace OkPlayer.App;

/// <summary>The Settings window — a left nav-rail + content pane over Mica (design band 9).
/// Appearance is the fully-built panel; the others are reserved slots for now.</summary>
public sealed partial class SettingsWindow : Window
{
    private static readonly string[] PanelNames =
        { "Appearance", "Playback", "Subtitles", "Video", "Audio", "Shortcuts", "Integration", "Advanced" };

    private bool _loaded;

    public SettingsWindow()
    {
        InitializeComponent();
        Title = "Settings";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(760, 560));
        ApplyTheme();
        App.Settings.Changed += ApplyTheme;
        Closed += (_, _) => App.Settings.Changed -= ApplyTheme;
        LoadAppearance();
        _loaded = true;
    }

    private void ApplyTheme()
    {
        if (Content is FrameworkElement root)
            root.RequestedTheme = App.Settings.Current.Theme == "Light" ? ElementTheme.Light : ElementTheme.Default;
    }

    private void OnNavChanged(object sender, SelectionChangedEventArgs e)
    {
        if (AppearancePanel is null) // SelectedIndex=0 fires during InitializeComponent, before the pane exists
            return;
        int i = NavList.SelectedIndex;
        bool appearance = i == 0;
        AppearancePanel.Visibility = appearance ? Visibility.Visible : Visibility.Collapsed;
        PlaceholderPanel.Visibility = appearance ? Visibility.Collapsed : Visibility.Visible;
        if (!appearance && i >= 0 && i < PanelNames.Length)
            PlaceholderTitle.Text = PanelNames[i];
    }

    // ── Appearance panel ───────────────────────────────────────────────

    private void LoadAppearance()
    {
        var s = App.Settings.Current;
        MicaTitlebarSlider.Value = s.MicaTitlebar;
        MicaPanelsSlider.Value = s.MicaPanels;
        MicaOverlaysSlider.Value = s.MicaOverlays;
        RefreshAppearance();
    }

    private void RefreshAppearance()
    {
        var s = App.Settings.Current;
        bool light = s.Theme == "Light";
        StyleSegment(ThemeLightBtn, light);
        StyleSegment(ThemeAutoBtn, !light);
        bool teal = s.AccentSource == "OkTeal";
        StyleCard(AccentTealBtn, teal);
        StyleCard(AccentSystemBtn, !teal);
        MicaTitlebarVal.Text = $"{s.MicaTitlebar}%";
        MicaPanelsVal.Text = $"{s.MicaPanels}%";
        MicaOverlaysVal.Text = $"{s.MicaOverlays}%";
    }

    private void StyleSegment(Button b, bool selected)
    {
        b.Background = selected ? Res("CardBackgroundFillColorDefaultBrush", new SolidColorBrush(Colors.White)) : Transparent;
        b.Foreground = selected ? AccentText : Res("OkTextBodyBrush", new SolidColorBrush(Color.FromArgb(0xDE, 0, 0, 0)));
        b.FontWeight = selected ? FontWeights.SemiBold : FontWeights.Normal;
    }

    private void StyleCard(Button b, bool selected)
    {
        b.BorderBrush = selected ? Accent : Res("OkStrokeBrush", new SolidColorBrush(Color.FromArgb(0x14, 0, 0, 0)));
        b.BorderThickness = new Thickness(selected ? 1.5 : 1);
        b.Background = selected ? AccentTint : Transparent;
    }

    private void OnThemeLight(object sender, RoutedEventArgs e) => SetTheme("Light");
    private void OnThemeAuto(object sender, RoutedEventArgs e) => SetTheme("Auto");

    private void SetTheme(string theme)
    {
        App.Settings.Current.Theme = theme;
        App.Settings.Save();   // raises Changed → this + the player window re-apply
        RefreshAppearance();
    }

    private void OnAccentSystem(object sender, RoutedEventArgs e) => SetAccent("System");
    private void OnAccentTeal(object sender, RoutedEventArgs e) => SetAccent("OkTeal");

    private void SetAccent(string source)
    {
        App.Settings.Current.AccentSource = source; // live accent swap is a later refinement; persist + reflect now
        App.Settings.Save();
        RefreshAppearance();
    }

    private void OnMicaChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (!_loaded)
            return;
        var s = App.Settings.Current;
        s.MicaTitlebar = (int)MicaTitlebarSlider.Value;
        s.MicaPanels = (int)MicaPanelsSlider.Value;
        s.MicaOverlays = (int)MicaOverlaysSlider.Value;
        App.Settings.Save();   // effect deferred (WinUI Mica has no per-surface intensity API); value persists
        RefreshAppearance();
    }

    // Themed brushes live in ThemeDictionaries (not the flat Resources map), so fall back to theme-stable
    // teal constants when a key can't be resolved from code.
    private static readonly SolidColorBrush Transparent = new(Colors.Transparent);
    private static readonly SolidColorBrush Accent = new(Color.FromArgb(0xFF, 0x10, 0x93, 0x8A));
    private static readonly SolidColorBrush AccentText = new(Color.FromArgb(0xFF, 0x0A, 0x65, 0x5F));
    private static readonly SolidColorBrush AccentTint = new(Color.FromArgb(0x1A, 0x10, 0x93, 0x8A));

    private static Brush Res(string key, Brush fallback)
    {
        if (Application.Current.Resources.TryGetValue(key, out var v) && v is Brush b)
            return b;
        return fallback;
    }
}
