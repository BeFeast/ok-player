using System;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json.Serialization;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Core;

namespace OkPlayer.App.Services;

/// <summary>Resolves synced (or plain) lyrics for an audio track, for the on-demand karaoke overlay. Order:
/// a <c>.lrc</c> sidecar next to the file (zero network, honours a library the user already has) → a local
/// cache of a prior fetch → <strong>LRCLIB</strong> by metadata (<c>/api/get</c> exact on artist+track+album+
/// duration, then <c>/api/search</c> fuzzy). Returns <see cref="LrcDocument.Empty"/> when nothing matches or
/// the track is flagged instrumental. Only metadata text leaves the machine (never the audio) — that's the
/// Level-2 karaoke service's job. All work runs off the UI thread; never throws.</summary>
public static class LyricsService
{
    private const string DefaultBaseUrl = "https://lrclib.net";
    private static readonly string CacheDir = Path.Combine(Path.GetTempPath(), "OkPlayer", "lyrics");

    // LRCLIB asks callers to identify themselves with a descriptive User-Agent (app + version + link).
    private static readonly HttpClient Http = CreateClient();

    private static HttpClient CreateClient()
    {
        var c = new HttpClient { Timeout = TimeSpan.FromSeconds(12) };
        string ver = typeof(LyricsService).Assembly.GetName().Version?.ToString() ?? "0";
        c.DefaultRequestHeaders.UserAgent.ParseAdd($"OkPlayer/{ver} (https://github.com/BeFeast/ok-player)");
        return c;
    }

    /// <summary>Resolve lyrics for a query. Local-first (sidecar, cache), then LRCLIB if
    /// <see cref="LyricsQuery.AllowNetwork"/>. A successful network fetch is cached when
    /// <see cref="LyricsQuery.AllowCacheWrite"/> (off in a private session).</summary>
    public static Task<LrcDocument> GetAsync(LyricsQuery query, CancellationToken ct = default)
        => Task.Run(() => ResolveAsync(query, ct), ct);

    private static async Task<LrcDocument> ResolveAsync(LyricsQuery q, CancellationToken ct)
    {
        // 1) A sidecar next to the media file (track.flac → track.lrc). Local libraries often ship these.
        if (TryReadSidecar(q.MediaPath) is { } sidecar)
            return Lrc.Parse(sidecar);

        // 2) A prior fetch cached by metadata key.
        string? cacheKey = CacheKey(q);
        if (cacheKey is { } key && TryReadCache(key) is { } cached)
            return Lrc.Parse(cached);

        if (!q.AllowNetwork || string.IsNullOrWhiteSpace(q.Artist) || string.IsNullOrWhiteSpace(q.Track))
            return LrcDocument.Empty; // nothing local and no usable metadata / network disabled

        string baseUrl = (string.IsNullOrWhiteSpace(q.LrcLibBaseUrl) ? DefaultBaseUrl : q.LrcLibBaseUrl!).TrimEnd('/');

        // 3) LRCLIB exact match (artist+track+album+duration, ±2 s server-side).
        string? lrc = await FetchExactAsync(baseUrl, q, ct);
        // 4) Fuzzy search fallback.
        lrc ??= await FetchSearchAsync(baseUrl, q, ct);
        if (lrc is null)
            return LrcDocument.Empty;

        if (cacheKey is { } k && q.AllowCacheWrite)
            TryWriteCache(k, lrc);
        return Lrc.Parse(lrc);
    }

    private static async Task<string?> FetchExactAsync(string baseUrl, LyricsQuery q, CancellationToken ct)
    {
        string url = $"{baseUrl}/api/get?artist_name={Esc(q.Artist)}&track_name={Esc(q.Track)}" +
                     $"&album_name={Esc(q.Album)}&duration={(int)Math.Round(q.DurationSeconds)}";
        return await GetLyricsAsync(url, ct);
    }

