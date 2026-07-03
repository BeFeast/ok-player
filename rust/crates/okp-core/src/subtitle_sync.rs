//! Subtitle↔audio auto-sync alignment — port of `src/OkPlayer.Core/SubtitleSyncAligner.cs`; the
//! C# suite in `tests/OkPlayer.Tests/SubtitleSyncTests.cs` is the executable spec (divergences
//! are recorded in `docs/core-compatibility.md`). Computes the subtitle delay that best aligns a
//! loaded subtitle track to the actual audio, from a short ASR sample taken at the current
//! position. For each subtitle cue we find where its words best match within the ASR sample (a
//! *broad* search across the whole track, since the track may be off by a large unknown amount);
//! each good match yields a candidate offset (matched-audio-time − cue-time). A constant delay
//! makes those candidates agree, so we cluster them and return the densest cluster's average.
//! Pure and engine-free → unit-tested.

use std::collections::HashMap;

use crate::srt::SrtCue;

/// One ASR word with its **absolute** media time in seconds (the clip's start time plus the
/// word's offset within the clip). Segment-level ASR works too — give every word in a segment the
/// segment's start time; timing is just coarser.
#[derive(Debug, Clone, PartialEq)]
pub struct AsrToken {
    pub text: String,
    pub time_seconds: f64,
}

/// The result of aligning an ASR sample against a subtitle track: the absolute `sub-delay` to
/// apply (seconds; positive = subtitles later), a 0..1 confidence (share of matched cues that
/// agree on this offset), and how many cues voted for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubtitleSyncResult {
    pub offset_seconds: f64,
    pub confidence: f64,
    pub votes: usize,
}

/// Tuning knobs for [`align`], mirroring the C# default parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlignOptions {
    /// Skip cues shorter than this — too few words to match distinctively.
    pub min_cue_words: usize,
    /// Per-cue match score (0..1 word overlap) required to vote.
    pub min_match: f64,
    /// Offset clustering granularity; candidates within a bin count as agreeing.
    pub bin_seconds: f64,
    /// Reject implausibly large offsets (likely a spurious match).
    pub max_offset_seconds: f64,
}

impl Default for AlignOptions {
    fn default() -> Self {
        Self {
            min_cue_words: 2,
            min_match: 0.6,
            bin_seconds: 0.25,
            max_offset_seconds: 120.0,
        }
    }
}

/// Align `asr` (ASR words with absolute media times) against `cues` (the loaded subtitle track's
/// cues). Returns the best offset + confidence, or `None` when nothing matched well enough.
pub fn align(
    asr: &[AsrToken],
    cues: &[SrtCue],
    options: AlignOptions,
) -> Option<SubtitleSyncResult> {
    if asr.is_empty() || cues.is_empty() {
        return None;
    }

    // Flatten ASR to (word, time) pairs.
    let mut words: Vec<String> = Vec::new();
    let mut times: Vec<f64> = Vec::new();
    for token in asr {
        for word in tokenize(&token.text) {
            words.push(word);
            times.push(token.time_seconds);
        }
    }
    if words.len() < 3 {
        return None;
    }

    // Typical gap between spoken words, from the ASR's own cadence — used to back out the cue's
    // start time when ASR drops a cue's leading word(s) (the first *matched* word is then a later
    // one, so subtracting the cue start raw would skew the vote later). Degrades to 0 for
    // segment-level ASR (no intra-segment timing).
    let word_gap = median_consecutive_gap(&times);

    let mut candidates: Vec<f64> = Vec::new();
    for cue in cues {
        let cue_words = tokenize(&cue.text);
        if cue_words.len() < options.min_cue_words {
            continue;
        }

        let Some((first_matched_at, score)) = best_window(&words, &cue_words) else {
            continue;
        };
        if score < options.min_match {
            continue;
        }

        // Where the matched word sits in the cue: estimate that word's spoken time as
        // cue start + pos·word_gap, so a dropped leading word doesn't bias the offset by its
        // position.
        let cue_pos = cue_words
            .iter()
            .position(|w| *w == words[first_matched_at])
            .unwrap_or(0);
        let offset = times[first_matched_at] - (cue.start_seconds + cue_pos as f64 * word_gap);
        if offset.abs() <= options.max_offset_seconds {
            candidates.push(offset);
        }
    }
    if candidates.is_empty() {
        return None;
    }

    // Cluster by a sliding window of width bin_seconds over the sorted offsets — a tolerance, not
    // fixed bin boundaries (so 3.12 and 3.13 always cluster together, where round(o/bin) could
    // split them). The densest window wins; its members are averaged for a precise result.
    candidates.sort_by(f64::total_cmp);
    let mut best_lo = 0;
    let mut best_count = 0;
    let mut hi = 0;
    for lo in 0..candidates.len() {
        if hi < lo {
            hi = lo;
        }
        while hi + 1 < candidates.len()
            && candidates[hi + 1] - candidates[lo] <= options.bin_seconds
        {
            hi += 1;
        }
        let count = hi - lo + 1;
        if count > best_count {
            best_count = count;
            best_lo = lo;
        }
    }
    let sum: f64 = candidates[best_lo..best_lo + best_count].iter().sum();
    Some(SubtitleSyncResult {
        offset_seconds: sum / best_count as f64,
        confidence: best_count as f64 / candidates.len() as f64,
        votes: best_count,
    })
}

