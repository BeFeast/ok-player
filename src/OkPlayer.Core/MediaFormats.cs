using System;
using System.Collections.Generic;
using System.IO;

namespace OkPlayer.Core;

/// <summary>The media file extensions OK Player recognizes — the single source of truth shared by the open
/// picker's filter and the folder-as-playlist scan, so the two never drift apart.</summary>
public static class MediaFormats
{
    public static readonly IReadOnlyList<string> Extensions = new[]
    {
        ".mkv", ".mp4", ".m4v", ".avi", ".mov", ".webm", ".m2ts", ".ts", ".wmv", ".flv",
        ".mp3", ".flac", ".m4a", ".opus", ".wav", ".ogg", ".mka",
    };

    /// <summary>External subtitle file extensions — the single source of truth shared by the "Add subtitle
    /// file" picker filter and the drag-drop router, so a dropped .srt loads as a track, not as media.</summary>
    public static readonly IReadOnlyList<string> SubtitleExtensions = new[]
    {
        ".srt", ".ass", ".ssa", ".sub", ".vtt", ".idx", ".sup",
    };

    private static readonly HashSet<string> Set = new(Extensions, StringComparer.OrdinalIgnoreCase);
    private static readonly HashSet<string> SubSet = new(SubtitleExtensions, StringComparer.OrdinalIgnoreCase);

    /// <summary>True if the path's extension is a media type we play (case-insensitive).</summary>
    public static bool IsMedia(string path) => Set.Contains(Path.GetExtension(path));

    /// <summary>True if the path's extension is an external subtitle file (case-insensitive).</summary>
    public static bool IsSubtitle(string path) => SubSet.Contains(Path.GetExtension(path));

    /// <summary>True when <paramref name="text"/> is a single absolute URL that can be handed to mpv as a stream
    /// (http/https/rtmp/rtsp/smb/…) — used to turn a dropped or pasted link into an open. Rejects a paragraph
    /// that merely contains a URL (it won't parse as an absolute Uri) and a bare local file path (it parses as a
    /// <c>file:</c> Uri, which we exclude; local files are opened by path, not through here). Trims the input.</summary>
    public static bool IsPlayableUrl(string? text)
        => !string.IsNullOrWhiteSpace(text)
           && Uri.TryCreate(text.Trim(), UriKind.Absolute, out var uri)
           && !uri.IsFile;
}
