using System.IO;
using System.Text;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using OkPlayer.Mpv;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.App;

/// <summary>
/// Phase 0 engine spike: hosts the MpvVideoPanel, plays a synthetic libmpv source (lavfi testsrc,
/// no file needed), and dumps libmpv logs + key properties to a temp log so the render pipeline can
/// be verified headlessly. This becomes the real player surface in the App-shell phase.
/// </summary>
public sealed partial class MainWindow : Window
{
    private static readonly string LogPath = Path.Combine(Path.GetTempPath(), "okplayer-spike.log");
    private readonly object _logLock = new();
    private DispatcherQueueTimer? _timer;

    public MainWindow()
    {
        InitializeComponent();
        try { File.WriteAllText(LogPath, $"=== OK Player engine spike {System.DateTime.Now:O} ===\n"); } catch { }
        Log("MainWindow constructed");
        Video.EngineReady += OnEngineReady;
        Video.Loaded += (_, _) => Log($"Video.Loaded; size={Video.ActualWidth}x{Video.ActualHeight}");
    }

    private void OnEngineReady(object? sender, System.EventArgs e)
    {
        Log("EngineReady — engine + render context created");
        try
        {
            MpvContext mpv = Video.Engine!;
            mpv.LogMessageReceived += (lvl, prefix, text) => Log($"[mpv {lvl}] {prefix}: {text.TrimEnd()}");
            mpv.RequestLogMessages(MpvLogLevel.V);
            Log("mpv-version = " + (mpv.GetPropertyString("mpv-version") ?? "<null>"));

            Video.Open("av://lavfi:testsrc2=size=1280x720:rate=30:duration=120");
            Log("loadfile issued: av://lavfi:testsrc2");

            int ticks = 0;
            _timer = DispatcherQueue.CreateTimer();
            _timer.Interval = System.TimeSpan.FromSeconds(3);
            _timer.Tick += (_, _) =>
            {
                ticks++;
                DumpDiagnostics(mpv, ticks);
                if (ticks >= 3) _timer!.Stop();
            };
            _timer.Start();
        }
        catch (System.Exception ex)
        {
            Log("OnEngineReady EXCEPTION: " + ex);
            UpdateStatus("ENGINE ERROR: " + ex.Message);
        }
    }

    private void DumpDiagnostics(MpvContext mpv, int tick)
    {
        var props = new[]
        {
            "time-pos", "duration", "width", "height", "dwidth", "dheight",
            "container-fps", "estimated-vf-fps", "frame-drop-count", "video-codec",
            "hwdec-current", "video-format", "pause", "core-idle",
        };
        var sb = new StringBuilder();
        sb.AppendLine($"--- diagnostics tick {tick} ---");
        foreach (var p in props)
            sb.AppendLine($"  {p} = {mpv.GetPropertyString(p) ?? "<null>"}");
        Log(sb.ToString().TrimEnd());

        UpdateStatus($"t={mpv.GetPropertyString("time-pos")}  " +
                     $"{mpv.GetPropertyString("width")}x{mpv.GetPropertyString("height")}  " +
                     $"fps={mpv.GetPropertyString("estimated-vf-fps")}  " +
                     $"drops={mpv.GetPropertyString("frame-drop-count")}");
    }

    private void UpdateStatus(string text) => DispatcherQueue.TryEnqueue(() => Status.Text = text);

    private void Log(string line)
    {
        lock (_logLock)
        {
            try { File.AppendAllText(LogPath, line + "\n"); } catch { }
        }
    }
}
