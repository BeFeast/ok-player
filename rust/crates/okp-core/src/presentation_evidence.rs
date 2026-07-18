//! Machine-readable live presentation evidence for the Linux Wayland path.
//!
//! The shell writes newline-delimited records so a terminated private run still
//! leaves durable timestamps. Acceptance is evaluated here rather than in GTK:
//! presentation cadence, decode mode, drop deltas, and playback-clock rate are
//! portable facts even though only a live Wayland operator can collect them.

use serde::{Deserialize, Serialize};

pub const PRESENTATION_EVIDENCE_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationBackend {
    NativeWaylandEgl,
    NativeWaylandDmabuf,
    GtkGlArea,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum PresentationRecord {
    Session {
        schema_version: u32,
        backend: PresentationBackend,
    },
    BackendSelected {
        monotonic_ns: u64,
        backend: PresentationBackend,
    },
    Present {
        monotonic_ns: u64,
        sequence: u64,
        width: i32,
        height: i32,
        boundary: String,
    },
    CompositorPresented {
        monotonic_ns: u64,
        backend: PresentationBackend,
        presented_ns: u64,
        sequence: u64,
        refresh_ns: u32,
        flags: u32,
        width: i32,
        height: i32,
    },
    CompositorDiscarded {
        monotonic_ns: u64,
        backend: PresentationBackend,
    },
    Playback {
        monotonic_ns: u64,
        time_pos: Option<f64>,
        speed: f64,
        hwdec_current: Option<String>,
        decoder_drops: i64,
        vo_drops: i64,
    },
    Action {
        monotonic_ns: u64,
        action: PresentationAction,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationAction {
    SeekForward,
    SeekBackward,
    SpeedDouble,
    SpeedNormal,
}

impl PresentationAction {
    pub fn seek_seconds(self) -> Option<f64> {
        match self {
            Self::SeekForward => Some(10.0),
            Self::SeekBackward => Some(-10.0),
            _ => None,
        }
    }

    pub fn speed(self) -> Option<f64> {
        match self {
            Self::SpeedDouble => Some(2.0),
            Self::SpeedNormal => Some(1.0),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresentationExercise {
    start_ns: Option<u64>,
    next_action: usize,
}

impl PresentationExercise {
    pub fn poll(&mut self, monotonic_ns: u64, playing: bool) -> Option<PresentationAction> {
        if !playing {
            return None;
        }
        let start_ns = *self.start_ns.get_or_insert(monotonic_ns);
        let elapsed_seconds = monotonic_ns.saturating_sub(start_ns) as f64 / 1_000_000_000.0;
        const SCHEDULE: &[(f64, PresentationAction)] = &[
            (20.0, PresentationAction::SeekForward),
            (23.0, PresentationAction::SeekBackward),
            (26.0, PresentationAction::SpeedDouble),
            (32.0, PresentationAction::SpeedNormal),
        ];
        let (threshold, action) = *SCHEDULE.get(self.next_action)?;
        if elapsed_seconds < threshold {
            return None;
        }
        self.next_action += 1;
        Some(action)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentationThresholds {
    pub window_seconds: f64,
    pub minimum_presents_per_second: f64,
    pub playback_rate_tolerance: f64,
    pub maximum_decoder_drop_delta: i64,
    pub maximum_vo_drop_delta: i64,
}

impl Default for PresentationThresholds {
    fn default() -> Self {
        Self {
            window_seconds: 15.0,
            minimum_presents_per_second: 55.0,
            playback_rate_tolerance: 0.05,
            maximum_decoder_drop_delta: 0,
            maximum_vo_drop_delta: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresentationSummary {
    pub window_start_ns: u64,
    pub window_end_ns: u64,
    pub presents: usize,
    pub presents_per_second: f64,
    pub backend: Option<PresentationBackend>,
    pub compositor_presented: usize,
    pub compositor_discarded: usize,
    pub median_interval_ms: Option<f64>,
    pub p95_interval_ms: Option<f64>,
    pub p99_interval_ms: Option<f64>,
    pub playback_rate: Option<f64>,
    pub hwdec_current: Option<String>,
    pub decoder_drop_delta: Option<i64>,
    pub vo_drop_delta: Option<i64>,
    pub errors: Vec<String>,
}

impl PresentationSummary {
    pub fn passed(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn summarize_window(
    records: &[PresentationRecord],
    window_start_ns: u64,
    thresholds: PresentationThresholds,
) -> PresentationSummary {
    let window_ns = (thresholds.window_seconds * 1_000_000_000.0).round() as u64;
    let window_end_ns = window_start_ns.saturating_add(window_ns);
    let mut backend = records.iter().find_map(|record| match record {
        PresentationRecord::Session { backend, .. } => Some(*backend),
        _ => None,
    });
    for record in records {
        if let PresentationRecord::BackendSelected {
            monotonic_ns,
            backend: selected,
        } = record
            && *monotonic_ns <= window_start_ns
        {
            backend = Some(*selected);
        }
    }
    let backend_changed = records.iter().any(|record| {
        matches!(record, PresentationRecord::BackendSelected { monotonic_ns, .. }
            if *monotonic_ns > window_start_ns && *monotonic_ns < window_end_ns)
    });
    let compositor_presented = records
        .iter()
        .filter(|record| {
            matches!(record, PresentationRecord::CompositorPresented { monotonic_ns, .. }
                if *monotonic_ns >= window_start_ns && *monotonic_ns < window_end_ns)
        })
        .count();
    let compositor_discarded = records
        .iter()
        .filter(|record| {
            matches!(record, PresentationRecord::CompositorDiscarded { monotonic_ns, .. }
                if *monotonic_ns >= window_start_ns && *monotonic_ns < window_end_ns)
        })
        .count();
    let legacy_presents = records
        .iter()
        .filter(|record| {
            matches!(record, PresentationRecord::Present { monotonic_ns, .. }
                if *monotonic_ns >= window_start_ns && *monotonic_ns < window_end_ns)
        })
        .count();
    let presents = if compositor_presented > 0 {
        compositor_presented
    } else {
        legacy_presents
    };
    let presents_per_second = presents as f64 / thresholds.window_seconds;
    let compositor_timestamps = records
        .iter()
        .filter_map(|record| match record {
            PresentationRecord::CompositorPresented {
                monotonic_ns,
                presented_ns,
                ..
            } if *monotonic_ns >= window_start_ns && *monotonic_ns < window_end_ns => {
                Some(*presented_ns)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let intervals_ms = compositor_timestamps
        .windows(2)
        .filter_map(|timestamps| {
            timestamps[1]
                .checked_sub(timestamps[0])
                .map(|interval| interval as f64 / 1_000_000.0)
        })
        .collect::<Vec<_>>();

    let playback = records
        .iter()
        .filter_map(|record| match record {
            PresentationRecord::Playback {
                monotonic_ns,
                time_pos,
                speed,
                hwdec_current,
                decoder_drops,
                vo_drops,
            } if *monotonic_ns >= window_start_ns && *monotonic_ns <= window_end_ns => Some((
                *monotonic_ns,
                *time_pos,
                *speed,
                hwdec_current.as_deref(),
                *decoder_drops,
                *vo_drops,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    let first = playback.first().copied();
    let last = playback.last().copied();
    let playback_rate = match (first, last) {
        (Some((first_ns, Some(first_pos), ..)), Some((last_ns, Some(last_pos), ..)))
            if last_ns > first_ns =>
        {
            Some((last_pos - first_pos) / ((last_ns - first_ns) as f64 / 1_000_000_000.0))
        }
        _ => None,
    };
    let hwdec_current = playback
        .iter()
        .rev()
        .find_map(|(_, _, _, hwdec, _, _)| hwdec.map(str::to_owned));
    let decoder_drop_delta = match (first, last) {
        (Some((_, _, _, _, first, _)), Some((_, _, _, _, last, _))) => Some(last - first),
        _ => None,
    };
    let vo_drop_delta = match (first, last) {
        (Some((_, _, _, _, _, first)), Some((_, _, _, _, _, last))) => Some(last - first),
        _ => None,
    };

    let mut errors = Vec::new();
    if backend_changed {
        errors.push("the presentation backend changed inside the accepted window".to_owned());
    }
    if presents_per_second < thresholds.minimum_presents_per_second {
        errors.push(format!(
            "presentation cadence was {presents_per_second:.2} fps, expected at least {:.2}",
            thresholds.minimum_presents_per_second
        ));
    }
    if hwdec_current.as_deref() != Some("vaapi") {
        errors.push(format!(
            "hwdec-current was {}, expected vaapi",
            hwdec_current.as_deref().unwrap_or("unavailable")
        ));
    }
    match playback_rate {
        Some(rate) if (rate - 1.0).abs() <= thresholds.playback_rate_tolerance => {}
        Some(rate) => errors.push(format!(
            "playback clock advanced at {rate:.3}x, expected 1.0x ± {:.3}",
            thresholds.playback_rate_tolerance
        )),
        None => errors.push("the window has no usable playback clock samples".to_owned()),
    }
    match decoder_drop_delta {
        Some(delta) if delta <= thresholds.maximum_decoder_drop_delta => {}
        Some(delta) => errors.push(format!(
            "decoder drops increased by {delta}, maximum is {}",
            thresholds.maximum_decoder_drop_delta
        )),
        None => errors.push("the window has no decoder-drop samples".to_owned()),
    }
    match vo_drop_delta {
        Some(delta) if delta <= thresholds.maximum_vo_drop_delta => {}
        Some(delta) => errors.push(format!(
            "VO drops increased by {delta}, maximum is {}",
            thresholds.maximum_vo_drop_delta
        )),
        None => errors.push("the window has no VO-drop samples".to_owned()),
    }

    PresentationSummary {
        window_start_ns,
        window_end_ns,
        presents,
        presents_per_second,
        backend,
        compositor_presented,
        compositor_discarded,
        median_interval_ms: percentile(&intervals_ms, 0.50),
        p95_interval_ms: percentile(&intervals_ms, 0.95),
        p99_interval_ms: percentile(&intervals_ms, 0.99),
        playback_rate,
        hwdec_current,
        decoder_drop_delta,
        vo_drop_delta,
        errors,
    }
}

fn percentile(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = (percentile * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.saturating_sub(1)).copied()
}

pub fn exercise_errors(
    records: &[PresentationRecord],
    thresholds: PresentationThresholds,
) -> Vec<String> {
    let mut errors = Vec::new();
    let action_time = |wanted| {
        records.iter().find_map(|record| match record {
            PresentationRecord::Action {
                monotonic_ns,
                action,
            } if *action == wanted => Some(*monotonic_ns),
            _ => None,
        })
    };
    let Some(seek_forward) = action_time(PresentationAction::SeekForward) else {
        return vec!["the presentation exercise did not record seek-forward".to_owned()];
    };
    let Some(seek_backward) = action_time(PresentationAction::SeekBackward) else {
        return vec!["the presentation exercise did not record seek-backward".to_owned()];
    };
    let Some(speed_double) = action_time(PresentationAction::SpeedDouble) else {
        return vec!["the presentation exercise did not record speed-double".to_owned()];
    };
    let Some(speed_normal) = action_time(PresentationAction::SpeedNormal) else {
        return vec!["the presentation exercise did not record speed-normal".to_owned()];
    };

    for (name, action_ns, minimum_delta) in [
        ("seek-forward", seek_forward, 8.0),
        ("seek-backward", seek_backward, -8.0),
    ] {
        let before = playback_position_at_or_before(records, action_ns);
        let after = playback_position_in_range(
            records,
            action_ns.saturating_add(200_000_000),
            action_ns.saturating_add(1_500_000_000),
        );
        match (before, after) {
            (Some(before), Some(after)) => {
                let delta = after - before;
                let responsive = if minimum_delta > 0.0 {
                    delta >= minimum_delta
                } else {
                    delta <= minimum_delta
                };
                if !responsive {
                    errors.push(format!(
                        "{name} moved the playback clock by {delta:+.2}s within 1.5s"
                    ));
                }
            }
            _ => errors.push(format!(
                "{name} has no usable before/after playback samples"
            )),
        }
        let recovery_fps = presents_per_second(
            records,
            action_ns.saturating_add(500_000_000),
            2_000_000_000,
        );
        if recovery_fps < thresholds.minimum_presents_per_second {
            errors.push(format!(
                "{name} recovered at {recovery_fps:.2} fps, expected at least {:.2}",
                thresholds.minimum_presents_per_second
            ));
        }
    }

    let double_rate = playback_rate_in_range(
        records,
        speed_double.saturating_add(500_000_000),
        speed_normal.saturating_sub(500_000_000),
    );
    match double_rate {
        Some(rate) if (rate - 2.0).abs() <= 0.10 => {}
        Some(rate) => errors.push(format!("2x playback advanced at {rate:.3}x")),
        None => errors.push("2x playback has no usable clock samples".to_owned()),
    }
    let double_fps = presents_per_second(
        records,
        speed_double.saturating_add(500_000_000),
        speed_normal
            .saturating_sub(500_000_000)
            .saturating_sub(speed_double.saturating_add(500_000_000)),
    );
    if double_fps < thresholds.minimum_presents_per_second {
        errors.push(format!(
            "2x playback presented at {double_fps:.2} fps, expected at least {:.2}",
            thresholds.minimum_presents_per_second
        ));
    }

    let recovered = summarize_window(
        records,
        speed_normal.saturating_add(1_000_000_000),
        thresholds,
    );
    errors.extend(
        recovered
            .errors
            .into_iter()
            .map(|error| format!("post-2x recovery: {error}")),
    );
    errors
}

fn playback_position_at_or_before(records: &[PresentationRecord], end_ns: u64) -> Option<f64> {
    records.iter().rev().find_map(|record| match record {
        PresentationRecord::Playback {
            monotonic_ns,
            time_pos: Some(time_pos),
            ..
        } if *monotonic_ns <= end_ns => Some(*time_pos),
        _ => None,
    })
}

fn playback_position_in_range(
    records: &[PresentationRecord],
    start_ns: u64,
    end_ns: u64,
) -> Option<f64> {
    records.iter().rev().find_map(|record| match record {
        PresentationRecord::Playback {
            monotonic_ns,
            time_pos: Some(time_pos),
            ..
        } if *monotonic_ns >= start_ns && *monotonic_ns <= end_ns => Some(*time_pos),
        _ => None,
    })
}

fn playback_rate_in_range(
    records: &[PresentationRecord],
    start_ns: u64,
    end_ns: u64,
) -> Option<f64> {
    let samples = records
        .iter()
        .filter_map(|record| match record {
            PresentationRecord::Playback {
                monotonic_ns,
                time_pos: Some(time_pos),
                ..
            } if *monotonic_ns >= start_ns && *monotonic_ns <= end_ns => {
                Some((*monotonic_ns, *time_pos))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let (first_ns, first_pos) = samples.first().copied()?;
    let (last_ns, last_pos) = samples.last().copied()?;
    (last_ns > first_ns)
        .then(|| (last_pos - first_pos) / ((last_ns - first_ns) as f64 / 1_000_000_000.0))
}

fn presents_per_second(records: &[PresentationRecord], start_ns: u64, duration_ns: u64) -> f64 {
    if duration_ns == 0 {
        return 0.0;
    }
    let end_ns = start_ns.saturating_add(duration_ns);
    let compositor_presents = records
        .iter()
        .filter(|record| {
            matches!(record, PresentationRecord::CompositorPresented { monotonic_ns, .. }
                if *monotonic_ns >= start_ns && *monotonic_ns < end_ns)
        })
        .count();
    let legacy_presents = records
        .iter()
        .filter(|record| {
            matches!(record, PresentationRecord::Present { monotonic_ns, .. }
                if *monotonic_ns >= start_ns && *monotonic_ns < end_ns)
        })
        .count();
    let presents = if compositor_presents > 0 {
        compositor_presents
    } else {
        legacy_presents
    };
    presents as f64 / (duration_ns as f64 / 1_000_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_fifteen_second_native_window_passes_at_fifty_five_presents_per_second() {
        let start = 10_000_000_000;
        let mut records = (0..825)
            .map(|index| PresentationRecord::Present {
                monotonic_ns: start + index * 1_000_000_000 / 55,
                sequence: index,
                width: 3840,
                height: 2160,
                boundary: "egl-swap-buffers".to_owned(),
            })
            .collect::<Vec<_>>();
        records.push(PresentationRecord::Playback {
            monotonic_ns: start,
            time_pos: Some(5.0),
            speed: 1.0,
            hwdec_current: Some("vaapi".to_owned()),
            decoder_drops: 0,
            vo_drops: 0,
        });
        records.push(PresentationRecord::Playback {
            monotonic_ns: start + 15_000_000_000,
            time_pos: Some(20.0),
            speed: 1.0,
            hwdec_current: Some("vaapi".to_owned()),
            decoder_drops: 0,
            vo_drops: 0,
        });

        let summary = summarize_window(&records, start, PresentationThresholds::default());
        assert!(summary.passed(), "{:?}", summary.errors);
        assert_eq!(summary.presents, 825);
    }

    #[test]
    fn callback_counts_cannot_hide_low_final_surface_cadence() {
        let start = 1_000_000_000;
        let mut records = (0..900)
            .map(|index| PresentationRecord::Present {
                monotonic_ns: start + index * 1_000_000_000 / 60,
                sequence: index,
                width: 3840,
                height: 2160,
                boundary: "egl-swap-buffers".to_owned(),
            })
            .collect::<Vec<_>>();
        records.extend(
            (0..375).map(|index| PresentationRecord::CompositorPresented {
                monotonic_ns: start + index * 40_000_000,
                backend: PresentationBackend::NativeWaylandEgl,
                presented_ns: 5_000_000_000 + index * 40_000_000,
                sequence: index,
                refresh_ns: 16_666_667,
                flags: 0,
                width: 3840,
                height: 2160,
            }),
        );
        records.push(PresentationRecord::CompositorDiscarded {
            monotonic_ns: start + 5_000_000_000,
            backend: PresentationBackend::NativeWaylandEgl,
        });
        for (offset, position) in [(0, 0.0), (15_000_000_000, 15.0)] {
            records.push(PresentationRecord::Playback {
                monotonic_ns: start + offset,
                time_pos: Some(position),
                speed: 1.0,
                hwdec_current: Some("vaapi".to_owned()),
                decoder_drops: 0,
                vo_drops: 0,
            });
        }

        let summary = summarize_window(&records, start, PresentationThresholds::default());
        assert!(!summary.passed());
        assert!(summary.errors[0].contains("25.00 fps"));
        assert_eq!(summary.compositor_presented, 375);
        assert_eq!(summary.compositor_discarded, 1);
        assert_eq!(summary.median_interval_ms, Some(40.0));
        assert_eq!(summary.p95_interval_ms, Some(40.0));
        assert_eq!(summary.p99_interval_ms, Some(40.0));
    }

    #[test]
    fn exercise_schedule_keeps_baseline_seek_and_speed_phases_distinct() {
        let mut exercise = PresentationExercise::default();
        assert_eq!(exercise.poll(1_000, false), None);
        assert_eq!(exercise.poll(1_000, true), None);
        assert_eq!(
            exercise.poll(20_000_001_000, true),
            Some(PresentationAction::SeekForward)
        );
        assert_eq!(
            exercise.poll(23_000_001_000, true),
            Some(PresentationAction::SeekBackward)
        );
        assert_eq!(
            exercise.poll(26_000_001_000, true),
            Some(PresentationAction::SpeedDouble)
        );
        assert_eq!(
            exercise.poll(32_000_001_000, true),
            Some(PresentationAction::SpeedNormal)
        );
    }

    #[test]
    fn complete_seek_and_speed_exercise_passes_with_sixty_final_swaps() {
        let mut records = (0..3_000_u64)
            .map(|index| PresentationRecord::Present {
                monotonic_ns: index * 1_000_000_000 / 60,
                sequence: index,
                width: 3840,
                height: 2160,
                boundary: "egl-swap-buffers".to_owned(),
            })
            .collect::<Vec<_>>();
        for index in 0..250_u64 {
            let seconds = index as f64 / 5.0;
            let time_pos = if seconds <= 20.0 {
                seconds
            } else if seconds <= 23.0 {
                seconds + 10.0
            } else if seconds <= 26.0 {
                seconds
            } else if seconds <= 32.0 {
                26.0 + (seconds - 26.0) * 2.0
            } else {
                seconds + 6.0
            };
            records.push(PresentationRecord::Playback {
                monotonic_ns: index * 200_000_000,
                time_pos: Some(time_pos),
                speed: if (26.0..32.0).contains(&seconds) {
                    2.0
                } else {
                    1.0
                },
                hwdec_current: Some("vaapi".to_owned()),
                decoder_drops: 0,
                vo_drops: 0,
            });
        }
        for (seconds, action) in [
            (20, PresentationAction::SeekForward),
            (23, PresentationAction::SeekBackward),
            (26, PresentationAction::SpeedDouble),
            (32, PresentationAction::SpeedNormal),
        ] {
            records.push(PresentationRecord::Action {
                monotonic_ns: seconds * 1_000_000_000,
                action,
            });
        }
        records.sort_by_key(|record| match record {
            PresentationRecord::Session { .. } => 0,
            PresentationRecord::BackendSelected { monotonic_ns, .. }
            | PresentationRecord::Present { monotonic_ns, .. }
            | PresentationRecord::CompositorPresented { monotonic_ns, .. }
            | PresentationRecord::CompositorDiscarded { monotonic_ns, .. }
            | PresentationRecord::Playback { monotonic_ns, .. }
            | PresentationRecord::Action { monotonic_ns, .. } => *monotonic_ns,
        });

        let errors = exercise_errors(&records, PresentationThresholds::default());
        assert!(errors.is_empty(), "{errors:?}");
    }
}
