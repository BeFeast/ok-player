using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text.RegularExpressions;

namespace OkPlayer.Core;

/// <summary>One lyric line. <paramref name="Time"/> is when it becomes the active line; <paramref name="Text"/>
/// is the line with any enhanced word-timestamp tags stripped. An empty <c>Text</c> is a deliberate gap
/// (an instrumental break carried in the sheet), kept so the highlight dwells on nothing during it.</summary>
public readonly record struct LrcLine(TimeSpan Time, string Text);

/// <summary>A parsed LRC lyric sheet. <see cref="HasTimings"/> distinguishes a real synced sheet (lines carry
/// <c>[mm:ss.xx]</c> stamps, sorted ascending — drive the karaoke highlight off <see cref="LyricSync"/>) from
/// plain lyrics (no stamps — every line is <see cref="TimeSpan.Zero"/>; render as a static scroll). ID tags
/// (<c>ti/ar/al/length</c>) are surfaced when present.</summary>
public sealed class LrcDocument
{
    /// <summary>The canonical "no lyrics" document.</summary>
    public static readonly LrcDocument Empty = new(Array.Empty<LrcLine>(), false, null, null, null, null);

    /// <summary>Lines in ascending time order when <see cref="HasTimings"/>; document order otherwise.</summary>
    public IReadOnlyList<LrcLine> Lines { get; }

    /// <summary>True when the sheet carries real timestamps (synced karaoke); false for plain text lyrics.</summary>
    public bool HasTimings { get; }

    public string? Title { get; }
    public string? Artist { get; }
    public string? Album { get; }

    /// <summary>The <c>[length:mm:ss]</c> tag if present — a cheap sanity check against the track duration.</summary>
    public TimeSpan? Length { get; }

    public bool IsEmpty => Lines.Count == 0;

    internal LrcDocument(IReadOnlyList<LrcLine> lines, bool hasTimings, string? title, string? artist,
                         string? album, TimeSpan? length)
    {
        Lines = lines;
        HasTimings = hasTimings;
        Title = title;
        Artist = artist;
        Album = album;
        Length = length;
    }
}

/// <summary>Parser for the LRC lyric format (the line-level synced format LRCLIB and the karaoke service emit:
/// <c>[mm:ss.xx] text</c>, optional ID tags, optional multiple stamps per line, optional <c>[offset:±ms]</c>).
/// Tolerant by design — a malformed tag is skipped, never throws — because lyric sheets are crowd-sourced and
/// ragged. Enhanced word-level tags (<c>&lt;mm:ss.xx&gt;</c>) are stripped to clean line text (word-level
/// highlight isn't a v1 target). Engine- and UI-free for headless tests.</summary>
public static class Lrc
{
    // A bracket time tag body: minutes:seconds(.fraction). Minutes may exceed 59; fraction 1–3 digits, '.' or ':'.
    private static readonly Regex TimeTag = new(@"^(\d+):([0-5]?\d)(?:[.:](\d{1,3}))?$", RegexOptions.Compiled);
    // Enhanced per-word stamps inside a line, e.g. "<00:12.50>word" — removed for the v1 line-level renderer.
    private static readonly Regex WordTag = new(@"<\d+:\d{1,2}(?:[.:]\d{1,3})?>", RegexOptions.Compiled);

    /// <summary>Parse LRC text into a document. Returns <see cref="LrcDocument.Empty"/> for null/blank input.
    /// When no line carries a timestamp the result is plain lyrics (<see cref="LrcDocument.HasTimings"/> false).</summary>
    public static LrcDocument Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
            return LrcDocument.Empty;

        string? title = null, artist = null, album = null;
        TimeSpan? length = null;
        double offsetMs = 0;
        var timed = new List<LrcLine>();
        var plain = new List<string>();

        foreach (string raw in SplitLines(text))
        {
            // Peel off the leading "[...]" groups: each is either a time stamp or an ID tag.
            var stamps = new List<TimeSpan>();
            int i = 0;
            while (i < raw.Length && raw[i] == '[')
            {
                int close = raw.IndexOf(']', i + 1);
                if (close < 0)
                    break;
                string tag = raw.Substring(i + 1, close - i - 1);
                if (TryParseTime(tag, out TimeSpan ts))
                    stamps.Add(ts);
                else
                    ApplyIdTag(tag, ref title, ref artist, ref album, ref length, ref offsetMs);
                i = close + 1;
            }

            string content = NormalizeSpaces(WordTag.Replace(raw.Substring(i), " ")).Trim();

            if (stamps.Count > 0)
                foreach (TimeSpan ts in stamps)
                    timed.Add(new LrcLine(ts, content)); // offset + sort applied once, below
            else if (content.Length > 0)
                plain.Add(content); // a text line with no stamp — only meaningful if the sheet has no timings at all
        }

