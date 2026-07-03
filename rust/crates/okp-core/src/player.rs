//! Core player state machine and the typed command/event contract that a shell — or
//! a future C-ABI consumer through `okp-ffi` — drives it with. This is the seam the
//! core-extraction epic (#134) calls C10: **commands** are the intents a consumer
//! sends, **events** are the facts an engine (libmpv, behind `okp-mpv`) reports, and
//! [`PlaybackSnapshot`] is the reduced, observable state both sides agree on. Engine-
//! and UI-free — no `okp-mpv` dependency, no I/O.
//!
//! The types are idiomatic Rust but deliberately *C-ABI-shaped*: every variant carries
//! only scalars, a single owned path/string, or a homogeneous list, so the `#[repr(C)]`
//! projection in `okp-ffi` is a mechanical flattening. Discriminant-only enums are
//! `#[repr(i32)]` so a C consumer can cast straight through, the same convention
//! [`crate::aspect_resize::ResizeEdge`] follows.
//!
//! # Flow
//!
//! ```text
//! consumer ──PlayerCommand──▶ PlayerMachine::apply_command ─▶ CommandOutcome ─▶ engine
//!                                     │  (gates + optimistic transition)
//! engine   ──PlayerEvent────▶ PlayerMachine::apply_event    ─▶ PlaybackSnapshot ─▶ consumer
//! ```
//!
//! `apply_command` validates a command against the current lifecycle state, applies the
//! optimistic transition (e.g. flipping paused), and hands back a monotonic
//! `request_id` so the eventual asynchronous [`CommandReply`] can be correlated — the
//! shape `mpv_command_async` reply userdata needs. `apply_event` folds engine facts
//! back into the snapshot.

use crate::playlist::PlaylistItem;

/// The lifecycle state of the player, observable by the consumer. Discriminants are
/// stable for the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum PlaybackStatus {
    /// No media loaded (the initial state, and the state after [`PlayerCommand::Close`]).
    #[default]
    Idle = 0,
    /// [`PlayerCommand::Open`] issued; awaiting the engine's [`PlayerEvent::Loaded`].
    Opening = 1,
    /// Media loaded and playing.
    Playing = 2,
    /// Media loaded and paused.
    Paused = 3,
    /// Playback reached the end; the media context stays current until the next
    /// `Open`/`Close`.
    Ended = 4,
}

impl PlaybackStatus {
    /// Whether media is loaded and interactive (`Playing` or `Paused`) — the states
    /// in which seeking, track selection, and pause toggling make sense.
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }
}

/// Whether a track is an audio or a subtitle stream. Discriminants are stable for the
/// C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TrackKind {
    Audio = 0,
    Subtitle = 1,
}

/// Whether a seek target is absolute (from the start) or relative to the current
/// position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum SeekMode {
    Absolute = 0,
    Relative = 1,
}

/// Why the current media stopped playing. Mirrors libmpv's `MPV_END_FILE_REASON_*`
/// intent at the core level; discriminants are stable for the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EndReason {
    /// Reached the natural end of the stream.
    Eof = 0,
    /// Stopped by request (`Close` / a `stop` command).
    Stopped = 1,
    /// The engine is quitting.
    Quit = 2,
    /// Ended because of a playback error.
    Error = 3,
    /// The stream redirected to another source.
    Redirect = 4,
}

/// Parameters for opening a media source. `resume_from` and the initial track ids carry
/// the companion-library launch-with-resume handoff (PRD §13.1).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    /// The media to load (a local path or a stream URL).
    pub source: PlaylistItem,
    /// Seek here once loaded, overriding any remembered position (seconds).
    pub resume_from: Option<f64>,
    /// Audio track id to select on load, if any.
    pub initial_audio: Option<i64>,
    /// Subtitle track id to select on load, if any.
    pub initial_subtitle: Option<i64>,
}