/// Slide a window the width of the cue across the ASR words; for each, count how many of the
/// cue's words are present (order-agnostic). Select by recall (share of the cue matched),
/// tie-broken by precision (share of the window that's cue words) so a window dominated by a
/// neighbouring cue's words — sharing only a common word like "the" — loses to the tight window
/// over the real phrase. Returns the FIRST matched ASR index (for timing) and the recall score,
/// or `None` when no window matched any word. O(asr·cue) — cheap for a short cue against a 10 s
/// sample.
fn best_window(asr: &[String], cue: &[String]) -> Option<(usize, f64)> {
    // Multiset of the cue's words (with multiplicity), so a repeated-word cue like "no no no" can
    // match all three — a plain set would cap its recall at 1/3 and silence it.
    let mut cue_counts: HashMap<&str, usize> = HashMap::new();
    for word in cue {
        *cue_counts.entry(word.as_str()).or_insert(0) += 1;
    }

    let mut best_first: Option<usize> = None;
    let mut best_recall = 0.0_f64;
    let mut best_precision = 0.0_f64;
    let width = cue.len();

    for s in 0..asr.len() {
        let end = asr.len().min(s + width);
        let mut matched = 0_usize;
        let mut first_matched: Option<usize> = None;
        let mut remaining = cue_counts.clone();
        for (i, word) in asr.iter().enumerate().take(end).skip(s) {
            if let Some(n) = remaining.get_mut(word.as_str())
                && *n > 0
            {
                *n -= 1;
                matched += 1;
                if first_matched.is_none() {
                    first_matched = Some(i);
                }
            }
        }
        let recall = matched as f64 / cue.len() as f64;
        let precision = if end > s {
            matched as f64 / (end - s) as f64
        } else {
            0.0
        };
        if recall > best_recall || (recall == best_recall && precision > best_precision) {
            best_recall = recall;
            best_precision = precision;
            best_first = first_matched;
            if recall >= 0.999 && precision >= 0.999 {
                break;
            }
        }
    }
    best_first.map(|first| (first, best_recall))
}

/// Median of the positive gaps between consecutive ASR word times — a robust per-clip word
/// cadence. Returns 0 when there's no intra-word timing (e.g. segment-level ASR where consecutive
/// words share a timestamp).
fn median_consecutive_gap(times: &[f64]) -> f64 {
    let mut gaps: Vec<f64> = times
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|gap| *gap > 0.0)
        .collect();
    if gaps.is_empty() {
        return 0.0;
    }
    gaps.sort_by(f64::total_cmp);
    gaps[gaps.len() / 2]
}

