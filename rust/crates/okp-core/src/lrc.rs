//! Parser for the LRC lyric format — port of `src/OkPlayer.Core/Lrc.cs`; the C# suite in
//! `tests/OkPlayer.Tests/LrcTests.cs` is the executable spec (divergences are recorded in
//! `docs/core-compatibility.md`). The line-level synced format LRCLIB and the karaoke service
//! emit: `[mm:ss.xx] text`, optional ID tags, optional multiple stamps per line, optional
//! `[offset:±ms]`. Tolerant by design — a malformed tag is skipped, never a failure — because
//! lyric sheets are crowd-sourced and ragged. Enhanced word-level tags (`<mm:ss.xx>`) are
//! stripped to clean line text (word-level highlight isn't a v1 target).

/// C# `TimeSpan` bounds. The port keeps times as `f64` seconds but honours the same range guards
/// so pathological stamps and offsets are skipped exactly where the C# parser skips them.
const TIMESPAN_MAX_SECONDS: f64 = i64::MAX as f64 * (1.0 / 10_000_000.0);
const TIMESPAN_MAX_MILLISECONDS: f64 = (i64::MAX / 10_000) as f64;

/// One lyric line. `time_seconds` is when it becomes the active line; `text` is the line with any
/// enhanced word-timestamp tags stripped. An empty `text` is a deliberate gap (an instrumental
/// break carried in the sheet), kept so the highlight dwells on nothing during it.
#[derive(Debug, Clone, PartialEq)]
pub struct LrcLine {
    pub time_seconds: f64,
    pub text: String,
}

/// A parsed LRC lyric sheet. `has_timings` distinguishes a real synced sheet (lines carry
/// `[mm:ss.xx]` stamps, sorted ascending — drive the karaoke highlight off [`active_index`]) from
/// plain lyrics (no stamps — every line sits at zero; render as a static scroll). ID tags
/// (`ti`/`ar`/`al`/`length`) are surfaced when present.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LrcDocument {
    /// Lines in ascending time order when `has_timings`; document order otherwise.
    pub lines: Vec<LrcLine>,
    /// True when the sheet carries real timestamps (synced karaoke); false for plain text lyrics.
    pub has_timings: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// The `[length:mm:ss]` tag if present — a cheap sanity check against the track duration.
    pub length_seconds: Option<f64>,
}

impl LrcDocument {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[derive(Default)]
struct IdTags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    length_seconds: Option<f64>,
    offset_ms: f64,
}

/// Parse LRC text into a document. Returns the empty document for `None`/blank input. When no
/// line carries a timestamp the result is plain lyrics (`has_timings` false).
pub fn parse(text: Option<&str>) -> LrcDocument {
    let Some(text) = text else {
        return LrcDocument::default();
    };
    if text.trim().is_empty() {
        return LrcDocument::default();
    }

    let mut tags = IdTags::default();
    let mut timed: Vec<LrcLine> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    for raw in normalized.split('\n') {
        // Peel off the leading "[...]" groups: each is either a time stamp or an ID tag.
        let bytes = raw.as_bytes();
        let mut stamps: Vec<f64> = Vec::new();
        let mut i = 0;
        while i < bytes.len() && bytes[i] == b'[' {
            let Some(gap) = raw[i + 1..].find(']') else {
                break;
            };
            let close = i + 1 + gap;
            let tag = &raw[i + 1..close];
            if let Some(seconds) = parse_time_tag(tag) {
                stamps.push(seconds);
            } else if !apply_id_tag(tag, &mut tags) {
                break; // an unrecognized bracket (e.g. a "[Chorus]" section header) — keep it as lyric text
            }
            i = close + 1;
        }

        let content = normalize_spaces(&strip_word_tags(&raw[i..]));

        if stamps.is_empty() {
            if !content.is_empty() {
                plain.push(content); // a text line with no stamp — only meaningful if the sheet has no timings at all
            }
        } else {
            for seconds in stamps {
                timed.push(LrcLine {
                    time_seconds: seconds,
                    text: content.clone(),
                }); // offset + sort applied once, below
            }
        }
    }

    if !timed.is_empty() {
        // A positive [offset] makes lyrics appear earlier, i.e. subtract it from each stamp; clamp
        // at zero. A pathological offset (huge / NaN) is ignored rather than allowed to overflow.
        let offset_seconds =
            if !tags.offset_ms.is_nan() && tags.offset_ms.abs() < TIMESPAN_MAX_MILLISECONDS {
                tags.offset_ms / 1000.0
            } else {
                0.0
            };
        for line in &mut timed {
            line.time_seconds = (line.time_seconds - offset_seconds).max(0.0);
        }
        timed.sort_by(|a, b| a.time_seconds.total_cmp(&b.time_seconds));
        return LrcDocument {
            lines: timed,
            has_timings: true,
            title: tags.title,
            artist: tags.artist,
            album: tags.album,
            length_seconds: tags.length_seconds,
        };
    }

    // No timestamps anywhere → plain lyrics. Keep line order; every line sits at zero.
    LrcDocument {
        lines: plain
            .into_iter()
            .map(|text| LrcLine {
                time_seconds: 0.0,
                text,
            })
            .collect(),
        has_timings: false,
        title: tags.title,
        artist: tags.artist,
        album: tags.album,
        length_seconds: tags.length_seconds,
    }
}

