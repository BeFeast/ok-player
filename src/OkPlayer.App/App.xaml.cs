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

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // apply engine-init settings before the video panel is created
        OkPlayer.Render.MpvVideoPanel.HardwareDecoding = Settings.Current.HardwareDecoding;
        _window = new MainWindow(GetLaunchFile());
        _window.Activate();
    }

    /// <summary>A file/URL passed on the command line (Explorer "Open with", a file association, or
    /// `OkPlayer.exe path`). Unpackaged apps receive it via the process argv, not the activation args.</summary>
    private static string? GetLaunchFile()
    {
        try
        {
            string[] argv = Environment.GetCommandLineArgs();
            for (int i = 1; i < argv.Length; i++)
            {
                string a = argv[i];
                if (a.Length == 0 || a[0] == '-' || a[0] == '/')
                    continue; // skip switches
                if (a.Contains("://", StringComparison.Ordinal) || File.Exists(a))
                    return a;
            }
        }
        catch { /* never let argv parsing block startup */ }
        return null;
    }
}
