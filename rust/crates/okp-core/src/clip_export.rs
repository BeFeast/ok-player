//! Portable eligibility and command boundary for exporting an A-B selection.
//!
//! The GTK shell owns runtime tooling discovery and presentation. This module
//! owns the selection rules so a future FFmpeg implementation, the Windows
//! shell, and the planned C ABI can agree on the same states and request shape.

/// The shortest deliberate export selection accepted by the product model.
pub const MIN_EXPORT_DURATION_SECONDS: f64 = 1.0;
/// The longest selection accepted by the placeholder export workflow.
pub const MAX_EXPORT_DURATION_SECONDS: f64 = 300.0;

/// Duration policy supplied to the eligibility evaluator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipExportLimits {
    pub min_seconds: f64,
    pub max_seconds: f64,
}

impl Default for ClipExportLimits {
    fn default() -> Self {
        Self {
            min_seconds: MIN_EXPORT_DURATION_SECONDS,
            max_seconds: MAX_EXPORT_DURATION_SECONDS,
        }
    }
}

impl ClipExportLimits {
    fn is_valid(self) -> bool {
        self.min_seconds.is_finite()
            && self.max_seconds.is_finite()
            && self.min_seconds >= 0.0
            && self.max_seconds >= self.min_seconds
    }
}

/// Runtime support the shell has discovered for the future encoder path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipExportTooling {
    Available,
    MissingFfmpeg,
}

/// A validated A-B interval in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipExportSelection {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl ClipExportSelection {
    pub fn duration_seconds(self) -> f64 {
        self.end_seconds - self.start_seconds
    }
}

/// The format selected by the eventual export-options surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipExportFormat {
    VideoClip,
    AnimatedGif,
}

/// Pure command payload ready for a shell-owned encoder implementation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipExportRequest {
    pub selection: ClipExportSelection,
    pub format: ClipExportFormat,
}

/// Eligibility shown by the A-B export entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipExportEligibility {
    NoSelection,
    InvalidRange,
    SelectionTooShort {
        duration_seconds: f64,
        min_seconds: f64,
    },
    SelectionTooLong {
        duration_seconds: f64,
        max_seconds: f64,
    },
    MissingTooling,
    Ready(ClipExportSelection),
}

impl ClipExportEligibility {
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready(_))
    }

    /// Builds the encoder-facing command only after all shared checks pass.
    pub const fn request(self, format: ClipExportFormat) -> Option<ClipExportRequest> {
        match self {
            Self::Ready(selection) => Some(ClipExportRequest { selection, format }),
            _ => None,
        }
    }
}

/// Evaluates the current A-B endpoints without performing any I/O.
pub fn clip_export_eligibility(
    a_seconds: Option<f64>,
    b_seconds: Option<f64>,
    tooling: ClipExportTooling,
    limits: ClipExportLimits,
) -> ClipExportEligibility {
    let (Some(start_seconds), Some(end_seconds)) = (a_seconds, b_seconds) else {
        return ClipExportEligibility::NoSelection;
    };
    if !limits.is_valid()
        || !start_seconds.is_finite()
        || !end_seconds.is_finite()
        || start_seconds < 0.0
        || end_seconds <= start_seconds
    {
        return ClipExportEligibility::InvalidRange;
    }

    let selection = ClipExportSelection {
        start_seconds,
        end_seconds,
    };
    let duration_seconds = selection.duration_seconds();
    if duration_seconds < limits.min_seconds {
        return ClipExportEligibility::SelectionTooShort {
            duration_seconds,
            min_seconds: limits.min_seconds,
        };
    }
    if duration_seconds > limits.max_seconds {
        return ClipExportEligibility::SelectionTooLong {
            duration_seconds,
            max_seconds: limits.max_seconds,
        };
    }
    if tooling == ClipExportTooling::MissingFfmpeg {
        return ClipExportEligibility::MissingTooling;
    }

    ClipExportEligibility::Ready(selection)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: ClipExportLimits = ClipExportLimits {
        min_seconds: 1.0,
        max_seconds: 30.0,
    };

    #[test]
    fn missing_endpoint_is_no_selection() {
        assert_eq!(
            clip_export_eligibility(None, None, ClipExportTooling::Available, LIMITS),
            ClipExportEligibility::NoSelection
        );
        assert_eq!(
            clip_export_eligibility(Some(5.0), None, ClipExportTooling::Available, LIMITS),
            ClipExportEligibility::NoSelection
        );
    }

    #[test]
    fn reversed_non_finite_and_negative_ranges_are_invalid() {
        for (a, b) in [
            (Some(8.0), Some(8.0)),
            (Some(9.0), Some(8.0)),
            (Some(-1.0), Some(8.0)),
            (Some(f64::NAN), Some(8.0)),
            (Some(1.0), Some(f64::INFINITY)),
        ] {
            assert_eq!(
                clip_export_eligibility(a, b, ClipExportTooling::Available, LIMITS),
                ClipExportEligibility::InvalidRange
            );
        }
    }

    #[test]
    fn duration_limits_are_inclusive_and_report_both_overflow_sides() {
        assert!(matches!(
            clip_export_eligibility(Some(5.0), Some(5.5), ClipExportTooling::Available, LIMITS),
            ClipExportEligibility::SelectionTooShort { .. }
        ));
        assert!(matches!(
            clip_export_eligibility(Some(5.0), Some(36.0), ClipExportTooling::Available, LIMITS),
            ClipExportEligibility::SelectionTooLong { .. }
        ));
        assert!(
            clip_export_eligibility(Some(5.0), Some(6.0), ClipExportTooling::Available, LIMITS)
                .is_ready()
        );
        assert!(
            clip_export_eligibility(Some(5.0), Some(35.0), ClipExportTooling::Available, LIMITS)
                .is_ready()
        );
    }

    #[test]
    fn valid_selection_without_ffmpeg_reports_missing_tooling() {
        assert_eq!(
            clip_export_eligibility(
                Some(12.0),
                Some(22.0),
                ClipExportTooling::MissingFfmpeg,
                LIMITS
            ),
            ClipExportEligibility::MissingTooling
        );
    }

    #[test]
    fn ready_selection_builds_the_future_encoder_command() {
        let eligibility =
            clip_export_eligibility(Some(12.0), Some(22.0), ClipExportTooling::Available, LIMITS);
        let request = eligibility
            .request(ClipExportFormat::AnimatedGif)
            .expect("ready selection should form a command");

        assert_eq!(request.selection.start_seconds, 12.0);
        assert_eq!(request.selection.end_seconds, 22.0);
        assert_eq!(request.selection.duration_seconds(), 10.0);
        assert_eq!(request.format, ClipExportFormat::AnimatedGif);
    }

    #[test]
    fn invalid_limits_fail_closed() {
        assert_eq!(
            clip_export_eligibility(
                Some(1.0),
                Some(2.0),
                ClipExportTooling::Available,
                ClipExportLimits {
                    min_seconds: 10.0,
                    max_seconds: 1.0,
                },
            ),
            ClipExportEligibility::InvalidRange
        );
    }
}
