use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use okp_core::project_health::ProjectHealthOutcome;
use okp_test_fixtures::unique_temp_dir;

#[test]
fn failed_fetch_bodies_reach_the_core_evaluator() {
    for failure in ["windows", "candidate", "stable"] {
        let output = run_live(failure);
        assert_ne!(
            output.status.code(),
            Some(2),
            "{failure} fetch must not become a collector startup failure: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let outcome: ProjectHealthOutcome =
            serde_json::from_slice(&output.stdout).expect("collector should emit outcome JSON");
        let (name, expected_blocking, expected_healthy, reason) = match failure {
            "windows" => (
                "windows-static-feed",
                true,
                false,
                "Windows static feed is unreachable",
            ),
            "candidate" => (
                "linux-candidate-delivery",
                true,
                false,
                "Linux candidate feed is unreachable",
            ),
            "stable" => (
                "linux-stable-release-cadence",
                false,
                true,
                "Stable Linux release query is unreachable",
            ),
            _ => unreachable!(),
        };
        let check = outcome
            .checks
            .iter()
            .find(|check| check.name == name)
            .expect("expected health check");
        assert_eq!(check.blocking, expected_blocking, "{failure}");
        assert_eq!(outcome.healthy, expected_healthy, "{failure}");
        assert!(
            check.summary.contains(reason)
                || check.details.iter().any(|detail| detail.contains(reason)),
            "{failure}: {check:?}"
        );
    }
}

#[test]
fn snapshot_mode_uses_a_prebuilt_release_evaluator_without_remote_commands() {
    let root = unique_temp_dir("okp-project-health-offline");
    let scripts = root.path().join("scripts");
    let fake_bin = root.path().join("bin");
    let target = root.path().join("rust/target/release");
    fs::create_dir_all(&scripts).expect("scripts directory should be created");
    fs::create_dir_all(&fake_bin).expect("fake bin should be created");
    fs::create_dir_all(&target).expect("target directory should be created");
    let copied_checker = scripts.join("check-project-outcome.sh");
    fs::copy(checker_path(), &copied_checker).expect("copy checker fixture");
    write_executable(&target.join("okp-candidate"), PREBUILT_EVALUATOR_WRAPPER);
    let marker = root.path().join("unexpected-command");
    for command in ["cargo", "gh", "curl"] {
        write_executable(&fake_bin.join(command), SENTINEL_COMMAND);
    }
    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/project_health/fresh-accepted-snapshot.json");

    let output = Command::new("bash")
        .arg(&copied_checker)
        .arg("--snapshot")
        .arg(&snapshot)
        .env_remove("OKP_PROJECT_HEALTH_BIN")
        .env("OKP_REAL_HEALTH_BIN", evaluator_path())
        .env("OKP_TEST_SENTINEL", &marker)
        .env("PATH", path_with(&fake_bin))
        .output()
        .expect("snapshot evaluation should run");
    assert_ne!(
        output.status.code(),
        Some(2),
        "snapshot evaluation should reach the evaluator: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = serde_json::from_slice::<ProjectHealthOutcome>(&output.stdout)
        .expect("snapshot evaluation should emit outcome JSON");
    assert!(outcome.healthy, "fresh accepted snapshot should be healthy");
    assert!(
        !marker.exists(),
        "snapshot mode invoked a remote/bootstrap command"
    );
}

#[test]
fn snapshot_mode_reports_a_precise_local_evaluator_prerequisite() {
    let root = unique_temp_dir("okp-project-health-no-evaluator");
    let scripts = root.path().join("scripts");
    let fake_bin = root.path().join("bin");
    fs::create_dir_all(&scripts).expect("scripts directory should be created");
    fs::create_dir_all(&fake_bin).expect("fake bin should be created");
    let copied_checker = scripts.join("check-project-outcome.sh");
    fs::copy(checker_path(), &copied_checker).expect("copy checker fixture");
    let marker = root.path().join("unexpected-command");
    write_executable(&fake_bin.join("cargo"), SENTINEL_COMMAND);
    let snapshot = root.path().join("snapshot.json");
    fs::write(&snapshot, b"{}\n").expect("write snapshot fixture");

    let output = Command::new("bash")
        .arg(&copied_checker)
        .arg("--snapshot")
        .arg(&snapshot)
        .env_remove("OKP_PROJECT_HEALTH_BIN")
        .env("OKP_TEST_SENTINEL", &marker)
        .env("PATH", path_with(&fake_bin))
        .output()
        .expect("missing evaluator fixture should run");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("offline snapshot evaluation requires executable"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "missing evaluator path invoked Cargo");
}

#[test]
fn live_mode_uses_a_prebuilt_repo_evaluator_without_cargo_and_stays_bounded() {
    let root = unique_temp_dir("okp-project-health-bounded-live");
    let scripts = root.path().join("scripts");
    let fake_bin = root.path().join("bin");
    let target = root.path().join("rust/target/debug");
    fs::create_dir_all(&scripts).expect("scripts directory should be created");
    fs::create_dir_all(&fake_bin).expect("fake bin should be created");
    fs::create_dir_all(&target).expect("target directory should be created");
    let copied_checker = scripts.join("check-project-outcome.sh");
    fs::copy(checker_path(), &copied_checker).expect("copy checker fixture");
    write_executable(&target.join("okp-candidate"), PREBUILT_EVALUATOR_WRAPPER);
    write_executable(&fake_bin.join("gh"), FAKE_GH);
    write_executable(&fake_bin.join("curl"), FAKE_CURL);
    write_executable(&fake_bin.join("date"), FAKE_DATE);
    let marker = root.path().join("unexpected-cargo");
    write_executable(&fake_bin.join("cargo"), SENTINEL_COMMAND);

    let started = Instant::now();
    let output = Command::new("bash")
        .arg(&copied_checker)
        .env_remove("OKP_PROJECT_HEALTH_BIN")
        .env("OKP_REAL_HEALTH_BIN", evaluator_path())
        .env("OKP_TEST_SENTINEL", &marker)
        .env("OKP_STUB_FAIL", "none")
        .env(
            "OKP_STUB_CANDIDATE_FEED",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/project_health/fresh-accepted.json"),
        )
        .env("PATH", path_with(&fake_bin))
        .output()
        .expect("bounded live collector should run");
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "bounded live collector failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<ProjectHealthOutcome>(&output.stdout)
        .expect("bounded live collector should emit outcome JSON");
    assert!(!marker.exists(), "bounded live path invoked Cargo");
    assert!(
        elapsed < Duration::from_secs(15),
        "bounded live collector took {elapsed:?}"
    );
}

fn run_live(failure: &str) -> Output {
    let root = unique_temp_dir(&format!("okp-project-health-{failure}"));
    let fake_bin = root.path().join("bin");
    fs::create_dir_all(&fake_bin).expect("fake bin should be created");
    write_executable(&fake_bin.join("gh"), FAKE_GH);
    write_executable(&fake_bin.join("curl"), FAKE_CURL);
    write_executable(&fake_bin.join("date"), FAKE_DATE);
    Command::new("bash")
        .arg(checker_path())
        .env("OKP_PROJECT_HEALTH_BIN", evaluator_path())
        .env("OKP_STUB_FAIL", failure)
        .env(
            "OKP_STUB_CANDIDATE_FEED",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/project_health/fresh-accepted.json"),
        )
        .env("PATH", path_with(&fake_bin))
        .output()
        .expect("collector fixture should run")
}

fn checker_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("scripts/check-project-outcome.sh")
}

fn evaluator_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_okp-candidate") {
        return path.into();
    }

    let test_executable = std::env::current_exe().expect("current test executable path");
    let profile_directory = test_executable
        .parent()
        .and_then(Path::parent)
        .expect("integration test executable should live under target profile/deps");
    let evaluator =
        profile_directory.join(format!("okp-candidate{}", std::env::consts::EXE_SUFFIX));
    assert!(
        evaluator.is_file(),
        "Cargo did not expose or build the okp-candidate test binary at {}",
        evaluator.display()
    );
    evaluator
}

