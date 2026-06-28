using System;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.Win32;
using OkPlayer.App.Services;
using OkPlayer.App.ViewModels;
using Windows.ApplicationModel.DataTransfer;
using Windows.UI;

namespace OkPlayer.App;

/// <summary>The Settings window — a left nav-rail + content pane over Mica (design band 9).
/// Appearance is the fully-built panel; the others are reserved slots for now.</summary>
public sealed partial class SettingsWindow : Window
{
    private static readonly string[] PanelNames =
        { "Appearance", "Playback", "Subtitles", "Video", "Audio", "Shortcuts", "Integration", "Advanced", "About" };

    private bool _loaded;

    // Reads the live Windows personalization accent so the "System accent" card can preview the real colour.
    private readonly Windows.UI.ViewManagement.UISettings _ui = new();

    /// <summary>Opens Settings on <paramref name="initialPanel"/> (by display name, e.g. "Subtitles"), or on
    /// About when null/unknown — About is the default landing page. The name argument is the deep-link seam:
    /// a caller that wants to jump straight to a specific panel passes its name.</summary>
    public SettingsWindow(string? initialPanel = null)
    {
        InitializeComponent();
        Title = "Settings";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        ResizeForDpi();
        ApplyTheme();
        App.Settings.Changed += ApplyTheme;
        App.MpvVersionChanged += OnMpvVersionChanged;
        if (Content is FrameworkElement rootEl)
            rootEl.ActualThemeChanged += OnActualThemeChanged;
        Closed += (_, _) =>
        {
            App.Settings.Changed -= ApplyTheme;
            App.MpvVersionChanged -= OnMpvVersionChanged;
            _copyStatusTimer?.Stop(); // don't let a pending copy-status tick write to a torn-down element
            if (Content is FrameworkElement r)
                r.ActualThemeChanged -= OnActualThemeChanged;
        };
        MpvOptionsList.ItemsSource = MpvOptions; // the rows mutate in place; the collection notifies the list
        LoadAppearance();
        ShowVersion();
        _loaded = true;
        SelectInitialPanel(initialPanel); // land on About by default (or the deep-linked panel)
    }

    [DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr hwnd);

    /// <summary>Size the window in LOGICAL pixels. The design's 760×560 is logical, but
    /// <see cref="Microsoft.UI.Windowing.AppWindow.Resize"/> takes physical pixels, so an unscaled call comes
    /// out cramped at >100% display scaling. Scale by the live window DPI; fall back to the raw 760×560 if the
    /// DPI can't be read.</summary>
    private void ResizeForDpi()
    {
        int width = 760, height = 560;
        try
        {
            IntPtr hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            uint dpi = GetDpiForWindow(hwnd);
            if (dpi > 0)
            {
                double scale = dpi / 96.0;
                width = (int)Math.Round(760 * scale);
                height = (int)Math.Round(560 * scale);
            }
        }
        catch { /* DPI query failed — keep the safe unscaled logical size */ }
        AppWindow.Resize(new Windows.Graphics.SizeInt32(width, height));
    }

    /// <summary>Drive the initial rail selection (rather than just toggling visibility) so the chosen panel's
    /// load hook runs via <see cref="OnNavChanged"/>. Defaults to About; an unknown name also falls back to it.</summary>
    private void SelectInitialPanel(string? panelName)
    {
        int index = ResolvePanelIndex(panelName);
        if (index == 8)
            NavFooterList.SelectedIndex = 0; // About lives in the bottom-pinned footer list
        else
            NavList.SelectedIndex = index;
    }

    private static int ResolvePanelIndex(string? name)
    {
        if (!string.IsNullOrWhiteSpace(name))
        {
            int i = Array.FindIndex(PanelNames, p => string.Equals(p, name, StringComparison.OrdinalIgnoreCase));
            if (i >= 0)
                return i;
        }
        return 8; // About — the default landing panel
    }

    /// <summary>The Advanced editor rows — one per mpv.conf option. <see cref="LoadMpvConf"/> repopulates it
    /// from disk; the add/remove handlers mutate it; <see cref="OnMpvConfSave"/> serialises it back.</summary>
    public ObservableCollection<MpvOptionRow> MpvOptions { get; } = new();

    /// <summary>Fill the About spec sheet (a static snapshot). The rail has no version footer — the version
    /// lives in the About hero chip and the App card.</summary>
    private void ShowVersion()
    {
        PopulateAbout();
    }

    private const string Dash = "—"; // em dash: the honest "no value" placeholder for an unavailable fact

