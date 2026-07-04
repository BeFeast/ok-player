//! Progress report-back seam for the companion library (PRD §13.1).
//!
//! OK Player stays a pure player; the companion library owns the catalogue. The MVP contract
//! is one-way report-back: while a file plays, the player emits **periodic position updates**
//! and a one-shot **"watched"** flag when the near-end threshold is crossed — "the same signal
//! it uses internally for resume" (the boundary is [`crate::resume::completion_start`]).
//!
//! This module is that signal, modelled as a pure state machine ([`ProgressReporter`]) that
//! is deliberately engine- and UI-free: a shell folds playback ticks in and gets back a
//! [`ReportOutcome`] describing what, if anything, to forward. The transport is left to the
//! shell via the [`ProgressSink`] seam — MVP wires a local channel (CLI / stderr / local IPC);
//! the Later shared-DB model swaps the sink without touching playback UX.
//!
//! **Privacy.** Private mode (PRD §12.3 / §13.3) gates every path that records *what was
//! watched*. The reporter honours it directly: a private tick produces nothing and advances no
//! internal state, so no progress or watched signal ever leaves the process while private.

use crate::resume::completion_start;

/// Tuning for how often a periodic progress update is worth emitting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReportConfig {
    /// Minimum change in playback position (seconds) since the last emitted update before
    /// another one is produced. A paused or micro-stepping playhead therefore does not spam
    /// the channel with near-identical reports. The near-end "watched" flag is not throttled.
    pub min_position_delta: f64,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            min_position_delta: 1.0,
        }
    }
}

/// A periodic playback-progress snapshot: the absolute position and duration (seconds) and the
/// watched fraction (`0.0..=1.0`) the companion renders as a progress bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub position: f64,
    pub duration: f64,
    pub percent: f64,
}

/// What a single [`ProgressReporter::tick`] decided to report. Both fields are empty while
/// private, or when there is nothing new to say.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReportOutcome {
    /// A periodic progress update, when the position moved past the throttle since the last one.
    pub progress: Option<Progress>,
    /// `true` on the single tick the near-end threshold is first crossed for this media.
    pub watched: bool,
}

impl ReportOutcome {
    /// Whether this tick produced anything to forward.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.progress.is_none() && !self.watched
    }
}

/// One report to forward toward the companion channel. The file identity is the shell's to
/// supply — the core model stays path-agnostic and pure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressEvent {
    /// Periodic playback progress.
    Progress(Progress),
    /// The file crossed the near-end threshold and counts as watched.
    Watched,
}

/// The pluggable report-back channel (PRD §13.1). MVP installs a local sink; the Later
/// shared-DB / `ok-player://` model replaces the impl without touching playback.
pub trait ProgressSink {
    /// Forward one report to the companion. Called at most twice per tick (a progress update
    /// and/or a watched flag), never while private.
    fn report(&mut self, event: ProgressEvent);
}

/// Folds playback ticks into a stream of report-back events (PRD §13.1). Reset per media with
/// [`ProgressReporter::begin`] so the watched flag latches once per file.
#[derive(Debug, Clone, Default)]
pub struct ProgressReporter {
    config: ReportConfig,
    /// The position at the last emitted progress update; `None` before the first.
    last_reported: Option<f64>,
    /// Whether the watched flag has already fired for the current media.
    watched_latched: bool,
}

impl ProgressReporter {
    /// A reporter with the given throttle configuration.
    #[must_use]
    pub fn new(config: ReportConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Reset for a newly opened media: clears the throttle baseline and the watched latch so
    /// the next crossing of the near-end threshold reports afresh.
    pub fn begin(&mut self) {
        self.last_reported = None;
        self.watched_latched = false;
    }

    /// Fold one playback tick. `position`/`duration` are the current playhead and media length
    /// in seconds; `private` is the live private-session flag. While private — or before a
    /// usable duration is known — nothing is reported and no internal state advances, so
    /// reporting resumes cleanly if private mode is later turned off.
    pub fn tick(&mut self, position: f64, duration: f64, private: bool) -> ReportOutcome {
        if private || !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return ReportOutcome::default();
        }

        let position = position.clamp(0.0, duration);
        let percent = (position / duration).clamp(0.0, 1.0);

        let progress = match self.last_reported {
            Some(last) if (position - last).abs() < self.config.min_position_delta => None,
            _ => {
                self.last_reported = Some(position);
                Some(Progress {
                    position,
                    duration,
                    percent,
                })
            }
        };

        let watched = if !self.watched_latched && position >= completion_start(duration) {
            self.watched_latched = true;
            true
        } else {
            false
        };

        ReportOutcome { progress, watched }
    }