        if (timed.Count > 0)
        {
            // A positive [offset] makes lyrics appear earlier, i.e. subtract it from each stamp; clamp at zero.
            TimeSpan offset = TimeSpan.FromMilliseconds(offsetMs);
            for (int k = 0; k < timed.Count; k++)
            {
                TimeSpan t = timed[k].Time - offset;
                timed[k] = timed[k] with { Time = t < TimeSpan.Zero ? TimeSpan.Zero : t };
            }
            timed.Sort(static (a, b) => a.Time.CompareTo(b.Time));
            return new LrcDocument(timed, true, title, artist, album, length);
        }

        // No timestamps anywhere → plain lyrics. Keep line order; every line sits at zero.
        var plainLines = new List<LrcLine>(plain.Count);
        foreach (string p in plain)
            plainLines.Add(new LrcLine(TimeSpan.Zero, p));
        return new LrcDocument(plainLines, false, title, artist, album, length);
    }

    private static IEnumerable<string> SplitLines(string text)
        => text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');

    private static bool TryParseTime(string tag, out TimeSpan time)
    {
        time = default;
        Match m = TimeTag.Match(tag.Trim());
        if (!m.Success)
            return false;
        int minutes = int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture);
        int seconds = int.Parse(m.Groups[2].Value, CultureInfo.InvariantCulture);
        double frac = 0;
        if (m.Groups[3].Success)
        {
            string f = m.Groups[3].Value;
            frac = int.Parse(f, CultureInfo.InvariantCulture) / Math.Pow(10, f.Length); // 1–3 digits → fractional s
        }
        time = TimeSpan.FromSeconds(minutes * 60 + seconds + frac);
        return true;
    }

    private static void ApplyIdTag(string tag, ref string? title, ref string? artist, ref string? album,
                                   ref TimeSpan? length, ref double offsetMs)
    {
        int colon = tag.IndexOf(':');
        if (colon <= 0)
            return;
        string key = tag.Substring(0, colon).Trim().ToLowerInvariant();
        string value = tag.Substring(colon + 1).Trim();
        switch (key)
        {
            case "ti": title = NullIfEmpty(value); break;
            case "ar": artist = NullIfEmpty(value); break;
            case "al": album = NullIfEmpty(value); break;
            case "length": if (TryParseTime(value, out TimeSpan len)) length = len; break;
            case "offset": if (double.TryParse(value, NumberStyles.Integer | NumberStyles.AllowLeadingSign,
                               CultureInfo.InvariantCulture, out double ms)) offsetMs = ms; break;
        }
    }

    private static string NormalizeSpaces(string s) => Regex.Replace(s, @"\s+", " ");
    private static string? NullIfEmpty(string s) => string.IsNullOrWhiteSpace(s) ? null : s;
}

/// <summary>Maps a playback position to the active lyric line — the core of the synced highlight. Pure and
/// allocation-free so it can run on every <c>time-pos</c> tick.</summary>
public static class LyricSync
{
    /// <summary>Index of the line that should be highlighted at <paramref name="positionSeconds"/>: the last line
    /// whose timestamp is ≤ the position. Returns −1 before the first line (or for an empty list). Assumes
    /// <paramref name="lines"/> is ascending by time (as <see cref="Lrc.Parse"/> produces for a synced sheet).</summary>
    public static int ActiveIndex(IReadOnlyList<LrcLine> lines, double positionSeconds)
    {
        if (lines is null || lines.Count == 0)
            return -1;
        int lo = 0, hi = lines.Count - 1, ans = -1;
        while (lo <= hi)
        {
            int mid = lo + ((hi - lo) >> 1);
            if (lines[mid].Time.TotalSeconds <= positionSeconds)
            {
                ans = mid;
                lo = mid + 1;
            }
            else
            {
                hi = mid - 1;
            }
        }
        return ans;
    }
}