    /// <summary>Fill the About spec sheet from cheap, locally-available facts, read once as a static snapshot.
    /// The libmpv line can still be unknown here (it lands at engine attach), so it is also refreshed by
    /// <see cref="RefreshEngineVersion"/> on About-shown and on <see cref="App.MpvVersionChanged"/>. The
    /// stale-build tag fills in asynchronously (see <see cref="StartStaleCheck"/>).</summary>
    private void PopulateAbout()
    {
        string version = App.AppVersion;
        AboutHeroVersionChip.Text = string.IsNullOrEmpty(version) ? Dash : version;
        AboutAppVersion.Text = string.IsNullOrEmpty(version) ? Dash : version;

        // Built commit's short SHA. Empty outside a git build → an em dash with no state tag. A "-dirty" suffix
        // is kept verbatim (uncommitted build) and skips the stale check (see StartStaleCheck).
        string sha = App.GitSha;
        AboutBuildSha.Text = string.IsNullOrEmpty(sha) ? Dash : sha;

        // Hardware decode + the render path. hwdec reflects the persisted preference and is re-read on
        // About-shown (RefreshHwdec) because the Video panel can toggle it while this window is open — a
        // one-time snapshot would go stale. The graphics backend is the app's fixed render path.
        RefreshHwdec();
        AboutEngineGraphics.Text = GraphicsBackend;

        // Host facts.
        AboutHostWindows.Text = WindowsBuildString();
        AboutHostDotnet.Text = DotNetVersionString();
        AboutHostAppSdk.Text = WindowsAppSdkVersionString();
        AboutHostCpu.Text = RuntimeInformation.ProcessArchitecture.ToString().ToLowerInvariant();

        RefreshEngineVersion();
    }

    // The real render path: native desktop OpenGL shared into D3D11 via WGL_NV_DX_interop — no ANGLE (see
    // OkPlayer.Render). Single-sourced here so the Engine card and the copied diagnostics can't disagree.
    private const string GraphicsBackend = "OpenGL · WGL_NV_DX_interop → D3D11";

    /// <summary>Reflect the persisted hardware-decoding preference in the Engine card: on → method + accent
    /// "ON" tag, off → software with no tag. Re-read on About-shown (not just once at populate) because the
    /// Video panel can toggle the setting in the same window, which would otherwise leave a stale snapshot.</summary>
    private void RefreshHwdec()
    {
        if (AboutEngineHwdec is null)
            return; // the spec sheet isn't realised yet
        bool hw = App.Settings.Current.HardwareDecoding;
        AboutEngineHwdec.Text = HwdecMethod(hw);
        AboutHwdecTag.Visibility = hw ? Visibility.Visible : Visibility.Collapsed;
    }

    // The decode-method label paired with the on/off state, derived from the same bool — so the card and the
    // copied diagnostics can never disagree (the old "off · d3d11va" mismatch).
    private static string HwdecMethod(bool hardwareDecoding) => hardwareDecoding ? "d3d11va" : "software";

    // App.MpvVersion is captured off the UI thread at engine attach, which can land after this window is
    // already open and even already sitting on About. Marshal to this window and refresh the About engine
    // line so it surfaces immediately, instead of waiting for the user to leave and re-enter the panel.
    private void OnMpvVersionChanged() => DispatcherQueue?.TryEnqueue(RefreshEngineVersion);

    /// <summary>Show the libmpv engine version when known, else an em dash. The engine attaches — and the
    /// off-thread <c>mpv-version</c> read completes — only after media starts playing, so this is re-run when
    /// the About panel is shown and on <see cref="App.MpvVersionChanged"/>. The "mpv " prefix is stripped so
    /// the value cell shows just the number.</summary>
    private void RefreshEngineVersion()
    {
        if (AboutEngineMpv is null)
            return; // the spec sheet isn't realised yet (called before InitializeComponent completes)
        string? mpv = App.MpvVersion;
        AboutEngineMpv.Text = string.IsNullOrWhiteSpace(mpv) ? Dash : StripMpvPrefix(mpv);
    }

