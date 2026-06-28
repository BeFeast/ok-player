using System;

namespace OkPlayer.Core;

/// <summary>Best-effort resolution of an audio track's (artist, title) for a lyrics lookup. Real tags win;
/// when an artist tag is missing it mines a <c>"Artist - Title"</c> display string (the mpv media-title, else
/// the file name) — the common shape for ripped/downloaded files that carry a title but no separate artist
/// tag. Either field may come back null when nothing usable is present. Pure / UI-free for headless tests.</summary>
public static class TrackTags
{
    /// <summary>Resolve (artist, track) from the available signals. <paramref name="tagArtist"/>/
    /// <paramref name="tagTitle"/> are the file's metadata tags; <paramref name="display"/> is the mpv
    /// media-title; <paramref name="fileStem"/> is the file name without extension. Splits on the first
    /// <c>" - "</c> only to fill a field a tag didn't provide.</summary>
    public static (string? Artist, string? Track) Resolve(string? tagArtist, string? tagTitle,
                                                          string? display, string? fileStem)
    {
        string? artist = Clean(tagArtist);
        string? track = Clean(tagTitle);
        string? source = Clean(display) ?? Clean(fileStem); // the string to mine when a tag is missing

        if ((artist is null || track is null) && source is not null)
        {
            int dash = source.IndexOf(" - ", StringComparison.Ordinal);
            if (dash > 0 && dash < source.Length - 3)
            {
                artist ??= Clean(source.Substring(0, dash));
                track ??= Clean(source.Substring(dash + 3));
            }
        }
        track ??= source; // last resort: treat the whole display/filename as the track name
        return (artist, track);
    }

    private static string? Clean(string? s) => string.IsNullOrWhiteSpace(s) ? null : s.Trim();
}
