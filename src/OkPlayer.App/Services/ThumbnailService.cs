using System;
using System.Globalization;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Mpv;

namespace OkPlayer.App.Services;

/// <summary>
/// Generates frame thumbnails on a dedicated decode-only libmpv instance (vo=null), so the main
/// playback engine is never disturbed. Each thumbnail is written to a temp PNG (named by its time
/// bucket, so repeats are cache hits) and returned as a file path the UI can load.
/// </summary>
public sealed class ThumbnailService : IDisposable
{
    private readonly string _tempDir;
    private readonly SemaphoreSlim _gate = new(1, 1); // serialize seeks: one frame in flight at a time
    private MpvContext? _mpv;
    private TaskCompletionSource<bool>? _loadTcs;
    private TaskCompletionSource<bool>? _restartTcs;
    private volatile bool _fileReady;
    private int _generation; // bumps per OpenAsync so stale in-flight requests can bail

    public ThumbnailService()
    {
        _tempDir = Path.Combine(Path.GetTempPath(), "OkPlayer", "thumbs");
        Directory.CreateDirectory(_tempDir);
    }

    /// <summary>Whether a file is loaded and ready to produce thumbnails.</summary>
    public bool IsReady => _fileReady;

    /// <summary>Point the thumbnail engine at a file. Safe to call repeatedly (replaces the prior file).</summary>
    public async Task<bool> OpenAsync(string path)
    {
        await _gate.WaitAsync().ConfigureAwait(false);
        try
        {
            int gen = ++_generation;
            TeardownEngine();
            ClearCache();

            var mpv = new MpvContext();
            mpv.SetOption("vo", "null");                 // decode only, no window
            mpv.SetOption("audio", "no");
            mpv.SetOption("hwdec", "no");                // CPU decode: reliable, no GPU contention
            mpv.SetOption("pause", "yes");
            mpv.SetOption("keep-open", "yes");
            mpv.SetOption("hr-seek", "yes");             // exact seeks
            mpv.SetOption("hr-seek-framedrop", "no");
            mpv.SetOption("vf", "scale=320:-2");         // small frames where the encoder honors it
            mpv.SetOption("ytdl", "no");
            mpv.SetOption("osc", "no");
            mpv.SetOption("input-default-bindings", "no");

            var loadTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            _loadTcs = loadTcs;
            mpv.FileLoaded += (_, _) => { _fileReady = true; loadTcs.TrySetResult(true); };
            mpv.EndFile += (_, _) => loadTcs.TrySetResult(_fileReady);
            mpv.PlaybackRestart += (_, _) => _restartTcs?.TrySetResult(true);

            mpv.Initialize();
            _mpv = mpv;
            mpv.Command("loadfile", path, "replace");

            bool ready = await WaitWithTimeout(loadTcs.Task, 10000).ConfigureAwait(false);
            return ready && gen == _generation;
        }
        catch
        {
            return false;
        }
        finally
        {
            _gate.Release();
        }
    }

    /// <summary>Produce (or reuse) a thumbnail near <paramref name="timeSeconds"/>; returns a PNG path or null.</summary>
    public async Task<string?> GetThumbnailAsync(double timeSeconds)
    {
        if (!_fileReady)
            return null;
        long bucketMs = (long)(Math.Max(0, timeSeconds)) * 1000;
        string file = Path.Combine(_tempDir, $"t{bucketMs}.png");
        if (File.Exists(file))
            return file; // cache hit — no seek

        await _gate.WaitAsync().ConfigureAwait(false);
        try
        {
            MpvContext? mpv = _mpv;
            if (mpv is null || !_fileReady)
                return null;
            if (File.Exists(file))
                return file;

            var restartTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            _restartTcs = restartTcs;
            mpv.Command("seek", timeSeconds.ToString(CultureInfo.InvariantCulture), "absolute+exact");
            await WaitWithTimeout(restartTcs.Task, 5000).ConfigureAwait(false);
            mpv.Command("screenshot-to-file", file, "video");
            return File.Exists(file) ? file : null;
        }
        catch
        {
            return null;
        }
        finally
        {
            _gate.Release();
        }
    }

    private static async Task<bool> WaitWithTimeout(Task<bool> task, int ms)
    {
        Task completed = await Task.WhenAny(task, Task.Delay(ms)).ConfigureAwait(false);
        return completed == task && task.Result;
    }

    private void ClearCache()
    {
        try
        {
            foreach (string f in Directory.EnumerateFiles(_tempDir, "*.png"))
            {
                try { File.Delete(f); } catch { /* in use — leave it */ }
            }
        }
        catch { /* best effort */ }
    }

    private void TeardownEngine()
    {
        _fileReady = false;
        _mpv?.Dispose();
        _mpv = null;
    }

    public void Dispose()
    {
        _gate.Wait();
        try { TeardownEngine(); }
        finally { _gate.Release(); _gate.Dispose(); }
    }
}
