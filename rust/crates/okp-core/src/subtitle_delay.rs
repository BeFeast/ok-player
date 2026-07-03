//! Subtitle-delay entry parsing and delay readout formatting, extracted from
//! the Linux GTK shell (EPIC #134, B7). The Windows flyout edits the delay
//! through a numeric NumberBox, so entry parsing has no C# counterpart; the
//! label mirrors the Windows toast/readout format (`{ms:+0;-0;0} ms`). See the
//! compatibility note in docs/core-compatibility.md.

/// Largest delay magnitude the entry accepts, in seconds (ten minutes).
pub const MAX_ENTRY_SECONDS: f64 = 600.0;

/// Parses delay entry text into seconds. A bare number is milliseconds (the
/// entry's display unit); an `ms` or `s` suffix selects the unit explicitly.
/// Values clamp to ±[`MAX_ENTRY_SECONDS`]. Returns `None` for anything that
/// is not a finite number.
pub fn parse_entry_seconds(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let seconds = if let Some(value) = lower.strip_suffix("ms") {
        value.trim().parse::<f64>().ok()? / 1000.0
    } else if let Some(value) = lower.strip_suffix('s') {
        value.trim().parse::<f64>().ok()?
    } else {
        lower.parse::<f64>().ok()? / 1000.0
    };

    seconds
        .is_finite()
        .then(|| seconds.clamp(-MAX_ENTRY_SECONDS, MAX_ENTRY_SECONDS))
}

/// Formats a delay for the entry box: whole milliseconds, no unit suffix.
pub fn format_entry(seconds: f64) -> String {
    whole_milliseconds(seconds).to_string()
}

/// Formats a delay for readout labels: signed whole milliseconds with the
/// unit — `+250 ms`, `-125 ms`, `0 ms` (zero carries no sign).
pub fn format_label(seconds: f64) -> String {
    let milliseconds = whole_milliseconds(seconds);
    if milliseconds > 0 {
        format!("+{milliseconds} ms")
    } else {
        format!("{milliseconds} ms")
    }
}

/// The Windows shell rounds `sub-delay` to whole milliseconds with C#
/// `Math.Round` (ties to even); ties-to-even here keeps the readouts identical.
fn whole_milliseconds(seconds: f64) -> i64 {
    (seconds * 1000.0).round_ties_even() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_delay(input: &str, expected: f64) {
        let actual = parse_entry_seconds(input).expect("delay should parse");
        okp_test_fixtures::assert_close(actual, expected, f64::EPSILON);
    }

    #[test]
    fn parses_entry_as_milliseconds_by_default() {
        assert_delay("250", 0.25);
        assert_delay("-125", -0.125);
        assert_delay("+500ms", 0.5);
    }

    #[test]
    fn parses_entry_seconds_suffix() {
        assert_delay("1.5s", 1.5);
        assert_delay("-0.25s", -0.25);
    }

    #[test]
    fn parses_entry_with_surrounding_whitespace() {
        assert_delay("  250  ", 0.25);
        assert_delay("500 ms", 0.5);
        assert_delay("1.5 s", 1.5);
    }

    #[test]
    fn rejects_invalid_entry() {
        assert!(parse_entry_seconds("").is_none());
        assert!(parse_entry_seconds("soon").is_none());
        assert!(parse_entry_seconds("nan").is_none());
    }

    #[test]
    fn clamps_entry_to_ten_minutes() {
        assert_delay("999999999", 600.0);
        assert_delay("-999999999", -600.0);
    }

    #[test]
    fn formats_entry_as_whole_milliseconds() {
        assert_eq!(format_entry(0.25), "250");
        assert_eq!(format_entry(-0.125), "-125");
        assert_eq!(format_entry(0.0), "0");
    }

    #[test]
    fn formats_label_with_explicit_sign() {
        assert_eq!(format_label(0.25), "+250 ms");
        assert_eq!(format_label(-0.125), "-125 ms");
        assert_eq!(format_label(0.0), "0 ms");
    }

    #[test]
    fn rounds_half_milliseconds_to_even_like_windows() {
        // 0.0625 s is exactly 62.5 ms; C# Math.Round (banker's) gives 62.
        assert_eq!(format_label(0.0625), "+62 ms");
        assert_eq!(format_label(-0.0625), "-62 ms");
        assert_eq!(format_entry(0.0625), "62");
    }

    #[test]
    fn entry_round_trips_through_parse() {
        let seconds = parse_entry_seconds("250").expect("valid entry");

        assert_eq!(format_entry(seconds), "250");
    }
}
