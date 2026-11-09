//! The audio and subtitle menus' view of the session, and the subtitle size.
//!
//! The `tracks` query answers what the track menus show: every audio and
//! subtitle track of the current media with the core's one-line label
//! (`track_label`), subtitles Off, and the subtitle delay and size with their
//! readouts (`subtitle_delay`, `subtitle_style`). A shell renders the rows and
//! sends `select_track` with a row's id; it labels nothing itself.
//!
//! libmpv reports the delay and the size asynchronously, so the values asked
//! for lead until it reports them: keys pressed faster than its reports all
//! count, and a `tracks` query right after a change reads the new value. Each
//! value also carries libmpv's own last report (`reported`), which is where a
//! change shows that it arrived. The subtitle size (`sub-scale`) is
//! engine-global like the volume and holds across files; it is not part of the
//! snapshot, so a shell reads it from `tracks`.

use std::borrow::Cow;

use okp_core::subtitle_delay::{self, MAX_ENTRY_SECONDS};
use okp_core::{media_formats, subtitle_style, subtitle_tracks, track_label};
use serde::{Deserialize, Serialize};

use crate::engine::Observation;
use crate::protocol::{Command, SessionSnapshot, Status, Track, TrackKind};

/// How far the subtitle size keys move the size, as on Linux.
pub const SUBTITLE_SCALE_STEP: f64 = 0.1;

/// Two sizes closer than this are the same size.
const SCALE_EPSILON: f64 = 0.0005;

/// Two delays closer than this (seconds) are the same delay.
const DELAY_EPSILON: f64 = 0.000_5;

/// What the audio and subtitle menus show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TracksView {
    /// The audio tracks in the engine's order; empty without media or audio.
    pub audio: Vec<TrackChoice>,
    /// Subtitles Off (`id: null`) first, then the subtitle tracks.
    pub subtitles: Vec<TrackChoice>,
    pub subtitle_delay: SubtitleValue,
    pub subtitle_scale: SubtitleValue,
    /// Media is loaded (`playing` or `paused`): a subtitle file can be added
    /// and the delay and size changed.
    pub adjustable: bool,
}

/// One row of a track menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackChoice {
    /// What `select_track` takes; `null` turns subtitles off.
    pub id: Option<i64>,
    /// The core's descriptor: name, language, channels, codec, source.
    pub label: String,
    pub selected: bool,
}

/// A subtitle adjustment and its readout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleValue {
    /// Seconds for the delay, a factor (1 = 100 %) for the size: the value
    /// asked for last until libmpv reports it, then libmpv's.
    pub value: f64,
    /// `+250 ms`, `0 ms`; `110%`.
    pub label: String,
    /// libmpv's last report of the value; `None` before its first. It differs
    /// from `value` while a change is on its way to libmpv.
    pub reported: Option<f64>,
}

/// The track details labels need and the snapshot leaves out, and the
/// subtitle delay and size asked for.
#[derive(Debug, Default)]
pub(crate) struct TrackState {
    /// The engine's last report of the current media's tracks.
    details: Vec<okp_mpv::Track>,
    /// The delay asked for and not yet reported.
    asked_delay: Option<f64>,
    /// The size asked for and not yet reported.
    asked_scale: Option<f64>,
    /// The size the engine reported last; `None` before its first report.
    observed_scale: Option<f64>,
}

impl TrackState {
    /// Fold in the engine's report. Track details are taken only while the
    /// current media is loaded, like the snapshot's tracks.
    pub(crate) fn observe(&mut self, observation: &Observation, loaded: bool) {
        if loaded {
            self.details.clone_from(&observation.tracks);
            if self
                .asked_delay
                .is_some_and(|asked| (asked - observation.subtitle_delay).abs() < DELAY_EPSILON)
            {
                self.asked_delay = None;
            }
        }
        let Some(observed) = observation.subtitle_scale else {
            return;
        };
        self.observed_scale = Some(observed);
        if self
            .asked_scale
            .is_some_and(|asked| (asked - observed).abs() < SCALE_EPSILON)
        {
            self.asked_scale = None;
        }
    }

    /// The delay the delay keys build on: the one asked for last, else
    /// `reported`, the snapshot's.
    pub(crate) fn delay(&self, reported: f64) -> f64 {
        self.asked_delay.unwrap_or(reported)
    }

    /// The size the next nudge builds on: the one asked for last, else the
    /// reported one.
    pub(crate) fn scale(&self) -> f64 {
        self.asked_scale
            .or(self.observed_scale)
            .unwrap_or(subtitle_style::DEFAULT_SCALE)
    }

