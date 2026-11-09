//! The transport controls' view of the session: the time readout, the scrubber's
//! progress, buffered range and chapter ticks, the volume and speed controls, and
//! when the on-screen controls may hide.
//!
//! Every rule here is the core's (`time_code`, `network_media`, `timeline_buffer`,
//! `chapter_math`, `volume`, `osc_visibility`), so a shell renders what this module
//! projects instead of re-implementing them. A shell asks for it with the
//! `transport` query after each drained batch; the projection reads the same
//! snapshot the `snapshot` query returns.

use okp_core::volume::{self, VolumeState};
use okp_core::{chapter_math, osc_visibility, time_code, timeline_buffer};
use serde::{Deserialize, Serialize};

use crate::protocol::{Command, SessionSnapshot, SourceKind, Status};

/// The choices of the speed menu, as on Linux.
pub const SPEED_PRESETS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// How long the controls stay up without activity while media plays: the single
/// idle timeout of the PRD (P1-D6, about 2.5 s).
pub const OSC_HIDE_AFTER_MS: u64 = 2500;

/// Two speeds closer than this are the same menu choice.
const SPEED_MATCH_EPSILON: f64 = 0.005;

/// What the transport controls show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportView {
    /// Elapsed time: `MM:SS`, or `HH:MM:SS` from the first hour.
    pub elapsed: String,
    /// Time remaining as `-MM:SS`; `--:--` for a stream without a known duration.
    pub remaining: String,
    /// The duration; `--:--` for a stream without a known duration.
    pub total: String,
    /// The playhead as a fraction of the duration; 0 while it is unknown.
    pub progress: f64,
    /// How far the cache reaches ahead of the playhead, as a fraction.
    pub buffered: f64,
    /// Chapter starts as fractions of the duration, for tick marks.
    pub chapter_ticks: Vec<f64>,
    /// A click on the scrubber can seek: media is loaded and its duration known.
    pub seekable: bool,
    pub volume: VolumeView,
    /// The current speed, as the speed control shows it (`1.00×`).
    pub speed: String,
    pub speeds: Vec<SpeedChoice>,
    pub osc: OscView,
}

/// The volume control: 0 to `max` percent, with the unity mark at
/// `unity_fraction` of the track and boost above it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeView {
    /// Output level in percent.
    pub level: f64,
    /// The highest level, in percent.
    pub max: f64,
    /// `level` as a fraction of `max`.
    pub fraction: f64,
    /// 100 % as a fraction of `max`.
    pub unity_fraction: f64,
    /// Audio is muted, or the level is zero.
    pub muted: bool,
    /// The level is above 100 %.
    pub boosted: bool,
    /// `Muted`, or the level as `85%` / `54.7%`.
    pub readout: String,
}

/// One entry of the speed menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedChoice {
    pub speed: f64,
    pub label: String,
    /// The current speed.
    pub selected: bool,
}

/// When the on-screen controls exist and when they may hide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OscView {
    /// The controls while shown.
    pub revealed: OscSurface,
    /// The controls once they have hidden for inactivity.
    pub hidden: OscSurface,
    /// The controls hide after `hide_after_ms` without activity. Only while
    /// media plays; paused or loading media keeps them up.
    pub auto_hide: bool,
    pub hide_after_ms: u64,
}

/// The core's projection of the controls' surface (`osc_visibility`). Without
/// media they are absent; hidden for inactivity they stay in place but take no
/// focus and no clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OscSurface {
    pub visible: bool,
    pub mapped: bool,
    pub focusable: bool,
    pub hit_testable: bool,
}

impl From<osc_visibility::OscVisibility> for OscSurface {
    fn from(value: osc_visibility::OscVisibility) -> Self {
        Self {
            visible: value.visible,
            mapped: value.mapped,
            focusable: value.focusable,
            hit_testable: value.hit_testable,
        }
    }
}

