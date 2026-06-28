using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using OkPlayer.App.Services;

namespace OkPlayer.App;

/// <summary>Application entry point. The generated Main bootstraps the Windows App SDK runtime
/// (unpackaged) before this type is constructed.</summary>
public partial class App : Application
{
    // The single main window, plus the cross-thread handoff for redirected launches. OnLaunched (UI thread)
    // builds the window; OnRedirectedActivation (a background thread) may fire before that. The lock makes the
    // two rendezvous: a redirect that loses the race stashes its file, and OnLaunched opens it once the window
    // exists — so a file double-clicked during startup is never silently dropped.
    private readonly object _redirectLock = new();
    private MainWindow? _mainWindow;        // guarded by _redirectLock; null until the window is built
    private string? _pendingRedirectFile;   // a redirect's file that arrived before the window existed

    /// <summary>The one shared user-settings instance (single source of truth across all windows).</summary>
    public static SettingsService Settings { get; } = new();

    /// <summary>The one shared watch-history instance (single source of truth across all windows), so a
    /// "Clear history" from Settings is reflected by the player's recents without a stale second copy.</summary>
    public static HistoryService History { get; } = new();

    /// <summary>The auto-update service (Velopack). A background check runs once on launch; it no-ops on dev and
    /// portable builds (only a Velopack-installed build can update itself). Shared so Settings → About can drive
    /// a manual check / restart.</summary>
    public static UpdateService Updates { get; } = new();