/// Index of the line that should be highlighted at `position_seconds`: the last line whose
/// timestamp is ≤ the position. Returns `None` before the first line (or for an empty list).
/// Assumes `lines` is ascending by time (as [`parse`] produces for a synced sheet). Pure and
/// allocation-free so it can run on every `time-pos` tick.
pub fn active_index(lines: &[LrcLine], position_seconds: f64) -> Option<usize> {
    lines
        .partition_point(|line| line.time_seconds <= position_seconds)
        .checked_sub(1)
}

/// Parse a bracket time tag body: minutes:seconds(.fraction). Minutes may exceed 59; the fraction
/// is 1–3 digits after a '.' or ':', read as a decimal fraction of a second by digit count
/// (tenths/centi/milli) — so `[mm:ss:03]` and `[mm:ss.03]` both mean +0.03 s, and `[mm:ss:500]`
/// is +0.5 s (not +5 s). Mirrors the C# `^(\d+):([0-5]?\d)(?:[.:](\d{1,3}))?$`. A pathological
/// field (overflowing minutes, out-of-range total) skips the stamp — the "never fails" contract.
fn parse_time_tag(tag: &str) -> Option<f64> {
    let tag = tag.trim();
    let bytes = tag.as_bytes();

    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    let minutes = tag[..i].parse::<i64>().ok()?;

    // [0-5]?\d — one or two digits; a two-digit value must stay below 60.
    let seconds_start = i + 1;
    let mut j = seconds_start;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    let seconds = match j - seconds_start {
        1 => u32::from(bytes[seconds_start] - b'0'),
        2 if bytes[seconds_start] <= b'5' => {
            u32::from(bytes[seconds_start] - b'0') * 10 + u32::from(bytes[seconds_start + 1] - b'0')
        }
        _ => return None,
    };

    let mut fraction = 0.0;
    if j < bytes.len() {
        if bytes[j] != b'.' && bytes[j] != b':' {
            return None;
        }
        let digits = &bytes[j + 1..];
        if digits.is_empty() || digits.len() > 3 || !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let value = tag[j + 1..].parse::<u32>().ok()?;
        fraction = f64::from(value) / 10f64.powi(digits.len() as i32);
    }

    let total = minutes as f64 * 60.0 + f64::from(seconds) + fraction;
    if !(0.0..=TIMESPAN_MAX_SECONDS).contains(&total) {
        return None; // out of range — skip rather than overflow
    }
    Some(total)
}

/// Apply a recognized ID tag (`ti`/`ar`/`al`/`length`/`offset`) and report whether it was one.
/// Returns false for anything else — including a plain bracketed section header like `[Chorus]` —
/// so the caller keeps that text as a lyric line instead of silently swallowing it.
fn apply_id_tag(tag: &str, tags: &mut IdTags) -> bool {
    let Some(colon) = tag.find(':') else {
        return false;
    };
    if colon == 0 {
        return false;
    }
    let key = tag[..colon].trim().to_lowercase();
    let value = tag[colon + 1..].trim();
    match key.as_str() {
        "ti" => {
            tags.title = none_if_empty(value);
            true
        }
        "ar" => {
            tags.artist = none_if_empty(value);
            true
        }
        "al" => {
            tags.album = none_if_empty(value);
            true
        }
        "length" => {
            if let Some(seconds) = parse_time_tag(value) {
                tags.length_seconds = Some(seconds);
            }
            true
        }
        "offset" => {
            if let Some(ms) = parse_offset_millis(value) {
                tags.offset_ms = ms;
            }
            true
        }
        _ => false,
    }
}