fn path_with(directory: &Path) -> String {
    format!(
        "{}:{}",
        directory.display(),
        std::env::var("PATH").expect("PATH should be set")
    )
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("fake executable should be written");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable should be executable");
}

const SENTINEL_COMMAND: &str = r#"#!/usr/bin/env bash
printf '%s\n' "$0" >"$OKP_TEST_SENTINEL"
exit 99
"#;

const PREBUILT_EVALUATOR_WRAPPER: &str = r#"#!/usr/bin/env bash
exec "$OKP_REAL_HEALTH_BIN" "$@"
"#;

const FAKE_DATE: &str = r#"#!/usr/bin/env bash
printf '1784340047\n'
"#;

const FAKE_CURL: &str = r#"#!/usr/bin/env bash
set -euo pipefail
url="${!#}"
if [[ "$url" == *releases.win.json ]]; then
  [[ "$OKP_STUB_FAIL" != windows ]] || exit 22
  printf '%s\n' '{"Assets":[{"PackageId":"OkPlayer","Version":"0.10.14","Type":"Full","FileName":"https://example.invalid/OkPlayer-full.nupkg","SHA256":"B6C45F3FDAD98FF02958A77C30DE0EFE2260AF518C392A01699F1397E9C70E80","Size":200597245}]}'
elif [[ "$url" == *candidate.linux.json ]]; then
  [[ "$OKP_STUB_FAIL" != candidate ]] || exit 22
  cat "$OKP_STUB_CANDIDATE_FEED"
else
  exit 64
fi
"#;

const FAKE_GH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == run && "${2:-}" == list ]]; then
  workflow=""
  while (( $# > 0 )); do
    if [[ "$1" == --workflow ]]; then workflow="$2"; break; fi
    shift
  done
  printf '[{"workflowName":"%s","headSha":"d5d531a58c830a01a7e25615e850593e9ff4493f","event":"push","status":"completed","conclusion":"success","url":"https://example.invalid/run"}]\n' "$workflow"
  exit 0
fi
[[ "${1:-}" == api ]] || exit 64
case "${2:-}" in
  repos/*/commits/main)
    printf '%s\n' '{"sha":"d5d531a58c830a01a7e25615e850593e9ff4493f"}'
    ;;
  repos/*/commits/d5d531a58c830a01a7e25615e850593e9ff4493f)
    printf '%s\n' '{"commit":{"committer":{"date":"2026-07-18T00:30:00Z"}}}'
    ;;
  repos/*/releases*)
    [[ "$OKP_STUB_FAIL" != stable ]] || exit 1
    printf '%s\n' '[{"tag_name":"linux-v0.1.0-linux-alpha.112","draft":false,"published_at":"2026-07-17T00:00:00Z"}]'
    ;;
  *)
    exit 64
    ;;
esac
"#;
