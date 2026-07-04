pub fn parse(text: Option<&str>) -> Option<f64> {
    let text = text?.trim();
    if text.is_empty() {
        return None;
    }

    let parts = text.split(':').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }

    let mut total = 0.0;
    let last_index = parts.len() - 1;
    for (index, part) in parts.iter().enumerate() {
        let last = index == last_index;
        let value = parse_field(part, last)?;
        total = total * 60.0 + value;
    }

    Some(total)
}

pub fn format(seconds: f64) -> String {
    let seconds = if seconds < 0.0 || seconds.is_nan() {
        0.0
    } else {
        seconds
    };

    let total = seconds.floor() as i64;
    let hours = total / 3600;
    let minutes = total % 3600 / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Formats seconds as the on-screen clock: zero-padded `MM:SS`, or `HH:MM:SS`
/// once the hour is reached. Non-finite and non-positive inputs render `00:00`.
/// Fractional seconds truncate (never round): the clock shows second N until
/// N+1 has fully elapsed, the same deliberate choice as the Windows shell's
/// clock (see the compatibility note in docs/core-compatibility.md).
pub fn format_clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00".to_owned();
    }

    let total = seconds.floor() as u64;
    let hours = total / 3600;
    let minutes = total % 3600 / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Formats a media's total-duration readout for the OSC. A known, positive,
/// finite duration renders as [`format_clock`]; an unknown duration — a live
/// stream, a URL that reports no length, or a file still probing — renders the
/// `--:--` placeholder instead of a misleading `00:00`, per the §2.3
/// "Live/URL unknown duration" state in the PRD.
pub fn format_total_clock(duration: Option<f64>) -> String {
    match duration {
        Some(seconds) if seconds.is_finite() && seconds > 0.0 => format_clock(seconds),
        _ => "--:--".to_owned(),
    }
}

fn parse_field(part: &str, allow_fraction: bool) -> Option<f64> {
    if part.is_empty() {
        return None;
    }

    let mut seen_digit = false;
    let mut seen_decimal = false;

    for ch in part.chars() {
        match ch {
            '0'..='9' => seen_digit = true,
            '.' if allow_fraction && !seen_decimal => seen_decimal = true,
            _ => return None,
        }
    }

    if !seen_digit {
        return None;
    }

    let value = part.parse::<f64>().ok()?;
    if value < 0.0 || !value.is_finite() {
        return None;
    }

    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_timecodes() {
        let cases = [
            ("90", 90.0),
            ("0", 0.0),
            ("1:30", 90.0),
            ("0:05", 5.0),
            ("1:23:45", 5025.0),
            ("2:00:00", 7200.0),
            ("83.5", 83.5),
            ("2:05.5", 125.5),
            ("  1:30  ", 90.0),
        ];

        for (text, expected) in cases {
            assert_eq!(parse(Some(text)), Some(expected));
        }
    }

    #[test]
    fn parse_invalid_timecodes_returns_none() {
        let cases = [
            Some(""),
            Some("   "),
            None,
            Some("abc"),
            Some("1:2:3:4"),
            Some("1::3"),
            Some("-5"),
            Some("1:-3"),
            Some("1.5:30"),
        ];

        for text in cases {
            assert_eq!(parse(text), None);
        }
    }

    #[test]
    fn format_renders_timecode() {
        let cases = [
            (90.0, "1:30"),
            (5.0, "0:05"),
            (5025.0, "1:23:45"),
            (0.0, "0:00"),
            (-3.0, "0:00"),
            (83.7, "1:23"),
            (59.9, "0:59"),
        ];

        for (seconds, expected) in cases {
            assert_eq!(format(seconds), expected);
        }
    }

    #[test]
    fn format_clock_renders_padded_clock() {
        let cases = [
            (0.0, "00:00"),
            (5.0, "00:05"),
            (90.0, "01:30"),
            (3599.0, "59:59"),
            (3600.0, "01:00:00"),
            (5025.0, "01:23:45"),
            (-3.0, "00:00"),
            (f64::NAN, "00:00"),
            (f64::INFINITY, "00:00"),
        ];

        for (seconds, expected) in cases {
            assert_eq!(format_clock(seconds), expected);
        }
    }

    #[test]
    fn format_clock_floors_fractional_seconds() {
        // Pins the round-vs-floor resolution: the clock stays at second N until
        // N+1 has elapsed, so 59.9 must render :59, not roll over to 1:00.
        let cases = [
            (0.999, "00:00"),
            (59.9, "00:59"),
            (83.7, "01:23"),
            (3599.9, "59:59"),
        ];

        for (seconds, expected) in cases {
            assert_eq!(format_clock(seconds), expected);
        }
    }

    #[test]
    fn format_total_clock_uses_placeholder_for_unknown_duration() {
        // Known, positive, finite durations mirror the elapsed clock exactly...
        assert_eq!(format_total_clock(Some(0.0)), "--:--");
        assert_eq!(format_total_clock(Some(5.0)), "00:05");
        assert_eq!(format_total_clock(Some(5025.0)), "01:23:45");

        // ...but every "duration not known" case renders --:-- rather than a
        // bogus 00:00 total: no reported duration (live/URL), zero, negative, or
        // non-finite values that mpv can briefly surface while probing.
        assert_eq!(format_total_clock(None), "--:--");
        assert_eq!(format_total_clock(Some(-3.0)), "--:--");
        assert_eq!(format_total_clock(Some(f64::NAN)), "--:--");
        assert_eq!(format_total_clock(Some(f64::INFINITY)), "--:--");
    }

    #[test]
    fn parse_then_format_round_trips_whole_seconds() {
        let seconds = parse(Some("1:23:45")).expect("valid timecode");

        assert_eq!(format(seconds), "1:23:45");
    }
}
