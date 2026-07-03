//! Tolerant SubRip (`.srt`) parser — port of `src/OkPlayer.Core/SrtDocument.cs`; the C# suite in
//! `tests/OkPlayer.Tests` is the executable spec. Handles CRLF/LF/CR, a leading BOM, comma or dot
//! millisecond separators, 1–2 digit hours, an optional missing index line, and multi-line cues;
//! skips malformed or empty blocks rather than failing.

/// One SubRip cue: 1-based index, start/end in seconds, and the display text (tags stripped,
/// lines joined with a space).
#[derive(Debug, Clone, PartialEq)]
pub struct SrtCue {
    pub index: i32,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

pub fn parse(text: Option<&str>) -> Vec<SrtCue> {
    let mut cues = Vec::new();
    let Some(text) = text else {
        return cues;
    };
    if text.trim().is_empty() {
        return cues;
    }

    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized.strip_prefix('\u{feff}').unwrap_or(&normalized);

    let mut auto_index = 0;
    for block in split_blocks(normalized) {
        if block.trim().is_empty() {
            continue;
        }
        let lines = block.split('\n').collect::<Vec<_>>();

        // The time line is line 0 (no index) or line 1 (index present); scan the first few to be safe.
        let Some((time_line_idx, (start_seconds, end_seconds))) = lines
            .iter()
            .take(3)
            .enumerate()
            .find_map(|(idx, line)| find_time_line(line).map(|times| (idx, times)))
        else {
            continue;
        };

        let joined = lines[time_line_idx + 1..].join(" ");
        let text = normalize_spaces(&strip_markup(&joined));
        if text.is_empty() {
            continue;
        }

        auto_index += 1;
        let index = if time_line_idx == 1 {
            lines[0].trim().parse().unwrap_or(auto_index)
        } else {
            auto_index
        };

        cues.push(SrtCue {
            index,
            start_seconds,
            end_seconds,
            text,
        });
    }
    cues
}

/// Split on one or more blank lines — tolerating whitespace-only separators that subtitle editors
/// emit (a plain "\n\n" split would merge two cues when the blank line holds spaces/tabs).
/// Mirrors the C# `Regex.Split(text, @"\n(?:[ \t]*\n)+")`, including its edge behavior of leaving
/// a single leading blank line attached to the first block.
fn split_blocks(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut blocks = Vec::new();
    let mut block_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // Consume as many "[ \t]*\n" repetitions as possible after the first newline.
            let mut separator_end = i + 1;
            let mut is_separator = false;
            loop {
                let mut k = separator_end;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'\n' {
                    separator_end = k + 1;
                    is_separator = true;
                } else {
                    break;
                }
            }
            if is_separator {
                blocks.push(&text[block_start..i]);
                block_start = separator_end;
                i = separator_end;
                continue;
            }
        }
        i += 1;
    }
    blocks.push(&text[block_start..]);
    blocks
}

/// Find `HH:MM:SS,mmm --> HH:MM:SS,mmm` anywhere in the line (comma or dot ms; 1–3 ms digits;
/// 1–2 hour digits), returning (start, end) in seconds.
fn find_time_line(line: &str) -> Option<(f64, f64)> {
    let chars = line.chars().collect::<Vec<_>>();
    (0..chars.len()).find_map(|start| match_time_pair(&chars, start))
}

fn match_time_pair(chars: &[char], start: usize) -> Option<(f64, f64)> {
    let (start_seconds, mut i) = match_timestamp(chars, start, false)?;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i + 3 > chars.len() || chars[i] != '-' || chars[i + 1] != '-' || chars[i + 2] != '>' {
        return None;
    }
    i += 3;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let (end_seconds, _) = match_timestamp(chars, i, true)?;
    Some((start_seconds, end_seconds))
}

