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

    private static readonly HashSet<string> Set = new(Extensions, StringComparer.OrdinalIgnoreCase);

    /// <summary>True if the path's extension is a media type we play (case-insensitive).</summary>
    public static bool IsMedia(string path) => Set.Contains(Path.GetExtension(path));
}
