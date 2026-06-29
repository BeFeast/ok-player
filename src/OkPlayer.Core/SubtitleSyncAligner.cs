using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;

namespace OkPlayer.Core;

/// <summary>One ASR word with its <b>absolute</b> media time in seconds (the clip's start time plus the word's
/// offset within the clip). Segment-level ASR works too — give every word in a segment the segment's start time;
/// timing is just coarser.</summary>
public sealed record AsrToken(string Text, double Time);

/// <summary>The result of aligning an ASR sample against a subtitle track: the absolute <c>sub-delay</c> to apply
/// (seconds; positive = subtitles later), a 0..1 confidence (share of matched cues that agree on this offset),
/// and how many cues voted for it.</summary>
public sealed record SubtitleSyncResult(double OffsetSeconds, double Confidence, int Votes);

/// <summary>
/// Computes the subtitle delay that best aligns a loaded subtitle track to the actual audio, from a short ASR
/// sample taken at the current position. For each subtitle cue we find where its words best match within the ASR
/// sample (a <i>broad</i> search across the whole track, since the track may be off by a large unknown amount);
/// each good match yields a candidate offset (matched-audio-time − cue-time). A constant delay makes those
/// candidates agree, so we cluster them and return the densest cluster's average. Pure and engine-free → unit-tested.
/// </summary>
public static class SubtitleSyncAligner
{
    /// <param name="asr">ASR words with absolute media times.</param>
    /// <param name="cues">The loaded subtitle track's cues.</param>
    /// <param name="minCueWords">Skip cues shorter than this — too few words to match distinctively.</param>
    /// <param name="minMatch">Per-cue match score (0..1 word overlap) required to vote.</param>
    /// <param name="binSeconds">Offset clustering granularity; candidates within a bin count as agreeing.</param>
    /// <param name="maxOffsetSeconds">Reject implausibly large offsets (likely a spurious match).</param>
    /// <returns>The best offset + confidence, or null when nothing matched well enough.</returns>
    public static SubtitleSyncResult? Align(
        IReadOnlyList<AsrToken> asr,
        IReadOnlyList<SrtCue> cues,
        int minCueWords = 2,
        double minMatch = 0.6,
        double binSeconds = 0.25,
        double maxOffsetSeconds = 120.0)
    {
        if (asr is null || cues is null || asr.Count == 0 || cues.Count == 0)
            return null;

        // Flatten ASR to (word, time) pairs.
        var words = new List<string>();
        var times = new List<double>();
        foreach (AsrToken t in asr)
            foreach (string w in Tokenize(t.Text))
            {
                words.Add(w);
                times.Add(t.Time);
            }
        if (words.Count < 3)
            return null;

        var candidates = new List<double>();
        foreach (SrtCue cue in cues)
        {
            var cueWords = Tokenize(cue.Text).ToList();
            if (cueWords.Count < minCueWords)
                continue;

            (int firstMatchedAt, double score) = BestWindow(words, cueWords);
            if (firstMatchedAt < 0 || score < minMatch)
                continue;

            double offset = times[firstMatchedAt] - cue.Start;
            if (Math.Abs(offset) <= maxOffsetSeconds)
                candidates.Add(offset);
        }
        if (candidates.Count == 0)
            return null;

        // Cluster offsets into bins; the densest bin wins. Average within the bin for a sub-bin-precise result.
        IGrouping<double, double> best = candidates
            .GroupBy(o => Math.Round(o / binSeconds))
            .OrderByDescending(g => g.Count())
            .ThenBy(g => Math.Abs(g.Average())) // tie-break toward the smaller correction
            .First();

        return new SubtitleSyncResult(best.Average(), (double)best.Count() / candidates.Count, best.Count());
    }

    // Slide a window the width of the cue across the ASR words; for each, count how many of the cue's words are
    // present (order-agnostic). Select by recall (share of the cue matched), tie-broken by precision (share of the
    // window that's cue words) so a window dominated by a neighbouring cue's words — sharing only a common word
    // like "the" — loses to the tight window over the real phrase. Returns the FIRST matched ASR index (for
    // timing) and the recall score. O(asr·cue) — cheap for a short cue against a 10 s sample.
    private static (int FirstMatchedAt, double Score) BestWindow(List<string> asr, List<string> cue)
    {
        var cueSet = new HashSet<string>(cue);
        int bestFirst = -1;
        double bestRecall = 0, bestPrecision = 0;
        int width = cue.Count;

        for (int s = 0; s < asr.Count; s++)
        {
            int end = Math.Min(asr.Count, s + width);
            int matched = 0, firstMatched = -1;
            var remaining = new HashSet<string>(cueSet);
            for (int i = s; i < end; i++)
            {
                if (remaining.Remove(asr[i]))
                {
                    matched++;
                    if (firstMatched < 0) firstMatched = i;
                }
            }
            double recall = (double)matched / cue.Count;
            double precision = end > s ? (double)matched / (end - s) : 0;
            if (recall > bestRecall || (recall == bestRecall && precision > bestPrecision))
            {
                bestRecall = recall;
                bestPrecision = precision;
                bestFirst = firstMatched;
                if (recall >= 0.999 && precision >= 0.999) break;
            }
        }
        return (bestFirst, bestRecall);
    }

    // Lowercase, split on any non-alphanumeric. Unicode letters/digits kept (works for non-Latin scripts too).
    private static IEnumerable<string> Tokenize(string s)
    {
        var sb = new StringBuilder();
        foreach (char c in s)
        {
            if (char.IsLetterOrDigit(c))
                sb.Append(char.ToLowerInvariant(c));
            else if (sb.Length > 0)
            {
                yield return sb.ToString();
                sb.Clear();
            }
        }
        if (sb.Length > 0)
            yield return sb.ToString();
    }
}
