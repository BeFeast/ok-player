using System;
using System.Globalization;
using System.Linq;

namespace OkPlayer.App.Services;

/// <summary>Which day-group a history row falls into. Rows are bucketed by how long ago the file was
/// last opened, then shown under a header (<see cref="HistoryFormat.BucketHeader"/>).</summary>
public enum HistoryBucket { Today, Yesterday, EarlierThisWeek, Earlier }

/// <summary>The watch-state a history row renders: a finished chip, a "time left" countdown, or a
/// "barely started" hint for files only a few minutes in.</summary>
public enum HistoryStateKind { Finished, Progress, Barely }

/// <summary>Derived row state: the kind, the watched fraction (0..1, for the thumbnail fill) and the
/// human label shown on the right (e.g. "Finished", "23m left", "2m in · 4%").</summary>
public readonly record struct HistoryRowState(HistoryStateKind Kind, double Percent, string Label);

/// <summary>Pure presentation helpers for the History list: day-bucketing, the relative "when" label,
/// the resume-state label, and a short folder breadcrumb. No engine or UI dependency, so it is unit
/// tested headlessly. Mirrors the Claude Design spec (design/OK-Player-History.dc.html — deriveState,
/// the bucket table and the master "when" strings) exactly.</summary>
public static class HistoryFormat
{
    /// <summary>Bucket a file by its last-opened day relative to <paramref name="nowLocal"/>:
    /// same day → Today, one day back → Yesterday, 2–6 days → EarlierThisWeek (a rolling week, not the
    /// calendar week), 7+ → Earlier. A future timestamp (clock skew) folds into Today.</summary>
    public static HistoryBucket BucketFor(DateTime lastOpenedLocal, DateTime nowLocal)
    {
        int days = (nowLocal.Date - lastOpenedLocal.Date).Days;
        if (days <= 0) return HistoryBucket.Today;
        if (days == 1) return HistoryBucket.Yesterday;
        if (days <= 6) return HistoryBucket.EarlierThisWeek;
        return HistoryBucket.Earlier;
    }

    /// <summary>The upper-case group header for a bucket (matches the design's bucket table).</summary>
    public static string BucketHeader(HistoryBucket bucket) => bucket switch
    {
        HistoryBucket.Today => "TODAY",
        HistoryBucket.Yesterday => "YESTERDAY",
        HistoryBucket.EarlierThisWeek => "EARLIER THIS WEEK",
        _ => "EARLIER",
    };

    /// <summary>The right-column timestamp label: "Today 21:14", "Yest. 16:40", a weekday + time
    /// ("Tue 21:48") within the week, else a day + month ("12 Jun"). 24-hour clock and invariant
    /// weekday/month names so the English UI never localizes to "вт"/"июн".</summary>
    public static string WhenLabel(DateTime lastOpenedLocal, DateTime nowLocal)
    {
        return BucketFor(lastOpenedLocal, nowLocal) switch
        {
            HistoryBucket.Today => "Today " + lastOpenedLocal.ToString("HH:mm", CultureInfo.InvariantCulture),
            HistoryBucket.Yesterday => "Yest. " + lastOpenedLocal.ToString("HH:mm", CultureInfo.InvariantCulture),
            HistoryBucket.EarlierThisWeek => lastOpenedLocal.ToString("ddd HH:mm", CultureInfo.InvariantCulture),
            _ => lastOpenedLocal.ToString("d MMM", CultureInfo.InvariantCulture),
        };
    }

    /// <summary>Resume state for a row. Finished files show a chip; files under 5% watched show a
    /// "barely started" hint ("3m in · 4%"); everything else shows time remaining ("23m left").
    /// Minutes are clamped to ≥ 1 so a just-started or nearly-done file never reads "0m".</summary>
    public static HistoryRowState DeriveState(double position, double duration, bool finished)
    {
        if (finished)
            return new HistoryRowState(HistoryStateKind.Finished, 0, "");
        double pct = duration > 0 ? position / duration : 0;
        if (pct < 0.05)
        {
            int minIn = Math.Max(1, (int)Math.Round(position / 60));
            return new HistoryRowState(HistoryStateKind.Barely, pct, $"{minIn}m in · {(int)Math.Round(pct * 100)}%");
        }
        int leftMin = Math.Max(1, (int)Math.Ceiling((duration - position) / 60));
        return new HistoryRowState(HistoryStateKind.Progress, pct, $"{leftMin}m left");
    }

    /// <summary>A short location breadcrumb for the row's secondary line: the last two folder segments
    /// of the file's directory joined with " › " (e.g. "Severance › Season 02"). Bare drive roots
    /// ("D:") are dropped from the breadcrumb but kept as a fallback when nothing else remains.
    /// Splits on both separators explicitly — OK Player is Windows-only (paths use '\'), but the
    /// engine-agnostic Core tests run headless on Linux, where Path.GetDirectoryName wouldn't see '\'.</summary>
    public static string FolderLabel(string path)
    {
        if (string.IsNullOrEmpty(path))
            return string.Empty;
        string[] parts = path.Split(new[] { '\\', '/' }, StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length <= 1)
            return string.Empty; // just a bare filename — no folder to show
        // Drop the file name (last part), then the bare drive letter ("C:") so it doesn't eat a slot.
        string[] dirs = parts.Take(parts.Length - 1).Where(s => !(s.Length == 2 && s[1] == ':')).ToArray();
        if (dirs.Length == 0)
            return parts[^2]; // e.g. a drive-root file "C:\clip.mkv" -> "C:"
        int take = Math.Min(2, dirs.Length);
        return string.Join(" › ", dirs[^take..]);
    }
}
