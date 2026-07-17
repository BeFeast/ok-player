//! Representative poster-frame selection for the "Continue watching"/History shelf.
//!
//! A pure port of the Windows poster pass (`PlayerView.GeneratePostersAsync` +
//! `PickRepresentativeFrameAsync`): the shell decodes candidate frames with its platform
//! codec and scores them with [`crate::image_luma`], while this module owns the parts that
//! must stay identical across shells and across both Linux surfaces (welcome shelf and the
//! full History list) — the cache identity, the bounded sampling plan, the luma thresholds,
//! and the pick/verdict. Keeping this here (not in a shell) is what lets both surfaces share
//! one cached result and one selection policy, and lets the logic be unit-tested without a
//! decoder.
//!
//! Why sample at all: a single fixed grab often lands on a fade/black scene (studio logos,
//! dark openings, end credits) and produces a black poster. We sample a handful of positions
//! across the runtime and keep the brightest, stopping early once a clearly-lit frame is in
//! hand. When even the brightest sample is essentially black, no poster is cached and the card
//! keeps its gradient placeholder rather than showing a black block.

use std::hash::{DefaultHasher, Hash, Hasher};

use crate::media_formats;
use crate::network_path;

/// Mean luma (0–255) at or above which a sampled frame counts as a clearly-lit scene, well
/// clear of black/fade frames. Reaching it stops the search early. (C# `litEnough`.)
pub const POSTER_LIT_ENOUGH: f64 = 48.0;

/// Mean luma (0–255) below which even the brightest sampled frame still reads as a black
/// block, so no poster is worth caching — the card keeps its placeholder. (C# `minUsableLuma`.)
pub const POSTER_MIN_USABLE_LUMA: f64 = 22.0;

/// Fractions of the runtime to sample. Films often open dark and only brighten mid-reel, so
/// the plan covers 15%–82% of the duration. (C# `fractions`.)
const SAMPLE_FRACTIONS: [f64; 7] = [0.15, 0.25, 0.38, 0.50, 0.62, 0.75, 0.82];

/// Never sample before this offset: the very first seconds are the most likely to be a black
/// intro/logo. (C# `Math.Max(3, …)`.)
const MIN_SAMPLE_OFFSET: f64 = 3.0;

/// The single offset used when the duration is unknown — one fixed grab. (C# `30`.)
const UNKNOWN_DURATION_OFFSET: f64 = 30.0;

/// What kind of poster a history entry can carry, decided from its path alone. Filesystem
/// existence and private-session suppression are shell concerns layered on top of this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosterSource {
    /// A local video container: sample frames and pick a representative one.
    LocalVideo,
    /// Audio-only: there is no video frame to grab (embedded/sidecar cover art is a separate
    /// concern), so the card keeps an honest non-video fallback.
    Audio,
    /// A URL or network path: not a bounded-sampleable local file, so no frame is generated.
    Remote,
}

/// Classify a history path into the poster policy it qualifies for. Remote (URL/UNC/network)
/// wins first so a network path that happens to end in an audio extension is still treated as
/// remote rather than probed for cover art on the decode thread.
pub fn classify_source(path: &str) -> PosterSource {
    if path.contains("://") || network_path::is_network(path, |_| None) {
        PosterSource::Remote
    } else if media_formats::is_audio(path) {
        PosterSource::Audio
    } else {
        PosterSource::LocalVideo
    }
}

/// The bounded set of timestamps (seconds) to sample for a representative frame. A known,
/// finite, positive duration yields the spread of [`SAMPLE_FRACTIONS`] (each floored to
/// [`MIN_SAMPLE_OFFSET`]); an unknown/degenerate duration yields a single fixed grab so a
/// live source or a record with no duration still gets one attempt.
pub fn poster_sample_offsets(duration: f64) -> Vec<f64> {
    if !(duration.is_finite() && duration > 0.0) {
        return vec![UNKNOWN_DURATION_OFFSET];
    }
    SAMPLE_FRACTIONS
        .iter()
        .map(|fraction| (duration * fraction).max(MIN_SAMPLE_OFFSET))
        .collect()
}

