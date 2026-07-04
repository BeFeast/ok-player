//! Transient navigation readout for precise seeking (PRD P4-N4).
//!
//! Frame step, fine seek, and timecode jump all surface a short overlay that
//! reports where playback landed: the clock timecode plus the current frame
//! number when the frame rate is known. The projection math lives here — pure
//! and testable — so the GTK shell only has to wire mpv commands to these
//! helpers and hand the string to its transient toast.

use crate::time_code;

/// Separator between the timecode and the frame number in the readout line.
const SEPARATOR: &str = " · ";

/// The 0-based frame index for a playback position at the given frame rate,
/// matching mpv's `estimated-frame-number` (round to nearest). Returns `None`
/// when the position or frame rate is missing or non-positive, so audio-only
/// and frame-rate-less sources omit the frame number rather than guess one.
pub fn frame_number(time_pos: f64, fps: Option<f64>) -> Option<i64> {
    let fps = fps?;
    if !time_pos.is_finite() || time_pos < 0.0 || !fps.is_finite() || fps <= 0.0 {
        return None;
    }

    Some((time_pos * fps).round().max(0.0) as i64)
}

/// Clamp a projected seek target into the media's valid range. An unknown or
/// non-positive `duration` (live streams, still-loading media) only clamps the
/// lower bound so seeking near the live edge is never pinned to zero.
pub fn clamp_target(target: f64, duration: f64) -> f64 {
    let target = if target.is_finite() {
        target.max(0.0)
    } else {
        0.0
    };
    if duration.is_finite() && duration > 0.0 {
        target.min(duration)
    } else {
        target
    }
}

/// The position a relative fine seek lands on: `time_pos + delta`, clamped to
/// the media range. Shared by the keyboard arrows and any UI seek so both paths
/// report the same target.
pub fn seek_target(time_pos: f64, delta: f64, duration: f64) -> f64 {
    clamp_target(time_pos + delta, duration)
}

/// The position a single frame step lands on. With a known frame rate it walks
/// the frame grid (a forward step reports frame N+1, back N-1, never below 0);
/// without one it falls back to the current position so the readout still shows
/// a timecode where mpv supports stepping an audio/fps-less stream.
pub fn frame_step_target(time_pos: f64, fps: Option<f64>, forward: bool, duration: f64) -> f64 {
    match (fps, frame_number(time_pos, fps)) {
        (Some(fps), Some(current)) => {
            let step = if forward { 1 } else { -1 };
            let target_frame = (current + step).max(0);
            clamp_target(target_frame as f64 / fps, duration)
        }
        _ => clamp_target(time_pos, duration),
    }
}

/// The transient readout line for a position: the clock timecode, plus the
/// frame number when the frame rate is known.
pub fn format_readout(time_pos: f64, fps: Option<f64>) -> String {
    let clock = time_code::format_clock(time_pos);
    match frame_number(time_pos, fps) {
        Some(frame) => format!("{clock}{SEPARATOR}Frame {frame}"),
        None => clock,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_number_rounds_to_nearest_frame() {
        // 24 fps: 5.0 s is exactly frame 120; a hair past still rounds to 120.
        assert_eq!(frame_number(5.0, Some(24.0)), Some(120));
        assert_eq!(frame_number(5.01, Some(24.0)), Some(120));
        // Half a frame past rounds up.
        assert_eq!(frame_number(5.0 + 0.5 / 24.0, Some(24.0)), Some(121));
        // Start of stream is frame 0, not 1.
        assert_eq!(frame_number(0.0, Some(30.0)), Some(0));
    }

    #[test]
    fn frame_number_is_absent_without_a_usable_frame_rate() {
        assert_eq!(frame_number(10.0, None), None);
        assert_eq!(frame_number(10.0, Some(0.0)), None);
        assert_eq!(frame_number(10.0, Some(-1.0)), None);
        assert_eq!(frame_number(10.0, Some(f64::NAN)), None);
        assert_eq!(frame_number(f64::INFINITY, Some(24.0)), None);
        assert_eq!(frame_number(-1.0, Some(24.0)), None);
    }

    #[test]
    fn clamp_target_pins_to_the_media_range() {
        assert_eq!(clamp_target(50.0, 120.0), 50.0);
        assert_eq!(clamp_target(-5.0, 120.0), 0.0);
        assert_eq!(clamp_target(500.0, 120.0), 120.0);
        // Unknown / live duration only clamps the lower bound.
        assert_eq!(clamp_target(500.0, 0.0), 500.0);
        assert_eq!(clamp_target(-5.0, 0.0), 0.0);
        assert_eq!(clamp_target(f64::NAN, 120.0), 0.0);
    }

    #[test]
    fn seek_target_applies_relative_delta_and_clamps() {
        assert_eq!(seek_target(30.0, 5.0, 120.0), 35.0);
        assert_eq!(seek_target(30.0, -5.0, 120.0), 25.0);
        // Cannot seek before the start or past the end.
        assert_eq!(seek_target(2.0, -5.0, 120.0), 0.0);
        assert_eq!(seek_target(118.0, 5.0, 120.0), 120.0);
        // Live/unknown duration still allows forward motion.
        assert_eq!(seek_target(600.0, 5.0, 0.0), 605.0);
    }

    #[test]
    fn frame_step_target_walks_the_frame_grid() {
        // Frame 120 (@24 fps) steps to 121 forward, 119 back.
        assert_eq!(
            frame_step_target(5.0, Some(24.0), true, 120.0),
            121.0 / 24.0
        );
        assert_eq!(
            frame_step_target(5.0, Some(24.0), false, 120.0),
            119.0 / 24.0
        );
        // Stepping back at the first frame stays at frame 0.
        assert_eq!(frame_step_target(0.0, Some(24.0), false, 120.0), 0.0);
    }

    #[test]
    fn frame_step_target_without_fps_holds_the_current_position() {
        // No frame rate: keep the current (clamped) timecode so the readout is
        // still meaningful for audio / fps-less sources mpv can step.
        assert_eq!(frame_step_target(42.0, None, true, 120.0), 42.0);
        assert_eq!(frame_step_target(200.0, None, true, 120.0), 120.0);
    }

    #[test]
    fn format_readout_pairs_timecode_with_frame_number() {
        assert_eq!(format_readout(5.0, Some(24.0)), "00:05 · Frame 120");
        assert_eq!(
            format_readout(5025.0, Some(25.0)),
            "01:23:45 · Frame 125625"
        );
        assert_eq!(format_readout(0.0, Some(30.0)), "00:00 · Frame 0");
    }

    #[test]
    fn format_readout_omits_frame_number_when_unavailable() {
        // Audio-only / unknown frame rate: timecode only, no dangling "Frame".
        assert_eq!(format_readout(5.0, None), "00:05");
        assert_eq!(format_readout(90.0, Some(0.0)), "01:30");
    }

    #[test]
    fn readout_round_trips_a_forward_frame_step() {
        // The projected step position reports exactly the next frame number.
        let target = frame_step_target(5.0, Some(24.0), true, 120.0);
        assert_eq!(frame_number(target, Some(24.0)), Some(121));
        assert_eq!(format_readout(target, Some(24.0)), "00:05 · Frame 121");
    }
}