/// C# `double.TryParse` with `NumberStyles.Integer | AllowLeadingSign`: an optional sign then
/// digits only — no decimals or exponents, but values beyond integer precision still round to
/// the nearest double like the C# original.
fn parse_offset_millis(value: &str) -> Option<f64> {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Strip enhanced per-word stamps inside a line, e.g. `<00:12.50>word` — removed for the v1
/// line-level renderer. Mirrors the C# `<\d+:\d{1,2}(?:[.:]\d{1,3})?>` replaced with a space.
fn strip_word_tags(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<'
            && let Some(end) = match_word_tag(bytes, i)
        {
            out.push_str(&text[copied..i]);
            out.push(' ');
            copied = end;
            i = end;
            continue;
        }
        i += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// Match `<\d+:\d{1,2}(?:[.:]\d{1,3})?>` at `start` (a '<'), returning the position just past the
/// '>'. Digit runs longer than the groups allow can never match: the following literal would land
/// on a digit, so no backtracking of the C# regex ever succeeds there.
fn match_word_tag(bytes: &[u8], start: usize) -> Option<usize> {
    let digit_run_end = |mut k: usize| {
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        k
    };

    let minutes_end = digit_run_end(start + 1);
    if minutes_end == start + 1 || minutes_end >= bytes.len() || bytes[minutes_end] != b':' {
        return None;
    }

    let seconds_start = minutes_end + 1;
    let seconds_end = digit_run_end(seconds_start);
    if !(1..=2).contains(&(seconds_end - seconds_start)) {
        return None;
    }

    if seconds_end < bytes.len() && (bytes[seconds_end] == b'.' || bytes[seconds_end] == b':') {
        let fraction_start = seconds_end + 1;
        let fraction_end = digit_run_end(fraction_start);
        if (1..=3).contains(&(fraction_end - fraction_start))
            && fraction_end < bytes.len()
            && bytes[fraction_end] == b'>'
        {
            return Some(fraction_end + 1);
        }
        return None;
    }
    if seconds_end < bytes.len() && bytes[seconds_end] == b'>' {
        return Some(seconds_end + 1);
    }
    None
}

fn normalize_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn none_if_empty(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_seconds(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    fn texts(doc: &LrcDocument) -> Vec<&str> {
        doc.lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn empty_or_blank_is_empty_document() {
        assert!(parse(None).is_empty());
        assert!(parse(Some("")).is_empty());
        assert!(parse(Some("   \n\t ")).is_empty());
        assert!(!parse(None).has_timings);
    }

    #[test]
    fn synced_lines_are_parsed_sorted_and_carry_metadata() {
        let doc = parse(Some(
            "[ar:A][ti:T][al:Al]\n[00:01.00]one\n[00:03.50]three\n[00:02.00]two\n",
        ));
        assert!(doc.has_timings);
        assert_eq!(texts(&doc), ["one", "two", "three"]);
        assert_seconds(doc.lines[0].time_seconds, 1.0);
        assert_seconds(doc.lines[1].time_seconds, 2.0);
        assert_seconds(doc.lines[2].time_seconds, 3.5);
        assert_eq!(doc.artist.as_deref(), Some("A"));
        assert_eq!(doc.title.as_deref(), Some("T"));
        assert_eq!(doc.album.as_deref(), Some("Al"));
    }

    #[test]
    fn multiple_stamps_on_one_line_fan_out_to_one_line_each() {
        let doc = parse(Some("[00:12.00][00:47.10]La la la"));
        assert_eq!(doc.lines.len(), 2);
        assert!(doc.lines.iter().all(|line| line.text == "La la la"));
        assert_seconds(doc.lines[0].time_seconds, 12.0);
        assert_seconds(doc.lines[1].time_seconds, 47.1);
    }

    #[test]
    fn fraction_and_minute_forms_parse_to_seconds() {
        let cases = [
            ("[01:02]x", 62.0),       // no fraction
            ("[01:02.5]x", 62.5),     // 1-digit = tenths
            ("[01:02.50]x", 62.5),    // 2-digit = centiseconds
            ("[01:02.500]x", 62.5),   // 3-digit = milliseconds
            ("[01:02.05]x", 62.05),   // leading-zero centiseconds
            ("[100:00.00]x", 6000.0), // minutes may exceed 59
        ];
        for (line, expected_seconds) in cases {
            let doc = parse(Some(line));
            assert_eq!(doc.lines.len(), 1, "{line}");
            assert_seconds(doc.lines[0].time_seconds, expected_seconds);
        }
    }

    #[test]
    fn offset_positive_shifts_earlier_negative_shifts_later_clamped_at_zero() {
        assert_seconds(
            parse(Some("[offset:500]\n[00:10.00]a")).lines[0].time_seconds,
            9.5,
        );
        assert_seconds(
            parse(Some("[offset:-500]\n[00:10.00]a")).lines[0].time_seconds,
            10.5,
        );
        assert_seconds(
            parse(Some("[offset:5000]\n[00:01.00]a")).lines[0].time_seconds,
            0.0,
        );
    }

    #[test]
    fn fractions_are_length_based_regardless_of_separator() {
        let cases = [
            ("[01:02:03]x", 62.03), // 2-digit = centiseconds (colon separator)
            ("[01:02:50]x", 62.5),
            ("[01:02:500]x", 62.5), // 3-digit colon = milliseconds — must be +0.5s, NOT +5s (=67s)
            ("[01:02:5]x", 62.5),   // 1-digit = tenths
            ("[01:02.500]x", 62.5), // the '.' separator parses identically (length-based)
        ];
        for (line, expected_seconds) in cases {
            let doc = parse(Some(line));
            assert_eq!(doc.lines.len(), 1, "{line}");
            assert_seconds(doc.lines[0].time_seconds, expected_seconds);
        }
    }

    #[test]
    fn bracketed_section_headers_are_preserved_in_plain_lyrics() {
        // A plain (untimed) sheet with [Chorus]/[Verse] markers: those header-only lines must
        // survive, not be swallowed as unknown tags and dropped.
        let doc = parse(Some(
            "[Chorus]\nWe're no strangers to love\n[Verse 1]\nYou know the rules",
        ));
        assert!(!doc.has_timings);
        assert_eq!(
            texts(&doc),
            [
                "[Chorus]",
                "We're no strangers to love",
                "[Verse 1]",
                "You know the rules"
            ]
        );
    }

    #[test]
    fn enhanced_word_tags_are_stripped_to_clean_line_text() {
        let doc = parse(Some("[00:05.00]<00:05.00>Hello <00:05.50>world"));
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].text, "Hello world");
    }

    #[test]
    fn gap_line_with_no_text_is_preserved() {
        let doc = parse(Some("[00:01.00]a\n[00:02.00]\n[00:03.00]b"));
        assert_eq!(doc.lines.len(), 3);
        assert_eq!(doc.lines[1].text, "");
    }

    #[test]
    fn malformed_lines_are_skipped_not_failures_in_a_synced_sheet() {
        let doc = parse(Some("[bad:tag]junk\n[00:01.00]good\n[not-a-time]more junk"));
        assert!(doc.has_timings);
        assert_eq!(texts(&doc), ["good"]);
    }

    #[test]
    fn no_timestamps_falls_back_to_plain_lyrics_in_order() {
        let doc = parse(Some("first line\nsecond line\nthird line"));
        assert!(!doc.has_timings);
        assert_eq!(texts(&doc), ["first line", "second line", "third line"]);
        assert!(doc.lines.iter().all(|line| line.time_seconds == 0.0));
    }

    #[test]
    fn overflow_minutes_are_skipped_not_failures() {
        // The parser must honour its "never fails" contract on pathological input and keep the
        // good line.
        let bad_lines = [
            "[99999999999:00.00]bad", // minutes fit i64 but overflow the TimeSpan range guard
            "[123456789012345678901:00.00]bad", // minutes overflow the i64 parse
        ];
        for bad_line in bad_lines {
            let doc = parse(Some(&format!("{bad_line}\n[00:01.00]good")));
            assert!(doc.has_timings, "{bad_line}");
            assert_eq!(texts(&doc), ["good"], "{bad_line}");
        }
    }

    #[test]
    fn huge_offset_is_ignored_not_overflowed() {
        // A pathological [offset:…] must not overflow the TimeSpan-range guard — it is ignored,
        // leaving the stamp untouched.
        let doc = parse(Some("[offset:999999999999999999]\n[00:10.00]a"));
        assert_eq!(doc.lines.len(), 1);
        assert_seconds(doc.lines[0].time_seconds, 10.0);
    }

    #[test]
    fn length_tag_is_captured() {
        let doc = parse(Some("[length:03:20]\n[00:01.00]a"));
        assert_seconds(doc.length_seconds.expect("length tag captured"), 200.0);
    }

    // ---- active_index ----

    fn sheet() -> Vec<LrcLine> {
        vec![
            LrcLine {
                time_seconds: 1.0,
                text: "one".to_owned(),
            },
            LrcLine {
                time_seconds: 2.0,
                text: "two".to_owned(),
            },
            LrcLine {
                time_seconds: 3.5,
                text: "three".to_owned(),
            },
        ]
    }

    #[test]
    fn active_index_picks_the_last_line_at_or_before_position() {
        let cases = [
            (0.0, None), // before the first line
            (0.99, None),
            (1.0, Some(0)), // exactly on a stamp → that line is active
            (1.5, Some(0)),
            (2.0, Some(1)),
            (3.49, Some(1)),
            (3.5, Some(2)),
            (120.0, Some(2)), // after the last line stays on the last
        ];
        for (position, expected) in cases {
            assert_eq!(active_index(&sheet(), position), expected, "at {position}");
        }
    }

    #[test]
    fn active_index_on_empty_or_negative_is_none() {
        assert_eq!(active_index(&[], 10.0), None);
        assert_eq!(active_index(&sheet(), -5.0), None);
    }
}
