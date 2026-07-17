//! Shared test fixtures and golden-test helpers for the OK Player Rust workspace.
//!
//! This is a `dev-dependency`-only crate (EPIC #134, item D13). It holds the small assertion
//! and fixture helpers that every crate's `#[cfg(test)]` module would otherwise re-implement,
//! so the ported golden tests compare floats and lay out temp files the same way everywhere.
//! Add media-specific helpers to a future `okp-media` crate only once the first one lands —
//! this crate stays engine- and media-free.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{Builder, TempDir};

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

/// An owned directory under the system temp directory for a filesystem fixture.
///
/// The directory is created immediately and removed when the returned guard is dropped, including
/// during panic unwind. The generated name begins with `{prefix}-`; give each test a distinct
/// prefix so a live fixture remains attributable while the test is running.
///
/// Destructors do not run after abort, `process::exit`, `SIGKILL`, or OOM termination. External
/// worker lease/runtime cleanup remains responsible for those cases.
pub fn unique_temp_dir(prefix: &str) -> TempDir {
    Builder::new()
        .prefix(&format!("{prefix}-"))
        .tempdir_in(std::env::temp_dir())
        .expect("temporary fixture directory should be created")
}

/// Output from a real Velopack pack invocation used by packaging contract
/// tests. Dropping the fixture removes its temporary package tree.
#[derive(Debug)]
pub struct VelopackPackFixture {
    _root: TempDir,
    pub output_dir: PathBuf,
}

/// Run the installed Velopack CLI against a minimal real Linux executable.
/// Callers opt in with `OKP_RUN_VELOPACK_PACK_TEST=1` so ordinary unit-test
/// loops do not require the external release CLI.
pub fn run_velopack_pack(
    package_id: &str,
    version: &str,
    channel: &str,
    icon: &Path,
) -> Result<VelopackPackFixture, String> {
    let root = unique_temp_dir("okp-real-velopack-pack");
    let pack_dir = root.path().join("pack");
    let output_dir = root.path().join("output");
    let fixture = VelopackPackFixture {
        _root: root,
        output_dir,
    };
    fs::create_dir_all(&pack_dir).map_err(|error| format!("{}: {error}", pack_dir.display()))?;
    fs::create_dir_all(&fixture.output_dir)
        .map_err(|error| format!("{}: {error}", fixture.output_dir.display()))?;
    let executable = pack_dir.join("ok-player");
    fs::copy("/bin/true", &executable)
        .map_err(|error| format!("{}: {error}", executable.display()))?;

    let vpk = velopack_cli().ok_or_else(|| {
        "vpk is required when OKP_RUN_VELOPACK_PACK_TEST=1; install Velopack CLI 1.2.0".to_owned()
    })?;
    let mut command = Command::new(&vpk);
    command.args([
        "pack",
        "--packId",
        package_id,
        "--packVersion",
        version,
        "--packDir",
    ]);
    command.arg(&pack_dir);
    command.args(["--mainExe", "ok-player", "--outputDir"]);
    command.arg(&fixture.output_dir);
    command.args([
        "--channel",
        channel,
        "--packTitle",
        "OK Player packaging contract",
        "--packAuthors",
        "BeFeast",
        "--icon",
    ]);
    command.arg(icon);
    command.args([
        "--categories",
        "AudioVideo;Player",
        "--skip-updates",
        "true",
    ]);
    if std::env::var_os("DOTNET_ROOT").is_none()
        && let Some(home) = std::env::var_os("HOME")
    {
        let dotnet_root = PathBuf::from(home).join(".dotnet");
        if dotnet_root.is_dir() {
            command.env("DOTNET_ROOT", dotnet_root);
        }
    }
    let output = command
        .output()
        .map_err(|error| format!("{}: {error}", vpk.display()))?;
    if !output.status.success() {
        return Err(format!(
            "vpk pack failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(fixture)
}

fn velopack_cli() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("VPK") {
        return Some(PathBuf::from(path));
    }
    if Command::new("vpk").arg("--help").output().is_ok() {
        return Some(PathBuf::from("vpk"));
    }
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".dotnet/tools/vpk");
    path.is_file().then_some(path)
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

        assert!(dir.path().starts_with(std::env::temp_dir()));
        let name = dir
            .path()
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
    fn unique_temp_dir_cleans_up_after_normal_return() {
        let path = {
            let dir = unique_temp_dir("okp-fixtures-normal-cleanup");
            let path = dir.path().to_owned();
            assert!(path.is_dir());
            path
        };

        assert!(!path.exists(), "fixture should be removed after return");
    }

    #[test]
    fn unique_temp_dir_cleans_up_during_panic_without_masking_the_panic() {
        let mut path = None;
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let dir = unique_temp_dir("okp-fixtures-panic-cleanup");
            path = Some(dir.path().to_owned());
            panic!("fixture panic sentinel");
        }))
        .expect_err("fixture scope should panic");

        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"fixture panic sentinel"),
            "cleanup must not replace the original panic"
        );
        assert!(
            !path.expect("fixture path should be recorded").exists(),
            "fixture should be removed during panic unwind"
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