impl OpenRequest {
    /// A bare open with no resume point and no track preselection.
    #[must_use]
    pub fn new(source: PlaylistItem) -> Self {
        Self {
            source,
            resume_from: None,
            initial_audio: None,
            initial_subtitle: None,
        }
    }
}

/// A seek target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeekRequest {
    pub mode: SeekMode,
    pub seconds: f64,
}

/// An intent the consumer sends to the player. [`PlayerMachine::apply_command`]
/// validates each against the current state before it should reach the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    /// Load a media source, replacing any current one.
    Open(OpenRequest),
    /// Stop playback and unload the current media, returning to [`PlaybackStatus::Idle`].
    Close,
    /// Seek within the current media.
    Seek(SeekRequest),
    /// Set the paused flag explicitly (`true` = pause, `false` = resume).
    SetPaused(bool),
    /// Toggle the paused flag.
    TogglePause,
    /// Select an audio or subtitle track (`None` deselects / turns it off).
    SelectTrack { kind: TrackKind, id: Option<i64> },
    /// Set the subtitle delay, in seconds.
    SetSubtitleDelay(f64),
    /// Set the playback speed multiplier.
    SetSpeed(f64),
    /// Set the output volume, as a percentage.
    SetVolume(f64),
    /// Request a screenshot (optionally with subtitles burned in).
    RequestScreenshot { include_subtitles: bool },
}

/// A single observed playback property changing. Mirrors the properties `okp-mpv`
/// observes; the state machine folds each into the snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropertyChange {
    /// Current playback position (seconds); `None` when unknown.
    TimePos(Option<f64>),
    /// Media duration (seconds); `None` when unknown.
    Duration(Option<f64>),
    /// The paused flag.
    Paused(bool),
    /// Output volume (percentage); `None` when unknown.
    Volume(Option<f64>),
    /// Playback speed multiplier; `None` when unknown.
    Speed(Option<f64>),
    /// Subtitle delay (seconds).
    SubtitleDelay(f64),
}

/// A media track as reported by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub id: i64,
    pub kind: TrackKind,
    pub selected: bool,
    pub external: bool,
    pub default: bool,
    pub title: Option<String>,
    pub lang: Option<String>,
}

/// A chapter marker as reported by the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterInfo {
    pub index: i64,
    pub time: f64,
    pub title: Option<String>,
}

/// A category for an engine-reported error, stable for the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PlayerErrorKind {
    /// The media failed to load.
    LoadFailed = 1,
    /// A command failed at the engine.
    CommandFailed = 2,
    /// An otherwise-uncategorized engine error.
    Engine = 3,
}

/// An error surfaced by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerError {
    pub kind: PlayerErrorKind,
    pub message: String,
}

/// Whether a previously issued command was carried out by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// The command succeeded.
    Ok,
    /// The command failed, with an engine message.
    Failed(String),
}

/// The outcome of a previously issued command, correlated by its request id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReply {
    /// The id [`PlayerMachine::apply_command`] handed out for the command.
    pub request_id: u64,
    /// Whether the engine carried the command out.
    pub result: CommandResult,
}

/// A fact the engine reports. [`PlayerMachine::apply_event`] folds each into the
/// snapshot.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// The current media finished loading.
    Loaded { duration: Option<f64> },
    /// Playback of the current media ended.
    Ended(EndReason),
    /// An observed property changed.
    Property(PropertyChange),
    /// The track list changed.
    Tracks(Vec<TrackInfo>),
    /// The chapter list changed.
    Chapters(Vec<ChapterInfo>),
    /// The engine reported an error.
    Error(PlayerError),
    /// A reply to a previously issued command.
    Reply(CommandReply),
}

/// Why the state machine refused a command in the current state. Stable for the C ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum RejectReason {
    /// The command needs interactive media, but none is active.
    NoActiveMedia = 1,
    /// The value the command carries is not finite (NaN or infinity).
    NotFinite = 2,
}

