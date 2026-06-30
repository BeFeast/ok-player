using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace OkPlayer.App.Services;

/// <summary>
/// A thin, robust wrapper around the bundled <c>ffmpeg.exe</c> for media-processing tasks (subtitle-sync audio
/// clips today; cut/convert/remux later). Resolves the binary next to the app (it ships in the same folder, fetched
/// into <c>native/ffmpeg/</c> and copied at build), falling back to <c>ffmpeg</c> on PATH for the dev loop.
/// Captures stderr, enforces a timeout/cancellation, and throws <see cref="FfmpegException"/> on failure.
/// </summary>
public static class FfmpegRunner
{
    /// <summary>Full path to a usable <c>ffmpeg.exe</c>, or just <c>"ffmpeg"</c> to defer to PATH.</summary>
    public static string ExecutablePath { get; } = ResolveExecutable();

    /// <summary>True when an ffmpeg binary is available (bundled or on PATH). Callers gate media-processing
    /// features on this so a missing engine degrades gracefully rather than throwing at the call site.</summary>
    public static bool IsAvailable =>
        ExecutablePath != "ffmpeg" || File.Exists(WhereOnPath("ffmpeg.exe"));

    private static string ResolveExecutable()
    {
        string bundled = Path.Combine(AppContext.BaseDirectory, "ffmpeg.exe");
        return File.Exists(bundled) ? bundled : "ffmpeg"; // PATH fallback (dev machines with ffmpeg installed)
    }

    private static string? WhereOnPath(string exe)
    {
        foreach (string dir in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator))
        {
            if (string.IsNullOrWhiteSpace(dir)) continue;
            string candidate = Path.Combine(dir.Trim(), exe);
            if (File.Exists(candidate)) return candidate;
        }
        return null;
    }

    /// <summary>Run ffmpeg with the given arguments. Returns when it exits 0; throws <see cref="FfmpegException"/>
    /// on a non-zero exit (with the tail of stderr) or a timeout. Runs off the UI thread.</summary>
    public static async Task RunAsync(string arguments, TimeSpan timeout, CancellationToken ct = default)
    {
        var psi = new ProcessStartInfo
        {
            FileName = ExecutablePath,
            Arguments = arguments,
            RedirectStandardError = true,
            RedirectStandardOutput = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };

        using var proc = new Process { StartInfo = psi };
        var stderr = new StringBuilder();
        proc.ErrorDataReceived += (_, e) => { if (e.Data is { } s) stderr.AppendLine(s); };

        try { proc.Start(); }
        catch (Exception ex) { throw new FfmpegException($"Could not start ffmpeg ({ExecutablePath}): {ex.Message}", ex); }

        proc.BeginErrorReadLine();
        proc.BeginOutputReadLine();

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeoutCts.CancelAfter(timeout);
        try
        {
            await proc.WaitForExitAsync(timeoutCts.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            TryKill(proc);
            if (ct.IsCancellationRequested) throw; // caller cancelled — propagate
            throw new FfmpegException($"ffmpeg timed out after {timeout.TotalSeconds:0}s");
        }

        if (proc.ExitCode != 0)
            throw new FfmpegException($"ffmpeg exited {proc.ExitCode}. {Tail(stderr.ToString())}");
    }

    private static void TryKill(Process p)
    {
        try { if (!p.HasExited) p.Kill(entireProcessTree: true); } catch { /* already gone */ }
    }

    // Last few stderr lines — enough to diagnose without dumping ffmpeg's full banner.
    private static string Tail(string s)
    {
        string[] lines = s.TrimEnd().Split('\n');
        int take = Math.Min(4, lines.Length);
        return string.Join(" | ", lines[^take..]).Trim();
    }
}

/// <summary>Thrown when an ffmpeg invocation fails (can't start, non-zero exit, or timeout).</summary>
public sealed class FfmpegException : Exception
{
    public FfmpegException(string message, Exception? inner = null) : base(message, inner) { }
}
