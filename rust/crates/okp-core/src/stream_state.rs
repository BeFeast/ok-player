//! Pure presentation models for network / live-stream playback. A direct
//! `http(s)://` (or other network) source is not loaded synchronously: it opens,
//! connects, buffers, and can fail or run indefinitely. This module models three
//! things the shell needs to render that lifecycle deliberately, without putting
//! the state machine in the shell (freeze-boundary discipline):
//!
//! 1. [`classify`] — where a source sits in its load → buffer → play lifecycle,
//!    from raw signals the shell samples each transport tick.
//! 2. [`timeline_readout`] — how the elapsed/duration labels and the seek slider
//!    read out, including the live case where the duration is unknown and the
//!    naive `position / duration` math is undefined.
//! 3. [`stream_error`] — a failed load rendered as a short human headline plus a
//!    copyable technical detail block, so the primary UI never dumps raw logs.
//!
//! No UI or engine dependency; the C# side has no counterpart yet (this is a
//! Linux-first PRD surface), so there is no executable spec to mirror — the tests
//! below are the spec.

use crate::time_code;

/// Placeholder shown for an unknown elapsed/duration readout — a live stream, or
/// a source still connecting — instead of a misleading `00:00` that would imply a
/// zero-length clip.
pub const UNKNOWN_TIME_TEXT: &str = "--:--";

/// The presented lifecycle phase of the current media source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamPhase {
    /// Nothing is loaded.
    Idle,
    /// Opened; waiting for the demuxer to connect and deliver the first data.
    Connecting,
    /// Playing (or paused by the user) with a healthy cache.
    Active,
    /// The cache ran dry after loading; re-buffering.
    Buffering,
    /// The source failed to open, or dropped mid-playback.
    Failed,
}

impl StreamPhase {
    /// Whether a loading/buffering indicator should be shown for this phase.
    pub fn is_busy(self) -> bool {
        matches!(self, Self::Connecting | Self::Buffering)
    }

    /// Whether the failure surface (with its recovery actions) should be shown.
    pub fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// Raw signals sampled from the player each transport tick. `file_loaded` and
/// `failed` are latched by the shell from mpv lifecycle events for the *current*
/// source (reset when a new source is opened); `paused_for_cache` is a live
/// property read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamSignals {
    pub has_media: bool,
    pub file_loaded: bool,
    pub failed: bool,
    pub paused_for_cache: bool,
}

/// Classify the load lifecycle phase from the sampled signals. A failure wins
/// over everything (so a mid-stream drop is not masked by a stale
/// `paused-for-cache`); a source that has not reached `FileLoaded` yet is
/// Connecting; once loaded, an empty cache means Buffering, otherwise Active.
pub fn classify(signals: StreamSignals) -> StreamPhase {
    if !signals.has_media {
        return StreamPhase::Idle;
    }
    if signals.failed {
        return StreamPhase::Failed;
    }
    if !signals.file_loaded {
        return StreamPhase::Connecting;
    }
    if signals.paused_for_cache {
        return StreamPhase::Buffering;
    }
    StreamPhase::Active
}

/// What the transport should display for the elapsed/duration labels and the
/// seek slider, given the live position and duration.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineReadout {
    /// The elapsed-time label (always a real clock — a live stream still counts
    /// up from where playback began).
    pub elapsed_text: String,
    /// The duration label, or [`UNKNOWN_TIME_TEXT`] when the duration is unknown.
    pub duration_text: String,
    /// Whether the seek slider should accept input. False for live/unknown
    /// duration, where there is no meaningful position to seek to.
    pub seekable: bool,
    /// Upper bound for the seek slider's range (never zero, so the widget stays
    /// well-formed even with no duration).
    pub range_max: f64,
    /// Where the seek slider's handle sits.
    pub value: f64,
}

