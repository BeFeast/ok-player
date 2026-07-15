//! Portable screenshot naming rules shared by every shell.

use std::path::Path;

/// Natural playback may advance while the shell prepares a saved capture.
/// A larger displacement means the requested frame is no longer the frame the
/// user selected, even when no explicit seek was observed.
pub const SAVED_CAPTURE_POSITION_TOLERANCE_SECONDS: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedCaptureContext {
    pub source_generation: u64,
    pub seek_generation: u64,
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedCaptureValidity {
    Valid,
    SourceChanged,
    Seeked,
    PositionChanged,
}

/// Compare the click-time capture context with the latest non-blocking player
/// snapshot before dispatching the screenshot command.
pub fn saved_capture_validity(
    requested: SavedCaptureContext,
    current: SavedCaptureContext,
) -> SavedCaptureValidity {
    if requested.source_generation != current.source_generation {
        return SavedCaptureValidity::SourceChanged;
    }
    if requested.seek_generation != current.seek_generation {
        return SavedCaptureValidity::Seeked;
    }

    match (
        finite_position(requested.position),
        finite_position(current.position),
    ) {
        (None, None) => SavedCaptureValidity::Valid,
        (Some(requested), Some(current))
            if (requested - current).abs() <= SAVED_CAPTURE_POSITION_TOLERANCE_SECONDS =>
        {
            SavedCaptureValidity::Valid
        }
        _ => SavedCaptureValidity::PositionChanged,
    }
}

fn finite_position(position: Option<f64>) -> Option<f64> {
    position.filter(|value| value.is_finite() && *value >= 0.0)
}

/// Build a screenshot filename candidate. Filesystem probing and atomic
/// publication remain platform concerns; `collision_suffix` is zero for the
/// preferred name and increments after each collision.
pub fn candidate_filename(
    media_path: Option<&Path>,
    position: Option<f64>,
    timestamp_millis: u128,
    extension: &str,
    collision_suffix: u32,
) -> String {
    let base_name = media_path
        .and_then(Path::file_stem)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(sanitize_filename)
        .unwrap_or_else(|| "ok-player".to_owned());
    let position = position
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| format!("-{}", time_slug(value)))
        .unwrap_or_default();
    let collision = if collision_suffix == 0 {
        String::new()
    } else {
        format!("-{collision_suffix}")
    };

    format!(
        "{base_name}{position}-{timestamp_millis}{collision}.{}",
        normalized_extension(extension)
    )
}

/// Replace filename punctuation that is unsafe on either supported platform.
pub fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            ch if ch.is_control() => '-',
            ch => ch,
        })
        .collect::<String>();

    let sanitized = sanitized
        .trim_matches(|ch| matches!(ch, ' ' | '.' | '-'))
        .chars()
        .take(80)
        .collect::<String>();
    if sanitized.is_empty() {
        "ok-player".to_owned()
    } else {
        sanitized
    }
}

fn normalized_extension(extension: &str) -> &str {
    match extension {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    }
}

fn time_slug(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}h{minutes:02}m{seconds:02}s")
    } else {
        format!("{minutes:02}m{seconds:02}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_cross_platform_filename_punctuation() {
        assert_eq!(
            sanitize_filename("  Movie: Cut/Scene?.mkv  "),
            "Movie- Cut-Scene-.mkv"
        );
        assert_eq!(sanitize_filename("...---"), "ok-player");
        assert_eq!(sanitize_filename("line\nfeed"), "line-feed");
    }

    #[test]
    fn candidates_include_position_format_and_collision_suffix() {
        let media = Path::new("/media/Movie: Final.mkv");

        assert_eq!(
            candidate_filename(Some(media), Some(53.2), 1234, "png", 0),
            "Movie- Final-00m53s-1234.png"
        );
        assert_eq!(
            candidate_filename(Some(media), Some(3222.0), 1234, "png", 0),
            "Movie- Final-53m42s-1234.png"
        );
        assert_eq!(
            candidate_filename(Some(media), Some(3906.0), 1234, "jpeg", 0),
            "Movie- Final-01h05m06s-1234.jpg"
        );
        assert_eq!(
            candidate_filename(Some(media), Some(3906.0), 1234, "jpeg", 2),
            "Movie- Final-01h05m06s-1234-2.jpg"
        );
    }

    #[test]
    fn candidates_fall_back_for_missing_media_and_invalid_values() {
        assert_eq!(
            candidate_filename(None, Some(f64::NAN), 55, "invalid", 0),
            "ok-player-55.png"
        );
    }

    #[test]
    fn saved_capture_rejects_a_superseded_media_generation() {
        let requested = SavedCaptureContext {
            source_generation: 4,
            seek_generation: 2,
            position: Some(18.0),
        };
        let current = SavedCaptureContext {
            source_generation: 5,
            ..requested
        };

        assert_eq!(
            saved_capture_validity(requested, current),
            SavedCaptureValidity::SourceChanged
        );
    }

    #[test]
    fn saved_capture_rejects_a_seek_even_before_position_is_republished() {
        let requested = SavedCaptureContext {
            source_generation: 4,
            seek_generation: 2,
            position: Some(18.0),
        };
        let current = SavedCaptureContext {
            seek_generation: 3,
            ..requested
        };

        assert_eq!(
            saved_capture_validity(requested, current),
            SavedCaptureValidity::Seeked
        );
    }

    #[test]
    fn saved_capture_allows_natural_drift_but_rejects_a_position_jump() {
        let requested = SavedCaptureContext {
            source_generation: 4,
            seek_generation: 2,
            position: Some(18.0),
        };
        let natural_playback = SavedCaptureContext {
            position: Some(18.75),
            ..requested
        };
        let position_jump = SavedCaptureContext {
            position: Some(24.0),
            ..requested
        };

        assert_eq!(
            saved_capture_validity(requested, natural_playback),
            SavedCaptureValidity::Valid
        );
        assert_eq!(
            saved_capture_validity(requested, position_jump),
            SavedCaptureValidity::PositionChanged
        );
    }
}