    /// Fold a tick and forward whatever it produced to `sink`. Convenience over [`Self::tick`]
    /// for a shell that has a ready [`ProgressSink`]; returns the same outcome.
    pub fn report_to(
        &mut self,
        position: f64,
        duration: f64,
        private: bool,
        sink: &mut dyn ProgressSink,
    ) -> ReportOutcome {
        let outcome = self.tick(position, duration, private);
        if let Some(progress) = outcome.progress {
            sink.report(ProgressEvent::Progress(progress));
        }
        if outcome.watched {
            sink.report(ProgressEvent::Watched);
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test sink that records every forwarded event, for exercising the [`ProgressSink`] seam.
    #[derive(Default)]
    struct RecordingSink {
        events: Vec<ProgressEvent>,
    }

    impl ProgressSink for RecordingSink {
        fn report(&mut self, event: ProgressEvent) {
            self.events.push(event);
        }
    }

    #[test]
    fn first_tick_reports_progress_with_the_watched_fraction() {
        let mut reporter = ProgressReporter::default();
        let outcome = reporter.tick(300.0, 600.0, false);
        assert_eq!(
            outcome.progress,
            Some(Progress {
                position: 300.0,
                duration: 600.0,
                percent: 0.5,
            })
        );
        assert!(!outcome.watched);
    }

    #[test]
    fn progress_is_throttled_until_the_position_moves_past_the_delta() {
        let mut reporter = ProgressReporter::default(); // min delta 1.0s
        assert!(reporter.tick(10.0, 600.0, false).progress.is_some());
        // A sub-delta nudge (and a repeat while paused) report nothing.
        assert!(reporter.tick(10.4, 600.0, false).progress.is_none());
        assert!(reporter.tick(10.4, 600.0, false).progress.is_none());
        // Moving past the delta reports again, from the new baseline.
        assert!(reporter.tick(11.5, 600.0, false).progress.is_some());
        assert!(reporter.tick(12.0, 600.0, false).progress.is_none());
    }

    #[test]
    fn a_backward_seek_past_the_delta_also_reports() {
        let mut reporter = ProgressReporter::default();
        assert!(reporter.tick(300.0, 600.0, false).progress.is_some());
        assert!(reporter.tick(100.0, 600.0, false).progress.is_some());
    }

    #[test]
    fn watched_latches_once_at_the_near_end_threshold() {
        let mut reporter = ProgressReporter::default();
        // completion_start(600) == 570.
        assert!(!reporter.tick(569.0, 600.0, false).watched);
        assert!(reporter.tick(570.0, 600.0, false).watched);
        // Latched: staying in the final stretch does not re-report watched.
        assert!(!reporter.tick(575.0, 600.0, false).watched);
        assert!(!reporter.tick(600.0, 600.0, false).watched);
    }

    #[test]
    fn begin_resets_the_latch_and_baseline_for_a_new_file() {
        let mut reporter = ProgressReporter::default();
        assert!(reporter.tick(570.0, 600.0, false).watched);
        assert!(reporter.tick(300.0, 600.0, false).progress.is_some());

        reporter.begin();
        // Fresh file: the first tick reports progress again and the near-end can re-fire.
        assert!(reporter.tick(300.0, 600.0, false).progress.is_some());
        assert!(reporter.tick(590.0, 600.0, false).watched);
    }

    #[test]
    fn private_mode_suppresses_every_report_and_advances_no_state() {
        let mut reporter = ProgressReporter::default();
        // Nothing while private — not even the near-end watched flag.
        assert_eq!(reporter.tick(570.0, 600.0, true), ReportOutcome::default());
        assert_eq!(reporter.tick(590.0, 600.0, true), ReportOutcome::default());
        // Turning private off resumes reporting from the current position; the latch was never
        // set while private, so the still-past-threshold position reports watched now.
        let outcome = reporter.tick(595.0, 600.0, false);
        assert!(outcome.progress.is_some());
        assert!(outcome.watched);
    }

    #[test]
    fn position_is_clamped_into_range_and_reaching_the_end_is_watched() {
        let mut reporter = ProgressReporter::default();
        let outcome = reporter.tick(700.0, 600.0, false);
        assert_eq!(
            outcome.progress,
            Some(Progress {
                position: 600.0,
                duration: 600.0,
                percent: 1.0,
            })
        );
        assert!(outcome.watched);
    }

    #[test]
    fn a_missing_or_zero_duration_reports_nothing() {
        let mut reporter = ProgressReporter::default();
        assert!(reporter.tick(10.0, 0.0, false).is_empty());
        assert!(reporter.tick(10.0, f64::NAN, false).is_empty());
        assert!(reporter.tick(f64::NAN, 600.0, false).is_empty());
    }

    #[test]
    fn report_to_forwards_progress_and_watched_to_the_sink() {
        let mut reporter = ProgressReporter::default();
        let mut sink = RecordingSink::default();

        reporter.report_to(300.0, 600.0, false, &mut sink); // progress only
        reporter.report_to(590.0, 600.0, false, &mut sink); // progress + watched
        reporter.report_to(595.0, 600.0, true, &mut sink); // private: nothing

        assert_eq!(sink.events.len(), 3);
        assert!(matches!(sink.events[0], ProgressEvent::Progress(_)));
        assert!(matches!(sink.events[1], ProgressEvent::Progress(_)));
        assert_eq!(sink.events[2], ProgressEvent::Watched);
    }
}
