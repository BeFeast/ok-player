using System;
using System.Collections.Generic;
using System.IO;

namespace OkPlayer.Core;

/// <summary>Recursively collect the media files under a dropped folder so dropping a folder can become a
/// playlist. The recursion is depth-bounded so a pathological or symlink-looped tree can't walk forever, and
/// reparse points (symlinks / junctions) are skipped for the same reason. Unreadable subdirectories are
/// skipped rather than fatal.</summary>
public static class FolderScan
{
    /// <summary>Default recursion depth (the root folder is depth 0). Deep enough for any real media library,
    /// bounded so a runaway or looped tree can't hang the drop.</summary>
    public const int DefaultMaxDepth = 12;

    /// <summary>Media files under <paramref name="root"/>, recursing up to <paramref name="maxDepth"/> levels
    /// below it (root = depth 0), natural-sorted by full path so the playlist plays in the obvious order.</summary>
    public static List<string> MediaFiles(string root, int maxDepth = DefaultMaxDepth)
    {
        var found = new List<string>();
        Walk(root, 0, maxDepth, found);
        found.Sort(NaturalComparer.Instance);
        return found;
    }

    private static void Walk(string dir, int depth, int maxDepth, List<string> acc)
    {
        // Files at this level. A dir whose files can't be listed still gets its subdirectories tried.
        foreach (string f in SafeList(() => Directory.EnumerateFiles(dir)))
            if (MediaFormats.IsMedia(f))
                acc.Add(f);

        if (depth >= maxDepth)
            return;

        foreach (string sub in SafeList(() => Directory.EnumerateDirectories(dir)))
        {
            try
            {
                // Symlinks/junctions can loop back into the tree (infinite walk) or escape it — skip them.
                if ((File.GetAttributes(sub) & FileAttributes.ReparsePoint) != 0)
                    continue;
            }
            catch { continue; }
            Walk(sub, depth + 1, maxDepth, acc);
        }
    }

    /// <summary>Materialize a directory enumeration, keeping whatever entries were produced before any fault.
    /// <see cref="Directory.EnumerateFiles(string)"/> / <see cref="Directory.EnumerateDirectories(string)"/>
    /// stream lazily, so a single transiently-removed or inaccessible entry can throw mid-iteration — which, if
    /// it aborted the whole level, would drop the siblings already seen AND skip every readable subfolder after
    /// it. Salvaging the partial result keeps the playlist as complete as the filesystem allowed.</summary>
    internal static List<string> SafeList(Func<IEnumerable<string>> enumerate)
    {
        var result = new List<string>();
        try
        {
            foreach (string item in enumerate())
                result.Add(item);
        }
        catch { /* keep the partial result gathered before the fault */ }
        return result;
    }
}
