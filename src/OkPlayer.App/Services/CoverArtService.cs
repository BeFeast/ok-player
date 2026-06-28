using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Mpv;
using OkPlayer.Mpv.Interop;

namespace OkPlayer.App.Services;

/// <summary>Resolves an audio file's cover art for the now-playing surface. Prefers a <em>sidecar</em> image
/// sitting next to the file (the Kodi/Jellyfin/Plex convention — <c>track.jpg</c> or a folder <c>cover.jpg</c>/
/// <c>folder.jpg</c>), which is typically higher-resolution and intentional; otherwise extracts the file's
/// embedded album art to a cached PNG via a throwaway <c>vo=null</c> mpv (no render API, so the
/// cover-art-as-video freeze that forces <c>audio-display=no</c> on the playback engine can't happen here).
/// Returns null when the file has neither.</summary>
public static class CoverArtService
{
    private static readonly string CacheDir = Path.Combine(Path.GetTempPath(), "OkPlayer", "coverart");

    // Sidecar art conventions. Extensions BitmapImage can decode; folder-cover stems in descending preference.
    private static readonly string[] ArtExtensions = { ".jpg", ".jpeg", ".png", ".webp" };
    private static readonly string[] FolderArtNames = { "cover", "folder", "front", "poster", "album", "albumart" };

    /// <summary>Path to a usable cover image (a sidecar image, or a cached PNG of the embedded art), or null if
    /// the file has neither / can't be read. Local files only. Never throws.</summary>
    public static Task<string?> GetAsync(string mediaPath, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return Task.FromResult<string?>(null); // local files only — a remote fetch could stall the extractor
        return Task.Run(async () => await ResolveSidecarAsync(mediaPath) ?? Extract(mediaPath, out _), ct);
    }

    /// <summary>Like <see cref="GetAsync"/>, but also reports whether the file <em>definitively</em> has no cover
    /// (no sidecar and no embedded picture, <c>DefinitelyNoArt</c>), distinct from a transient failure (timeout,
    /// locked file, …). A caller that caches a "no art" verdict should only trust <c>DefinitelyNoArt</c> — a
    /// transient null must not become a permanent gradient.</summary>
    public static Task<(string? Path, bool DefinitelyNoArt)> GetWithStatusAsync(string mediaPath, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return Task.FromResult<(string?, bool)>((null, false));
        return Task.Run<(string?, bool)>(async () =>
        {
            if (await ResolveSidecarAsync(mediaPath) is { } sidecar)
                return (sidecar, false); // a sidecar image IS art — never a "no art" verdict
            string? p = Extract(mediaPath, out bool noArt);
            return (p, noArt);
        }, ct);
    }

    /// <summary>Just the sidecar resolution — no embedded extraction. Lets a caller cheaply re-check for a cover
    /// dropped in next to a track <em>after</em> it was first marked art-less (a sidecar can appear without the
    /// media file changing, so a cached "no art" verdict must not block it). Null when there's no usable sidecar.</summary>
    public static Task<string?> GetSidecarAsync(string mediaPath, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return Task.FromResult<string?>(null);
        return Task.Run(() => ResolveSidecarAsync(mediaPath), ct);
    }