/// The outcome of scoring the sampled frames for one file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PosterVerdict {
    /// Cache the frame at this offset — its luma cleared the usable floor.
    Usable { offset: f64, luma: f64 },
    /// Frames decoded but the brightest was still essentially black: record a durable
    /// "no usable poster" sentinel and keep the placeholder. Do not re-derive it.
    Unusable,
    /// Nothing decoded at all (a transient decode failure): retry on a later pass rather than
    /// giving up, so a momentarily-unreadable file is not marked posterless forever.
    NoFrame,
}

/// Incremental brightest-frame selector. The shell feeds it the mean luma of each decoded
/// candidate; [`Self::is_satisfied`] lets the caller stop early once a clearly-lit frame is in
/// hand, and [`Self::verdict`] renders the final decision.
#[derive(Clone, Copy, Debug)]
pub struct PosterFrameScorer {
    best_offset: f64,
    best_luma: f64,
    decoded_any: bool,
}

impl Default for PosterFrameScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl PosterFrameScorer {
    pub fn new() -> Self {
        Self {
            best_offset: 0.0,
            best_luma: -1.0,
            decoded_any: false,
        }
    }

    /// Record a decoded candidate's offset and mean luma. A non-finite luma is ignored (a
    /// broken/partial decode must never win), but still does not count as "decoded".
    pub fn observe(&mut self, offset: f64, luma: f64) {
        if !luma.is_finite() {
            return;
        }
        self.decoded_any = true;
        if luma > self.best_luma {
            self.best_luma = luma;
            self.best_offset = offset;
        }
    }

    /// True once the brightest candidate so far is a clearly-lit scene, so no further sampling
    /// is worthwhile.
    pub fn is_satisfied(&self) -> bool {
        self.best_luma >= POSTER_LIT_ENOUGH
    }

    /// The final decision after all sampling (or an early stop).
    pub fn verdict(&self) -> PosterVerdict {
        if !self.decoded_any {
            PosterVerdict::NoFrame
        } else if self.best_luma >= POSTER_MIN_USABLE_LUMA {
            PosterVerdict::Usable {
                offset: self.best_offset,
                luma: self.best_luma,
            }
        } else {
            PosterVerdict::Unusable
        }
    }
}

