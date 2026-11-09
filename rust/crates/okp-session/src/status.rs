//! The status surface over the video, and the short notices a shell shows as
//! toasts.
//!
//! The surface says what covers the video now: nothing, a loading indicator
//! while a source opens, a buffering indicator while libmpv waits for its
//! cache, or a failure card for a source that did not play. The card's
//! classification, wording and actions are the core's (`playback_failure`,
//! `network_media`), so every shell shows the same card. A shell asks for the
//! surface with the `status_surface` query after each drained batch.
//!
//! A notice is one line confirming what a command changed (volume, speed,
//! subtitle delay, a track, a relative seek), a resume, or an Up Next edit,
//! worded with the core's readouts. The session queues it as a `notice` event
//! and the shell shows the text.

use okp_core::network_media::{self, LOAD_FAILURE_ACTIONS, LoadFailureAction, LoadFailureSource};
use okp_core::playback_failure::{
    self, CodecEnvironment, PlaybackFailureDiagnostic, PlaybackFailureKind,
};
use okp_core::player::{PlaybackSnapshot, TrackInfo, TrackKind as CoreTrackKind};
use okp_core::playlist::PlaylistItem;
use okp_core::volume::{self, VolumeState};
use okp_core::{seek_readout, subtitle_delay, time_code, track_label};
use okp_mpv::EndFileReason;
use serde::{Deserialize, Serialize};

use crate::protocol::{
    Command, DispatchResult, MediaSource, PlaylistAction, QueueMode, SeekMode, SessionSnapshot,
    Status, TrackKind,
};
use crate::transport::format_speed;

/// What covers the video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StatusSurface {
    /// Nothing: the video, or the shell's idle screen, shows.
    Clear {},
    /// A source is opening; `label` names it (`Opening film.mkv`).
    Loading { label: String },
    /// Playback waits for libmpv's cache to refill. `percent` is how far it has
    /// refilled towards resuming, when libmpv reports it; `label` is the line
    /// to show (`Buffering 45%`).
    Buffering { percent: Option<u8>, label: String },
    /// The current source did not play.
    Failed { card: FailureCard },
}

/// A failed load, as the core classifies and words it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureCard {
    pub kind: FailureKind,
    pub title: String,
    pub message: String,
    /// What "Copy details" copies: the source and a short reason, never the
    /// engine's log (`network_media::failure_detail`).
    pub detail: String,
    /// What Retry opens again.
    pub source: MediaSource,
    /// The card's buttons, left to right.
    pub actions: Vec<FailureActionView>,
}

/// `playback_failure::PlaybackFailureKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The file needs a codec the engine does not have.
    MissingCodec,
    /// Video output could not start.
    Renderer,
    /// Anything else: a missing file, a refused stream.
    Application,
}

/// `network_media::LoadFailureAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureAction {
    /// Open the card's `source` again.
    Retry,
    /// Choose another source.
    OpenAnother,
    /// Copy the card's `detail`.
    CopyDetails,
}

/// One button of the failure card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureActionView {
    pub action: FailureAction,
    pub label: String,
}

impl From<PlaybackFailureKind> for FailureKind {
    fn from(kind: PlaybackFailureKind) -> Self {
        match kind {
            PlaybackFailureKind::MissingCodec => Self::MissingCodec,
            PlaybackFailureKind::Renderer => Self::Renderer,
            PlaybackFailureKind::Application => Self::Application,
        }
    }
}

impl From<LoadFailureAction> for FailureAction {
    fn from(action: LoadFailureAction) -> Self {
        match action {
            LoadFailureAction::Retry => Self::Retry,
            LoadFailureAction::OpenAnother => Self::OpenAnother,
            LoadFailureAction::CopyDetails => Self::CopyDetails,
        }
    }
}

