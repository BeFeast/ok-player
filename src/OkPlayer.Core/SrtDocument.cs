using System;
using System.Collections.Generic;
using System.Text.RegularExpressions;

namespace OkPlayer.Core;

/// <summary>One SubRip cue: 1-based index, start/end in seconds, and the display text (tags stripped, lines
/// joined with a space).</summary>
public sealed record SrtCue(int Index, double Start, double End, string Text);

/// <summary>
/// A tolerant SubRip (<c>.srt</c>) parser. Engine- and UI-free so the subtitle-sync aligner and its tests can run
/// headlessly. Handles CRLF/LF/CR, a leading BOM, comma or dot millisecond separators, 1–2 digit hours, an
/// optional missing index line, and multi-line cues; skips malformed or empty blocks rather than throwing.
/// </summary>
public static class SrtDocument
{
    // HH:MM:SS,mmm --> HH:MM:SS,mmm  (comma or dot ms; 1–3 ms digits; 1–2 hour digits)
    private static readonly Regex TimeLine = new(
        @"(\d{1,2}):(\d{2}):(\d{2})[,.](\d{1,3})\s*-->\s*(\d{1,2}):(\d{2}):(\d{2})[,.](\d{1,3})",
        RegexOptions.Compiled);

    // <i>…</i> HTML-ish tags and {\an8}-style ASS overrides — dropped for clean matching text.
    private static readonly Regex Tag = new(@"<[^>]+>|\{[^}]*\}", RegexOptions.Compiled);

    public static IReadOnlyList<SrtCue> Parse(string? text)
    {
        var cues = new List<SrtCue>();
        if (string.IsNullOrWhiteSpace(text))
            return cues;

        text = text.Replace("\r\n", "\n").Replace('\r', '\n');
        if (text.Length > 0 && text[0] == '﻿')
            text = text[1..];

        int autoIndex = 0;
        // Split on one or more blank lines — tolerating whitespace-only separators that subtitle editors emit
        // (a plain "\n\n" split would merge two cues when the blank line holds spaces/tabs).
        foreach (string block in Regex.Split(text, @"\n(?:[ \t]*\n)+"))
        {
            if (string.IsNullOrWhiteSpace(block))
                continue;
            string[] lines = block.Split('\n');

            // The time line is line 0 (no index) or line 1 (index present); scan the first few to be safe.
            int timeLineIdx = -1;
            Match m = Match.Empty;
            for (int i = 0; i < lines.Length && i < 3; i++)
            {
                Match mm = TimeLine.Match(lines[i]);
                if (mm.Success) { timeLineIdx = i; m = mm; break; }
            }
            if (timeLineIdx < 0)
                continue;

            double start = ToSeconds(m, 1);
            double end = ToSeconds(m, 5);

            string joined = string.Join(' ', lines[(timeLineIdx + 1)..]);
            string cueText = Tag.Replace(joined, " ").Trim();
            cueText = Regex.Replace(cueText, @"\s+", " ");
            if (cueText.Length == 0)
                continue;

            autoIndex++;
            int index = autoIndex;
            if (timeLineIdx == 1 && int.TryParse(lines[0].Trim(), out int parsed))
                index = parsed;

            cues.Add(new SrtCue(index, start, end, cueText));
        }
        return cues;
    }

    private static double ToSeconds(Match m, int g) =>
        int.Parse(m.Groups[g].Value) * 3600
        + int.Parse(m.Groups[g + 1].Value) * 60
        + int.Parse(m.Groups[g + 2].Value)
        + int.Parse(m.Groups[g + 3].Value.PadRight(3, '0')) / 1000.0;
}