/// A stable per-file cache identity: path plus byte length plus modification time. The same
/// file reuses its poster across sessions; an edited or replaced file (different size or mtime)
/// derives a fresh key so a stale frame is never shown; two different files never collide.
/// Mirrors the Windows `ThumbnailService` file key and the Linux hover/chapter fingerprint, and
/// is the invalidation contract the shelf relies on ("a changed file invalidates its old
/// thumbnail; an unchanged file does not").
pub fn poster_cache_key(path: &str, len: u64, modified_secs: u64, modified_nanos: u32) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    len.hash(&mut hasher);
    modified_secs.hash(&mut hasher);
    modified_nanos.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::assert_close;

    #[test]
    fn classify_separates_local_video_audio_and_remote() {
        assert_eq!(
            classify_source("/media/movie.mkv"),
            PosterSource::LocalVideo
        );
        assert_eq!(classify_source("/media/clip.MP4"), PosterSource::LocalVideo);
        // No/unknown extension on a local path is still treated as a video candidate — the
        // decode either yields a frame or fails transiently, it is never assumed audio.
        assert_eq!(classify_source("/media/rip"), PosterSource::LocalVideo);
        assert_eq!(classify_source("/music/song.flac"), PosterSource::Audio);
        assert_eq!(classify_source("/music/podcast.opus"), PosterSource::Audio);
        assert_eq!(
            classify_source("https://example.com/movie.mkv"),
            PosterSource::Remote
        );
        assert_eq!(
            classify_source("smb://nas/share/film.mkv"),
            PosterSource::Remote
        );
        assert_eq!(
            classify_source(r"\\server\share\film.mkv"),
            PosterSource::Remote
        );
        // A network path is remote even when it ends in an audio extension.
        assert_eq!(
            classify_source("https://example.com/song.flac"),
            PosterSource::Remote
        );
    }

    #[test]
    fn sample_offsets_spread_across_a_known_duration() {
        let offsets = poster_sample_offsets(600.0);
        let expected = [90.0, 150.0, 228.0, 300.0, 372.0, 450.0, 492.0];
        assert_eq!(offsets.len(), expected.len());
        for (actual, expected) in offsets.iter().zip(expected) {
            assert_close(*actual, expected, 1e-6);
        }
    }

    #[test]
    fn sample_offsets_are_floored_away_from_a_black_intro() {
        // A very short clip: 15% of 10s is 1.5s, floored up to the 3s minimum.
        let offsets = poster_sample_offsets(10.0);
        assert_eq!(offsets[0], MIN_SAMPLE_OFFSET);
        assert!(offsets.iter().all(|offset| *offset >= MIN_SAMPLE_OFFSET));
    }

    #[test]
    fn sample_offsets_fall_back_to_a_single_grab_when_duration_is_unknown() {
        for duration in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                poster_sample_offsets(duration),
                vec![UNKNOWN_DURATION_OFFSET]
            );
        }
    }

    #[test]
    fn scorer_reports_no_frame_before_anything_decodes() {
        assert_eq!(PosterFrameScorer::new().verdict(), PosterVerdict::NoFrame);
    }

    #[test]
    fn scorer_keeps_the_brightest_frame_and_reports_it_usable() {
        let mut scorer = PosterFrameScorer::new();
        scorer.observe(90.0, 20.0);
        scorer.observe(300.0, 64.0);
        scorer.observe(492.0, 41.0);
        match scorer.verdict() {
            PosterVerdict::Usable { offset, luma } => {
                assert_close(offset, 300.0, 1e-9);
                assert_close(luma, 64.0, 1e-9);
            }
            other => panic!("expected usable, got {other:?}"),
        }
    }

    #[test]
    fn scorer_stops_early_once_a_lit_frame_is_found() {
        let mut scorer = PosterFrameScorer::new();
        assert!(!scorer.is_satisfied());
        scorer.observe(90.0, 30.0);
        assert!(!scorer.is_satisfied());
        scorer.observe(300.0, POSTER_LIT_ENOUGH);
        assert!(scorer.is_satisfied());
    }

    #[test]
    fn scorer_marks_an_all_black_film_unusable() {
        let mut scorer = PosterFrameScorer::new();
        scorer.observe(90.0, 5.0);
        scorer.observe(300.0, 18.0); // brightest, but still below the usable floor
        assert_eq!(scorer.verdict(), PosterVerdict::Unusable);
    }

    #[test]
    fn scorer_ignores_a_broken_decode_that_would_otherwise_win() {
        let mut scorer = PosterFrameScorer::new();
        scorer.observe(90.0, f64::NAN);
        // The only real decode is below the floor, and the NaN candidate must not have counted
        // as decoded either — but a real decode did happen, so this is Unusable, not NoFrame.
        scorer.observe(300.0, 10.0);
        assert_eq!(scorer.verdict(), PosterVerdict::Unusable);
    }

    #[test]
    fn scorer_reports_no_frame_when_every_candidate_failed_to_decode() {
        let mut scorer = PosterFrameScorer::new();
        scorer.observe(90.0, f64::NAN);
        scorer.observe(300.0, f64::INFINITY);
        assert_eq!(scorer.verdict(), PosterVerdict::NoFrame);
    }

    #[test]
    fn cache_key_is_stable_for_the_same_identity() {
        let a = poster_cache_key("/media/movie.mkv", 1_024, 1_700_000_000, 500);
        let b = poster_cache_key("/media/movie.mkv", 1_024, 1_700_000_000, 500);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn cache_key_changes_when_the_file_changes() {
        let base = poster_cache_key("/media/movie.mkv", 1_024, 1_700_000_000, 0);
        assert_ne!(
            base,
            poster_cache_key("/media/movie.mkv", 2_048, 1_700_000_000, 0)
        );
        assert_ne!(
            base,
            poster_cache_key("/media/movie.mkv", 1_024, 1_700_000_001, 0)
        );
        assert_ne!(
            base,
            poster_cache_key("/media/movie.mkv", 1_024, 1_700_000_000, 1)
        );
        assert_ne!(
            base,
            poster_cache_key("/media/other.mkv", 1_024, 1_700_000_000, 0)
        );
    }
}