    private static string StripMpvPrefix(string s)
    {
        s = s.Trim();
        const string prefix = "mpv ";
        return s.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) ? s[prefix.Length..].Trim() : s;
    }

    /// <summary>The Windows build, with the UBR revision appended when the registry exposes it
    /// (e.g. "26100.1742"); degrades to the bare build number if the value can't be read.</summary>
    private static string WindowsBuildString()
    {
        int build = Environment.OSVersion.Version.Build;
        try
        {
            using var key = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
            if (key?.GetValue("UBR") is int ubr)
                return $"{build}.{ubr}";
        }
        catch { /* registry read blocked — fall back to the bare build number */ }
        return build.ToString();
    }

    /// <summary>The running .NET runtime version, e.g. ".NET 9.0.6" → "9.0.6"; the raw description otherwise.</summary>
    private static string DotNetVersionString()
    {
        string d = RuntimeInformation.FrameworkDescription.Trim();
        const string prefix = ".NET ";
        return d.StartsWith(prefix, StringComparison.Ordinal) ? d[prefix.Length..].Trim() : d;
    }

    /// <summary>The Windows App SDK / WinUI 3 version, read from the loaded Microsoft.WinUI assembly (file
    /// version preferred, assembly version as a fallback). An em dash if neither is readable.</summary>
    private static string WindowsAppSdkVersionString()
    {
        try
        {
            System.Reflection.Assembly asm = typeof(Microsoft.UI.Xaml.Application).Assembly;
            string location = asm.Location;
            if (!string.IsNullOrEmpty(location))
            {
                string? fileVersion = System.Diagnostics.FileVersionInfo.GetVersionInfo(location).FileVersion;
                if (!string.IsNullOrWhiteSpace(fileVersion))
                    return fileVersion.Trim();
            }
            Version? v = asm.GetName().Version;
            if (v is not null && v != new Version(0, 0, 0, 0))
                return v.ToString();
        }
        catch { /* unreadable assembly metadata — fall back to the em dash */ }
        return Dash;
    }

    // ── About: diagnostics copy ────────────────────────────────────────

    private void OnCopyDiagnostics(object sender, RoutedEventArgs e)
    {
        try
        {
            var data = new DataPackage { RequestedOperation = DataPackageOperation.Copy };
            data.SetText(BuildDiagnosticsText());
            Clipboard.SetContent(data);
            ShowCopyStatus("Diagnostics copied");
        }
        catch
        {
            ShowCopyStatus("Couldn't copy");
        }
    }

    /// <summary>The whole sheet as plain text (the README diagnostics format), built from the same live
    /// values shown in the cards.</summary>
    private string BuildDiagnosticsText()
    {
        string ver = string.IsNullOrEmpty(App.AppVersion) ? Dash : App.AppVersion;
        string buildLine = $"Build {AboutBuildSha.Text}";

        bool hw = App.Settings.Current.HardwareDecoding;
        string hwLine = $"{(hw ? "on" : "off")} · {HwdecMethod(hw)}"; // both from the live bool — never "off · d3d11va"

        var sb = new System.Text.StringBuilder();
        sb.Append("OK Player ").Append(ver).Append(" (Stable)\n");
        sb.Append(buildLine).Append('\n');
        sb.Append("License GPL-3.0-or-later\n\n");
        sb.Append("Engine\n");
        AppendDiagLine(sb, "libmpv", AboutEngineMpv.Text);
        AppendDiagLine(sb, "FFmpeg", Dash);
        AppendDiagLine(sb, "Render API", "libmpv render");
        AppendDiagLine(sb, "Graphics", GraphicsBackend);
        AppendDiagLine(sb, "Hardware decode", hwLine);
        sb.Append("\nHost\n");
        AppendDiagLine(sb, "Windows 11", AboutHostWindows.Text);
        AppendDiagLine(sb, ".NET", AboutHostDotnet.Text);
        AppendDiagLine(sb, "Windows App SDK", AboutHostAppSdk.Text);
        AppendDiagLine(sb, "CPU", AboutHostCpu.Text);
        AppendDiagLine(sb, "GPU", Dash);
        return sb.ToString();
    }

    private static void AppendDiagLine(System.Text.StringBuilder sb, string label, string value)
        => sb.Append("  ").Append(label.PadRight(17)).Append(value).Append('\n');

    private Microsoft.UI.Xaml.DispatcherTimer? _copyStatusTimer;

    private void ShowCopyStatus(string text)
    {
        AboutCopyStatus.Text = text;
        _copyStatusTimer ??= CreateCopyStatusTimer();
        _copyStatusTimer.Stop();
        _copyStatusTimer.Start();
    }

    private Microsoft.UI.Xaml.DispatcherTimer CreateCopyStatusTimer()
    {
        var t = new Microsoft.UI.Xaml.DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
        t.Tick += (_, _) => { t.Stop(); AboutCopyStatus.Text = string.Empty; };
        return t;
    }

    private void ApplyTheme()
    {
        if (Content is FrameworkElement root)
            root.RequestedTheme = ThemeFor(App.Settings.Current.Theme);
    }

    /// <summary>Map the persisted theme preference to an <see cref="ElementTheme"/>: explicit Light/Dark, or
    /// Default ("Auto" — follow the system) for anything else. Shared by the player window.</summary>
    internal static ElementTheme ThemeFor(string theme) => theme switch
    {
        "Light" => ElementTheme.Light,
        "Dark" => ElementTheme.Dark,
        _ => ElementTheme.Default,
    };

    // The segment pills and the Shortcuts key chips bake theme-dependent colors when they are built (the
    // chips only once). ActualThemeChanged fires whenever the effective theme flips — by setting change or
    // a system light/dark switch while on Auto — and the new theme is already in effect, so rebuild the
    // chips and re-style the visible panel here so their contrast tracks the theme.
    private void OnActualThemeChanged(FrameworkElement sender, object args)
    {
        if (!_loaded)
            return;
        // Re-style only the theme-dependent visuals — never reload panel data (reloading Advanced would
        // discard unsaved mpv.conf edits). The key chips bake the theme when built, so drop them and let
        // them rebuild (now if visible, else on next show); the selected segment pill is re-styled in place.
        _shortcutsBuilt = false;
        ShortcutsHost.Children.Clear();
        if (AppearancePanel.Visibility == Visibility.Visible)
            RefreshAppearance(); // the theme/accent pills are themed too — and this panel hosts the switch
        else if (ShortcutsPanel.Visibility == Visibility.Visible)
            LoadShortcuts();
        else if (PlaybackPanel.Visibility == Visibility.Visible)
            RefreshPlayback();
        else if (SubtitlesPanel.Visibility == Visibility.Visible)
            LoadSubtitles();
        else if (VideoPanel.Visibility == Visibility.Visible)
            LoadVideo();
        else if (AudioPanel.Visibility == Visibility.Visible)
            LoadAudio();
    }

    private bool _navSyncing; // guards the cross-list deselect from re-entering this handler

    // The rail is split: the main list holds Appearance..Advanced, and About is a bottom-pinned one-item
    // footer list. Selecting in one clears the other so only one row is ever highlighted. Index 8 == About.
    private void OnNavChanged(object sender, SelectionChangedEventArgs e)
    {
        if (AppearancePanel is null) // SelectedIndex=0 fires during InitializeComponent, before the pane exists
            return;
        if (_navSyncing)
            return; // a deselection we just triggered on the other list — ignore it
        int i;
        if (ReferenceEquals(sender, NavFooterList))
        {
            if (NavFooterList.SelectedIndex < 0)
                return;
            i = 8; // About
            _navSyncing = true; NavList.SelectedIndex = -1; _navSyncing = false;
        }
        else
        {
            if (NavList.SelectedIndex < 0)
                return;
            i = NavList.SelectedIndex;
            _navSyncing = true; NavFooterList.SelectedIndex = -1; _navSyncing = false;
        }
        ShowPanel(i);
    }

    private void ShowPanel(int i)
    {
        bool appearance = i == 0;
        bool playback = i == 1;
        bool subtitles = i == 2;
        bool video = i == 3;
        bool audio = i == 4;
        bool shortcuts = i == 5;
        bool integration = i == 6;
        bool advanced = i == 7;
        bool about = i == 8;
        AppearancePanel.Visibility = appearance ? Visibility.Visible : Visibility.Collapsed;
        PlaybackPanel.Visibility = playback ? Visibility.Visible : Visibility.Collapsed;
        SubtitlesPanel.Visibility = subtitles ? Visibility.Visible : Visibility.Collapsed;
        VideoPanel.Visibility = video ? Visibility.Visible : Visibility.Collapsed;
        AudioPanel.Visibility = audio ? Visibility.Visible : Visibility.Collapsed;
        ShortcutsPanel.Visibility = shortcuts ? Visibility.Visible : Visibility.Collapsed;
        IntegrationPanel.Visibility = integration ? Visibility.Visible : Visibility.Collapsed;
        AdvancedPanel.Visibility = advanced ? Visibility.Visible : Visibility.Collapsed;
        AboutPanel.Visibility = about ? Visibility.Visible : Visibility.Collapsed;
        if (advanced)
        {
            LoadMpvConf();
        }
        else if (about)
        {
            RefreshHwdec();         // the Video panel can toggle hwdec while this window is open — re-read it
            RefreshEngineVersion(); // engine version may have been captured after this window opened
        }
        else if (integration)
            LoadIntegration();
        else if (playback)
            LoadPlayback();
        else if (subtitles)
            LoadSubtitles();
        else if (video)
            LoadVideo();
        else if (audio)
            LoadAudio();
        else if (shortcuts)
            LoadShortcuts();
    }

    // ── Shortcuts panel (keyboard reference) ───────────────────────────

    private bool _shortcutsBuilt;

    private void LoadShortcuts()
    {
        if (_shortcutsBuilt)
            return;
        (string Cat, string Action, string[] Keys)[] map =
        {
            ("PLAYBACK", "Play / pause", new[] { "Space", "K" }),
            ("PLAYBACK", "Seek backward / forward", new[] { "←", "→" }),
            ("PLAYBACK", "Jump 10 seconds back / forward", new[] { "J", "L" }),
            ("PLAYBACK", "Frame step back / forward", new[] { ",", "." }),
            ("AUDIO", "Volume up / down", new[] { "↑", "↓" }),
            ("AUDIO", "Mute", new[] { "M" }),
            ("VIEW", "Fullscreen", new[] { "F" }),
            ("VIEW", "Chapters panel", new[] { "C" }),
            ("VIEW", "Media info", new[] { "I" }),
            ("VIEW", "Close panel / exit fullscreen", new[] { "Esc" }),
            ("CAPTURE", "Screenshot", new[] { "S" }),
        };
        string? lastCat = null;
        foreach (var (cat, action, keys) in map)
        {
            if (cat != lastCat)
            {
                ShortcutsHost.Children.Add(new TextBlock
                {
                    Text = cat,
                    FontSize = 12,
                    FontWeight = FontWeights.SemiBold,
                    CharacterSpacing = 60,
                    Foreground = Res("OkTextSecondaryBrush", new SolidColorBrush(Color.FromArgb(0x99, 0, 0, 0))),
                    Margin = new Thickness(0, lastCat is null ? 0 : 18, 0, 8),
                });
                lastCat = cat;
            }
            ShortcutsHost.Children.Add(BuildShortcutRow(action, keys));
        }
        _shortcutsBuilt = true;
    }

    private FrameworkElement BuildShortcutRow(string action, string[] keys)
    {
        var grid = new Grid { Margin = new Thickness(0, 0, 0, 3), MaxWidth = 440, HorizontalAlignment = HorizontalAlignment.Left };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.Children.Add(new TextBlock { Text = action, FontSize = 12.5, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(2, 6, 24, 6) });
        var chips = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 5, VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(chips, 1);
        foreach (string k in keys)
            chips.Children.Add(KeyChip(k));
        grid.Children.Add(chips);
        return grid;
    }

    private FrameworkElement KeyChip(string key) => new Border
    {
        // a faint dark fill on light, a faint light fill on dark — 5% black vanishes on dark Mica
        Background = new SolidColorBrush((Content as FrameworkElement)?.ActualTheme == ElementTheme.Dark
            ? Color.FromArgb(0x18, 0xFF, 0xFF, 0xFF) : Color.FromArgb(0x0D, 0, 0, 0)),
        BorderBrush = Res("OkStrokeBrush", new SolidColorBrush(Color.FromArgb(0x14, 0, 0, 0))),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(5),
        Padding = new Thickness(8, 3, 8, 3),
        MinWidth = 28,
        Child = new TextBlock
        {
            Text = key,
            FontSize = 11.5,
            FontFamily = new FontFamily("Consolas"),
            HorizontalAlignment = HorizontalAlignment.Center,
            Foreground = Res("OkTextBodyBrush", new SolidColorBrush(Color.FromArgb(0xDE, 0, 0, 0))),
        },
    };

    // ── Subtitles panel ────────────────────────────────────────────────

    private void LoadSubtitles()
    {
        var s = App.Settings.Current;
        StyleSegment(SubSmall, Math.Abs(s.SubtitleScale - 0.8) < 0.001);
        StyleSegment(SubNormal, Math.Abs(s.SubtitleScale - 1.0) < 0.001);
        StyleSegment(SubLarge, Math.Abs(s.SubtitleScale - 1.4) < 0.001);
        StyleSegment(SubBottom, s.SubtitlePosition == 100);
        StyleSegment(SubRaised, s.SubtitlePosition == 90);
        string style = OkPlayer.Core.SubtitleStyle.FromKey(s.SubtitleStyle).Key;
        StyleSegment(SubStyleDefault, style == "Default");
        StyleSegment(SubStyleBold, style == "Bold");
        StyleSegment(SubStyleClassic, style == "Classic");
        StyleSegment(SubStyleContrast, style == "Contrast");
    }

    private void OnSubSize(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string t } &&
            double.TryParse(t, System.Globalization.NumberStyles.Any, System.Globalization.CultureInfo.InvariantCulture, out double v))
        {
            App.Settings.Current.SubtitleScale = v;
            App.Settings.Save();
            LoadSubtitles();
        }
    }

    private void OnSubPos(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string t } && int.TryParse(t, out int v))
        {
            App.Settings.Current.SubtitlePosition = v;
            App.Settings.Save();
            LoadSubtitles();
        }
    }

    private void OnSubStyle(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string key })
        {
            // Normalize through FromKey so only a known preset key is ever persisted (an unknown Tag, or a
            // hand-edited settings value, collapses to Default rather than sticking an invalid key).
            App.Settings.Current.SubtitleStyle = OkPlayer.Core.SubtitleStyle.FromKey(key).Key;
            App.Settings.Save();
            LoadSubtitles();
        }
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

    private bool _audioReady; // suppress the Toggled that fires while we set the initial toggle state

    private void LoadAudio()
    {
        int v = App.Settings.Current.DefaultVolume;
        StyleSegment(Vol50, v == 50);
        StyleSegment(Vol75, v == 75);
        StyleSegment(Vol100, v == 100);
        _audioReady = false;
        NormalizeToggle.IsOn = App.Settings.Current.AudioNormalization;
        _audioReady = true;
    }

    private void OnNormalizeToggled(object sender, RoutedEventArgs e)
    {
        if (!_audioReady)
            return; // reflecting the persisted value, not a user change — don't re-save/re-apply
        App.Settings.Current.AudioNormalization = NormalizeToggle.IsOn;
        App.Settings.Save(); // raises Changed → the player applies/removes the audio filter live
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
        HidePausedToggle.Toggled -= OnHidePausedToggled;
        HidePausedToggle.IsOn = App.Settings.Current.HideControlsWhenPaused;
        HidePausedToggle.Toggled += OnHidePausedToggled;
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

    private void OnHidePausedToggled(object sender, RoutedEventArgs e)
    {
        App.Settings.Current.HideControlsWhenPaused = HidePausedToggle.IsOn;
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

    // Retention dropdown index → days kept (0 = forever). Order matches the ComboBoxItems in XAML.
    private static readonly int[] RetentionDays = { 0, 7, 30, 90, 365 };
    private bool _retentionReady; // suppress the SelectionChanged that fires while we set the initial index

    private void LoadIntegration()
    {
        // Reflect the persisted retention window without firing OnRetentionChanged.
        _retentionReady = false;
        int days = App.Settings.Current.HistoryRetentionDays;
        int idx = Array.IndexOf(RetentionDays, days);
        RetentionCombo.SelectedIndex = idx >= 0 ? idx : 0; // unknown value falls back to "Forever"
        _retentionReady = true;

        if (_assocBuilt)
        {
            RefreshAssocChecks();
            return;
        }
        BuildAssoc(AssocVideoPanel, VideoExts);
        BuildAssoc(AssocAudioPanel, AudioExts);
        _assocBuilt = true;
    }

    private void OnRetentionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_retentionReady)
            return;
        int idx = RetentionCombo.SelectedIndex;
        int days = idx >= 0 && idx < RetentionDays.Length ? RetentionDays[idx] : 0;
        App.Settings.Current.HistoryRetentionDays = days;
        App.Settings.Save();
        int pruned = App.History.PruneOlderThan(days); // apply the new window immediately
        HistoryStatus.Text = days == 0 ? "History kept forever"
            : pruned > 0 ? $"Removed {pruned} older item{(pruned == 1 ? "" : "s")}"
            : "Retention updated";
    }

    private async void OnClearHistory(object sender, RoutedEventArgs e)
    {
        var dialog = new ContentDialog
        {
            Title = "Clear watch history?",
            Content = "This removes all resume positions, Continue Watching entries, bookmarks and your added chapters. This can't be undone.",
            PrimaryButtonText = "Clear",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = Content.XamlRoot,
        };
        try
        {
            if (await dialog.ShowAsync() != ContentDialogResult.Primary)
                return;
        }
        catch { return; } // another dialog is already open — ignore the concurrent open
        int removed = App.History.Clear();
        HistoryStatus.Text = removed == 0 ? "History was already empty"
            : $"Cleared {removed} item{(removed == 1 ? "" : "s")}";
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

    private void OnAssocSelectAll(object sender, RoutedEventArgs e) => SetAllAssoc(true);
    private void OnAssocClearAll(object sender, RoutedEventArgs e) => SetAllAssoc(false);

    /// <summary>Assign or unassign OK Player for every listed type in one go, so the user doesn't tick (or untick)
    /// each extension by hand. Updates the registry once per type, ticks the boxes without re-firing the per-box
    /// handler, then notifies the shell a single time — including after a partial failure, since some writes may
    /// already have landed.</summary>
    private void SetAllAssoc(bool on)
    {
        try
        {
            foreach (Panel host in new[] { (Panel)AssocVideoPanel, AssocAudioPanel })
                foreach (var child in host.Children)
                    if (child is CheckBox { Tag: string ext } cb)
                    {
                        if (on) _assoc.Assign(ext); else _assoc.Unassign(ext);
                        cb.Checked -= OnAssocToggle;
                        cb.Unchecked -= OnAssocToggle;
                        cb.IsChecked = on;
                        cb.Checked += OnAssocToggle;
                        cb.Unchecked += OnAssocToggle;
                    }
            AssocStatus.Text = on ? "All types added" : "All types cleared";
        }
        catch
        {
            AssocStatus.Text = "Couldn't update";
            RefreshAssocChecks();
        }
        finally
        {
            // A partial bulk update still mutated the registry — refresh the shell regardless so Explorer /
            // Default Apps don't keep showing the pre-change state for the writes that did land.
            _assoc.NotifyShell();
        }
    }

    private void OnOpenDefaultApps(object sender, RoutedEventArgs e) => FileAssociationService.OpenWindowsDefaultApps();

    // ── Advanced panel (the raw-mpv-config escape hatch) ───────────────

    // Reloads each time the Advanced tab is shown (matching the old raw-TextBox behaviour); a theme restyle
    // deliberately does NOT call this, so it can't discard unsaved edits (see OnActualThemeChanged).
    private void LoadMpvConf()
    {
        MpvOptions.Clear();
        try
        {
            string path = OkPlayer.Render.MpvVideoPanel.UserConfigPath;
            string text = File.Exists(path) ? File.ReadAllText(path) : string.Empty;
            // Comments and blank lines are dropped — the editor is key/value only (acceptable for v1). An
            // empty/absent file yields no rows.
            foreach (MpvOption opt in MpvConfText.Parse(text))
                MpvOptions.Add(new MpvOptionRow(opt.Key, opt.Value));
        }
        catch { /* unreadable file — start from an empty editor rather than throw */ }
        MpvConfStatus.Text = string.Empty;
    }

    private void OnAddOption(object sender, RoutedEventArgs e)
    {
        // AutoFocus has the new row's key field grab focus once its container is realised (OnRowKeyLoaded);
        // the ItemsControl container isn't available synchronously right after the add.
        MpvOptions.Add(new MpvOptionRow { AutoFocus = true });
    }

    private void OnRemoveOption(object sender, RoutedEventArgs e)
    {
        if (sender is FrameworkElement { DataContext: MpvOptionRow row })
            MpvOptions.Remove(row);
    }

    // One-shot: only the just-added row (AutoFocus set) grabs focus; rows loaded from disk load with it clear.
    private void OnRowKeyLoaded(object sender, RoutedEventArgs e)
    {
        if (sender is TextBox { DataContext: MpvOptionRow { AutoFocus: true } row } box)
        {
            row.AutoFocus = false;
            box.Focus(FocusState.Programmatic);
        }
    }

    private void OnMpvConfSave(object sender, RoutedEventArgs e)
    {
        try
        {
            string path = OkPlayer.Render.MpvVideoPanel.UserConfigPath;
            // Don't persist protected options. The engine loader skips them anyway (they're flagged "ignored"
            // in the UI), so writing them back would leave the saved mpv.conf contradicting that hint and
            // mislead anyone hand-editing the file. Drop them here so the file only holds options that apply.
            string text = MpvConfText.Serialize(
                MpvOptions.Where(r => !OkPlayer.Render.MpvVideoPanel.IsProtectedOption(r.Key))
                          .Select(r => new MpvOption(r.Key, r.Value)));
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            WriteConfigAtomic(path, text);
            MpvConfStatus.Text = "Saved · restart to apply";
        }
        catch
        {
            MpvConfStatus.Text = "Couldn't save";
        }
    }

    // Write to a temp file then atomically replace, retrying across the brief shared locks Defender / the
    // Search indexer take on a freshly written file under %APPDATA% — the same transient-lock fix as
    // SettingsService.Save()/HistoryService.Save(), so a sharing violation can't truncate or drop the save.
    // After the retries the exception propagates to OnMpvConfSave, which reports "Couldn't save".
    private static void WriteConfigAtomic(string path, string text)
    {
        string tmp = path + ".tmp";
        for (int attempt = 0; ; attempt++)
        {
            try
            {
                File.WriteAllText(tmp, text);
                File.Move(tmp, path, overwrite: true); // replace in one step so a crash can't truncate the config
                return;
            }
            catch (Exception ex) when (attempt < 8 && ex is IOException or UnauthorizedAccessException)
            {
                System.Threading.Thread.Sleep(30); // up to ~240ms; the scanner's lock clears well within this
            }
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

    private void LoadAppearance() => RefreshAppearance();

    private void RefreshAppearance()
    {
        var s = App.Settings.Current;
        // "Auto" is anything that isn't an explicit Light/Dark — mirrors ThemeFor's default arm.
        StyleSegment(ThemeLightBtn, s.Theme == "Light");
        StyleSegment(ThemeDarkBtn, s.Theme == "Dark");
        StyleSegment(ThemeAutoBtn, s.Theme is not ("Light" or "Dark"));
        bool teal = s.AccentSource == "OkTeal";
        StyleCard(AccentTealBtn, teal);
        StyleCard(AccentSystemBtn, !teal);
        // Preview the real Windows accent on the System card (the teal card's swatch is a fixed teal in XAML),
        // so the two options read as distinct choices instead of two identical swatches.
        SystemAccentSwatch.Background = new SolidColorBrush(_ui.GetColorValue(Windows.UI.ViewManagement.UIColorType.Accent));
    }

    private void StyleSegment(Button b, bool selected)
    {
        // The selected "pill" must lift off the track in both themes: a light card on light Mica, a
        // translucent-white pill on dark (the system card fill is too faint there). Pair it with the
        // theme-aware accent text so the label stays readable (the light-mode dark teal is unreadable on dark).
        bool dark = b.ActualTheme == ElementTheme.Dark;
        b.Background = selected
            ? (dark ? new SolidColorBrush(Color.FromArgb(0x33, 0xFF, 0xFF, 0xFF))
                    : Res("CardBackgroundFillColorDefaultBrush", new SolidColorBrush(Colors.White)))
            : Transparent;
        b.Foreground = selected
            ? Res("OkAccentTextBrush", AccentText)
            : Res("OkTextBodyBrush", new SolidColorBrush(Color.FromArgb(0xDE, 0, 0, 0)));
        b.FontWeight = selected ? FontWeights.SemiBold : FontWeights.Normal;
    }

    private void StyleCard(Button b, bool selected)
    {
        // Use the live accent brushes so the selected card previews the chosen accent (system or teal),
        // not a hardcoded teal swatch.
        b.BorderBrush = selected ? Res("OkAccentBrush", Accent) : Res("OkStrokeBrush", new SolidColorBrush(Color.FromArgb(0x14, 0, 0, 0)));
        b.BorderThickness = new Thickness(selected ? 1.5 : 1);
        b.Background = selected ? Res("OkAccentTintBrush", AccentTint) : Transparent;
    }

    private void OnThemeLight(object sender, RoutedEventArgs e) => SetTheme("Light");
    private void OnThemeDark(object sender, RoutedEventArgs e) => SetTheme("Dark");
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

    private static readonly SolidColorBrush Transparent = new(Colors.Transparent);
    private static readonly SolidColorBrush Accent = new(Color.FromArgb(0xFF, 0x10, 0x93, 0x8A));
    private static readonly SolidColorBrush AccentText = new(Color.FromArgb(0xFF, 0x0A, 0x65, 0x5F));
    private static readonly SolidColorBrush AccentTint = new(Color.FromArgb(0x1A, 0x10, 0x93, 0x8A));

    /// <summary>Resolve a brush from the merged resources for the window's CURRENT theme. System brushes (e.g.
    /// CardBackgroundFillColorDefaultBrush) live flat and resolve directly; the design-system Ok* brushes live
    /// ONLY inside <c>ResourceDictionary.ThemeDictionaries</c> (Light/Dark/HighContrast), which a flat
    /// <c>TryGetValue</c> can't see — so for those we walk the merged dictionaries' theme dictionaries for the
    /// active ActualTheme. Without this, code-set Ok* foregrounds fell back to the light-only color and were
    /// near-black on dark (the "black text on black buttons" bug). The fallback is a last resort only.</summary>
    private Brush Res(string key, Brush fallback)
    {
        if (Application.Current.Resources.TryGetValue(key, out var v) && v is Brush flat)
            return flat;
        bool dark = (Content as FrameworkElement)?.ActualTheme == ElementTheme.Dark;
        return TryThemedBrush(Application.Current.Resources, key, dark ? "Dark" : "Light", out var themed)
            ? themed
            : fallback;
    }

    private static bool TryThemedBrush(ResourceDictionary dict, string key, string themeKey, out Brush brush)
    {
        brush = Transparent;
        if (dict.ThemeDictionaries.TryGetValue(themeKey, out var themeObj) && themeObj is ResourceDictionary themed
            && themed.TryGetValue(key, out var v) && v is Brush b)
        {
            brush = b;
            return true;
        }
        foreach (var md in dict.MergedDictionaries)
            if (TryThemedBrush(md, key, themeKey, out brush))
                return true;
        return false;
    }
}
