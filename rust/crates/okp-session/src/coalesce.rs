//! The coalescing event queue between the session state and the shell.
//!
//! The queue remembers the snapshot the shell will hold once it has drained
//! everything queued so far (`published`) and the set of fields that changed
//! since the last drain. A field that changes several times between drains is
//! reported once, with its latest value. Errors and shutdown are discrete and
//! keep their order. The playhead fields (`time_pos`, `cache_ahead`) change on
//! every frame, so on their own they are published at most once per
//! [`TIMELINE_INTERVAL`]; whenever anything else changes they ride along at once.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::protocol::{Event, SessionSnapshot, SnapshotChange};

/// Minimum spacing of playhead-only updates: about 10 Hz.
pub const TIMELINE_INTERVAL: Duration = Duration::from_millis(100);

/// Snapshot fields in the canonical order a `snapshot_changed` event lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Status,
    Source,
    TimePos,
    Duration,
    Volume,
    Mute,
    Speed,
    SubtitleDelay,
    Tracks,
    Chapters,
    EndReason,
    Title,
    Video,
    Buffering,
    CacheAhead,
}

impl Field {
    const ALL: [Field; 15] = [
        Field::Status,
        Field::Source,
        Field::TimePos,
        Field::Duration,
        Field::Volume,
        Field::Mute,
        Field::Speed,
        Field::SubtitleDelay,
        Field::Tracks,
        Field::Chapters,
        Field::EndReason,
        Field::Title,
        Field::Video,
        Field::Buffering,
        Field::CacheAhead,
    ];

    fn is_timeline(self) -> bool {
        matches!(self, Field::TimePos | Field::CacheAhead)
    }

    fn differs(self, a: &SessionSnapshot, b: &SessionSnapshot) -> bool {
        match self {
            Field::Status => a.status != b.status,
            Field::Source => a.source != b.source,
            Field::TimePos => a.time_pos != b.time_pos,
            Field::Duration => a.duration != b.duration,
            Field::Volume => a.volume != b.volume,
            Field::Mute => a.mute != b.mute,
            Field::Speed => a.speed != b.speed,
            Field::SubtitleDelay => a.subtitle_delay != b.subtitle_delay,
            Field::Tracks => a.tracks != b.tracks,
            Field::Chapters => a.chapters != b.chapters,
            Field::EndReason => a.end_reason != b.end_reason,
            Field::Title => a.title != b.title,
            Field::Video => a.video != b.video,
            Field::Buffering => a.buffering != b.buffering,
            Field::CacheAhead => a.cache_ahead != b.cache_ahead,
        }
    }

    fn copy(self, to: &mut SessionSnapshot, from: &SessionSnapshot) {
        match self {
            Field::Status => to.status = from.status,
            Field::Source => to.source.clone_from(&from.source),
            Field::TimePos => to.time_pos = from.time_pos,
            Field::Duration => to.duration = from.duration,
            Field::Volume => to.volume = from.volume,
            Field::Mute => to.mute = from.mute,
            Field::Speed => to.speed = from.speed,
            Field::SubtitleDelay => to.subtitle_delay = from.subtitle_delay,
            Field::Tracks => to.tracks.clone_from(&from.tracks),
            Field::Chapters => to.chapters.clone_from(&from.chapters),
            Field::EndReason => to.end_reason = from.end_reason,
            Field::Title => to.title.clone_from(&from.title),
            Field::Video => to.video.clone_from(&from.video),
            Field::Buffering => to.buffering = from.buffering,
            Field::CacheAhead => to.cache_ahead = from.cache_ahead,
        }
    }

