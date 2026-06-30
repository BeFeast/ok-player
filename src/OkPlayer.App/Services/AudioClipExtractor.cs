using System;
using System.Globalization;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

namespace OkPlayer.App.Services;

/// <summary>A short audio clip cut for ASR: the WAV path and the clip's <b>absolute</b> start time in the media
/// (so ASR word times, which are relative to the clip, map back to media time for subtitle alignment).</summary>
public sealed record AudioClip(string WavPath, double ClipStartSeconds);

/// <summary>
/// Cuts a short audio window around the current playback position via the bundled ffmpeg, for the subtitle
/// auto-sync flow (transcribe the clip, match it against the loaded track). Mono 16 kHz PCM keeps the payload
/// small and Whisper-ready. Local files only for v1 (a remote re-open could stall). Off the UI thread; never throws.
/// </summary>
public static class AudioClipExtractor
{
    private static readonly string CacheDir = Path.Combine(Path.GetTempPath(), "OkPlayer", "syncclips");

    /// <summary>Extract ~<paramref name="lengthSeconds"/> s of audio starting <paramref name="leadInSeconds"/> s
    /// before <paramref name="positionSeconds"/> (clamped at 0), so the line currently on screen is included.
    /// Returns the clip (+ its absolute start), or null when ffmpeg is unavailable / fails / the source is remote.</summary>
    public static async Task<AudioClip?> ExtractAsync(
        string mediaPath,
        double positionSeconds,
        double leadInSeconds = 2.0,
        double lengthSeconds = 10.0,
        CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return null; // local files only for v1
        if (!FfmpegRunner.IsAvailable)
            return null;

        double start = Math.Max(0, positionSeconds - leadInSeconds);
        string outPath = "";
        try
        {
            Directory.CreateDirectory(CacheDir); // inside the guard: an unwritable temp dir degrades to null, not a throw
            outPath = Path.Combine(CacheDir, $"clip-{Environment.ProcessId}-{Guid.NewGuid():N}.wav");

            // -ss BEFORE -i = fast input seek (keyframe-accurate, sub-second drift is fine — the aligner is robust);
            // -vn drops video; mono / 16 kHz / s16 PCM is the small, Whisper-ready shape.
            string args =
                $"-nostdin -loglevel error -y -ss {Inv(start)} -t {Inv(lengthSeconds)} -i \"{mediaPath}\" " +
                $"-vn -ac 1 -ar 16000 -c:a pcm_s16le \"{outPath}\"";

            await FfmpegRunner.RunAsync(args, TimeSpan.FromSeconds(30), ct).ConfigureAwait(false);
            var fi = new FileInfo(outPath);
            if (fi is { Exists: true, Length: > 1024 }) // a real 10 s 16 kHz mono clip is ~320 KB
                return new AudioClip(outPath, start);
        }
        catch (FfmpegException)
        {
            // unavailable / decode failure / timeout — treat as "no clip"; the caller surfaces a retry
        }
        catch (OperationCanceledException)
        {
            // caller cancelled (e.g. user moved on)
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or System.Security.SecurityException)
        {
            // temp dir unwritable / disk full / locked — degrade to "no clip" per the never-throws contract
        }
        TryDelete(outPath);
        return null;
    }

    /// <summary>Delete a clip once it's been sent/used. Safe to call with a missing file.</summary>
    public static void Cleanup(string? wavPath) => TryDelete(wavPath);

    private static void TryDelete(string? path)
    {
        try { if (!string.IsNullOrEmpty(path) && File.Exists(path)) File.Delete(path); }
        catch { /* best effort */ }
    }

    private static string Inv(double v) => v.ToString("0.###", CultureInfo.InvariantCulture);
}
