using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;

namespace OkPlayer.App.Services;

/// <summary>Always-on crash/hang diagnostics. One plain-text log file per launch under
/// <c>%LOCALAPPDATA%\OkPlayer\logs</c>, flushed every line so the last action before a hard hang (where the
/// OS kills a frozen window) still reaches disk. Captures: an OS/version header, lifecycle breadcrumbs,
/// unhandled exceptions from every surface, mpv's own log messages, and a UI-thread-stall watchdog that
/// notices when the dispatcher stops pumping (the classic "the whole window froze" symptom). Every method is
/// best-effort and never throws — diagnostics must not become the failure they're meant to record.</summary>
public static class Log
{
    private static readonly object Gate = new();
    private static StreamWriter? _writer;
    private static string? _path;

    /// <summary>Absolute path of the current session's log file (null if the file couldn't be opened).</summary>
    public static string? FilePath => _path;

    /// <summary>The directory logs are written to — surfaced so the UI can offer "open log folder".</summary>
    public static string Directory { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "OkPlayer", "logs");

    // The most recent breadcrumb, kept so the watchdog (and a crash) can name "what we were doing" even when
    // nothing threw — for a pure hang this is the single most useful field in the file.
    private static volatile string _lastStep = "(startup)";
    public static string LastStep => _lastStep;

    /// <summary>Open the per-launch file and write the environment header. Call once, as early as possible
    /// (before anything else can fault). Safe to call before <see cref="App"/> is constructed.</summary>
    public static void Init()
    {
        try
        {
            System.IO.Directory.CreateDirectory(Directory);
            // Include the PID so two processes that start in the same second (e.g. a second launch that
            // single-instance-redirects and exits) get distinct files instead of truncating each other's.
            string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss");
            _path = Path.Combine(Directory, $"okplayer-{stamp}-{Environment.ProcessId}.log");
            // FileShare.ReadWrite so the tester can open/copy the file while the app is still running (or hung).
            var fs = new FileStream(_path, FileMode.Create, FileAccess.Write, FileShare.ReadWrite);
            _writer = new StreamWriter(fs) { AutoFlush = true };
            Prune(keep: 10); // after creating ours, so the count stays bounded including this session
        }
        catch { /* logging must never break startup */ }

        Raw("==================== OK Player session ====================");
        Raw($"version  : {Safe(() => App.AppVersion)}+{Safe(() => App.GitSha)}");
        Raw($"os       : {Safe(() => RuntimeInformation.OSDescription)}  (Environment.OSVersion={Safe(() => Environment.OSVersion.Version.ToString())})");
        Raw($"arch     : process={Safe(() => RuntimeInformation.ProcessArchitecture.ToString())} os={Safe(() => RuntimeInformation.OSArchitecture.ToString())}");
        Raw($"runtime  : {Safe(() => RuntimeInformation.FrameworkDescription)}");
        Raw($"cpu      : {Safe(() => Environment.ProcessorCount.ToString())} logical cores");
        Raw($"pid      : {Safe(() => Environment.ProcessId.ToString())}");
        Raw($"logfile  : {_path}");
        Raw("==========================================================");
    }

    /// <summary>Wire the process-wide last-resort exception handlers (everything not caught by the XAML
    /// <c>Application.UnhandledException</c>). Call from the process entry point.</summary>
    public static void InstallGlobalHandlers()
    {
        try
        {
            AppDomain.CurrentDomain.UnhandledException += (_, e) =>
            {
                if (e.ExceptionObject is Exception ex) Exception("AppDomain.UnhandledException", ex);
                else Write("FATAL", $"AppDomain.UnhandledException: {e.ExceptionObject} (terminating={e.IsTerminating})");
            };
            TaskScheduler.UnobservedTaskException += (_, e) =>
            {
                Exception("TaskScheduler.UnobservedTaskException", e.Exception);
                e.SetObserved(); // already logged; don't escalate to a process kill
            };
        }
        catch { }
    }

    public static void Info(string msg) => Write("INFO", msg);
    public static void Warn(string msg) => Write("WARN", msg);
    public static void Error(string msg) => Write("ERROR", msg);

    /// <summary>A lifecycle breadcrumb: logged AND remembered as "the last thing we did", so a hang's last
    /// step (named by the watchdog and any later crash) points straight at the culprit.</summary>
    public static void Step(string msg)
    {
        _lastStep = msg;
        Write("STEP", msg);
    }

    public static void Exception(string where, Exception? ex) => Write("FATAL", $"{where}: {ex}");

    /// <summary>Route an mpv log message (from <c>MpvContext.LogMessageReceived</c>) into the file.</summary>
    public static void Mpv(string level, string prefix, string text)
        => Write("MPV", $"[{level}] {prefix}: {text.TrimEnd()}");

    private static void Write(string level, string msg)
    {
        string line = $"{Now()} [{level,-5}] (t{Environment.CurrentManagedThreadId,2}) {msg}";
        lock (Gate)
        {
            try { _writer?.WriteLine(line); } catch { }
            try { Debug.WriteLine("OKP " + line); } catch { }
        }
    }

    private static void Raw(string s)
    {
        lock (Gate)
        {
            try { _writer?.WriteLine(s); } catch { }
            try { Debug.WriteLine("OKP " + s); } catch { }
        }
    }

    private static string Now()
    {
        try { return DateTime.Now.ToString("HH:mm:ss.fff"); } catch { return "??:??:??.???"; }
    }

    private static string Safe(Func<string> f) { try { return f() ?? "?"; } catch { return "?"; } }

    private static void Prune(int keep)
    {
        try
        {
            var files = new DirectoryInfo(Directory).GetFiles("okplayer-*.log");
            if (files.Length <= keep) return;
            Array.Sort(files, (a, b) => b.LastWriteTimeUtc.CompareTo(a.LastWriteTimeUtc));
            for (int i = keep; i < files.Length; i++)
                try { files[i].Delete(); } catch { }
        }
        catch { }
    }

    // ---- UI-thread stall watchdog -------------------------------------------------------------------------

    private static Timer? _watchdog;
    private static long _heartbeat;

    /// <summary>Start watching the UI thread for the "everything froze" symptom: every ~2s the watchdog asks the
    /// dispatcher to bump a heartbeat; if it stops advancing, the UI thread isn't pumping and we log a stall
    /// (with the last breadcrumb) — the only way to capture a pure deadlock, which leaves no exception.</summary>
    public static void StartUiWatchdog(DispatcherQueue ui)
    {
        if (ui is null) return;
        long last = Interlocked.Read(ref _heartbeat);
        int misses = 0;
        bool stalled = false;
        try
        {
            _watchdog = new Timer(_ =>
            {
                try
                {
                    long now = Interlocked.Read(ref _heartbeat);
                    bool advanced = now != last;
                    last = now;
                    bool queued = ui.TryEnqueue(() => Interlocked.Increment(ref _heartbeat));
                    if (advanced || !queued)
                    {
                        if (stalled && advanced) { stalled = false; Warn($"UI thread RECOVERED after ~{misses * 2}s; last step: {_lastStep}"); }
                        misses = 0;
                        return;
                    }
                    misses++;
                    if (misses == 3 && !stalled) // ~6s of no pumping
                    {
                        stalled = true;
                        Error($"UI THREAD STALLED >= 6s (window not responding) — last step: {_lastStep}");
                    }
                    else if (stalled && misses % 5 == 0) // re-log every ~10s it stays stuck
                    {
                        Error($"UI THREAD STILL STALLED ~{misses * 2}s — last step: {_lastStep}");
                    }
                }
                catch { }
            }, null, 2000, 2000);
        }
        catch { }
    }
}