    /// <summary>A cover image sitting next to the media file: a same-named image first (<c>track.flac</c> →
    /// <c>track.jpg</c>), else a well-known folder cover (<c>cover</c>/<c>folder</c>/<c>front</c>/<c>poster</c>/…).
    /// One directory listing, matched case-insensitively (NFS/SMB can be case-sensitive, so don't trust
    /// <c>File.Exists</c> casing). Selection is fully deterministic regardless of enumeration order: same-named
    /// beats folder covers, a better <see cref="ArtExtensions"/> rank wins within that, and an ordinal path
    /// compare breaks any remaining tie (e.g. a case-sensitive volume holding both <c>cover.jpg</c> and
    /// <c>COVER.JPG</c>). Each candidate is then <em>fully decoded</em> in preference order and the first that
    /// decodes wins — a header-valid but truncated/corrupt image must not win and suppress embedded extraction,
    /// which a magic-byte sniff alone can't guarantee. Returns null when no candidate decodes or the directory
    /// can't be read. All filesystem and decode work runs off the UI thread (callers wrap this in
    /// <see cref="Task.Run"/>), so a network mount can't stall the dispatcher.</summary>
    private static async Task<string?> ResolveSidecarAsync(string mediaPath)
    {
        try
        {
            string? dir = Path.GetDirectoryName(mediaPath);
            if (string.IsNullOrEmpty(dir))
                return null;
            string baseName = Path.GetFileNameWithoutExtension(mediaPath);
            var candidates = new List<(int Slot, int Rank, string Path)>(); // Slot: -1 = same-named, else FolderArtNames index
            foreach (string file in Directory.EnumerateFiles(dir))
            {
                int rank = ArtExtensionRank(Path.GetExtension(file));
                if (rank < 0)
                    continue;                                           // not a cover-image extension
                string stem = Path.GetFileNameWithoutExtension(file);
                bool isSameName = string.Equals(stem, baseName, StringComparison.OrdinalIgnoreCase);
                int slot = isSameName ? -1 : FolderStemIndex(stem);
                if (!isSameName && slot < 0)
                    continue;                                           // neither same-name nor a known folder cover
                if (!HasImageHeader(file))
                    continue;                                           // cheap reject: zero-byte / not a JPEG/PNG/WebP
                candidates.Add((slot, rank, file));
            }
            // Deterministic preference: same-named (slot -1) before folder covers, better extension rank within
            // that, then ordinal path so the choice never depends on filesystem enumeration order.
            candidates.Sort((a, b) =>
            {
                int c = a.Slot.CompareTo(b.Slot);
                if (c != 0) return c;
                c = a.Rank.CompareTo(b.Rank);
                if (c != 0) return c;
                return string.CompareOrdinal(a.Path, b.Path);
            });
            // Full-decode in order; the first that actually decodes wins. A truncated/corrupt file that passed the
            // header sniff is rejected here so it can't mask valid embedded art (we fall through to the next
            // candidate, and ultimately to embedded extraction when none decode).
            foreach (var candidate in candidates)
                if (await CanDecodeAsync(candidate.Path))
                    return candidate.Path;
            return null;
        }
        catch { return null; } // unreadable directory — fall back to embedded extraction
    }

    /// <summary>Index of <paramref name="ext"/> in <see cref="ArtExtensions"/> (0 = most preferred), or -1.</summary>
    private static int ArtExtensionRank(string ext)
    {
        for (int i = 0; i < ArtExtensions.Length; i++)
            if (string.Equals(ext, ArtExtensions[i], StringComparison.OrdinalIgnoreCase))
                return i;
        return -1;
    }

    /// <summary>Index of <paramref name="stem"/> in <see cref="FolderArtNames"/> (0 = most preferred), or -1.</summary>
    private static int FolderStemIndex(string stem)
    {
        for (int i = 0; i < FolderArtNames.Length; i++)
            if (string.Equals(stem, FolderArtNames[i], StringComparison.OrdinalIgnoreCase))
                return i;
        return -1;
    }

    /// <summary>Cheap prefilter: reject a candidate that obviously isn't a JPEG/PNG/WebP (zero-byte, truncated
    /// below the magic length, or wrong type) by sniffing only its leading bytes — so we don't spin up a full
    /// image decoder for junk. Passing this is necessary but not sufficient; <see cref="CanDecodeAsync"/> is the
    /// authoritative usability gate, since a valid header can still front a truncated or corrupt body.</summary>
    private static bool HasImageHeader(string path)
    {
        try
        {
            using var fs = File.OpenRead(path);
            if (fs.Length < 12)
                return false;
            Span<byte> h = stackalloc byte[12];
            fs.ReadExactly(h);
            if (h[0] == 0xFF && h[1] == 0xD8 && h[2] == 0xFF)
                return true; // JPEG
            if (h[0] == 0x89 && h[1] == 0x50 && h[2] == 0x4E && h[3] == 0x47)
                return true; // PNG
            if (h[0] == (byte)'R' && h[1] == (byte)'I' && h[2] == (byte)'F' && h[3] == (byte)'F'
                && h[8] == (byte)'W' && h[9] == (byte)'E' && h[10] == (byte)'B' && h[11] == (byte)'P')
                return true; // WebP
            return false;
        }
        catch { return false; }
    }

    /// <summary>The authoritative usability check: actually decode the whole image (via the platform WIC decoder —
    /// the same path the posters use) and report whether it succeeds. A full decode is the only thing that catches
    /// a header-valid but truncated or corrupt file — exactly the case that must not win and suppress embedded
    /// extraction. Its callers run it off the UI thread (wrapped in <see cref="Task.Run"/>); never throws.</summary>
    private static async Task<bool> CanDecodeAsync(string path)
    {
        try
        {
            var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
            using var stream = await file.OpenAsync(Windows.Storage.FileAccessMode.Read);
            var decoder = await Windows.Graphics.Imaging.BitmapDecoder.CreateAsync(stream);
            await decoder.GetPixelDataAsync(); // forces a full decode → throws on a truncated/corrupt body
            return true;
        }
        catch { return false; }
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
