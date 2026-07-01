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
    fn parse_then_format_round_trips_whole_seconds() {
        let seconds = parse(Some("1:23:45")).expect("valid timecode");

        assert_eq!(format(seconds), "1:23:45");
    }
}