impl FailureCard {
    /// The card for `source` failing with `diagnostic`.
    #[must_use]
    pub fn new(source: &PlaylistItem, diagnostic: &PlaybackFailureDiagnostic) -> Self {
        let retry = match source {
            PlaylistItem::Local(path) => LoadFailureSource::local(path.clone()),
            PlaylistItem::Url(url) => LoadFailureSource::url(url.clone()),
        };
        Self {
            kind: diagnostic.kind.into(),
            title: diagnostic.title.clone(),
            message: diagnostic.message.clone(),
            detail: network_media::failure_detail(&retry, &diagnostic.detail),
            source: MediaSource::from_item(source),
            actions: LOAD_FAILURE_ACTIONS
                .iter()
                .map(|&action| FailureActionView {
                    action: action.into(),
                    label: action.label().to_owned(),
                })
                .collect(),
        }
    }
}

/// The core's diagnosis of an end of file that carried an error: the same
/// ends `reconcile::endfile_error` reports.
pub(crate) fn diagnose_end(
    reason: EndFileReason,
    messages: &[String],
) -> Option<PlaybackFailureDiagnostic> {
    match reason {
        EndFileReason::Error(code) | EndFileReason::Unknown(code) => Some(
            playback_failure::diagnose_mpv_failure(code, messages, CodecEnvironment::System),
        ),
        EndFileReason::Eof => {
            playback_failure::diagnose_mpv_eof(messages, CodecEnvironment::System)
        }
        _ => None,
    }
}

/// What the surface needs besides the snapshot: the current source's failure
/// and libmpv's buffering progress.
#[derive(Debug, Default)]
pub(crate) struct StatusState {
    failure: Option<FailureCard>,
    buffering_percent: Option<u8>,
}

impl StatusState {
    /// An accepted open or close: an earlier failure no longer applies.
    pub(crate) fn begin_source(&mut self) {
        self.failure = None;
    }

    /// The current source, `source`, did not play.
    pub(crate) fn failed(&mut self, source: &PlaylistItem, diagnostic: &PlaybackFailureDiagnostic) {
        self.failure = Some(FailureCard::new(source, diagnostic));
    }

    /// The engine's latest buffering progress.
    pub(crate) fn observe(&mut self, buffering_percent: Option<i64>) {
        self.buffering_percent =
            buffering_percent.map(|percent| u8::try_from(percent.clamp(0, 100)).unwrap_or(100));
    }

    /// The surface for `snapshot`.
    #[must_use]
    pub(crate) fn surface(&self, snapshot: &SessionSnapshot) -> StatusSurface {
        StatusSurface::project(snapshot, self.failure.as_ref(), self.buffering_percent)
    }
}

impl StatusSurface {
    /// The surface for `snapshot`, given the current source's `failure` and
    /// libmpv's `buffering_percent`: a failure wins, then an opening source,
    /// then a cache stall of loaded media.
    #[must_use]
    pub fn project(
        snapshot: &SessionSnapshot,
        failure: Option<&FailureCard>,
        buffering_percent: Option<u8>,
    ) -> Self {
        if let Some(card) = failure {
            return Self::Failed { card: card.clone() };
        }
        match (snapshot.status, &snapshot.source) {
            (Status::Opening, Some(source)) => Self::Loading {
                label: format!(
                    "Opening {}",
                    crate::session::source_title(&source.to_item())
                ),
            },
            (Status::Playing | Status::Paused, Some(_)) if snapshot.buffering => Self::Buffering {
                percent: buffering_percent,
                label: buffering_percent.map_or_else(
                    || "Buffering…".to_owned(),
                    |percent| format!("Buffering {percent}%"),
                ),
            },
            _ => Self::Clear {},
        }
    }
}

const MUTED: &str = "Muted";

/// The notice for `command`, which the engine just accepted; `state` is the
/// core state it was accepted in.
pub(crate) fn command_notice(command: &Command, state: &PlaybackSnapshot) -> Option<String> {
    match command {
        Command::SetVolume { volume } => Some(volume_notice(*volume, state.mute)),
        Command::SetMute { muted: true } => Some(MUTED.to_owned()),
        Command::SetMute { muted: false } => Some(volume_notice(
            state.volume.unwrap_or(volume::DEFAULT_RESTORE_VOLUME),
            false,
        )),
        Command::SetSpeed { speed } => Some(format!("Speed {}", format_speed(*speed))),
        Command::SetSubtitleDelay { seconds } => Some(format!(
            "Subtitle delay: {}",
            subtitle_delay::format_label(*seconds)
        )),
        Command::SelectTrack { kind, id } => Some(track_notice(*kind, *id, &state.tracks)),
        Command::Seek {
            seconds,
            mode: SeekMode::Relative,
            ..
        } => state.time_pos.map(|position| {
            let target =
                seek_readout::seek_target(position, *seconds, state.duration.unwrap_or(0.0));
            seek_readout::format_readout(target, None)
        }),
        _ => None,
    }
}