impl TransportView {
    /// The controls for `snapshot`. With `at`, the time readout and progress are
    /// projected at that position instead of the playhead, which is what a
    /// scrubber shows while it is dragged.
    #[must_use]
    pub fn project(snapshot: &SessionSnapshot, at: Option<f64>) -> Self {
        let has_media = snapshot.source.is_some();
        let is_url = snapshot
            .source
            .as_ref()
            .is_some_and(|source| source.kind == SourceKind::Url);
        let duration = snapshot
            .duration
            .filter(|value| value.is_finite() && *value > 0.0);
        let position = at
            .or(snapshot.time_pos)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0)
            .max(0.0);
        // Past the end the readout stays at the end, as on Linux.
        let position = duration.map_or(position, |duration| position.min(duration));
        let loaded = matches!(
            snapshot.status,
            Status::Playing | Status::Paused | Status::Ended
        );
        let chapter_times: Vec<f64> = snapshot
            .chapters
            .iter()
            .map(|chapter| chapter.time)
            .collect();
        Self {
            elapsed: time_code::format_clock(position),
            remaining: time_code::format_trailing(
                time_code::TrailingTimeMode::Remaining,
                is_url,
                position,
                duration,
            ),
            total: time_code::format_trailing(
                time_code::TrailingTimeMode::Total,
                is_url,
                position,
                duration,
            ),
            progress: duration.map_or(0.0, |duration| (position / duration).clamp(0.0, 1.0)),
            // The cache reaches ahead of the real playhead, wherever a drag is.
            buffered: timeline_buffer::fraction(snapshot.time_pos, snapshot.cache_ahead, duration),
            chapter_ticks: chapter_math::fractions(&chapter_times, duration.unwrap_or(0.0)),
            seekable: has_media && loaded && duration.is_some(),
            volume: VolumeView::project(snapshot.volume, snapshot.mute),
            speed: format_speed(current_speed(snapshot.speed)),
            speeds: SPEED_PRESETS
                .iter()
                .map(|&speed| SpeedChoice {
                    speed,
                    label: format_speed(speed),
                    selected: (current_speed(snapshot.speed) - speed).abs() < SPEED_MATCH_EPSILON,
                })
                .collect(),
            osc: OscView {
                revealed: osc_visibility::project(has_media, false, true).into(),
                hidden: osc_visibility::project(has_media, false, false).into(),
                auto_hide: has_media && snapshot.status == Status::Playing,
                hide_after_ms: OSC_HIDE_AFTER_MS,
            },
        }
    }
}

/// Whether audio is muted: libmpv's mute flag is on, or the level is the core's
/// mute level of 0 (`toggle_mute`). The transport view's `muted` and the
/// registry's Mute check mark both read it.
pub(crate) fn audio_muted(level: Option<f64>, mute: bool) -> bool {
    mute || volume_state(level).is_muted()
}

fn volume_state(level: Option<f64>) -> VolumeState {
    VolumeState::new(level.unwrap_or(volume::DEFAULT_RESTORE_VOLUME))
}

impl VolumeView {
    fn project(level: Option<f64>, mute: bool) -> Self {
        let state = volume_state(level);
        Self {
            level: state.level(),
            max: volume::MAX_VOLUME,
            fraction: state.level_fraction(),
            unity_fraction: VolumeState::unity_fraction(),
            muted: audio_muted(level, mute),
            boosted: state.is_boosted(),
            readout: if mute {
                "Muted".to_owned()
            } else {
                state.readout()
            },
        }
    }
}

fn current_speed(speed: Option<f64>) -> f64 {
    speed.filter(|value| value.is_finite()).unwrap_or(1.0)
}

/// `1.25×`, as the Linux speed control shows it.
pub(crate) fn format_speed(speed: f64) -> String {
    format!("{:.2}×", speed.clamp(0.25, 4.0))
}