/// The result of feeding a command to [`PlayerMachine::apply_command`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum CommandOutcome {
    /// The command is valid; forward it to the engine and use `request_id` to correlate
    /// the eventual [`PlayerEvent::Reply`].
    Accepted { request_id: u64 },
    /// The command is valid but has no effect in the current state; do not forward it.
    NoOp,
    /// The command is not valid in the current state; do not forward it.
    Rejected(RejectReason),
}

/// The reduced, observable player state. The consumer renders from this; the engine
/// reconciles it through [`PlayerEvent`]s.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlaybackSnapshot {
    pub status: PlaybackStatus,
    /// The current media source; `None` when [`PlaybackStatus::Idle`].
    pub source: Option<PlaylistItem>,
    pub time_pos: Option<f64>,
    pub duration: Option<f64>,
    pub volume: Option<f64>,
    pub speed: Option<f64>,
    pub subtitle_delay: f64,
    pub tracks: Vec<TrackInfo>,
    pub chapters: Vec<ChapterInfo>,
    /// Why playback ended; set while `status == Ended`.
    pub end_reason: Option<EndReason>,
    /// The most recent engine error, if any.
    pub last_error: Option<PlayerError>,
}

/// The core player state machine. It gates commands against the current lifecycle
/// state, hands out a request id per accepted command (for asynchronous reply
/// correlation), and folds engine events back into an observable [`PlaybackSnapshot`].
///
/// It holds no engine handle and performs no I/O: a consumer pairs it with an engine
/// adapter (`okp-mpv`) that turns [`CommandOutcome::Accepted`] into real libmpv calls
/// and libmpv wake-ups back into [`PlayerEvent`]s.
#[derive(Debug, Clone, Default)]
pub struct PlayerMachine {
    snapshot: PlaybackSnapshot,
    next_request_id: u64,
}

impl PlayerMachine {
    /// A fresh machine in [`PlaybackStatus::Idle`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The current observable state.
    #[must_use]
    pub fn snapshot(&self) -> &PlaybackSnapshot {
        &self.snapshot
    }

    /// The current lifecycle status.
    #[must_use]
    pub fn status(&self) -> PlaybackStatus {
        self.snapshot.status
    }

    /// Validate `command` against the current state, apply the optimistic lifecycle
    /// transition, and report whether it should be forwarded to the engine. Only an
    /// [`CommandOutcome::Accepted`] consumes a request id.
    pub fn apply_command(&mut self, command: &PlayerCommand) -> CommandOutcome {
        match command {
            PlayerCommand::Open(request) => {
                // A resume point, if given, must be finite before it becomes the seek
                // target: reject NaN/infinity here rather than record it in the snapshot.
                if request
                    .resume_from
                    .is_some_and(|resume| !resume.is_finite())
                {
                    return CommandOutcome::Rejected(RejectReason::NotFinite);
                }
                // Volume and speed are engine-global and persist across loads; keep them.
                let volume = self.snapshot.volume;
                let speed = self.snapshot.speed;
                self.snapshot = PlaybackSnapshot {
                    status: PlaybackStatus::Opening,
                    source: Some(request.source.clone()),
                    time_pos: request.resume_from,
                    volume,
                    speed,
                    ..PlaybackSnapshot::default()
                };
                self.accept()
            }
            PlayerCommand::Close => {
                if self.snapshot.status == PlaybackStatus::Idle {
                    return CommandOutcome::NoOp;
                }
                let volume = self.snapshot.volume;
                let speed = self.snapshot.speed;
                self.snapshot = PlaybackSnapshot {
                    volume,
                    speed,
                    ..PlaybackSnapshot::default()
                };
                self.accept()
            }
            PlayerCommand::Seek(request) => self.gate_active_finite(request.seconds),
            PlayerCommand::SetPaused(paused) => self.set_paused(*paused),
            PlayerCommand::TogglePause => match self.snapshot.status {
                PlaybackStatus::Playing => self.set_paused(true),
                PlaybackStatus::Paused => self.set_paused(false),
                _ => CommandOutcome::Rejected(RejectReason::NoActiveMedia),
            },
            PlayerCommand::SelectTrack { .. } | PlayerCommand::RequestScreenshot { .. } => {
                if self.snapshot.status.is_active() {
                    self.accept()
                } else {
                    CommandOutcome::Rejected(RejectReason::NoActiveMedia)
                }
            }
            PlayerCommand::SetSubtitleDelay(seconds) => self.gate_active_finite(*seconds),
            // Speed and volume are engine-global: settable before a media load, and
            // they persist across loads. Only the value must be finite.
            PlayerCommand::SetSpeed(value) | PlayerCommand::SetVolume(value) => {
                if value.is_finite() {
                    self.accept()
                } else {
                    CommandOutcome::Rejected(RejectReason::NotFinite)
                }
            }
        }
    }