/// Derive the transport readout. When the duration is known and positive the
/// slider tracks `position` within `[0, duration]`; when it is unknown (a live
/// stream, or a source still connecting) the duration reads `--:--`, the slider
/// is inert at zero, and the elapsed clock keeps counting — deliberate live
/// behavior instead of a `position / 0` division.
pub fn timeline_readout(time_pos: Option<f64>, duration: Option<f64>) -> TimelineReadout {
    let raw_time = time_pos
        .filter(|time| time.is_finite())
        .unwrap_or(0.0)
        .max(0.0);

    match duration.filter(|duration| duration.is_finite() && *duration > 0.0) {
        Some(duration) => {
            let value = raw_time.min(duration);
            TimelineReadout {
                elapsed_text: time_code::format_clock(value),
                duration_text: time_code::format_clock(duration),
                seekable: true,
                range_max: duration.max(1.0),
                value,
            }
        }
        None => TimelineReadout {
            elapsed_text: time_code::format_clock(raw_time),
            duration_text: UNKNOWN_TIME_TEXT.to_owned(),
            seekable: false,
            range_max: 1.0,
            value: 0.0,
        },
    }
}

/// A failed load rendered for the transport's error surface: a short human
/// `title` and `hint` for the primary UI, plus a `detail` block that stays
/// copyable but is never shown as the primary message (no raw log dumping into
/// the main surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamError {
    pub title: String,
    pub hint: String,
    pub detail: String,
}

/// Build the error presentation from the failed source, whether it was a network
/// source (stream-vs-file wording), the mpv error code, and an optional raw
/// engine message. The raw message only ever lands in `detail`.
pub fn stream_error(
    source: &str,
    is_network: bool,
    code: i32,
    raw_message: Option<&str>,
) -> StreamError {
    let title = if is_network {
        "Can't play this stream"
    } else {
        "Can't play this file"
    }
    .to_owned();

    let hint = failure_hint(code, is_network).to_owned();

    let mut detail = format!("Source: {source}\nmpv error {code}");
    if let Some(raw) = raw_message
        .map(str::trim)
        .filter(|message| !message.is_empty())
    {
        detail.push_str(": ");
        detail.push_str(raw);
    }

    StreamError {
        title,
        hint,
        detail,
    }
}