    /// The command the engine gets for `command`: a size nudge becomes a
    /// `set_subtitle_scale` from the current size, and a size is kept within
    /// the core's range (`subtitle_style::normalized_scale`). A non-finite
    /// value passes through for the core state machine to reject.
    pub(crate) fn resolve<'a>(&self, command: &'a Command) -> Cow<'a, Command> {
        match command {
            Command::NudgeSubtitleScale { delta } if delta.is_finite() => {
                Cow::Owned(Command::SetSubtitleScale {
                    scale: normalized(self.scale() + delta),
                })
            }
            Command::NudgeSubtitleScale { delta } => {
                Cow::Owned(Command::SetSubtitleScale { scale: *delta })
            }
            Command::SetSubtitleScale { scale } if scale.is_finite() => {
                Cow::Owned(Command::SetSubtitleScale {
                    scale: normalized(*scale),
                })
            }
            _ => Cow::Borrowed(command),
        }
    }

    /// The engine took `command`.
    pub(crate) fn forwarded(&mut self, command: &Command) {
        match command {
            Command::SetSubtitleScale { scale } => self.asked_scale = Some(*scale),
            // As okp-mpv clamps it.
            Command::SetSubtitleDelay { seconds } => {
                self.asked_delay = Some(seconds.clamp(-MAX_ENTRY_SECONDS, MAX_ENTRY_SECONDS));
            }
            // The delay belongs to one file; the next starts from libmpv's.
            Command::Open { .. } | Command::Close {} => self.asked_delay = None,
            _ => {}
        }
    }

    /// The menus for `snapshot`, the state the shell renders.
    pub(crate) fn view(&self, snapshot: &SessionSnapshot) -> TracksView {
        let of_kind = |kind| {
            snapshot
                .tracks
                .iter()
                .filter(move |track| track.kind == kind)
        };
        let audio = of_kind(TrackKind::Audio)
            .map(|track| self.choice(track))
            .collect();
        let has_primary = subtitle_tracks::has_primary_subtitle(
            of_kind(TrackKind::Subtitle).map(|track| (track.id, track.selected)),
            None,
        );
        let subtitles = std::iter::once(TrackChoice {
            id: None,
            label: "Off".to_owned(),
            selected: !has_primary,
        })
        .chain(of_kind(TrackKind::Subtitle).map(|track| self.choice(track)))
        .collect();
        let scale = self.scale();
        let delay = self.delay(snapshot.subtitle_delay);
        TracksView {
            audio,
            subtitles,
            subtitle_delay: SubtitleValue {
                value: delay,
                label: subtitle_delay::format_label(delay),
                reported: Some(snapshot.subtitle_delay),
            },
            subtitle_scale: SubtitleValue {
                value: scale,
                label: scale_label(scale),
                reported: self.observed_scale,
            },
            adjustable: matches!(snapshot.status, Status::Playing | Status::Paused),
        }
    }

    fn choice(&self, track: &Track) -> TrackChoice {
        let detail = self.details.iter().find(|detail| {
            detail.id == track.id
                && matches!(
                    (detail.kind, track.kind),
                    (okp_mpv::TrackKind::Audio, TrackKind::Audio)
                        | (okp_mpv::TrackKind::Subtitle, TrackKind::Subtitle)
                )
        });
        let codec = detail.and_then(|detail| detail.codec.as_deref());
        let title = track.title.as_deref();
        let lang = track.lang.as_deref();
        let label = match track.kind {
            TrackKind::Audio => track_label::audio_track_label(
                track.id,
                title,
                lang,
                detail.and_then(|detail| detail.audio_channels.as_deref()),
                codec,
            ),
            TrackKind::Subtitle => track_label::subtitle_track_label(
                track.id,
                title,
                lang,
                codec,
                detail.and_then(|detail| detail.external_filename.as_deref()),
                track.external,
                track.default,
            ),
        };
        TrackChoice {
            id: Some(track.id),
            label,
            selected: track.selected,
        }
    }
}

/// Why `command` cannot be carried out whatever the session's state: an
/// `add_subtitle` of a file that is not a subtitle by its extension.
pub(crate) fn invalid_argument(command: &Command) -> Option<String> {
    match command {
        Command::AddSubtitle { path } if !media_formats::is_subtitle(path) => {
            Some(format!("`{path}` is not a subtitle file"))
        }
        _ => None,
    }
}

/// A size within the core's range, on the hundredths a nudge moves along, so
/// repeated nudges do not drift.
fn normalized(scale: f64) -> f64 {
    (subtitle_style::normalized_scale(Some(scale)) * 100.0).round() / 100.0
}

