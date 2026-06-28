using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Mpv;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.App.Services;

/// <summary>Extracts an audio file's embedded cover art (album art) to a cached PNG so the now-playing surface
/// can show it. Runs on a throwaway <c>vo=null</c> mpv — no render API, so the cover-art-as-video freeze that
/// forces <c>audio-display=no</c> on the playback engine can't happen here. Returns null when the file carries
/// no embedded picture.</summary>
public static class CoverArtService
{
    private static readonly string CacheDir = Path.Combine(Path.GetTempPath(), "OkPlayer", "coverart");

    /// <summary>Path to a PNG of the file's embedded cover art, or null if it has none / can't be read. The
    /// result is cached on disk keyed by path+mtime+size, so reopening a track is instant. Never throws.</summary>
    public static Task<string?> GetAsync(string mediaPath, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return Task.FromResult<string?>(null); // local files only — a remote fetch could stall the extractor
        return Task.Run(() => Extract(mediaPath, out _), ct);
    }

    /// <summary>Like <see cref="GetAsync"/>, but also reports whether the file <em>definitively</em> carries no
    /// embedded picture (<c>DefinitelyNoArt</c>), distinct from a transient failure (timeout, locked file, …).
    /// A caller that caches a "no art" verdict should only trust <c>DefinitelyNoArt</c> — a transient null must
    /// not become a permanent gradient.</summary>
    public static Task<(string? Path, bool DefinitelyNoArt)> GetWithStatusAsync(string mediaPath, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return Task.FromResult<(string?, bool)>((null, false));
        return Task.Run(() => { string? p = Extract(mediaPath, out bool noArt); return (p, noArt); }, ct);
    }

    private static string? Extract(string mediaPath, out bool definitelyNoArt)
    {
        definitelyNoArt = false; // only the "no attached picture" branch flips this; every failure leaves it false

        string outPng;
        try
        {
            var fi = new FileInfo(mediaPath);
            string raw = $"{mediaPath}|{fi.LastWriteTimeUtc.Ticks}|{fi.Length}";
            string hash = Convert.ToHexString(System.Security.Cryptography.SHA1.HashData(
                System.Text.Encoding.UTF8.GetBytes(raw)))[..16].ToLowerInvariant();
            outPng = Path.Combine(CacheDir, hash + ".png");
            if (File.Exists(outPng) && new FileInfo(outPng).Length > 0)
                return outPng; // cached from a previous open
            Directory.CreateDirectory(CacheDir);
        }
        catch { return null; }

        MpvContext? mpv = null;
        try
        {
            mpv = new MpvContext();
            mpv.SetOption("vo", "null");   // decode only, no render API — the cover-art-as-video freeze can't occur
            mpv.SetOption("audio", "no");  // we only want the picture, not playback
            mpv.SetOption("hwdec", "no");
            mpv.SetOption("pause", "yes");
            mpv.SetOption("keep-open", "yes");
            mpv.SetOption("osc", "no");
            mpv.SetOption("input-default-bindings", "no");
            // (audio-display left at its default here, so the embedded picture IS decoded as the video to grab.)

            using var loaded = new ManualResetEventSlim(false);
            bool failed = false;
            mpv.FileLoaded += (_, _) => loaded.Set();
            mpv.EndFile += (_, r) => { if (r == MpvEndFileReason.Error) { failed = true; loaded.Set(); } };
            mpv.Initialize();
            mpv.Command("loadfile", mediaPath, "replace"); // sync is fine off the UI thread

            if (!loaded.Wait(TimeSpan.FromSeconds(10)) || failed)
                return null;

            // Decide "has embedded art" from the TRACK LIST, not from dwidth: the track list is final at
            // file-loaded, whereas a cover-art video track's dwidth may still read 0 until its single frame
            // decodes a moment later. mpv represents embedded album art as a video track, so "no video track at
            // all" is the only safe, cacheable "no art" verdict. (Treating a transient dwidth==0 as no-art would
            // permanently strand a track that actually has art — the bug this guards against.)
            if (!HasVideoTrack(mpv))
            {
                definitelyNoArt = true; // genuinely no picture stream — a real, cacheable verdict
                return null;
            }
            // There is a picture track; wait briefly for its frame to decode so dwidth (and the screenshot) are
            // ready. If it never decodes, that's a transient failure — retry later, don't cache "no art".
            for (int waited = 0; (mpv.GetPropertyLong("dwidth") ?? 0) <= 0; waited += 50)
            {
                if (waited >= 3000)
                    return null;
                System.Threading.Thread.Sleep(50);
            }
            mpv.Command("screenshot-to-file", outPng, "video"); // blocks until written
            return File.Exists(outPng) && new FileInfo(outPng).Length > 0 ? outPng : null;
        }
        catch { return null; }
        finally { mpv?.Dispose(); }
    }

    /// <summary>True if the loaded file exposes any video track (embedded album art counts — mpv decodes it as a
    /// single-frame video). Read from the track list, which is final at <c>file-loaded</c>, so it doesn't depend
    /// on the picture frame having decoded yet.</summary>
    private static bool HasVideoTrack(MpvContext mpv)
    {
        long count = mpv.GetPropertyLong("track-list/count") ?? 0;
        for (long i = 0; i < count; i++)
            if (string.Equals(mpv.GetPropertyString($"track-list/{i}/type"), "video", StringComparison.Ordinal))
                return true;
        return false;
    }
}