/// `Volume 85%`, or `Muted` at 0 % or under libmpv's own mute flag, as the
/// volume control reads.
fn volume_notice(level: f64, mute_flag: bool) -> String {
    let mut state = VolumeState::new(volume::DEFAULT_RESTORE_VOLUME);
    state.set_level(level);
    if mute_flag || state.is_muted() {
        MUTED.to_owned()
    } else {
        format!("Volume {}", state.readout())
    }
}

/// `Audio: English`, `Subtitles: Commentary`, `Subtitles off`.
fn track_notice(kind: TrackKind, id: Option<i64>, tracks: &[TrackInfo]) -> String {
    let (prefix, core_kind) = match kind {
        TrackKind::Audio => ("Audio", CoreTrackKind::Audio),
        TrackKind::Subtitle => ("Subtitles", CoreTrackKind::Subtitle),
    };
    let Some(id) = id else {
        return format!("{prefix} off");
    };
    let track = tracks
        .iter()
        .find(|track| track.kind == core_kind && track.id == id);
    let name = track_label::primary_track_name(
        id,
        track.and_then(|track| track.title.as_deref()),
        track.and_then(|track| track.lang.as_deref()),
    );
    format!("{prefix}: {name}")
}

/// The notice for media that started from its remembered position.
pub(crate) fn resumed_notice(position: f64) -> String {
    format!("Resumed at {}", time_code::format_clock(position))
}

/// The notice for a playlist command and how the session answered it: what an
/// Up Next edit did.
pub(crate) fn playlist_notice(action: &PlaylistAction, result: &DispatchResult) -> Option<String> {
    if !matches!(result, DispatchResult::Applied {}) {
        return None;
    }
    match action {
        PlaylistAction::Queue {
            mode: QueueMode::Append,
            ..
        } => Some("Added to Up Next".to_owned()),
        PlaylistAction::Queue {
            mode: QueueMode::PlayNext,
            ..
        }
        | PlaylistAction::PlayNext { .. } => Some("Plays next".to_owned()),
        _ => None,
    }
}

