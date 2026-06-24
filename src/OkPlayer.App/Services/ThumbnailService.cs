using System;
using System.Globalization;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Mpv;

namespace OkPlayer.App.Services;

/// <summary>
/// Generates frame thumbnails on a dedicated decode-only libmpv instance (vo=null), so the main
/// playback engine is never disturbed. Each thumbnail is written to a temp PNG named by its open
/// generation + time bucket (so repeats are cache hits and a new file can never serve a prior
/// file's frame) and returned as a path the UI can load. One frame is produced at a time.
/// </summary>
public sealed class ThumbnailService : IDisposable
{
    private readonly string _tempDir;
    private readonly SemaphoreSlim _gate = new(1, 1);     // serialize seeks: one frame in flight
    private readonly CancellationTokenSource _shutdown = new();
    private MpvContext? _mpv;
    private TaskCompletionSource<bool>? _restartTcs;
    private volatile bool _fileReady;
    private volatile bool _seekPending;                    // true only while a seek's PlaybackRestart is expected
    private volatile bool _disposed;
    private int _generation;                               // bumps per OpenAsync; namespaces the cache + bails stale requests

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
        if (_disposed)
            return false;
        try { await _gate.WaitAsync(_shutdown.Token).ConfigureAwait(false); }
        catch (OperationCanceledException) { return false; }
        catch (ObjectDisposedException) { return false; }
        try
        {
            if (_disposed)
                return false; // disposed while queued — don't spin up a new engine
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
            mpv.SetOption("osc", "no");
            mpv.SetOption("input-default-bindings", "no");

            var loadTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            mpv.FileLoaded += (_, _) => { _fileReady = true; loadTcs.TrySetResult(true); };
            mpv.EndFile += (_, _) => loadTcs.TrySetResult(_fileReady);
            // Only a seek we are awaiting may complete the wait — ignore the initial load restart and
            // any restart left over from a previous (timed-out) seek.
            mpv.PlaybackRestart += (_, _) => { if (_seekPending) _restartTcs?.TrySetResult(true); };

            mpv.Initialize();
            _mpv = mpv;
            mpv.Command("loadfile", path, "replace");

            bool ready = await WaitWithTimeout(loadTcs.Task, 10000).ConfigureAwait(false);
            return ready && gen == _generation && !_disposed;
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

    /// <summary>Produce (or reuse) a thumbnail near <paramref name="timeSeconds"/>; returns a PNG path or null.
    /// <paramref name="isStale"/>, if it returns true once the gate is held, skips the (expensive) seek.</summary>
    public Task<string?> GetThumbnailAsync(double timeSeconds) => GetThumbnailAsync(timeSeconds, null);

    public async Task<string?> GetThumbnailAsync(double timeSeconds, Func<bool>? isStale)
    {
        if (_disposed || !_fileReady)
            return null;
        int gen = _generation;
        long bucketSec = (long)Math.Max(0, timeSeconds); // 1-second thumbnail granularity
        string file = Path.Combine(_tempDir, $"g{gen}_t{bucketSec}.png");
        if (File.Exists(file))
            return file; // cache hit — no seek

        try { await _gate.WaitAsync().ConfigureAwait(false); }
        catch (ObjectDisposedException) { return null; } // disposed while queued
        try
        {
            MpvContext? mpv = _mpv;
            if (_disposed || gen != _generation || mpv is null || !_fileReady)
                return null;       // a different file was opened (or we're shutting down) while we waited
            if (File.Exists(file))
                return file;
            if (isStale?.Invoke() == true)
                return null;       // superseded (e.g. the cursor moved on) — don't pay for a seek

            // Keep the seek inside the file so PlaybackRestart fires reliably at the tail.
            double target = timeSeconds;
            if (mpv.GetPropertyDouble("duration") is double d && d > 0.1 && target > d - 0.1)
                target = Math.Max(0, d - 0.1);

            var restartTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            _restartTcs = restartTcs;
            _seekPending = true;
            bool restarted;
            try
            {
                mpv.Command("seek", target.ToString(CultureInfo.InvariantCulture), "absolute+exact");
                restarted = await WaitWithTimeout(restartTcs.Task, 5000).ConfigureAwait(false);
            }
            finally
            {
                _seekPending = false; // a late restart must not complete the next caller's wait
            }

            if (!restarted)
                return null;          // frame not ready in time — don't cache a stale frame; allow retry

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

    private async Task<bool> WaitWithTimeout(Task<bool> task, int ms)
    {
        try
        {
            Task completed = await Task.WhenAny(task, Task.Delay(ms, _shutdown.Token)).ConfigureAwait(false);
            return completed == task && task.Result;
        }
        catch (OperationCanceledException)
        {
            return false; // shutting down
        }
    }

    private void ClearCache()
    {
        try
        {
            foreach (string f in Directory.EnumerateFiles(_tempDir, "*.png"))
            {
                try { File.Delete(f); } catch { /* locked — harmless: filenames are generation-namespaced */ }
            }
        }
        catch { /* best effort */ }
    }

    private void TeardownEngine()
    {
        _fileReady = false;
        _seekPending = false;
        _mpv?.Dispose();
        _mpv = null;
    }

    public void Dispose()
    {
        if (_disposed)
            return;
        _disposed = true;
        _shutdown.Cancel();   // unblock any in-flight gated wait so the gate frees promptly
        _gate.Wait();
        try { TeardownEngine(); }
        finally { _gate.Release(); _shutdown.Dispose(); } // do NOT dispose _gate: pending waiters would race
    }
}