/// Lowercase, split on any non-alphanumeric. Unicode letters/digits kept (works for non-Latin
/// scripts too).
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::assert_close;

    fn cue(index: i32, start: f64, end: f64, text: &str) -> SrtCue {
        SrtCue {
            index,
            start_seconds: start,
            end_seconds: end,
            text: text.to_string(),
        }
    }

    fn token(text: &str, time: f64) -> AsrToken {
        AsrToken {
            text: text.to_string(),
            time_seconds: time,
        }
    }

    /// Three cues as AUTHORED in the .srt.
    fn cues() -> Vec<SrtCue> {
        vec![
            cue(1, 10.0, 12.0, "The quick brown fox"),
            cue(2, 13.0, 15.0, "jumps over the lazy dog"),
            cue(3, 16.0, 18.0, "hello there general kenobi"),
        ]
    }

    /// Build an ASR sample for the same lines spoken at `t1`, `t2`, `t3`.
    fn spoken(t1: f64, t2: f64, t3: f64) -> Vec<AsrToken> {
        let mut list = Vec::new();
        for (text, start) in [
            ("the quick brown fox", t1),
            ("jumps over the lazy dog", t2),
            ("hello there general kenobi", t3),
        ] {
            for (i, word) in text.split(' ').enumerate() {
                list.push(token(word, start + i as f64 * 0.4));
            }
        }
        list
    }

    #[test]
    fn subtitles_early_returns_positive_delay() {
        // Audio actually happens 3 s LATER than the cues are authored → subs need +3 s delay.
        let asr = spoken(13.0, 16.0, 19.0);
        let r = align(&asr, &cues(), AlignOptions::default()).expect("aligned");
        assert_close(r.offset_seconds, 3.0, 0.05);
        assert!(r.votes >= 2, "votes {}", r.votes);
        assert!(r.confidence > 0.6, "confidence {}", r.confidence);
    }

    #[test]
    fn subtitles_late_returns_negative_delay() {
        // Audio happens 2 s EARLIER than authored → subs need −2 s delay.
        let asr = spoken(8.0, 11.0, 14.0);
        let r = align(&asr, &cues(), AlignOptions::default()).expect("aligned");
        assert_close(r.offset_seconds, -2.0, 0.05);
    }

    #[test]
    fn already_in_sync_returns_near_zero() {
        let asr = spoken(10.0, 13.0, 16.0);
        let r = align(&asr, &cues(), AlignOptions::default()).expect("aligned");
        assert_close(r.offset_seconds, 0.0, 0.05);
    }

    #[test]
    fn imperfect_asr_still_aligns() {
        // One word wrong / dropped per line — overlap match should still carry it.
        let asr = vec![
            token("the", 13.0),
            token("quick", 13.4),
            token("BROWN", 13.8), // case-insensitive
            token("jumps", 16.0),
            token("over", 16.4),
            token("lazy", 17.2), // "the"/"dog" dropped
        ];
        let r = align(&asr, &cues(), AlignOptions::default()).expect("aligned");
        assert_close(r.offset_seconds, 3.0, 0.05);
    }

    #[test]
    fn repeated_word_cue_still_matches() {
        // "no no no" must match all three occurrences (multiset), not cap recall at 1/3.
        let cues = [cue(1, 20.0, 21.0, "No no no")];
        let asr = vec![token("no", 24.0), token("no", 24.5), token("no", 25.0)];
        let r = align(&asr, &cues, AlignOptions::default()).expect("aligned");
        assert_close(r.offset_seconds, 4.0, 0.05); // 24.0 − 20.0
    }

    #[test]
    fn near_boundary_offsets_cluster_together() {
        // Candidate offsets straddling a fixed-bin boundary (≈3.12 / ≈3.13) must still cluster
        // (tolerance window).
        let cues = [
            cue(1, 10.00, 12.0, "alpha bravo charlie"),
            cue(2, 20.00, 22.0, "delta echo foxtrot"),
        ];
        let asr = vec![
            token("alpha", 13.12),
            token("bravo", 13.5),
            token("charlie", 13.9),
            token("delta", 23.13),
            token("echo", 23.5),
            token("foxtrot", 23.9),
        ];
        let r = align(&asr, &cues, AlignOptions::default()).expect("aligned");
        assert_eq!(r.votes, 2); // both cues in one cluster despite the boundary
        assert_close(r.offset_seconds, 3.125, 0.005);
    }

    #[test]
    fn dropped_leading_word_does_not_skew_offset() {
        // ASR misses each cue's first word; the offset must still resolve to +3.0, not
        // +3.0+wordgap, because the matched word's cue position is backed out with the ASR word
        // cadence.
        let asr = vec![
            token("quick", 13.4), // cue1 minus "The"
            token("brown", 13.8),
            token("fox", 14.2),
            token("over", 16.4), // cue2 minus "jumps"
            token("the", 16.8),
            token("lazy", 17.2),
            token("dog", 17.6),
        ];
        let r = align(&asr, &cues(), AlignOptions::default()).expect("aligned");
        assert_close(r.offset_seconds, 3.0, 0.05);
    }

    #[test]
    fn no_match_returns_none() {
        let asr = vec![
            token("completely", 5.0),
            token("unrelated", 5.5),
            token("spoken", 6.0),
            token("words", 6.5),
        ];
        assert_eq!(align(&asr, &cues(), AlignOptions::default()), None);
    }

    #[test]
    fn empty_inputs_return_none() {
        assert_eq!(align(&[], &cues(), AlignOptions::default()), None);
        assert_eq!(
            align(&spoken(10.0, 13.0, 16.0), &[], AlignOptions::default()),
            None
        );
    }
}