/// The size readout, as the Linux subtitle popover writes it: `110%`.
fn scale_label(scale: f64) -> String {
    format!("{:.0}%", scale * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{MediaSource, SourceKind};

    fn track(id: i64, kind: TrackKind, selected: bool) -> Track {
        Track {
            id,
            kind,
            selected,
            external: false,
            default: false,
            title: None,
            lang: None,
        }
    }

    fn detail(id: i64, kind: okp_mpv::TrackKind) -> okp_mpv::Track {
        okp_mpv::Track {
            id,
            kind,
            selected: false,
            external: false,
            external_filename: None,
            default: false,
            title: None,
            lang: None,
            codec: None,
            audio_channels: None,
        }
    }

    fn playing(tracks: Vec<Track>) -> SessionSnapshot {
        SessionSnapshot {
            status: Status::Playing,
            source: Some(MediaSource {
                kind: SourceKind::Local,
                location: "/media/film.mkv".to_owned(),
            }),
            tracks,
            ..SessionSnapshot::default()
        }
    }

    fn observed(tracks: Vec<okp_mpv::Track>, scale: f64) -> Observation {
        Observation {
            tracks,
            subtitle_scale: Some(scale),
            ..Observation::default()
        }
    }

    #[test]
    fn rows_carry_the_core_labels_from_the_engines_track_details() {
        let mut state = TrackState::default();
        let mut audio = detail(1, okp_mpv::TrackKind::Audio);
        audio.codec = Some("eac3".to_owned());
        audio.audio_channels = Some("5.1".to_owned());
        let mut commentary = detail(2, okp_mpv::TrackKind::Audio);
        commentary.codec = Some("ac3".to_owned());
        commentary.audio_channels = Some("2.0".to_owned());
        let mut sidecar = detail(1, okp_mpv::TrackKind::Subtitle);
        sidecar.codec = Some("subrip".to_owned());
        sidecar.external_filename = Some("/media/film.en.srt".to_owned());
        state.observe(&observed(vec![audio, commentary, sidecar], 1.0), true);

        let mut english = track(1, TrackKind::Audio, true);
        english.title = Some("English".to_owned());
        english.lang = Some("eng".to_owned());
        let mut director = track(2, TrackKind::Audio, false);
        director.title = Some("Director's Commentary".to_owned());
        director.lang = Some("eng".to_owned());
        let mut external = track(1, TrackKind::Subtitle, true);
        external.external = true;
        external.lang = Some("en".to_owned());
        let view = state.view(&playing(vec![english, director, external]));

        let labels =
            |rows: &[TrackChoice]| rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>();
        assert_eq!(
            labels(&view.audio),
            [
                "English · 5.1 · EAC3",
                "Director's Commentary · ENG · 2.0 · AC3"
            ]
        );
        assert_eq!(labels(&view.subtitles), ["Off", "en · SRT · EXT"]);
        assert_eq!(
            view.subtitles.iter().map(|row| row.id).collect::<Vec<_>>(),
            [None, Some(1)],
            "audio and subtitle ids are separate: subtitle 1 is not audio 1"
        );
        assert_eq!(
            view.audio
                .iter()
                .map(|row| row.selected)
                .collect::<Vec<_>>(),
            [true, false]
        );
        assert!(
            !view.subtitles[0].selected,
            "a subtitle is on, so Off is not"
        );
    }

    #[test]
    fn off_is_selected_when_no_subtitle_is_and_present_without_any_track() {
        let state = TrackState::default();
        let view = state.view(&playing(vec![
            track(1, TrackKind::Audio, true),
            track(2, TrackKind::Subtitle, false),
        ]));
        assert_eq!(view.subtitles.len(), 2);
        assert!(view.subtitles[0].selected);
        assert_eq!(
            view.subtitles[1].label, "Track 2",
            "a track without details still has a name"
        );

        let idle = state.view(&SessionSnapshot::default());
        assert!(idle.audio.is_empty());
        assert_eq!(
            idle.subtitles,
            [TrackChoice {
                id: None,
                label: "Off".to_owned(),
                selected: true,
            }]
        );
        assert!(!idle.adjustable);
    }

    #[test]
    fn details_of_media_that_is_not_loaded_yet_are_not_taken() {
        let mut state = TrackState::default();
        let mut aac = detail(1, okp_mpv::TrackKind::Audio);
        aac.codec = Some("aac".to_owned());
        state.observe(&observed(vec![aac], 1.0), true);
        // The next file is opening: the engine still reports the old file's tracks.
        let mut flac = detail(1, okp_mpv::TrackKind::Audio);
        flac.codec = Some("flac".to_owned());
        state.observe(&observed(vec![flac], 1.0), false);
        let view = state.view(&playing(vec![track(1, TrackKind::Audio, true)]));
        assert_eq!(view.audio[0].label, "Track 1 · AAC");
    }

    #[test]
    fn the_readouts_follow_the_delay_and_the_size() {
        let mut state = TrackState::default();
        state.observe(&observed(Vec::new(), 1.1), true);
        let mut snapshot = playing(Vec::new());
        snapshot.subtitle_delay = -0.125;
        let view = state.view(&snapshot);
        assert_eq!(
            view.subtitle_delay,
            SubtitleValue {
                value: -0.125,
                label: "-125 ms".to_owned(),
                reported: Some(-0.125),
            }
        );
        assert_eq!(view.subtitle_scale.label, "110%");
        assert_eq!(view.subtitle_scale.reported, Some(1.1));
        assert!(view.adjustable);
        snapshot.status = Status::Ended;
        assert!(!state.view(&snapshot).adjustable);
    }

    #[test]
    fn nudges_build_on_the_size_asked_for_until_the_engine_reports_it() {
        let mut state = TrackState::default();
        state.observe(&observed(Vec::new(), 1.0), true);
        let up = Command::NudgeSubtitleScale {
            delta: SUBTITLE_SCALE_STEP,
        };
        let first = state.resolve(&up).into_owned();
        assert_eq!(first, Command::SetSubtitleScale { scale: 1.1 });
        state.forwarded(&first);
        // A second press before the engine has reported the first.
        state.observe(&observed(Vec::new(), 1.0), true);
        let second = state.resolve(&up).into_owned();
        assert_eq!(second, Command::SetSubtitleScale { scale: 1.2 });
        state.forwarded(&second);
        let pending = state.view(&playing(Vec::new())).subtitle_scale;
        assert_eq!(pending.label, "120%");
        assert_eq!(
            pending.reported,
            Some(1.0),
            "libmpv's own value stays apart from the size asked for"
        );
        // Once reported, the engine's value is the one nudges build on.
        state.observe(&observed(Vec::new(), 1.2), true);
        assert_eq!(state.scale(), 1.2);
        assert_eq!(
            state.view(&playing(Vec::new())).subtitle_scale.reported,
            Some(1.2)
        );
    }

    #[test]
    fn the_delay_asked_for_leads_until_the_engine_reports_it_and_ends_with_the_file() {
        let mut state = TrackState::default();
        let mut snapshot = playing(Vec::new());
        state.forwarded(&Command::SetSubtitleDelay { seconds: 0.05 });
        assert_eq!(state.delay(snapshot.subtitle_delay), 0.05);
        assert_eq!(state.view(&snapshot).subtitle_delay.label, "+50 ms");
        // Reported: the snapshot's value is the one keys build on again.
        let reported = Observation {
            subtitle_delay: 0.05,
            ..Observation::default()
        };
        state.observe(&reported, true);
        snapshot.subtitle_delay = -0.3;
        assert_eq!(state.delay(snapshot.subtitle_delay), -0.3);

        state.forwarded(&Command::SetSubtitleDelay { seconds: -900.0 });
        assert_eq!(
            state.delay(0.0),
            -MAX_ENTRY_SECONDS,
            "clamped as okp-mpv clamps it"
        );
        state.forwarded(&Command::Close {});
        assert_eq!(
            state.delay(0.0),
            0.0,
            "a delay asked for one file does not outlive it"
        );
    }

    #[test]
    fn the_size_stays_within_the_core_range_and_off_the_float_drift() {
        let mut state = TrackState::default();
        state.observe(&observed(Vec::new(), 0.3), true);
        let down = Command::NudgeSubtitleScale { delta: -0.1 };
        assert_eq!(
            state.resolve(&down).into_owned(),
            Command::SetSubtitleScale {
                scale: subtitle_style::MIN_SCALE
            }
        );
        assert_eq!(
            state
                .resolve(&Command::SetSubtitleScale { scale: 9.0 })
                .into_owned(),
            Command::SetSubtitleScale {
                scale: subtitle_style::MAX_SCALE
            }
        );
        let mut scale = 1.0;
        for _ in 0..7 {
            let Command::SetSubtitleScale { scale: next } = state
                .resolve(&Command::NudgeSubtitleScale { delta: 0.1 })
                .into_owned()
            else {
                panic!("a nudge becomes a size")
            };
            scale = next;
            state.forwarded(&Command::SetSubtitleScale { scale });
        }
        assert_eq!(scale, 1.0, "0.3 plus seven tenths is exactly 1");
        // A non-finite value is left for the core state machine to reject.
        assert!(matches!(
            state
                .resolve(&Command::NudgeSubtitleScale { delta: f64::NAN })
                .into_owned(),
            Command::SetSubtitleScale { scale } if scale.is_nan()
        ));
    }
}
