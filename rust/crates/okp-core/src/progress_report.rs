//! Portable progress / watched report-back model for the companion-library seam (PRD §13.1).
//! The UI shell supplies periodic playback snapshots; this module validates them, derives the
//! near-end watched transition, and dispatches typed local events to a replaceable sink. It owns no
//! storage or transport, so a later local IPC companion can replace the no-op sink without changing
//! playback or rendering code.

use crate::recents_shelf::completion_start;

/// A periodic playback progress update.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressUpdate {
    pub media: String,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub fraction: f64,
}

/// The one-shot transition emitted when playback crosses the completion window.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchedUpdate {
    pub media: String,
    pub position_seconds: f64,
    pub duration_seconds: f64,
}

/// Events a future companion transport receives. They are intentionally independent from UI state
/// and persistence schemas.
#[derive(Debug, Clone, PartialEq)]
pub enum ProgressEvent {
    Progress(ProgressUpdate),
    Watched(WatchedUpdate),
}

/// Replaceable local report channel. Implementations decide how to handle delivery failures so
/// playback never depends on companion availability.
pub trait ProgressSink {
    fn report(&mut self, event: ProgressEvent);
}

/// MVP sink: the seam is live but no companion transport is configured yet.
#[derive(Default)]
pub struct NoopProgressSink;

impl ProgressSink for NoopProgressSink {
    fn report(&mut self, _event: ProgressEvent) {}
}

/// Stateful derivation of periodic progress plus one watched transition per media source.
#[derive(Debug, Default)]
pub struct ProgressTracker {
    media: Option<String>,
    watched_emitted: bool,
}

impl ProgressTracker {
    /// Derive report events from one playback snapshot. Invalid/unknown-duration snapshots produce
    /// no event. `finished` is the engine EOF signal and marks watched even if the final observed
    /// time position trails the duration slightly.
    #[must_use]
    pub fn observe(
        &mut self,
        media: &str,
        position_seconds: f64,
        duration_seconds: f64,
        finished: bool,
    ) -> Vec<ProgressEvent> {
        if media.is_empty()
            || !position_seconds.is_finite()
            || !duration_seconds.is_finite()
            || duration_seconds <= 0.0
        {
            return Vec::new();
        }

        if self.media.as_deref() != Some(media) {
            self.media = Some(media.to_owned());
            self.watched_emitted = false;
        }

        let position_seconds = position_seconds.clamp(0.0, duration_seconds);
        let mut events = vec![ProgressEvent::Progress(ProgressUpdate {
            media: media.to_owned(),
            position_seconds,
            duration_seconds,
            fraction: position_seconds / duration_seconds,
        })];

        let watched = finished || position_seconds >= completion_start(duration_seconds);
        if watched && !self.watched_emitted {
            self.watched_emitted = true;
            events.push(ProgressEvent::Watched(WatchedUpdate {
                media: media.to_owned(),
                position_seconds,
                duration_seconds,
            }));
        }

        events
    }
}

/// Owns the derivation state and the replaceable delivery channel. Privacy is checked here so every
/// caller uses the same suppression boundary.
pub struct ProgressReporter {
    tracker: ProgressTracker,
    sink: Box<dyn ProgressSink>,
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new(NoopProgressSink)
    }
}

impl ProgressReporter {
    #[must_use]
    pub fn new(sink: impl ProgressSink + 'static) -> Self {
        Self {
            tracker: ProgressTracker::default(),
            sink: Box::new(sink),
        }
    }

    /// Dispatch one playback observation unless the session is private. The sink is deliberately
    /// fire-and-forget: companion availability cannot affect playback UX.
    pub fn observe(
        &mut self,
        private_session: bool,
        media: &str,
        position_seconds: f64,
        duration_seconds: f64,
        finished: bool,
    ) {
        if private_session {
            return;
        }

        for event in self
            .tracker
            .observe(media, position_seconds, duration_seconds, finished)
        {
            self.sink.report(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Clone)]
    struct RecordingSink(Rc<RefCell<Vec<ProgressEvent>>>);

    impl ProgressSink for RecordingSink {
        fn report(&mut self, event: ProgressEvent) {
            self.0.borrow_mut().push(event);
        }
    }

    #[test]
    fn progress_is_clamped_and_reports_a_fraction() {
        let mut tracker = ProgressTracker::default();
        let events = tracker.observe("movie.mkv", 700.0, 600.0, false);

        assert_eq!(
            events[0],
            ProgressEvent::Progress(ProgressUpdate {
                media: "movie.mkv".to_owned(),
                position_seconds: 600.0,
                duration_seconds: 600.0,
                fraction: 1.0,
            })
        );
    }

    #[test]
    fn watched_emits_once_when_the_near_end_threshold_is_crossed() {
        let mut tracker = ProgressTracker::default();

        assert_eq!(tracker.observe("movie.mkv", 569.0, 600.0, false).len(), 1);
        let crossing = tracker.observe("movie.mkv", 570.0, 600.0, false);
        assert!(matches!(
            crossing.as_slice(),
            [ProgressEvent::Progress(_), ProgressEvent::Watched(_)]
        ));
        assert_eq!(tracker.observe("movie.mkv", 590.0, 600.0, false).len(), 1);
    }

    #[test]
    fn eof_marks_short_or_stale_position_as_watched() {
        let mut tracker = ProgressTracker::default();
        let events = tracker.observe("clip.mkv", 8.0, 20.0, true);

        assert!(matches!(
            events.as_slice(),
            [ProgressEvent::Progress(_), ProgressEvent::Watched(_)]
        ));
    }

    #[test]
    fn a_new_media_source_gets_its_own_watched_transition() {
        let mut tracker = ProgressTracker::default();
        assert_eq!(tracker.observe("a.mkv", 590.0, 600.0, false).len(), 2);
        assert_eq!(tracker.observe("b.mkv", 590.0, 600.0, false).len(), 2);
    }

    #[test]
    fn private_session_suppresses_progress_and_watched_delivery() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut reporter = ProgressReporter::new(RecordingSink(Rc::clone(&events)));

        reporter.observe(true, "private.mkv", 590.0, 600.0, false);

        assert!(events.borrow().is_empty());
    }
}
