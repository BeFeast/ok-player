//! Pure buffered-timeline projection shared by playback shells.

/// Convert the observed playhead plus mpv's forward cache duration into a
/// normalized buffered fraction. Invalid or unknown values produce no buffered
/// band; the result is always clamped to the media duration.
pub fn fraction(position: Option<f64>, cache_duration: Option<f64>, duration: Option<f64>) -> f64 {
    let Some(duration) = duration.filter(|value| value.is_finite() && *value > 0.0) else {
        return 0.0;
    };
    let position = position
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let cache_duration = cache_duration
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    ((position + cache_duration) / duration).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_forward_cache_to_fraction() {
        assert_eq!(fraction(Some(30.0), Some(45.0), Some(120.0)), 0.625);
    }

    #[test]
    fn clamps_buffered_end_to_duration() {
        assert_eq!(fraction(Some(100.0), Some(90.0), Some(120.0)), 1.0);
    }

    #[test]
    fn invalid_or_unknown_values_are_safe() {
        assert_eq!(fraction(Some(30.0), Some(10.0), None), 0.0);
        assert_eq!(
            fraction(Some(f64::NAN), Some(f64::INFINITY), Some(120.0)),
            0.0
        );
        assert_eq!(fraction(Some(-5.0), Some(-2.0), Some(120.0)), 0.0);
    }
}
