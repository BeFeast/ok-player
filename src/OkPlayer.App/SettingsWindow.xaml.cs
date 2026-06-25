using System;
using System.IO;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using OkPlayer.App.Services;
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
        bool playback = i == 1;
        bool video = i == 3;
        bool audio = i == 4;
        bool integration = i == 6;
        bool advanced = i == 7;
        AppearancePanel.Visibility = appearance ? Visibility.Visible : Visibility.Collapsed;
        PlaybackPanel.Visibility = playback ? Visibility.Visible : Visibility.Collapsed;
        VideoPanel.Visibility = video ? Visibility.Visible : Visibility.Collapsed;
        AudioPanel.Visibility = audio ? Visibility.Visible : Visibility.Collapsed;
        IntegrationPanel.Visibility = integration ? Visibility.Visible : Visibility.Collapsed;
        AdvancedPanel.Visibility = advanced ? Visibility.Visible : Visibility.Collapsed;
        PlaceholderPanel.Visibility = (!appearance && !playback && !video && !audio && !integration && !advanced)
            ? Visibility.Visible : Visibility.Collapsed;
        if (advanced)
            LoadMpvConf();
        else if (integration)
            LoadIntegration();
        else if (playback)
            LoadPlayback();
        else if (video)
            LoadVideo();
        else if (audio)
            LoadAudio();
        else if (!appearance && i >= 0 && i < PanelNames.Length)
            PlaceholderTitle.Text = PanelNames[i];
    }

    // ── Video / Audio panels ───────────────────────────────────────────

    private void LoadVideo()
    {
        bool hw = App.Settings.Current.HardwareDecoding;
        StyleSegment(Hwdec1, hw);
        StyleSegment(Hwdec0, !hw);
    }

    private void OnHwdec(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string t })
        {
            App.Settings.Current.HardwareDecoding = t == "1";
            App.Settings.Save();
            LoadVideo();
        }
    }

    private void LoadAudio()
    {
        int v = App.Settings.Current.DefaultVolume;
        StyleSegment(Vol50, v == 50);
        StyleSegment(Vol75, v == 75);
        StyleSegment(Vol100, v == 100);
    }

    private void OnDefaultVolume(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string t } && int.TryParse(t, out int v))
        {
            App.Settings.Current.DefaultVolume = v;
            App.Settings.Save();
            LoadAudio();
        }
    }

    // ── Playback panel ─────────────────────────────────────────────────

    private void LoadPlayback()
    {
        ResumeToggle.Toggled -= OnResumeToggled;
        ResumeToggle.IsOn = App.Settings.Current.ResumePlayback;
        ResumeToggle.Toggled += OnResumeToggled;
        RefreshPlayback();
    }

    private void RefreshPlayback()
    {
        var s = App.Settings.Current;
        StyleSegment(Speed075, Math.Abs(s.DefaultSpeed - 0.75) < 0.001);
        StyleSegment(Speed100, Math.Abs(s.DefaultSpeed - 1.0) < 0.001);
        StyleSegment(Speed125, Math.Abs(s.DefaultSpeed - 1.25) < 0.001);
        StyleSegment(Speed150, Math.Abs(s.DefaultSpeed - 1.5) < 0.001);
        StyleSegment(Speed200, Math.Abs(s.DefaultSpeed - 2.0) < 0.001);
        StyleSegment(Skip5, s.SkipStep == 5);
        StyleSegment(Skip10, s.SkipStep == 10);
        StyleSegment(Skip30, s.SkipStep == 30);
    }

    private void OnResumeToggled(object sender, RoutedEventArgs e)
    {
        App.Settings.Current.ResumePlayback = ResumeToggle.IsOn;
        App.Settings.Save();
    }

    private void OnSpeedDefault(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string t } &&
            double.TryParse(t, System.Globalization.NumberStyles.Any, System.Globalization.CultureInfo.InvariantCulture, out double v))
        {
            App.Settings.Current.DefaultSpeed = v;
            App.Settings.Save();
            RefreshPlayback();
        }
    }

    private void OnSkipStep(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string t } && int.TryParse(t, out int v))
        {
            App.Settings.Current.SkipStep = v;
            App.Settings.Save();
            RefreshPlayback();
        }
    }

    // ── Integration panel (file-type associations) ─────────────────────

    private static readonly string[] VideoExts = { ".mkv", ".mp4", ".m4v", ".avi", ".mov", ".webm", ".m2ts", ".ts", ".wmv", ".flv" };
    private static readonly string[] AudioExts = { ".mp3", ".flac", ".m4a", ".opus", ".wav", ".ogg", ".mka" };
    private readonly FileAssociationService _assoc = new();
    private bool _assocBuilt;

    private void LoadIntegration()
    {
        if (_assocBuilt)
        {
            RefreshAssocChecks();
            return;
        }
        BuildAssoc(AssocVideoPanel, VideoExts);
        BuildAssoc(AssocAudioPanel, AudioExts);
        _assocBuilt = true;
    }

    private void BuildAssoc(Panel host, string[] exts)
    {
        foreach (string ext in exts)
        {
            var cb = new CheckBox { Content = ext, Tag = ext, FontSize = 12.5, MinWidth = 0, IsChecked = _assoc.IsAssigned(ext) };
            cb.Checked += OnAssocToggle;
            cb.Unchecked += OnAssocToggle;
            host.Children.Add(cb);
        }
    }

    private void RefreshAssocChecks()
    {
        foreach (Panel host in new[] { (Panel)AssocVideoPanel, AssocAudioPanel })
            foreach (var child in host.Children)
                if (child is CheckBox { Tag: string ext } cb)
                {
                    cb.Checked -= OnAssocToggle;
                    cb.Unchecked -= OnAssocToggle;
                    cb.IsChecked = _assoc.IsAssigned(ext); // re-sync without firing the toggle handler
                    cb.Checked += OnAssocToggle;
                    cb.Unchecked += OnAssocToggle;
                }
    }

    private void OnAssocToggle(object sender, RoutedEventArgs e)
    {
        if (sender is not CheckBox { Tag: string ext } cb)
            return;
        try
        {
            if (cb.IsChecked == true)
                _assoc.Assign(ext);
            else
                _assoc.Unassign(ext);
            _assoc.NotifyShell();
            AssocStatus.Text = "Updated";
        }
        catch
        {
            AssocStatus.Text = "Couldn't update";
            cb.IsChecked = _assoc.IsAssigned(ext);
        }
    }

    private void OnOpenDefaultApps(object sender, RoutedEventArgs e) => FileAssociationService.OpenWindowsDefaultApps();

    // ── Advanced panel (the raw-mpv-config escape hatch) ───────────────

    private void LoadMpvConf()
    {
        try
        {
            string path = OkPlayer.Render.MpvVideoPanel.UserConfigPath;
            MpvConfEditor.Text = File.Exists(path) ? File.ReadAllText(path) : string.Empty;
        }
        catch { MpvConfEditor.Text = string.Empty; }
        MpvConfStatus.Text = string.Empty;
    }

    private void OnMpvConfSave(object sender, RoutedEventArgs e)
    {
        try
        {
            string path = OkPlayer.Render.MpvVideoPanel.UserConfigPath;
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, MpvConfEditor.Text);
            MpvConfStatus.Text = "Saved · restart to apply";
        }
        catch
        {
            MpvConfStatus.Text = "Couldn't save";
        }
    }

    private void OnOpenConfigFolder(object sender, RoutedEventArgs e)
    {
        try
        {
            string dir = Path.GetDirectoryName(OkPlayer.Render.MpvVideoPanel.UserConfigPath)!;
            Directory.CreateDirectory(dir);
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(dir) { UseShellExecute = true });
        }
        catch { /* best effort */ }
    }

    // ── Appearance panel ───────────────────────────────────────────────

    private void LoadAppearance()
    {
        var s = App.Settings.Current;
        // settings.json is hand-editable: normalize before binding to the 0..100 sliders.
        s.MicaTitlebar = Math.Clamp(s.MicaTitlebar, 0, 100);
        s.MicaPanels = Math.Clamp(s.MicaPanels, 0, 100);
        s.MicaOverlays = Math.Clamp(s.MicaOverlays, 0, 100);
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
