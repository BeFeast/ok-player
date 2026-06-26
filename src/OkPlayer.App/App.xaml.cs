using System;
using System.IO;
using System.Reflection;
using Microsoft.UI.Xaml;
using OkPlayer.App.Services;

namespace OkPlayer.App;

/// <summary>Application entry point. The generated Main bootstraps the Windows App SDK runtime
/// (unpackaged) before this type is constructed.</summary>
public partial class App : Application
{
    private Window? _window;

    /// <summary>The one shared user-settings instance (single source of truth across all windows).</summary>
    public static SettingsService Settings { get; } = new();

    /// <summary>The one shared watch-history instance (single source of truth across all windows), so a
    /// "Clear history" from Settings is reflected by the player's recents without a stale second copy.</summary>
    public static HistoryService History { get; } = new();

    /// <summary>App version as Major.Minor.Build, read from the assembly (single-sourced from the csproj
    /// <c>&lt;Version&gt;</c>). Shown in Settings → Advanced (About) and the Settings nav-rail footer.</summary>
    public static string AppVersion { get; } = GetAppVersion();

    /// <summary>libmpv's human version string (e.g. "mpv 0.39.0"), captured off-thread when the engine
    /// attaches; null until then. Cosmetic — read by the Settings About block. Setting it raises
    /// <see cref="MpvVersionChanged"/> so an already-open Settings window can refresh its engine line.</summary>
    public static string? MpvVersion
    {
        get => _mpvVersion;
        set { _mpvVersion = value; MpvVersionChanged?.Invoke(); }
    }
    private static string? _mpvVersion;

    /// <summary>Raised when <see cref="MpvVersion"/> is set — which happens off the UI thread at engine
    /// attach, possibly after a Settings window is already open. Handlers must marshal to their own
    /// dispatcher before touching UI.</summary>
    public static event Action? MpvVersionChanged;

    private static string GetAppVersion()
    {
        try
        {
            var v = typeof(App).Assembly.GetName().Version;
            return v is null ? string.Empty : $"{v.Major}.{v.Minor}.{v.Build}";
        }
        catch { return string.Empty; }
    }

    /// <summary>Short git SHA the build was produced from (e.g. "ab12cd3", or "ab12cd3-dirty" when the working
    /// tree had uncommitted changes); empty when built outside a git checkout. Parsed from the build-metadata
    /// suffix of AssemblyInformationalVersion, which the csproj StampGitShaRevision target stamps. Shown in
    /// Settings → Advanced (About) so a stale build or a build off the wrong branch is obvious — the dirty-build
    /// failure mode this guards against.</summary>
    public static string GitSha { get; } = GetGitSha();

    private static string GetGitSha()
    {
        try
        {
            string? info = typeof(App).Assembly
                .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;
            int plus = info?.IndexOf('+') ?? -1;
            return plus >= 0 ? info![(plus + 1)..] : string.Empty;
        }
        catch { return string.Empty; }
    }

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // apply engine-init settings before the video panel is created
        OkPlayer.Render.MpvVideoPanel.HardwareDecoding = Settings.Current.HardwareDecoding;
        History.PruneOlderThan(Settings.Current.HistoryRetentionDays); // honour the retention window on launch
        var (file, resume, sub, audio) = GetLaunchTarget();
        _window = new MainWindow(file, resume, sub, audio);
        _window.Activate();
    }

    /// <summary>A file/URL passed on the command line (Explorer "Open with", a file association, or a
    /// companion-library launch `OkPlayer.exe path --resume &lt;seconds&gt; [--sub N] [--audio N]` per PRD
    /// §13.1). Unpackaged apps receive argv on the process command line, not the activation args. Returns the
    /// first positional that is a URL or an existing file, plus the explicit resume position and the
    /// subtitle/audio track preselection (each null when absent/malformed).</summary>
    private static (string? File, double? Resume, int? Sub, int? Audio) GetLaunchTarget()
    {
        try
        {
            string[] argv = Environment.GetCommandLineArgs();
            var (files, resume, sub, audio) = OkPlayer.Core.LaunchArgs.Parse(argv.Length > 1 ? argv[1..] : Array.Empty<string>());
            foreach (string f in files)
                if (f.Contains("://", StringComparison.Ordinal) || File.Exists(f))
                    return (f, resume, sub, audio);
        }
        catch { /* never let argv parsing block startup */ }
        return (null, null, null, null);
    }
}
