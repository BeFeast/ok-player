#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TrailingTimeMode {
    Total,
    #[default]
    Remaining,
}

impl TrailingTimeMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Total => Self::Remaining,
            Self::Remaining => Self::Total,
        }
    }
}

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

/// Formats the on-screen **total** readout for an optional duration: the padded
/// clock when the duration is known, or `--:--` when it is absent or not yet
/// resolved — the live / unknown-duration sentinel (PRD §3 transport: Live/URL
/// unknown-duration state). A non-finite or non-positive duration is treated as
/// unknown, matching [`format_clock`]'s contract, so a stream that never reports
/// a duration shows `--:--` instead of the broken `00:00` total. Pure core so the
/// Linux and Windows shells render the same sentinel.
pub fn format_duration(seconds: Option<f64>) -> String {
    match seconds {
        Some(value) if value.is_finite() && value > 0.0 => format_clock(value),
        _ => "--:--".to_owned(),
    }
}

/// Formats the trailing transport readout as time remaining. A known duration
/// renders a leading minus sign and clamps positions past the end to zero;
/// unknown or invalid durations use the live `--:--` sentinel.
pub fn format_remaining(position: f64, duration: Option<f64>) -> String {
    let Some(duration) = duration.filter(|value| value.is_finite() && *value > 0.0) else {
        return "--:--".to_owned();
    };
    let position = if position.is_finite() {
        position.max(0.0)
    } else {
        0.0
    };
    format!("-{}", format_clock((duration - position).max(0.0)))
}

/// Format the clickable trailing OSC label in total or remaining mode while
/// preserving the local-loading versus live-URL sentinel contract.
pub fn format_trailing(
    mode: TrailingTimeMode,
    is_url: bool,
    position: f64,
    duration: Option<f64>,
) -> String {
    match mode {
        TrailingTimeMode::Total => crate::network_media::format_duration_total(is_url, duration),
        TrailingTimeMode::Remaining => {
            crate::network_media::format_remaining_total(is_url, position, duration)
        }
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
    fn parse_accepts_millisecond_input() {
        // P4-N2: HH:MM:SS with optional `.mmm` on the seconds field seeks
        // precisely, so fractions must survive parsing to full precision.
        let cases = [
            ("1:23:45.678", 5025.678),
            ("0:00.250", 0.25),
            ("2:05.5", 125.5),
            ("90.125", 90.125),
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
    fn parse_then_format_round_trips_whole_seconds() {
        let seconds = parse(Some("1:23:45")).expect("valid timecode");

        assert_eq!(format(seconds), "1:23:45");
    }

    #[test]
    fn format_duration_unknown_is_the_live_sentinel() {
        // A stream that never reports a duration (or hasn't resolved one yet)
        // shows the live `--:--` total instead of the broken `00:00`.
        for seconds in [
            None,
            Some(0.0),
            Some(-5.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert_eq!(format_duration(seconds), "--:--", "{seconds:?}");
        }
    }

    #[test]
    fn format_duration_known_renders_the_padded_clock() {
        assert_eq!(format_duration(Some(90.0)), "01:30");
        assert_eq!(format_duration(Some(5025.0)), "01:23:45");
    }

    #[test]
    fn format_remaining_clamps_and_uses_unknown_sentinel() {
        assert_eq!(format_remaining(30.0, Some(90.0)), "-01:00");
        assert_eq!(format_remaining(95.0, Some(90.0)), "-00:00");
        assert_eq!(format_remaining(f64::NAN, Some(90.0)), "-01:30");
        assert_eq!(format_remaining(10.0, None), "--:--");
        assert_eq!(format_remaining(10.0, Some(f64::INFINITY)), "--:--");
    }

    #[test]
    fn trailing_mode_toggles_total_and_remaining() {
        let mode = TrailingTimeMode::default();
        assert_eq!(mode, TrailingTimeMode::Remaining);
        assert_eq!(mode.toggled(), TrailingTimeMode::Total);
        assert_eq!(mode.toggled().toggled(), mode);

        assert_eq!(
            format_trailing(TrailingTimeMode::Remaining, false, 30.0, Some(90.0)),
            "-01:00"
        );
        assert_eq!(
            format_trailing(TrailingTimeMode::Total, false, 30.0, Some(90.0)),
            "01:30"
        );
        assert_eq!(
            format_trailing(TrailingTimeMode::Total, true, 30.0, None),
            "--:--"
        );
    }
}
