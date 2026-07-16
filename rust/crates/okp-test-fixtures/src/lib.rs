//! Shared test fixtures and golden-test helpers for the OK Player Rust workspace.
//!
//! This is a `dev-dependency`-only crate (EPIC #134, item D13). It holds the small assertion
//! and fixture helpers that every crate's `#[cfg(test)]` module would otherwise re-implement,
//! so the ported golden tests compare floats and lay out temp files the same way everywhere.
//! Add media-specific helpers to a future `okp-media` crate only once the first one lands —
//! this crate stays engine- and media-free.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactVideoFixture {
    pub codec: &'static str,
    pub profile: &'static str,
    pub pixel_format: &'static str,
    pub width: u32,
    pub height: u32,
    pub frame_rate: &'static str,
}

pub const HEVC_MAIN10_4K60: ExactVideoFixture = ExactVideoFixture {
    codec: "hevc",
    profile: "Main 10",
    pixel_format: "yuv420p10le",
    width: 3840,
    height: 2160,
    frame_rate: "60/1",
};

/// Assert that `actual` is within `tolerance` of `expected`, panicking with a message that
/// shows all three values.
///
/// The comparison is a strict `(actual - expected).abs() < tolerance`, so a difference exactly
/// equal to `tolerance` fails — this matches the hand-written checks it replaces across the
/// core's parser/geometry golden tests. `#[track_caller]` makes a failure point at the test
/// that called it, not at this helper.
#[track_caller]
pub fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "expected {expected} ± {tolerance}, got {actual}"
    );
}

/// A time-stamped path under the system temp directory for a filesystem fixture.
///
/// The directory is NOT created — callers build exactly the layout their test needs. The
/// nanosecond suffix keeps runs that share a `prefix` from colliding on disk in practice; give
/// each test a distinct `prefix` so a failure names the test that left the directory behind.
pub fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_close_accepts_a_difference_inside_the_tolerance() {
        assert_close(1.0, 1.0 + 1e-10, 1e-9);
        assert_close(-3.5, -3.5, f64::EPSILON);
    }

    #[test]
    #[should_panic(expected = "expected 1.1 ± 0.000000001, got 1")]
    fn assert_close_rejects_a_difference_outside_the_tolerance() {
        assert_close(1.0, 1.1, 1e-9);
    }

    #[test]
    #[should_panic(expected = "got 1")]
    fn assert_close_rejects_a_difference_exactly_equal_to_the_tolerance() {
        // Strict `<`: a gap of exactly `tolerance` must fail.
        assert_close(1.0, 2.0, 1.0);
    }

    #[test]
    fn unique_temp_dir_sits_under_temp_with_the_prefix() {
        let dir = unique_temp_dir("okp-fixtures-selftest");

        assert!(dir.starts_with(std::env::temp_dir()));
        let name = dir
            .file_name()
            .expect("has a file name")
            .to_string_lossy()
            .into_owned();
        assert!(
            name.starts_with("okp-fixtures-selftest-"),
            "unexpected name: {name}"
        );
    }

    #[test]
    fn live_wayland_fixture_contract_is_exact_4k_hevc_main10_60() {
        assert_eq!(HEVC_MAIN10_4K60.width, 3840);
        assert_eq!(HEVC_MAIN10_4K60.height, 2160);
        assert_eq!(HEVC_MAIN10_4K60.codec, "hevc");
        assert_eq!(HEVC_MAIN10_4K60.profile, "Main 10");
        assert_eq!(HEVC_MAIN10_4K60.pixel_format, "yuv420p10le");
        assert_eq!(HEVC_MAIN10_4K60.frame_rate, "60/1");
    }
}