    private static async Task<string?> FetchSearchAsync(string baseUrl, LyricsQuery q, CancellationToken ct)
    {
        string url = $"{baseUrl}/api/search?artist_name={Esc(q.Artist)}&track_name={Esc(q.Track)}";
        try
        {
            using HttpResponseMessage res = await Http.GetAsync(url, ct);
            if (!res.IsSuccessStatusCode)
                return null;
            LrcLibResult[]? hits = await res.Content.ReadFromJsonAsync<LrcLibResult[]>(ct);
            if (hits is null)
                return null;
            // Prefer a synced hit closest in duration; ignore instrumental entries.
            LrcLibResult? best = null;
            double bestDelta = double.MaxValue;
            foreach (LrcLibResult h in hits)
            {
                if (h.Instrumental || PickLyrics(h) is null)
                    continue;
                double delta = Math.Abs(h.Duration - q.DurationSeconds);
                bool better = best is null
                    || (HasSynced(h) && !HasSynced(best))           // a synced hit always beats a plain-only one
                    || (HasSynced(h) == HasSynced(best) && delta < bestDelta);
                if (better) { best = h; bestDelta = delta; }
            }
            return best is null ? null : PickLyrics(best);
        }
        catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException or NotSupportedException
                                      or System.Text.Json.JsonException)
        {
            return null;
        }
    }

    private static async Task<string?> GetLyricsAsync(string url, CancellationToken ct)
    {
        try
        {
            using HttpResponseMessage res = await Http.GetAsync(url, ct);
            if (res.StatusCode == HttpStatusCode.NotFound || !res.IsSuccessStatusCode)
                return null;
            LrcLibResult? hit = await res.Content.ReadFromJsonAsync<LrcLibResult>(ct);
            return hit is null || hit.Instrumental ? null : PickLyrics(hit);
        }
        catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException or NotSupportedException
                                      or System.Text.Json.JsonException)
        {
            return null; // network down / timeout / malformed body — treat as "no lyrics", retry on a later request
        }
    }

    private static bool HasSynced(LrcLibResult h) => !string.IsNullOrWhiteSpace(h.SyncedLyrics);
    private static string? PickLyrics(LrcLibResult h)
        => !string.IsNullOrWhiteSpace(h.SyncedLyrics) ? h.SyncedLyrics
         : !string.IsNullOrWhiteSpace(h.PlainLyrics) ? h.PlainLyrics
         : null;

    private static string Esc(string? s) => Uri.EscapeDataString(s ?? string.Empty);

    /// <summary>A same-named <c>.lrc</c> next to a local media file, read as text. Null when there's no local
    /// path, no sidecar, or it can't be read (e.g. an unreadable network mount).</summary>
    private static string? TryReadSidecar(string? mediaPath)
    {
        try
        {
            if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
                return null;
            string? dir = Path.GetDirectoryName(mediaPath);
            if (string.IsNullOrEmpty(dir))
                return null;
            string lrc = Path.Combine(dir, Path.GetFileNameWithoutExtension(mediaPath) + ".lrc");
            return File.Exists(lrc) ? File.ReadAllText(lrc) : null;
        }
        catch { return null; }
    }

    /// <summary>Stable cache key from the metadata (so the same track resolves once across sessions). Null when
    /// there isn't enough metadata to key on.</summary>
    private static string? CacheKey(LyricsQuery q)
    {
        if (string.IsNullOrWhiteSpace(q.Artist) || string.IsNullOrWhiteSpace(q.Track))
            return null;
        string raw = $"{q.Artist}|{q.Track}|{q.Album}|{(int)Math.Round(q.DurationSeconds)}".ToLowerInvariant();
        return Convert.ToHexString(System.Security.Cryptography.SHA1.HashData(
            System.Text.Encoding.UTF8.GetBytes(raw)))[..16].ToLowerInvariant();
    }

    private static string? TryReadCache(string key)
    {
        try
        {
            string path = Path.Combine(CacheDir, key + ".lrc");
            return File.Exists(path) ? File.ReadAllText(path) : null;
        }
        catch { return null; }
    }

    private static void TryWriteCache(string key, string lrc)
    {
        try
        {
            Directory.CreateDirectory(CacheDir);
            File.WriteAllText(Path.Combine(CacheDir, key + ".lrc"), lrc);
        }
        catch { /* cache is best-effort */ }
    }

    private sealed record LrcLibResult(
        [property: JsonPropertyName("syncedLyrics")] string? SyncedLyrics,
        [property: JsonPropertyName("plainLyrics")] string? PlainLyrics,
        [property: JsonPropertyName("instrumental")] bool Instrumental,
        [property: JsonPropertyName("duration")] double Duration);
}

/// <summary>What to resolve lyrics for. <see cref="MediaPath"/> drives the sidecar check; the metadata drives
/// the LRCLIB lookup. <see cref="AllowNetwork"/>/<see cref="AllowCacheWrite"/> let a private session stay
/// local-only and leave no trace. <see cref="LrcLibBaseUrl"/> overrides the public endpoint (self-hosted
/// LRCLIB / the karaoke service's lyrics endpoint).</summary>
public sealed record LyricsQuery(
    string? MediaPath,
    string? Artist,
    string? Track,
    string? Album,
    double DurationSeconds,
    bool AllowNetwork = true,
    bool AllowCacheWrite = true,
    string? LrcLibBaseUrl = null);
