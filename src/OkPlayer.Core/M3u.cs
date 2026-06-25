using System.Collections.Generic;
using System.IO;
using System.Text;

namespace OkPlayer.Core;

/// <summary>
/// Minimal `.m3u` read/write for portable playlists: write is one path per line under an `#EXTM3U` header;
/// parse keeps line order (a playlist's order is meaningful, unlike a folder's), skips blank lines and
/// `#` directives/comments, and resolves relative entries against the playlist file's folder. Pure/testable.
/// </summary>
public static class M3u
{
    /// <summary>Serialize paths to `.m3u` text (header + one path per line, LF-terminated).</summary>
    public static string Write(IEnumerable<string> paths)
    {
        var sb = new StringBuilder("#EXTM3U\n");
        foreach (var p in paths)
            sb.Append(p).Append('\n');
        return sb.ToString();
    }

    /// <summary>Parse `.m3u` text into ordered entry paths. Relative entries resolve against
    /// <paramref name="baseDir"/> (the playlist file's folder); absolute paths and URLs pass through.</summary>
    public static List<string> Parse(string text, string? baseDir)
    {
        var result = new List<string>();
        foreach (var raw in text.Split('\n'))
        {
            string line = raw.Trim();
            if (line.Length == 0 || line[0] == '#')
                continue; // blank line or #EXTM3U / #EXTINF directive / comment
            bool absolute = Path.IsPathRooted(line) || line.Contains("://");
            result.Add(absolute || baseDir is null ? line : Path.GetFullPath(Path.Combine(baseDir, line)));
        }
        return result;
    }
}