/// Match `(\d{1,2}):(\d{2}):(\d{2})[,.](\d{1,3})` at `start`, returning the seconds value and the
/// position just past the match. `last` marks the second timestamp, where the millisecond group
/// sits at the end of the pattern: there the greedy group simply takes the first three digits of a
/// longer run, while mid-pattern a 4+ digit run can never match (`\s*-->` would land on a digit).
fn match_timestamp(chars: &[char], start: usize, last: bool) -> Option<(f64, usize)> {
    let digit_run = |from: usize| {
        chars[from..]
            .iter()
            .take_while(|ch| ch.is_ascii_digit())
            .count()
    };

    // \d{1,2} followed by ':' — a 3+ digit run can never match (the ':' would land on a digit).
    let hours_len = digit_run(start);
    if hours_len == 0 || hours_len > 2 {
        return None;
    }
    let hours = to_number(chars, start, hours_len);
    let mut i = start + hours_len;
    if i >= chars.len() || chars[i] != ':' {
        return None;
    }
    i += 1;

    if digit_run(i) != 2 {
        return None;
    }
    let minutes = to_number(chars, i, 2);
    i += 2;
    if i >= chars.len() || chars[i] != ':' {
        return None;
    }
    i += 1;

    if digit_run(i) != 2 {
        return None;
    }
    let seconds = to_number(chars, i, 2);
    i += 2;
    if i >= chars.len() || (chars[i] != ',' && chars[i] != '.') {
        return None;
    }
    i += 1;

    let ms_run = digit_run(i);
    let ms_len = if last {
        ms_run.min(3)
    } else if (1..=3).contains(&ms_run) {
        ms_run
    } else {
        return None;
    };
    if ms_len == 0 {
        return None;
    }
    // C# pads the capture to three digits: "5" reads as 500 ms and "50" as 500 ms.
    let millis = to_number(chars, i, ms_len) * 10u32.pow(3 - ms_len as u32);
    i += ms_len;

    let total = f64::from(hours * 3600 + minutes * 60 + seconds) + f64::from(millis) / 1000.0;
    Some((total, i))
}

fn to_number(chars: &[char], start: usize, len: usize) -> u32 {
    chars[start..start + len]
        .iter()
        .fold(0, |acc, ch| acc * 10 + ch.to_digit(10).expect("digit run"))
}

/// Drop `<i>…</i>` HTML-ish tags and `{\an8}`-style ASS overrides for clean matching text —
/// the C# `<[^>]+>|\{[^}]*\}` replaced with a space (note: `<>` is kept, `{}` is dropped).
fn strip_markup(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        let tag_end = match bytes[i] {
            b'<' => match text[i + 1..].find('>') {
                Some(gap) if gap > 0 => Some(i + 1 + gap + 1),
                _ => None,
            },
            b'{' => text[i + 1..].find('}').map(|gap| i + 1 + gap + 1),
            _ => None,
        };
        if let Some(end) = tag_end {
            out.push_str(&text[copied..i]);
            out.push(' ');
            copied = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn normalize_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

    #[test]
    fn parses_basic_cues() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nThe quick brown fox\n\n\
                   2\n00:00:05,500 --> 00:00:08,250\njumps over\nthe lazy dog";
        let cues = parse(Some(srt));
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].index, 1);
        assert_seconds(cues[0].start_seconds, 1.0);
        assert_seconds(cues[0].end_seconds, 4.0);
        assert_eq!(cues[0].text, "The quick brown fox");
        assert_seconds(cues[1].start_seconds, 5.5);
        assert_seconds(cues[1].end_seconds, 8.25);
        assert_eq!(cues[1].text, "jumps over the lazy dog"); // multi-line joined
    }

    #[test]
    fn strips_tags_and_tolerates_dot_ms_crlf_bom() {
        let srt = "\u{feff}1\r\n00:00:02.000 --> 00:00:03.000\r\n<i>Hello</i> {\\an8}world\r\n";
        let cues = parse(Some(srt));
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "Hello world");
        assert_seconds(cues[0].start_seconds, 2.0);
    }

    #[test]
    fn parses_when_index_line_missing() {
        let cues = parse(Some("00:01:00,000 --> 00:01:02,000\nLine without an index"));
        assert_eq!(cues.len(), 1);
        assert_seconds(cues[0].start_seconds, 60.0);
        assert_eq!(cues[0].text, "Line without an index");
    }

    #[test]
    fn garbage_yields_no_cues() {
        let inputs = [
            None,
            Some(""),
            Some("   "),
            Some("not a subtitle file"),
            Some("1\nno timecode here\njust text"),
        ];
        for input in inputs {
            assert!(parse(input).is_empty(), "{input:?}");
        }
    }

    #[test]
    fn splits_on_whitespace_only_separator_lines() {
        // A separator line with spaces/tabs (common from subtitle editors) must still split the cues.
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nFirst\n \t \n2\n00:00:03,000 --> 00:00:04,000\nSecond";
        let cues = parse(Some(srt));
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "First");
        assert_eq!(cues[1].text, "Second");
        assert_seconds(cues[1].start_seconds, 3.0);
    }
}