    fn change(self, snapshot: &SessionSnapshot) -> SnapshotChange {
        match self {
            Field::Status => SnapshotChange::Status {
                value: snapshot.status,
            },
            Field::Source => SnapshotChange::Source {
                value: snapshot.source.clone(),
            },
            Field::TimePos => SnapshotChange::TimePos {
                value: snapshot.time_pos,
            },
            Field::Duration => SnapshotChange::Duration {
                value: snapshot.duration,
            },
            Field::Volume => SnapshotChange::Volume {
                value: snapshot.volume,
            },
            Field::Mute => SnapshotChange::Mute {
                value: snapshot.mute,
            },
            Field::Speed => SnapshotChange::Speed {
                value: snapshot.speed,
            },
            Field::SubtitleDelay => SnapshotChange::SubtitleDelay {
                value: snapshot.subtitle_delay,
            },
            Field::Tracks => SnapshotChange::Tracks {
                value: snapshot.tracks.clone(),
            },
            Field::Chapters => SnapshotChange::Chapters {
                value: snapshot.chapters.clone(),
            },
            Field::EndReason => SnapshotChange::EndReason {
                value: snapshot.end_reason,
            },
            Field::Title => SnapshotChange::Title {
                value: snapshot.title.clone(),
            },
            Field::Video => SnapshotChange::Video {
                value: snapshot.video.clone(),
            },
            Field::Buffering => SnapshotChange::Buffering {
                value: snapshot.buffering,
            },
            Field::CacheAhead => SnapshotChange::CacheAhead {
                value: snapshot.cache_ahead,
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct Coalescer {
    published: SessionSnapshot,
    dirty: BTreeSet<Field>,
    discrete: Vec<Event>,
    last_timeline: Option<Instant>,
}

impl Coalescer {
    pub(crate) fn new() -> Self {
        Self {
            published: SessionSnapshot::default(),
            dirty: BTreeSet::new(),
            discrete: Vec::new(),
            last_timeline: None,
        }
    }

    /// The state the shell holds after draining everything queued so far.
    pub(crate) fn published(&self) -> &SessionSnapshot {
        &self.published
    }

    /// Fold the current session state in. Returns the instant at which a held
    /// playhead update becomes due, if one is being held back.
    pub(crate) fn observe(&mut self, current: &SessionSnapshot, now: Instant) -> Option<Instant> {
        let changed: Vec<Field> = Field::ALL
            .into_iter()
            .filter(|field| field.differs(&self.published, current))
            .collect();
        let others_changed = changed.iter().any(|field| !field.is_timeline());
        let timeline_due = self
            .last_timeline
            .is_none_or(|last| now >= last + TIMELINE_INTERVAL);
        let mut held = false;
        for field in changed {
            if field.is_timeline() && !others_changed && !timeline_due {
                held = true;
                continue;
            }
            if field.is_timeline() {
                self.last_timeline = Some(now);
            }
            field.copy(&mut self.published, current);
            self.dirty.insert(field);
        }
        held.then(|| self.last_timeline.map(|last| last + TIMELINE_INTERVAL))
            .flatten()
    }

    pub(crate) fn push(&mut self, event: Event) {
        self.discrete.push(event);
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.dirty.is_empty() || !self.discrete.is_empty()
    }

    /// Take everything queued as one batch.
    pub(crate) fn drain(&mut self) -> Vec<Event> {
        let mut events = Vec::with_capacity(1 + self.discrete.len());
        if !self.dirty.is_empty() {
            let changes = std::mem::take(&mut self.dirty)
                .into_iter()
                .map(|field| field.change(&self.published))
                .collect();
            events.push(Event::SnapshotChanged { changes });
        }
        // Shutdown is always reported last, after any error it caused.
        let (shutdown, mut rest): (Vec<Event>, Vec<Event>) = std::mem::take(&mut self.discrete)
            .into_iter()
            .partition(|event| matches!(event, Event::Shutdown {}));
        events.append(&mut rest);
        events.extend(shutdown.into_iter().take(1));
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ErrorKind, Status};

    fn playing_at(time: f64) -> SessionSnapshot {
        SessionSnapshot {
            status: Status::Playing,
            time_pos: Some(time),
            ..SessionSnapshot::default()
        }
    }

    fn changes(events: &[Event]) -> Vec<SnapshotChange> {
        match events.first() {
            Some(Event::SnapshotChanged { changes }) => changes.clone(),
            other => panic!("expected snapshot_changed first, got {other:?}"),
        }
    }

    #[test]
    fn repeated_changes_between_drains_report_the_latest_value_once() {
        let mut queue = Coalescer::new();
        let start = Instant::now();
        for volume in [10.0, 20.0, 30.0] {
            queue.observe(
                &SessionSnapshot {
                    volume: Some(volume),
                    ..SessionSnapshot::default()
                },
                start,
            );
        }
        let events = queue.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(
            changes(&events),
            vec![SnapshotChange::Volume { value: Some(30.0) }]
        );
        assert!(!queue.has_pending());
        assert!(queue.drain().is_empty(), "a drained queue stays empty");
    }

    #[test]
    fn playhead_only_updates_are_limited_to_about_ten_per_second() {
        let mut queue = Coalescer::new();
        let start = Instant::now();
        assert_eq!(queue.observe(&playing_at(0.0), start), None);
        queue.drain();

        // 60 frames in one second: only every ~100 ms gets through.
        let mut published = 0;
        let mut held_until = None;
        for frame in 1..=60 {
            let now = start + Duration::from_millis(frame * 1000 / 60);
            let deadline = queue.observe(&playing_at(frame as f64 / 60.0), now);
            if queue.has_pending() {
                published += 1;
                queue.drain();
            } else {
                held_until = deadline;
            }
        }
        assert!(
            (9..=11).contains(&published),
            "expected about 10 playhead updates in one second, got {published}"
        );
        assert!(held_until.is_some(), "a held update names when it is due");
    }

    #[test]
    fn a_held_playhead_update_is_released_at_its_deadline() {
        let mut queue = Coalescer::new();
        let start = Instant::now();
        queue.observe(&playing_at(1.0), start);
        queue.drain();

        let soon = start + Duration::from_millis(30);
        let deadline = queue
            .observe(&playing_at(1.03), soon)
            .expect("an update inside the interval is held");
        assert!(!queue.has_pending());
        assert_eq!(deadline, start + TIMELINE_INTERVAL);

        assert_eq!(queue.observe(&playing_at(1.03), deadline), None);
        assert_eq!(
            changes(&queue.drain()),
            vec![SnapshotChange::TimePos { value: Some(1.03) }]
        );
    }

    #[test]
    fn the_playhead_rides_along_with_any_other_change() {
        let mut queue = Coalescer::new();
        let start = Instant::now();
        queue.observe(&playing_at(1.0), start);
        queue.drain();

        let paused = SessionSnapshot {
            status: Status::Paused,
            time_pos: Some(1.01),
            ..SessionSnapshot::default()
        };
        assert_eq!(
            queue.observe(&paused, start + Duration::from_millis(10)),
            None
        );
        assert_eq!(
            changes(&queue.drain()),
            vec![
                SnapshotChange::Status {
                    value: Status::Paused
                },
                SnapshotChange::TimePos { value: Some(1.01) },
            ]
        );
    }

    #[test]
    fn errors_keep_their_order_and_shutdown_comes_last_once() {
        let mut queue = Coalescer::new();
        queue.push(Event::Shutdown {});
        for message in ["first", "second"] {
            queue.push(Event::Error {
                kind: ErrorKind::CommandFailed,
                message: message.to_owned(),
                request_id: None,
            });
        }
        queue.push(Event::Shutdown {});
        queue.observe(
            &SessionSnapshot {
                status: Status::Ended,
                ..SessionSnapshot::default()
            },
            Instant::now(),
        );

        let events = queue.drain();
        assert!(matches!(events[0], Event::SnapshotChanged { .. }));
        assert!(matches!(&events[1], Event::Error { message, .. } if message == "first"));
        assert!(matches!(&events[2], Event::Error { message, .. } if message == "second"));
        assert_eq!(events[3], Event::Shutdown {});
        assert_eq!(events.len(), 4);
    }
}