/// The volume the session asks the engine for, with the core's mute rule: mute
/// is a level of 0 that remembers the last level above it, and unmuting goes
/// back there (`volume::VolumeState::toggle_mute`, as on Linux).
///
/// libmpv reports a volume change asynchronously, so the level asked for leads
/// until the engine reports it: nudges and mute toggles that arrive faster than
/// its reports build on that level instead of the stale report (core
/// `volume::reconcile_observed_level`).
#[derive(Debug)]
pub(crate) struct VolumeTarget {
    /// The level asked for and not yet reported.
    pending: Option<f64>,
    /// The last level above zero, asked for or reported; unmuting restores it.
    remembered: f64,
}

impl Default for VolumeTarget {
    fn default() -> Self {
        Self {
            pending: None,
            remembered: volume::DEFAULT_RESTORE_VOLUME,
        }
    }
}

impl VolumeTarget {
    /// The command the engine gets for `command`, given the level the engine
    /// reported last (`observed`) and libmpv's own mute flag (`muted`):
    ///
    /// - `nudge_volume` becomes a `set_volume` to the nudged level, clamped by
    ///   the core to 0-130 %. A non-finite nudge becomes a non-finite
    ///   `set_volume`, which the core state machine rejects.
    /// - `toggle_mute` becomes a `set_volume` to 0, or back to the remembered
    ///   level from 0. While libmpv's mute flag is on (a `set_mute` asked for
    ///   it) it becomes `set_mute(false)` instead, so the first toggle always
    ///   unmutes.
    /// - Every other command passes through; a `set_volume` moves the level the
    ///   next nudge builds on.
    pub(crate) fn resolve<'a>(
        &mut self,
        command: &'a Command,
        observed: Option<f64>,
        muted: bool,
    ) -> std::borrow::Cow<'a, Command> {
        use std::borrow::Cow;
        match command {
            Command::NudgeVolume { delta } if delta.is_finite() => {
                let mut state = self.state(observed);
                let volume = state.nudge(*delta);
                self.ask(state);
                Cow::Owned(Command::SetVolume { volume })
            }
            Command::NudgeVolume { delta } => Cow::Owned(Command::SetVolume { volume: *delta }),
            Command::SetVolume { volume } if volume.is_finite() => {
                let mut state = self.state(observed);
                state.set_level(*volume);
                self.ask(state);
                Cow::Borrowed(command)
            }
            Command::ToggleMute {} if muted => Cow::Owned(Command::SetMute { muted: false }),
            Command::ToggleMute {} => {
                let mut state = self.state(observed);
                let volume = state.toggle_mute();
                self.ask(state);
                Cow::Owned(Command::SetVolume { volume })
            }
            _ => Cow::Borrowed(command),
        }
    }

    /// The core volume model at the level asked for last, or the reported one,
    /// remembering the last level above zero.
    fn state(&self, observed: Option<f64>) -> VolumeState {
        let mut state = VolumeState::new(self.remembered);
        state.set_level(
            self.pending
                .or(observed)
                .unwrap_or(volume::DEFAULT_RESTORE_VOLUME),
        );
        state
    }

    fn ask(&mut self, state: VolumeState) {
        self.pending = Some(state.level());
        self.remembered = state.remembered_nonzero();
    }

    /// The engine reported `observed`; once it matches the level asked for,
    /// nudges build on the engine's reports again. A reported level above zero
    /// is the one unmuting restores.
    pub(crate) fn observe(&mut self, observed: Option<f64>) {
        if let Some(observed) = observed
            && let Some(level) = volume::reconcile_observed_level(&mut self.pending, observed)
            && level > volume::MIN_VOLUME
        {
            self.remembered = level;
        }
    }

    /// The engine refused the level asked for.
    pub(crate) fn forget(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Chapter, MediaSource};

    fn playing(location: &str, kind: SourceKind) -> SessionSnapshot {
        SessionSnapshot {
            status: Status::Playing,
            source: Some(MediaSource {
                kind,
                location: location.to_owned(),
            }),
            ..SessionSnapshot::default()
        }
    }

    #[test]
    fn the_readout_uses_the_core_clock_and_clamps_past_the_end() {
        let snapshot = SessionSnapshot {
            time_pos: Some(75.9),
            duration: Some(3725.0),
            ..playing("/media/film.mkv", SourceKind::Local)
        };
        let view = TransportView::project(&snapshot, None);
        assert_eq!(view.elapsed, "01:15");
        assert_eq!(view.remaining, "-01:00:49");
        assert_eq!(view.total, "01:02:05");
        assert!((view.progress - 75.9 / 3725.0).abs() < 1e-12);
        assert!(view.seekable);

        let past_end = SessionSnapshot {
            time_pos: Some(4000.0),
            ..snapshot
        };
        let view = TransportView::project(&past_end, None);
        assert_eq!(view.elapsed, "01:02:05");
        assert_eq!(view.remaining, "-00:00");
        assert_eq!(view.progress, 1.0);
    }

    #[test]
    fn a_stream_without_a_duration_shows_the_live_sentinel_and_cannot_seek() {
        let snapshot = SessionSnapshot {
            time_pos: Some(12.0),
            ..playing("https://example.com/live.m3u8", SourceKind::Url)
        };
        let view = TransportView::project(&snapshot, None);
        assert_eq!(view.elapsed, "00:12");
        assert_eq!(view.remaining, "--:--");
        assert_eq!(view.total, "--:--");
        assert_eq!(view.progress, 0.0);
        assert!(!view.seekable);

        // A local file that has not reported its duration yet is loading, not live.
        let local = SessionSnapshot {
            time_pos: Some(12.0),
            ..playing("/media/film.mkv", SourceKind::Local)
        };
        let view = TransportView::project(&local, None);
        assert_eq!(view.remaining, "-00:00");
        assert_eq!(view.total, "00:00");
    }

    #[test]
    fn a_drag_position_moves_the_readout_but_not_the_buffered_range() {
        let snapshot = SessionSnapshot {
            time_pos: Some(30.0),
            duration: Some(120.0),
            cache_ahead: Some(15.0),
            ..playing("/media/film.mkv", SourceKind::Local)
        };
        let view = TransportView::project(&snapshot, Some(90.0));
        assert_eq!(view.elapsed, "01:30");
        assert_eq!(view.remaining, "-00:30");
        assert_eq!(view.progress, 0.75);
        assert_eq!(view.buffered, 0.375, "the cache is ahead of the playhead");

        let before_start = TransportView::project(&snapshot, Some(-4.0));
        assert_eq!(before_start.elapsed, "00:00");
        assert_eq!(before_start.progress, 0.0);
    }

    #[test]
    fn chapter_ticks_are_fractions_of_the_duration() {
        let snapshot = SessionSnapshot {
            duration: Some(200.0),
            chapters: vec![
                Chapter {
                    index: 0,
                    time: 0.0,
                    title: None,
                },
                Chapter {
                    index: 1,
                    time: 50.0,
                    title: Some("Two".to_owned()),
                },
                Chapter {
                    index: 2,
                    time: 150.0,
                    title: None,
                },
            ],
            ..playing("/media/film.mkv", SourceKind::Local)
        };
        assert_eq!(
            TransportView::project(&snapshot, None).chapter_ticks,
            [0.0, 0.25, 0.75]
        );
        let unknown = SessionSnapshot {
            duration: None,
            ..snapshot
        };
        assert!(
            TransportView::project(&unknown, None)
                .chapter_ticks
                .is_empty()
        );
    }

    #[test]
    fn the_volume_follows_the_core_range_mute_and_readout() {
        let view = TransportView::project(
            &SessionSnapshot {
                volume: Some(117.0),
                ..SessionSnapshot::default()
            },
            None,
        )
        .volume;
        assert_eq!(view.max, 130.0);
        assert_eq!(view.fraction, 0.9);
        assert!((view.unity_fraction - 100.0 / 130.0).abs() < 1e-12);
        assert!(view.boosted);
        assert!(!view.muted);
        assert_eq!(view.readout, "117%");

        let muted = TransportView::project(
            &SessionSnapshot {
                volume: Some(54.7),
                mute: true,
                ..SessionSnapshot::default()
            },
            None,
        )
        .volume;
        assert!(muted.muted);
        assert_eq!(muted.level, 54.7, "muting keeps the level");
        assert_eq!(muted.readout, "Muted");

        let unknown = TransportView::project(&SessionSnapshot::default(), None).volume;
        assert_eq!(unknown.level, 100.0);
    }

    #[test]
    fn the_speed_menu_marks_the_current_speed() {
        let snapshot = SessionSnapshot {
            speed: Some(1.25),
            ..SessionSnapshot::default()
        };
        let view = TransportView::project(&snapshot, None);
        assert_eq!(view.speed, "1.25×");
        let labels: Vec<_> = view.speeds.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            ["0.50×", "0.75×", "1.00×", "1.25×", "1.50×", "2.00×"]
        );
        let selected: Vec<_> = view
            .speeds
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.speed)
            .collect();
        assert_eq!(selected, [1.25]);

        let odd = TransportView::project(
            &SessionSnapshot {
                speed: Some(1.1),
                ..SessionSnapshot::default()
            },
            None,
        );
        assert_eq!(odd.speed, "1.10×");
        assert!(odd.speeds.iter().all(|choice| !choice.selected));
    }

    #[test]
    fn the_controls_auto_hide_only_while_media_plays() {
        let idle = TransportView::project(&SessionSnapshot::default(), None).osc;
        assert!(!idle.revealed.visible, "no media, no controls");
        assert!(!idle.auto_hide);

        let shown = TransportView::project(&playing("/media/a.mkv", SourceKind::Local), None).osc;
        assert!(shown.auto_hide);
        assert_eq!(shown.hide_after_ms, 2500);
        assert_eq!(
            shown.revealed,
            OscSurface {
                visible: true,
                mapped: true,
                focusable: true,
                hit_testable: true,
            }
        );
        assert_eq!(
            shown.hidden,
            OscSurface {
                visible: true,
                mapped: true,
                focusable: false,
                hit_testable: false,
            },
            "hidden controls stay in place but take no clicks"
        );

        for status in [Status::Paused, Status::Opening, Status::Ended] {
            let snapshot = SessionSnapshot {
                status,
                ..playing("/media/a.mkv", SourceKind::Local)
            };
            assert!(
                !TransportView::project(&snapshot, None).osc.auto_hide,
                "{status:?}"
            );
        }
    }

    #[test]
    fn nudges_faster_than_the_engine_reports_build_on_the_level_asked_for() {
        let mut target = VolumeTarget::default();
        let nudge = Command::NudgeVolume { delta: 1.0 };
        assert_eq!(
            target.resolve(&nudge, Some(80.0), false).into_owned(),
            Command::SetVolume { volume: 81.0 }
        );
        // The engine still reports 80; the next nudge starts from 81.
        target.observe(Some(80.0));
        assert_eq!(
            target.resolve(&nudge, Some(80.0), false).into_owned(),
            Command::SetVolume { volume: 82.0 }
        );
        // Once the engine has caught up, its reports lead again.
        target.observe(Some(82.0));
        assert_eq!(
            target.resolve(&nudge, Some(60.0), false).into_owned(),
            Command::SetVolume { volume: 61.0 }
        );
    }

    #[test]
    fn nudges_clamp_to_the_core_range_and_set_volume_moves_the_base() {
        let mut target = VolumeTarget::default();
        assert_eq!(
            target
                .resolve(&Command::NudgeVolume { delta: 5.0 }, Some(128.0), false)
                .into_owned(),
            Command::SetVolume { volume: 130.0 }
        );
        assert_eq!(
            target
                .resolve(&Command::SetVolume { volume: 40.0 }, Some(130.0), false)
                .into_owned(),
            Command::SetVolume { volume: 40.0 },
            "set_volume itself passes through"
        );
        assert_eq!(
            target
                .resolve(&Command::NudgeVolume { delta: -50.0 }, Some(130.0), false)
                .into_owned(),
            Command::SetVolume { volume: 0.0 },
            "the nudge starts from the level set last and stops at zero"
        );
        target.forget();
        assert_eq!(
            target
                .resolve(&Command::NudgeVolume { delta: -1.0 }, None, false)
                .into_owned(),
            Command::SetVolume { volume: 99.0 },
            "without any level the core default is the base"
        );
    }

    #[test]
    fn a_non_finite_nudge_stays_non_finite_for_the_core_to_reject() {
        let mut target = VolumeTarget::default();
        let resolved = target
            .resolve(&Command::NudgeVolume { delta: f64::NAN }, Some(50.0), false)
            .into_owned();
        assert!(matches!(resolved, Command::SetVolume { volume } if volume.is_nan()));
        assert_eq!(
            target
                .resolve(&Command::NudgeVolume { delta: 1.0 }, Some(50.0), false)
                .into_owned(),
            Command::SetVolume { volume: 51.0 },
            "the refused nudge left no target behind"
        );
    }

    fn level(command: std::borrow::Cow<'_, Command>) -> f64 {
        match command.into_owned() {
            Command::SetVolume { volume } => volume,
            other => panic!("not a set_volume: {other:?}"),
        }
    }

    #[test]
    fn toggle_mute_goes_to_zero_and_back_to_the_level_before_it() {
        let mut target = VolumeTarget::default();
        let toggle = Command::ToggleMute {};
        assert_eq!(level(target.resolve(&toggle, Some(72.5), false)), 0.0);
        // libmpv still reports 72.5; the next toggle builds on the 0 asked for.
        target.observe(Some(72.5));
        assert_eq!(level(target.resolve(&toggle, Some(72.5), false)), 72.5);
    }

    #[test]
    fn unmuting_from_zero_restores_the_last_level_above_it() {
        let mut target = VolumeTarget::default();
        // libmpv started at 64 %, and the slider was dragged to 0 %.
        assert_eq!(
            level(target.resolve(&Command::SetVolume { volume: 0.0 }, Some(64.0), false)),
            0.0
        );
        target.observe(Some(0.0));
        assert_eq!(
            level(target.resolve(&Command::ToggleMute {}, Some(0.0), false)),
            64.0,
            "dragged to 0 %, the mute button brings the sound back"
        );

        // Levels libmpv reported without being asked count as well.
        let mut reported = VolumeTarget::default();
        reported.observe(Some(37.0));
        reported.observe(Some(0.0));
        assert_eq!(
            level(reported.resolve(&Command::ToggleMute {}, Some(0.0), false)),
            37.0
        );

        // With no level above zero ever seen, the core default comes back.
        let mut fresh = VolumeTarget::default();
        fresh.observe(Some(0.0));
        assert_eq!(
            level(fresh.resolve(&Command::ToggleMute {}, Some(0.0), false)),
            volume::DEFAULT_RESTORE_VOLUME
        );
    }

    #[test]
    fn a_nudge_up_from_mute_is_audible_and_leaves_the_mute_level_behind() {
        let mut target = VolumeTarget::default();
        assert_eq!(
            level(target.resolve(&Command::ToggleMute {}, Some(80.0), false)),
            0.0
        );
        assert_eq!(
            level(target.resolve(&Command::NudgeVolume { delta: 1.0 }, Some(80.0), false)),
            1.0
        );
        assert_eq!(
            level(target.resolve(&Command::ToggleMute {}, Some(80.0), false)),
            0.0,
            "the level is audible again, so the next toggle mutes"
        );
        assert_eq!(
            level(target.resolve(&Command::ToggleMute {}, Some(80.0), false)),
            1.0
        );
    }

    #[test]
    fn toggle_mute_clears_libmpvs_mute_flag_first() {
        let mut target = VolumeTarget::default();
        assert_eq!(
            target
                .resolve(&Command::ToggleMute {}, Some(55.0), true)
                .into_owned(),
            Command::SetMute { muted: false }
        );
        assert_eq!(
            level(target.resolve(&Command::ToggleMute {}, Some(55.0), false)),
            0.0,
            "the flag did not move the level"
        );
    }
}