/// The notice for an open whose folders and playlist files held nothing to
/// play. The failure is reported as a notice, not a card: the playlist it
/// would have replaced may still be playing.
pub(crate) const NOTHING_PLAYABLE: &str = "Nothing playable was found";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SourceKind;

    fn local(path: &str) -> PlaylistItem {
        PlaylistItem::Local(path.into())
    }

    fn snapshot(status: Status, location: &str) -> SessionSnapshot {
        SessionSnapshot {
            status,
            source: Some(MediaSource {
                kind: SourceKind::Local,
                location: location.to_owned(),
            }),
            ..SessionSnapshot::default()
        }
    }

    #[test]
    fn an_opening_source_shows_its_file_name() {
        let state = StatusState::default();
        assert_eq!(
            state.surface(&snapshot(Status::Opening, "/Movies/Show/film.mkv")),
            StatusSurface::Loading {
                label: "Opening film.mkv".to_owned()
            }
        );
        let url = SessionSnapshot {
            status: Status::Opening,
            source: Some(MediaSource {
                kind: SourceKind::Url,
                location: "https://example.com/live.m3u8".to_owned(),
            }),
            ..SessionSnapshot::default()
        };
        assert_eq!(
            state.surface(&url),
            StatusSurface::Loading {
                label: "Opening https://example.com/live.m3u8".to_owned()
            }
        );
    }

    #[test]
    fn a_cache_stall_of_loaded_media_shows_buffering_with_the_engines_progress() {
        let mut state = StatusState::default();
        let mut playing = snapshot(Status::Playing, "/Movies/film.mkv");
        assert_eq!(state.surface(&playing), StatusSurface::Clear {});
        playing.buffering = true;
        assert_eq!(
            state.surface(&playing),
            StatusSurface::Buffering {
                percent: None,
                label: "Buffering…".to_owned()
            }
        );
        state.observe(Some(45));
        assert_eq!(
            state.surface(&playing),
            StatusSurface::Buffering {
                percent: Some(45),
                label: "Buffering 45%".to_owned()
            }
        );
        state.observe(Some(250));
        assert!(matches!(
            state.surface(&playing),
            StatusSurface::Buffering {
                percent: Some(100),
                ..
            }
        ));
        // Opening wins over a stall; ended or idle media shows nothing.
        let mut opening = snapshot(Status::Opening, "/Movies/film.mkv");
        opening.buffering = true;
        assert!(matches!(
            state.surface(&opening),
            StatusSurface::Loading { .. }
        ));
        let mut ended = snapshot(Status::Ended, "/Movies/film.mkv");
        ended.buffering = true;
        assert_eq!(state.surface(&ended), StatusSurface::Clear {});
        assert_eq!(
            state.surface(&SessionSnapshot::default()),
            StatusSurface::Clear {}
        );
    }

    #[test]
    fn a_failure_shows_the_cores_card_until_the_next_source_begins() {
        let mut state = StatusState::default();
        let diagnostic = playback_failure::diagnose_mpv_failure(
            -13,
            &["[ffmpeg/video] Decoder not found for codec hevc".to_owned()],
            CodecEnvironment::System,
        );
        state.failed(&local("/Movies/film.mkv"), &diagnostic);
        let StatusSurface::Failed { card } =
            state.surface(&snapshot(Status::Ended, "/Movies/film.mkv"))
        else {
            panic!("no failure card");
        };
        assert_eq!(card.kind, FailureKind::MissingCodec);
        assert_eq!(card.title, "Codec unavailable");
        assert_eq!(card.message, diagnostic.message);
        assert_eq!(
            card.detail,
            "OK Player could not open the media.\nPath: /Movies/film.mkv\n\
             Reason: Required system codec is unavailable (libmpv error -13)."
        );
        assert_eq!(
            card.source,
            MediaSource {
                kind: SourceKind::Local,
                location: "/Movies/film.mkv".to_owned()
            }
        );
        let actions: Vec<_> = card
            .actions
            .iter()
            .map(|view| (view.action, view.label.as_str()))
            .collect();
        assert_eq!(
            actions,
            vec![
                (FailureAction::Retry, "Retry"),
                (FailureAction::OpenAnother, "Open another"),
                (FailureAction::CopyDetails, "Copy details"),
            ]
        );
        state.begin_source();
        assert!(matches!(
            state.surface(&snapshot(Status::Opening, "/Movies/film.mkv")),
            StatusSurface::Loading { .. }
        ));
    }

    #[test]
    fn only_failing_ends_are_diagnosed() {
        let codec = vec!["Failed to open codec".to_owned()];
        assert_eq!(
            diagnose_end(EndFileReason::Error(-13), &[]).map(|d| d.kind),
            Some(PlaybackFailureKind::Application)
        );
        assert_eq!(
            diagnose_end(EndFileReason::Eof, &codec).map(|d| d.kind),
            Some(PlaybackFailureKind::MissingCodec)
        );
        assert_eq!(diagnose_end(EndFileReason::Eof, &[]), None);
        assert_eq!(diagnose_end(EndFileReason::Stop, &codec), None);
    }

    fn playing_state() -> PlaybackSnapshot {
        PlaybackSnapshot {
            time_pos: Some(95.5),
            duration: Some(100.0),
            volume: Some(85.0),
            tracks: vec![
                TrackInfo {
                    id: 1,
                    kind: CoreTrackKind::Audio,
                    selected: true,
                    external: false,
                    default: true,
                    title: None,
                    lang: Some("eng".to_owned()),
                },
                TrackInfo {
                    id: 2,
                    kind: CoreTrackKind::Subtitle,
                    selected: false,
                    external: true,
                    default: false,
                    title: Some("Commentary".to_owned()),
                    lang: Some("en".to_owned()),
                },
            ],
            ..PlaybackSnapshot::default()
        }
    }

    fn notice(command: Command) -> Option<String> {
        command_notice(&command, &playing_state())
    }

    #[test]
    fn accepted_commands_are_confirmed_with_the_cores_readouts() {
        assert_eq!(
            notice(Command::SetVolume { volume: 72.0 }).as_deref(),
            Some("Volume 72%")
        );
        assert_eq!(
            notice(Command::SetVolume { volume: 54.7 }).as_deref(),
            Some("Volume 54.7%")
        );
        assert_eq!(
            notice(Command::SetVolume { volume: 0.0 }).as_deref(),
            Some("Muted")
        );
        assert_eq!(
            notice(Command::SetMute { muted: true }).as_deref(),
            Some("Muted")
        );
        assert_eq!(
            notice(Command::SetMute { muted: false }).as_deref(),
            Some("Volume 85%")
        );
        assert_eq!(
            notice(Command::SetSpeed { speed: 1.25 }).as_deref(),
            Some("Speed 1.25×")
        );
        assert_eq!(
            notice(Command::SetSubtitleDelay { seconds: 0.25 }).as_deref(),
            Some("Subtitle delay: +250 ms")
        );
        assert_eq!(
            notice(Command::SelectTrack {
                kind: TrackKind::Audio,
                id: Some(1)
            })
            .as_deref(),
            Some("Audio: eng")
        );
        assert_eq!(
            notice(Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id: Some(2)
            })
            .as_deref(),
            Some("Subtitles: Commentary")
        );
        assert_eq!(
            notice(Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id: Some(9)
            })
            .as_deref(),
            Some("Subtitles: Track 9")
        );
        assert_eq!(
            notice(Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id: None
            })
            .as_deref(),
            Some("Subtitles off")
        );
        // A relative seek lands within the media.
        assert_eq!(
            notice(Command::Seek {
                seconds: 10.0,
                mode: SeekMode::Relative,
                precision: crate::protocol::SeekPrecision::Keyframe
            })
            .as_deref(),
            Some("01:40")
        );
        assert_eq!(
            notice(Command::Seek {
                seconds: -5.0,
                mode: SeekMode::Relative,
                precision: crate::protocol::SeekPrecision::Keyframe
            })
            .as_deref(),
            Some("01:30")
        );
    }

    #[test]
    fn commands_with_their_own_feedback_are_not_confirmed() {
        // A scrubber drag shows its own readout, and pause shows on the controls.
        assert_eq!(
            notice(Command::Seek {
                seconds: 30.0,
                mode: SeekMode::Absolute,
                precision: crate::protocol::SeekPrecision::Keyframe
            }),
            None
        );
        assert_eq!(notice(Command::TogglePause {}), None);
        assert_eq!(notice(Command::Close {}), None);
        // Without a position there is nothing to read out.
        let idle = PlaybackSnapshot::default();
        assert_eq!(
            command_notice(
                &Command::Seek {
                    seconds: 5.0,
                    mode: SeekMode::Relative,
                    precision: crate::protocol::SeekPrecision::Keyframe
                },
                &idle
            ),
            None
        );
    }

    #[test]
    fn resumes_and_up_next_edits_are_confirmed() {
        assert_eq!(resumed_notice(754.5), "Resumed at 12:34");
        let source = MediaSource {
            kind: SourceKind::Local,
            location: "/Movies/next.mkv".to_owned(),
        };
        let applied = DispatchResult::Applied {};
        let queue = |mode| PlaylistAction::Queue {
            sources: vec![source.clone()],
            mode,
        };
        assert_eq!(
            playlist_notice(&queue(QueueMode::Append), &applied).as_deref(),
            Some("Added to Up Next")
        );
        assert_eq!(
            playlist_notice(&queue(QueueMode::PlayNext), &applied).as_deref(),
            Some("Plays next")
        );
        assert_eq!(
            playlist_notice(&PlaylistAction::PlayNext { index: 2 }, &applied).as_deref(),
            Some("Plays next")
        );
        // Nothing queued, nothing to confirm.
        assert_eq!(
            playlist_notice(&queue(QueueMode::Append), &DispatchResult::NoOp {}),
            None
        );
        assert_eq!(playlist_notice(&PlaylistAction::Next {}, &applied), None);
    }
}
