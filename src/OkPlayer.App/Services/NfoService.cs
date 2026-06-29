using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Core;

namespace OkPlayer.App.Services;

/// <summary>
/// Resolves a Kodi/Jellyfin/Emby <c>.nfo</c> metadata sidecar next to a local media file. Tries a same-basename
/// <c>&lt;file&gt;.nfo</c> first (the per-item convention for movies-in-their-own-folder and TV episodes), then a
/// folder-level <c>movie.nfo</c> (Kodi's single-movie-folder convention). Reads and parses off the UI thread;
/// returns null for a URL, a missing/empty/oversized/unreadable file, or a non-XML <c>.nfo</c>. Never throws.
/// </summary>
public static class NfoService
{
    private const long MaxBytes = 2 * 1024 * 1024; // a real .nfo is a few KB — cap guards against a pathological file

    /// <summary>Parsed <c>.nfo</c> for <paramref name="mediaPath"/>, or null when there's no usable sidecar.
    /// Local files only (a remote read could stall). Never throws.</summary>
    public static Task<NfoMetadata?> GetAsync(string mediaPath, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(mediaPath) || mediaPath.Contains("://", StringComparison.Ordinal))
            return Task.FromResult<NfoMetadata?>(null);
        return Task.Run(() =>
        {
            try
            {
                string? dir = Path.GetDirectoryName(mediaPath);
                if (string.IsNullOrEmpty(dir))
                    return null;
                string sameName = Path.Combine(dir, Path.GetFileNameWithoutExtension(mediaPath) + ".nfo");
                string folderMovie = Path.Combine(dir, "movie.nfo");
                foreach (string candidate in new[] { sameName, folderMovie })
                {
                    ct.ThrowIfCancellationRequested();
                    if (Read(candidate) is { } xml && NfoMetadata.Parse(xml) is { } nfo)
                        return nfo;
                }
            }
            catch { /* unreadable path / permissions / cancellation — treat as no .nfo */ }
            return null;
        }, ct);
    }

    private static string? Read(string path)
    {
        try
        {
            var fi = new FileInfo(path);
            if (!fi.Exists || fi.Length == 0 || fi.Length > MaxBytes)
                return null;
            return File.ReadAllText(path); // auto-detects a BOM (UTF-8/UTF-16); defaults to UTF-8 otherwise
        }
        catch { return null; }
    }
}