/// Map an mpv error code to a friendly, non-technical cause. Codes are from
/// libmpv's `mpv_error` enum (client.h): loading failed `-18`, unknown format
/// `-19`, nothing to play `-21`, unsupported `-22`.
fn failure_hint(code: i32, is_network: bool) -> &'static str {
    match code {
        -18 if is_network => {
            "The stream could not be opened. Check the address and your connection."
        }
        -18 => "The file could not be opened. It may be missing or unreadable.",
        -19 => "The media format isn't supported.",
        -21 => "There's nothing to play at this source.",
        -22 => "This media uses a feature that isn't supported.",
        _ if is_network => "Playback failed. The stream may be offline or unreachable.",
        _ => "Playback failed.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(
        has_media: bool,
        file_loaded: bool,
        failed: bool,
        paused_for_cache: bool,
    ) -> StreamSignals {
        StreamSignals {
            has_media,
            file_loaded,
            failed,
            paused_for_cache,
        }
    }

    #[test]
    fn classify_walks_the_load_lifecycle() {
        let cases = [
            // has_media, file_loaded, failed, paused_for_cache -> phase
            (signals(false, false, false, false), StreamPhase::Idle),
            (signals(true, false, false, false), StreamPhase::Connecting),
            (signals(true, true, false, false), StreamPhase::Active),
            (signals(true, true, false, true), StreamPhase::Buffering),
            (signals(true, false, true, false), StreamPhase::Failed),
        ];
        for (input, expected) in cases {
            assert_eq!(classify(input), expected, "{input:?}");
        }
    }

    #[test]
    fn classify_failure_wins_over_a_stale_cache_pause() {
        // A mid-stream drop must not be masked by a lingering paused-for-cache.
        let phase = classify(signals(true, true, true, true));
        assert_eq!(phase, StreamPhase::Failed);
    }

    #[test]
    fn phase_busy_and_failed_predicates() {
        assert!(StreamPhase::Connecting.is_busy());
        assert!(StreamPhase::Buffering.is_busy());
        assert!(!StreamPhase::Active.is_busy());
        assert!(!StreamPhase::Idle.is_busy());
        assert!(StreamPhase::Failed.is_failed());
        assert!(!StreamPhase::Buffering.is_failed());
    }

    #[test]
    fn timeline_readout_tracks_a_known_duration() {
        let readout = timeline_readout(Some(65.0), Some(3600.0));
        assert_eq!(readout.elapsed_text, "01:05");
        assert_eq!(readout.duration_text, "01:00:00");
        assert!(readout.seekable);
        assert_eq!(readout.range_max, 3600.0);
        assert_eq!(readout.value, 65.0);
    }

    #[test]
    fn timeline_readout_clamps_position_into_the_duration() {
        // An overshooting time-pos (a decoder running slightly past EOF) must not
        // push the slider past the end or misreport the elapsed clock.
        let readout = timeline_readout(Some(120.0), Some(90.0));
        assert_eq!(readout.value, 90.0);
        assert_eq!(readout.elapsed_text, "01:30");
    }

    #[test]
    fn timeline_readout_unknown_duration_is_live_style() {
        // Unknown duration (live stream / still connecting): the duration reads
        // the placeholder, the slider is inert, and the elapsed clock keeps
        // counting up rather than dividing by a zero duration.
        for duration in [None, Some(0.0), Some(f64::NAN), Some(f64::INFINITY)] {
            let readout = timeline_readout(Some(42.0), duration);
            assert_eq!(readout.duration_text, UNKNOWN_TIME_TEXT, "{duration:?}");
            assert!(!readout.seekable, "{duration:?}");
            assert_eq!(readout.range_max, 1.0, "{duration:?}");
            assert_eq!(readout.value, 0.0, "{duration:?}");
            assert_eq!(readout.elapsed_text, "00:42", "{duration:?}");
        }
    }

    #[test]
    fn timeline_readout_absent_position_reads_zero() {
        let readout = timeline_readout(None, None);
        assert_eq!(readout.elapsed_text, "00:00");
        assert_eq!(readout.duration_text, UNKNOWN_TIME_TEXT);
    }

    #[test]
    fn stream_error_uses_network_wording_and_maps_the_code() {
        let error = stream_error(
            "https://example.com/live.m3u8",
            true,
            -18,
            Some("Failed to open."),
        );
        assert_eq!(error.title, "Can't play this stream");
        assert_eq!(
            error.hint,
            "The stream could not be opened. Check the address and your connection."
        );
        assert!(error.detail.contains("https://example.com/live.m3u8"));
        assert!(error.detail.contains("mpv error -18"));
        assert!(error.detail.contains("Failed to open."));
    }

    #[test]
    fn stream_error_local_wording_and_generic_fallback() {
        let error = stream_error("/media/clip.mkv", false, -40, None);
        assert_eq!(error.title, "Can't play this file");
        assert_eq!(error.hint, "Playback failed.");
        // No raw message: the detail carries source + code, and nothing more.
        assert_eq!(error.detail, "Source: /media/clip.mkv\nmpv error -40");
    }

    #[test]
    fn stream_error_hint_distinguishes_known_codes() {
        assert_eq!(
            stream_error("x", true, -19, None).hint,
            "The media format isn't supported."
        );
        assert_eq!(
            stream_error("x", true, -21, None).hint,
            "There's nothing to play at this source."
        );
        assert_eq!(
            stream_error("x", true, -99, None).hint,
            "Playback failed. The stream may be offline or unreachable."
        );
    }

    #[test]
    fn stream_error_detail_ignores_blank_raw_message() {
        let error = stream_error("rtsp://host/live", true, -18, Some("   "));
        assert_eq!(error.detail, "Source: rtsp://host/live\nmpv error -18");
    }
}
