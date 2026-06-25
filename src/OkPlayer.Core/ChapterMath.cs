using System;
using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>A chapter after merging the file's own with the user's: time-sorted and re-indexed.</summary>
public readonly record struct MergedChapter(int Index, double Time, string Title, bool IsUserDefined);

/// <summary>Pure chapter logic — the merge, current-by-time, prev/next and seek-bar-fraction math behind
/// the player's chapter list. No UI or engine dependency, so it is headless-unit-testable.</summary>
public static class ChapterMath
{
    /// <summary>Merge the file's chapters (read-only) with the user's into one time-sorted, re-indexed list.
    /// A stable sort keeps a file and user chapter at the same timestamp in file-then-user order.</summary>
    public static List<MergedChapter> Merge(
        IReadOnlyList<(double Time, string Title)> fileChapters,
        IReadOnlyList<(double Time, string Title)> userChapters)
    {
        var tagged = new List<(double Time, string Title, bool User, int Order)>();
        int order = 0;
        foreach (var (time, title) in fileChapters)
            tagged.Add((time, title, false, order++));
        foreach (var (time, title) in userChapters)
            tagged.Add((time, title, true, order++));
        tagged.Sort((a, b) =>
        {
            int t = a.Time.CompareTo(b.Time);
            return t != 0 ? t : a.Order.CompareTo(b.Order); // stable: preserve insertion order on ties
        });

        var result = new List<MergedChapter>(tagged.Count);
        for (int i = 0; i < tagged.Count; i++)
            result.Add(new MergedChapter(i, tagged[i].Time, tagged[i].Title, tagged[i].User));
        return result;
    }

    /// <summary>Index of the chapter containing <paramref name="position"/> (the last start &lt;= position,
    /// within <paramref name="epsilon"/>), or -1 before the first chapter. <paramref name="times"/> ascending.</summary>
    public static int CurrentIndex(IReadOnlyList<double> times, double position, double epsilon = 0.25)
    {
        int idx = -1;
        for (int i = 0; i < times.Count; i++)
        {
            if (times[i] <= position + epsilon)
                idx = i;
            else
                break;
        }
        return idx;
    }

    /// <summary>Target index for a prev/next-chapter jump, or null when already at the first/last chapter —
    /// so a jump at a boundary does nothing rather than rewinding to the current chapter's own start.</summary>
    public static int? JumpTarget(int current, int delta, int count)
    {
        if (count == 0)
            return null;
        int target = current + delta;
        return target >= 0 && target < count ? target : null;
    }

    /// <summary>Chapter start positions as 0..1 fractions for the seek-bar tick markers (empty if no duration).</summary>
    public static List<double> Fractions(IReadOnlyList<double> times, double duration)
    {
        var list = new List<double>();
        if (duration > 0)
            foreach (double t in times)
                list.Add(Math.Clamp(t / duration, 0, 1));
        return list;
    }
}
