using System;
using System.IO;
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