    /// <summary>The release channel this build updates from — every build is a pre-1.0 beta for now, so the feed
    /// is the repo's GitHub pre-releases. Surfaced in Settings → About.</summary>
    public const string ReleaseChannel = "beta";

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
        // Last-resort XAML exception sink — log it, but don't change crash behaviour (leave Handled=false).
        UnhandledException += (_, e) => Log.Exception("App.UnhandledException", e.Exception);
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Log.Step("App.OnLaunched: start");
        // Point the render layer's diagnostic sinks at the file logger BEFORE the video panel is created, so a
        // hang inside GL/D3D/libmpv init (which throws nothing) is still pinned to its last breadcrumb, and
        // mpv's own warnings land in the same file.
        OkPlayer.Render.MpvVideoPanel.Diagnostics = Log.Step;
        OkPlayer.Render.MpvVideoPanel.MpvLogMessage = Log.Mpv;
        // apply engine-init settings before the video panel is created
        OkPlayer.Render.MpvVideoPanel.HardwareDecoding = Settings.Current.HardwareDecoding;
        Log.Step($"settings: hwdec={Settings.Current.HardwareDecoding}, theme={Settings.Current.Theme}");
        History.PruneOlderThan(Settings.Current.HistoryRetentionDays); // honour the retention window on launch
        // Resolve the accent (teal or the live Windows system accent) into the brushes before first render.
        AccentManager.Initialize();
        Settings.Changed += AccentManager.Apply; // re-apply when the accent source is toggled in Settings
        Log.Step("GetLaunchTarget (File.Exists on argv)");
        var (file, resume, sub, audio) = GetLaunchTarget();
        Log.Step($"launch target: file={(file is not null)}, resume={(resume is not null)}");
        Log.Step("MainWindow ctor: start");
        var mw = new MainWindow(file, resume, sub, audio);
        Log.Step("MainWindow ctor: done");
        // Publish the window and drain any redirect that raced ahead of it, atomically under the lock so a
        // redirect can't slip a file in after we read the stash but before the window is visible.
        string? pending;
        lock (_redirectLock)
        {
            _mainWindow = mw;
            pending = _pendingRedirectFile;
            _pendingRedirectFile = null;
        }
        mw.Activate();
        Log.Step("MainWindow activated");
        Log.StartUiWatchdog(mw.DispatcherQueue); // watch for the "window froze" symptom from here on
        if (pending is not null)
            mw.DispatcherQueue.TryEnqueue(() => mw.OpenFileFromRedirect(pending));
        // Fire-and-forget background update check: ask GitHub whether a newer release exists and pre-download it
        // (off the UI thread, failure-silent). Gated on the user preference; also a no-op on dev/portable builds
        // and until the Velopack feed is live.
        if (Settings.Current.AutoCheckUpdates)
            _ = Updates.CheckAndDownloadAsync();
        // If a Velopack update moved the install path, repoint the file-type association command at this exe so a
        // double-click still launches the current build. Off the UI thread (registry I/O); logged; no-op when the
        // user has no associations registered or the path already matches.
        System.Threading.Tasks.Task.Run(() =>
        {
            try
            {
                if (new FileAssociationService().RefreshCommandIfStale())
                    Log.Info("file associations: open-command refreshed to the current exe path");
            }
            catch (Exception ex) { Log.Warn("file associations: refresh failed: " + ex.Message); }
        });
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
                // A URL or a network path (UNC / mapped drive) is accepted WITHOUT File.Exists: statting a dead
                // SMB mount on the UI thread here, before the window exists, freezes startup for the full SMB
                // timeout (~60s). Hand the path to libmpv, which opens it off-thread and reports any failure.
                if (f.Contains("://", StringComparison.Ordinal) || OkPlayer.Core.NetworkPath.IsNetwork(f) || File.Exists(f))
                    return (f, resume, sub, audio);
        }
        catch { /* never let argv parsing block startup */ }
        return (null, null, null, null);
    }

    /// <summary>A second launch redirected its activation here (single instance — see <see cref="Program"/>).
    /// Open the file it carried in this already-running instance and bring the window forward; if it carried no
    /// file, just surface the window. The Activated event runs on a background thread, so marshal to the
    /// window's UI thread before touching it.</summary>
    public void OnRedirectedActivation(AppActivationArguments args)
    {
        string? file = ExtractLaunchFile(args);
        MainWindow? mw;
        lock (_redirectLock)
        {
            mw = _mainWindow;
            if (mw is null)
            {
                // The window isn't built yet — a second launch raced startup. Stash the file so OnLaunched
                // opens it once the window exists; otherwise this redirect returns success and the file is lost.
                // A bare bring-to-front (no file) needs no stash: the window is about to Activate() regardless.
                if (file is not null)
                    _pendingRedirectFile = file;
                return;
            }
        }
        mw.DispatcherQueue.TryEnqueue(() =>
        {
            if (file is not null)
                mw.OpenFileFromRedirect(file);
            else
                mw.BringToForeground();
        });
    }

    /// <summary>Pull the first openable file/URL out of a redirected launch's command line. Unpackaged apps get
    /// the command line on the AppLifecycle Launch arguments (unlike the empty XAML OnLaunched args), so we
    /// tokenize it and reuse <see cref="OkPlayer.Core.LaunchArgs"/> exactly like startup. The running exe's own
    /// path is dropped first so it can't be mistaken for the target file.</summary>
    private static string? ExtractLaunchFile(AppActivationArguments args)
    {
        try
        {
            if (args.Data is not Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs launch)
                return null;
            string self = Environment.ProcessPath ?? string.Empty;
            string[] rest = Tokenize(launch.Arguments)
                .Where(t => !string.Equals(t, self, StringComparison.OrdinalIgnoreCase))
                .ToArray();
            var (files, _, _, _) = OkPlayer.Core.LaunchArgs.Parse(rest);
            foreach (string f in files)
                if (f.Contains("://", StringComparison.Ordinal) || File.Exists(f))
                    return f;
        }
        catch { /* malformed args -> just surface the window */ }
        return null;
    }

    /// <summary>Split a raw Win32 command-line string into argv via CommandLineToArgvW (correct quote/space
    /// handling), so a path containing spaces survives intact.</summary>
    private static string[] Tokenize(string commandLine)
    {
        if (string.IsNullOrEmpty(commandLine))
            return Array.Empty<string>();
        IntPtr argv = CommandLineToArgvW(commandLine, out int argc);
        if (argv == IntPtr.Zero)
            return Array.Empty<string>();
        try
        {
            var result = new string[argc];
            for (int i = 0; i < argc; i++)
                result[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(argv, i * IntPtr.Size)) ?? string.Empty;
            return result;
        }
        finally { LocalFree(argv); }
    }

    [DllImport("shell32.dll", SetLastError = true)]
    private static extern IntPtr CommandLineToArgvW([MarshalAs(UnmanagedType.LPWStr)] string lpCmdLine, out int pNumArgs);
    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr hMem);
}