    /// Fold an engine event into the observable snapshot.
    pub fn apply_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::Loaded { duration } => {
                // A load completes an `Opening` transition (or replaces current media);
                // a stray load with nothing in flight is ignored.
                if self.snapshot.status != PlaybackStatus::Idle {
                    self.snapshot.status = PlaybackStatus::Playing;
                    self.snapshot.end_reason = None;
                    if duration.is_some() {
                        self.snapshot.duration = duration;
                    }
                }
            }
            PlayerEvent::Ended(reason) => {
                if self.snapshot.status != PlaybackStatus::Idle {
                    self.snapshot.status = PlaybackStatus::Ended;
                    self.snapshot.end_reason = Some(reason);
                }
            }
            PlayerEvent::Property(change) => self.apply_property(change),
            PlayerEvent::Tracks(tracks) => self.snapshot.tracks = tracks,
            PlayerEvent::Chapters(chapters) => self.snapshot.chapters = chapters,
            PlayerEvent::Error(error) => self.snapshot.last_error = Some(error),
            PlayerEvent::Reply(reply) => {
                if let CommandResult::Failed(message) = reply.result {
                    self.snapshot.last_error = Some(PlayerError {
                        kind: PlayerErrorKind::CommandFailed,
                        message,
                    });
                }
            }
        }
    }

    fn apply_property(&mut self, change: PropertyChange) {
        match change {
            PropertyChange::TimePos(value) => self.snapshot.time_pos = value,
            PropertyChange::Duration(value) => self.snapshot.duration = value,
            PropertyChange::Paused(paused) => {
                // Reconcile the lifecycle only while media is interactive; ignore stray
                // pause echoes while Idle/Opening/Ended.
                if self.snapshot.status.is_active() {
                    self.snapshot.status = if paused {
                        PlaybackStatus::Paused
                    } else {
                        PlaybackStatus::Playing
                    };
                }
            }
            PropertyChange::Volume(value) => self.snapshot.volume = value,
            PropertyChange::Speed(value) => self.snapshot.speed = value,
            PropertyChange::SubtitleDelay(value) => self.snapshot.subtitle_delay = value,
        }
    }

    /// Accept a command that requires interactive media and a finite value.
    fn gate_active_finite(&mut self, value: f64) -> CommandOutcome {
        if !value.is_finite() {
            CommandOutcome::Rejected(RejectReason::NotFinite)
        } else if self.snapshot.status.is_active() {
            self.accept()
        } else {
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        }
    }

    fn set_paused(&mut self, paused: bool) -> CommandOutcome {
        if self.snapshot.status.is_active() {
            self.snapshot.status = if paused {
                PlaybackStatus::Paused
            } else {
                PlaybackStatus::Playing
            };
            self.accept()
        } else {
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        }
    }

    fn accept(&mut self) -> CommandOutcome {
        self.next_request_id += 1;
        CommandOutcome::Accepted {
            request_id: self.next_request_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn local(path: &str) -> PlaylistItem {
        PlaylistItem::Local(PathBuf::from(path))
    }

    fn request_id(outcome: CommandOutcome) -> u64 {
        match outcome {
            CommandOutcome::Accepted { request_id } => request_id,
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    /// Open a machine straight to `Playing` with a loaded local file.
    fn playing() -> PlayerMachine {
        let mut machine = PlayerMachine::new();
        let _ = machine.apply_command(&PlayerCommand::Open(OpenRequest::new(local(
            "/media/a.mkv",
        ))));
        machine.apply_event(PlayerEvent::Loaded {
            duration: Some(120.0),
        });
        assert_eq!(machine.status(), PlaybackStatus::Playing);
        machine
    }

    #[test]
    fn new_machine_starts_idle_and_empty() {
        let machine = PlayerMachine::new();
        assert_eq!(machine.status(), PlaybackStatus::Idle);
        assert_eq!(machine.snapshot(), &PlaybackSnapshot::default());
    }

    #[test]
    fn open_transitions_to_opening_and_records_resume_point() {
        let mut machine = PlayerMachine::new();
        let outcome = machine.apply_command(&PlayerCommand::Open(OpenRequest {
            source: local("/media/a.mkv"),
            resume_from: Some(42.5),
            initial_audio: Some(2),
            initial_subtitle: None,
        }));

        assert_eq!(request_id(outcome), 1);
        assert_eq!(machine.status(), PlaybackStatus::Opening);
        assert_eq!(machine.snapshot().source, Some(local("/media/a.mkv")));
        assert_eq!(machine.snapshot().time_pos, Some(42.5));
    }

    #[test]
    fn open_rejects_a_non_finite_resume_point() {
        for resume in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut machine = PlayerMachine::new();
            assert_eq!(
                machine.apply_command(&PlayerCommand::Open(OpenRequest {
                    source: local("/media/a.mkv"),
                    resume_from: Some(resume),
                    initial_audio: None,
                    initial_subtitle: None,
                })),
                CommandOutcome::Rejected(RejectReason::NotFinite)
            );
            // A rejected open records nothing and burns no request id: the machine is
            // untouched, exactly as it was fresh.
            assert_eq!(machine.snapshot(), &PlaybackSnapshot::default());

            // The very next accepted command still takes request id 1.
            assert_eq!(
                request_id(
                    machine.apply_command(&PlayerCommand::Open(OpenRequest::new(local(
                        "/media/a.mkv",
                    ))))
                ),
                1
            );
        }
    }

    #[test]
    fn loaded_event_starts_playing_and_sets_duration() {
        let mut machine = PlayerMachine::new();
        let _ = machine.apply_command(&PlayerCommand::Open(OpenRequest::new(local(
            "/media/a.mkv",
        ))));
        assert_eq!(machine.snapshot().time_pos, None);

        machine.apply_event(PlayerEvent::Loaded {
            duration: Some(90.0),
        });
        assert_eq!(machine.status(), PlaybackStatus::Playing);
        assert_eq!(machine.snapshot().duration, Some(90.0));
    }

    #[test]
    fn stray_loaded_while_idle_is_ignored() {
        let mut machine = PlayerMachine::new();
        machine.apply_event(PlayerEvent::Loaded {
            duration: Some(10.0),
        });
        assert_eq!(machine.status(), PlaybackStatus::Idle);
        assert_eq!(machine.snapshot().duration, None);
    }

    #[test]
    fn pause_toggles_between_playing_and_paused() {
        let mut machine = playing();

        assert!(matches!(
            machine.apply_command(&PlayerCommand::SetPaused(true)),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(machine.status(), PlaybackStatus::Paused);

        assert!(matches!(
            machine.apply_command(&PlayerCommand::TogglePause),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(machine.status(), PlaybackStatus::Playing);

        assert!(matches!(
            machine.apply_command(&PlayerCommand::TogglePause),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(machine.status(), PlaybackStatus::Paused);
    }

    #[test]
    fn pause_and_toggle_require_active_media() {
        let mut machine = PlayerMachine::new();
        assert_eq!(
            machine.apply_command(&PlayerCommand::SetPaused(true)),
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        );
        assert_eq!(
            machine.apply_command(&PlayerCommand::TogglePause),
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        );
        // Still Idle: a rejected command changed nothing.
        assert_eq!(machine.status(), PlaybackStatus::Idle);
    }

    #[test]
    fn seek_requires_active_media_and_a_finite_target() {
        let mut idle = PlayerMachine::new();
        assert_eq!(
            idle.apply_command(&PlayerCommand::Seek(SeekRequest {
                mode: SeekMode::Absolute,
                seconds: 10.0,
            })),
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        );
        // The finiteness guard fires before the state guard.
        assert_eq!(
            idle.apply_command(&PlayerCommand::Seek(SeekRequest {
                mode: SeekMode::Relative,
                seconds: f64::NAN,
            })),
            CommandOutcome::Rejected(RejectReason::NotFinite)
        );

        let mut machine = playing();
        assert!(matches!(
            machine.apply_command(&PlayerCommand::Seek(SeekRequest {
                mode: SeekMode::Absolute,
                seconds: 30.0,
            })),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(
            machine.apply_command(&PlayerCommand::Seek(SeekRequest {
                mode: SeekMode::Relative,
                seconds: f64::INFINITY,
            })),
            CommandOutcome::Rejected(RejectReason::NotFinite)
        );
    }

    #[test]
    fn track_select_and_screenshot_require_active_media() {
        let mut idle = PlayerMachine::new();
        assert_eq!(
            idle.apply_command(&PlayerCommand::SelectTrack {
                kind: TrackKind::Subtitle,
                id: Some(3),
            }),
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        );
        assert_eq!(
            idle.apply_command(&PlayerCommand::RequestScreenshot {
                include_subtitles: true,
            }),
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        );

        let mut machine = playing();
        assert!(matches!(
            machine.apply_command(&PlayerCommand::SelectTrack {
                kind: TrackKind::Audio,
                id: None,
            }),
            CommandOutcome::Accepted { .. }
        ));
        assert!(matches!(
            machine.apply_command(&PlayerCommand::RequestScreenshot {
                include_subtitles: false,
            }),
            CommandOutcome::Accepted { .. }
        ));
    }

    #[test]
    fn subtitle_delay_needs_active_media_and_finite_value() {
        let mut idle = PlayerMachine::new();
        assert_eq!(
            idle.apply_command(&PlayerCommand::SetSubtitleDelay(1.5)),
            CommandOutcome::Rejected(RejectReason::NoActiveMedia)
        );

        let mut machine = playing();
        assert!(matches!(
            machine.apply_command(&PlayerCommand::SetSubtitleDelay(-0.75)),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(
            machine.apply_command(&PlayerCommand::SetSubtitleDelay(f64::NAN)),
            CommandOutcome::Rejected(RejectReason::NotFinite)
        );
    }

    #[test]
    fn speed_and_volume_are_settable_without_media() {
        let mut machine = PlayerMachine::new();
        assert!(matches!(
            machine.apply_command(&PlayerCommand::SetVolume(80.0)),
            CommandOutcome::Accepted { .. }
        ));
        assert!(matches!(
            machine.apply_command(&PlayerCommand::SetSpeed(1.25)),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(
            machine.apply_command(&PlayerCommand::SetVolume(f64::INFINITY)),
            CommandOutcome::Rejected(RejectReason::NotFinite)
        );
        // No media loaded, so setting them optimistically changed no status.
        assert_eq!(machine.status(), PlaybackStatus::Idle);
    }

    #[test]
    fn close_is_a_noop_while_idle_and_unloads_otherwise() {
        let mut machine = PlayerMachine::new();
        assert_eq!(
            machine.apply_command(&PlayerCommand::Close),
            CommandOutcome::NoOp
        );

        let mut machine = playing();
        machine.apply_event(PlayerEvent::Property(PropertyChange::Volume(Some(64.0))));
        assert!(matches!(
            machine.apply_command(&PlayerCommand::Close),
            CommandOutcome::Accepted { .. }
        ));
        assert_eq!(machine.status(), PlaybackStatus::Idle);
        assert_eq!(machine.snapshot().source, None);
        // Volume is engine-global and survives the unload.
        assert_eq!(machine.snapshot().volume, Some(64.0));
    }

    #[test]
    fn property_changes_fold_into_the_snapshot() {
        let mut machine = playing();
        machine.apply_event(PlayerEvent::Property(PropertyChange::TimePos(Some(12.0))));
        machine.apply_event(PlayerEvent::Property(PropertyChange::Duration(Some(200.0))));
        machine.apply_event(PlayerEvent::Property(PropertyChange::Volume(Some(55.0))));
        machine.apply_event(PlayerEvent::Property(PropertyChange::Speed(Some(2.0))));
        machine.apply_event(PlayerEvent::Property(PropertyChange::SubtitleDelay(0.5)));

        let snapshot = machine.snapshot();
        assert_eq!(snapshot.time_pos, Some(12.0));
        assert_eq!(snapshot.duration, Some(200.0));
        assert_eq!(snapshot.volume, Some(55.0));
        assert_eq!(snapshot.speed, Some(2.0));
        assert_eq!(snapshot.subtitle_delay, 0.5);
    }

    #[test]
    fn paused_property_reconciles_status_only_while_active() {
        let mut machine = playing();
        machine.apply_event(PlayerEvent::Property(PropertyChange::Paused(true)));
        assert_eq!(machine.status(), PlaybackStatus::Paused);
        machine.apply_event(PlayerEvent::Property(PropertyChange::Paused(false)));
        assert_eq!(machine.status(), PlaybackStatus::Playing);

        // A stray pause echo while Idle must not manufacture a lifecycle transition.
        let mut idle = PlayerMachine::new();
        idle.apply_event(PlayerEvent::Property(PropertyChange::Paused(true)));
        assert_eq!(idle.status(), PlaybackStatus::Idle);
    }

    #[test]
    fn end_event_records_reason_and_reload_clears_it() {
        let mut machine = playing();
        machine.apply_event(PlayerEvent::Ended(EndReason::Eof));
        assert_eq!(machine.status(), PlaybackStatus::Ended);
        assert_eq!(machine.snapshot().end_reason, Some(EndReason::Eof));

        let _ = machine.apply_command(&PlayerCommand::Open(OpenRequest::new(local(
            "/media/b.mkv",
        ))));
        machine.apply_event(PlayerEvent::Loaded { duration: None });
        assert_eq!(machine.status(), PlaybackStatus::Playing);
        assert_eq!(machine.snapshot().end_reason, None);
    }

    #[test]
    fn failed_open_ends_from_opening() {
        let mut machine = PlayerMachine::new();
        let _ = machine.apply_command(&PlayerCommand::Open(OpenRequest::new(local(
            "/media/gone.mkv",
        ))));
        machine.apply_event(PlayerEvent::Ended(EndReason::Error));
        assert_eq!(machine.status(), PlaybackStatus::Ended);
        assert_eq!(machine.snapshot().end_reason, Some(EndReason::Error));
    }

    #[test]
    fn tracks_and_chapters_events_replace_the_lists() {
        let mut machine = playing();
        machine.apply_event(PlayerEvent::Tracks(vec![TrackInfo {
            id: 1,
            kind: TrackKind::Audio,
            selected: true,
            external: false,
            default: true,
            title: Some("English".to_owned()),
            lang: Some("eng".to_owned()),
        }]));
        machine.apply_event(PlayerEvent::Chapters(vec![ChapterInfo {
            index: 0,
            time: 0.0,
            title: Some("Intro".to_owned()),
        }]));

        assert_eq!(machine.snapshot().tracks.len(), 1);
        assert_eq!(machine.snapshot().chapters.len(), 1);
    }

    #[test]
    fn error_event_and_failed_reply_record_last_error() {
        let mut machine = playing();
        machine.apply_event(PlayerEvent::Error(PlayerError {
            kind: PlayerErrorKind::LoadFailed,
            message: "no such file".to_owned(),
        }));
        assert_eq!(
            machine.snapshot().last_error,
            Some(PlayerError {
                kind: PlayerErrorKind::LoadFailed,
                message: "no such file".to_owned(),
            })
        );

        machine.apply_event(PlayerEvent::Reply(CommandReply {
            request_id: 7,
            result: CommandResult::Failed("busy".to_owned()),
        }));
        assert_eq!(
            machine.snapshot().last_error,
            Some(PlayerError {
                kind: PlayerErrorKind::CommandFailed,
                message: "busy".to_owned(),
            })
        );
    }

    #[test]
    fn ok_reply_does_not_touch_last_error() {
        let mut machine = playing();
        machine.apply_event(PlayerEvent::Reply(CommandReply {
            request_id: 1,
            result: CommandResult::Ok,
        }));
        assert_eq!(machine.snapshot().last_error, None);
    }

    #[test]
    fn request_ids_are_monotonic_and_only_accepted_commands_consume_them() {
        let mut machine = PlayerMachine::new();
        // Open consumes id 1, the pause below consumes id 2.
        let open = request_id(machine.apply_command(&PlayerCommand::Open(OpenRequest::new(
            local("/media/a.mkv"),
        ))));
        machine.apply_event(PlayerEvent::Loaded {
            duration: Some(10.0),
        });
        let pause = request_id(machine.apply_command(&PlayerCommand::SetPaused(true)));

        // A rejected command (seek needs a finite target) must not burn an id...
        assert_eq!(
            machine.apply_command(&PlayerCommand::Seek(SeekRequest {
                mode: SeekMode::Absolute,
                seconds: f64::NAN,
            })),
            CommandOutcome::Rejected(RejectReason::NotFinite)
        );
        // ...and neither must a no-op (Close after the unload below).
        let _ = machine.apply_command(&PlayerCommand::Close); // consumes id 3, unloads
        assert_eq!(
            machine.apply_command(&PlayerCommand::Close),
            CommandOutcome::NoOp
        );

        // The next accepted command is id 4: the rejected and no-op commands consumed
        // none, and ids never repeat or go backwards.
        let reopen = request_id(machine.apply_command(&PlayerCommand::Open(OpenRequest::new(
            local("/media/b.mkv"),
        ))));

        assert_eq!(open, 1);
        assert_eq!(pause, 2);
        assert_eq!(reopen, 4);
    }

    #[test]
    fn open_preserves_engine_global_volume_and_speed() {
        let mut machine = PlayerMachine::new();
        let _ = machine.apply_command(&PlayerCommand::SetVolume(70.0));
        machine.apply_event(PlayerEvent::Property(PropertyChange::Volume(Some(70.0))));
        machine.apply_event(PlayerEvent::Property(PropertyChange::Speed(Some(1.5))));

        let _ = machine.apply_command(&PlayerCommand::Open(OpenRequest::new(local(
            "/media/a.mkv",
        ))));
        assert_eq!(machine.snapshot().volume, Some(70.0));
        assert_eq!(machine.snapshot().speed, Some(1.5));
        // A fresh open clears per-file state.
        assert_eq!(machine.snapshot().subtitle_delay, 0.0);
        assert!(machine.snapshot().tracks.is_empty());
    }
}
