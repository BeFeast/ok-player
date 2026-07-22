#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;
use tempfile::TempDir;

#[test]
fn session_infrastructure_failure_retries_once_and_preserves_first_attempt() {
    let fixture = PolicyFixture::new("okp-portability-infra-retry", INFRA_THEN_PASS);

    let output = fixture.run();

    assert_success(&output);
    assert_eq!(fs::read_to_string(&fixture.counter).unwrap(), "2\n");
    assert!(fixture.output.join("success.txt").is_file());
    let evidence = fs::read_to_string(
        fixture
            .evidence
            .join("narrow-width/attempt-1/retry-evidence.txt"),
    )
    .expect("first attempt evidence should be persisted");
    assert!(evidence.contains("exit_status=75"));
    assert!(evidence.contains("failure_kind=session-infra"));
    assert!(evidence.contains("retried=true"));
    assert!(
        fixture
            .evidence
            .join("narrow-width/attempt-1/session.log")
            .is_file()
    );
}

#[test]
fn product_assertion_failure_is_not_retried() {
    let fixture = PolicyFixture::new("okp-portability-assertion", ASSERTION_FAILURE);

    let output = fixture.run();

    assert_eq!(output.status.code(), Some(19));
    assert_eq!(fs::read_to_string(&fixture.counter).unwrap(), "1\n");
    let evidence = fs::read_to_string(
        fixture
            .evidence
            .join("narrow-width/attempt-1/retry-evidence.txt"),
    )
    .expect("assertion evidence should be persisted");
    assert!(evidence.contains("exit_status=19"));
    assert!(evidence.contains("failure_kind=command"));
    assert!(evidence.contains("retried=false"));
    assert!(!fixture.evidence.join("narrow-width/attempt-2").exists());
}

#[test]
fn consecutive_smokes_do_not_share_xdg_persistence() {
    let root = unique_temp_dir("okp-portability-xdg-isolation");
    let runner = root.path().join("runner.sh");
    let shared_xdg = root.path().join("shared-xdg");
    write_executable(&runner, PERSISTENCE_PROBE);

    let output = Command::new("bash")
        .arg("-c")
        .arg(
            "set -euo pipefail; source \"$1\"; shift; \
             okp_run_linux_smoke_with_infra_retry first \"$1/first\" \"$1/evidence\" \"$2\"; \
             okp_run_linux_smoke_with_infra_retry second \"$1/second\" \"$1/evidence\" \"$2\"",
        )
        .arg("bash")
        .arg(policy_script())
        .arg(root.path())
        .arg(&runner)
        .env("XDG_CONFIG_HOME", shared_xdg.join("config"))
        .env("XDG_STATE_HOME", shared_xdg.join("state"))
        .env("XDG_CACHE_HOME", shared_xdg.join("cache"))
        .env("XDG_DATA_HOME", shared_xdg.join("data"))
        .output()
        .expect("consecutive policy helpers should run");

    assert_success(&output);
    let first = fs::read_to_string(root.path().join("first/session.log")).unwrap();
    let second = fs::read_to_string(root.path().join("second/session.log")).unwrap();
    assert!(first.contains("first-attempt-1/.xdg/state"), "{first}");
    assert!(second.contains("second-attempt-1/.xdg/state"), "{second}");
    assert_ne!(first, second);
    assert!(!shared_xdg.join("state/ok-player/history.json").exists());
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "policy helper should pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct PolicyFixture {
    root: TempDir,
    runner: PathBuf,
    counter: PathBuf,
    output: PathBuf,
    evidence: PathBuf,
}

impl PolicyFixture {
    fn new(name: &str, runner: &str) -> Self {
        let root = unique_temp_dir(name);
        let runner_path = root.path().join("runner.sh");
        write_executable(&runner_path, runner);
        Self {
            counter: root.path().join("counter.txt"),
            output: root.path().join("smoke-output"),
            evidence: root.path().join("evidence"),
            root,
            runner: runner_path,
        }
    }

    fn run(&self) -> Output {
        Command::new("bash")
            .arg("-c")
            .arg(
                "set -euo pipefail; source \"$1\"; shift; \
                 okp_run_linux_smoke_with_infra_retry narrow-width \"$1\" \"$2\" \"$3\"",
            )
            .arg("bash")
            .arg(policy_script())
            .arg(&self.output)
            .arg(&self.evidence)
            .arg(&self.runner)
            .env("OKP_TEST_COUNTER", &self.counter)
            .current_dir(self.root.path())
            .output()
            .expect("policy helper should run")
    }
}

fn policy_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("scripts/linux-bundled-mpv-runtime-policy.sh")
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("runner should be written");
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

const INFRA_THEN_PASS: &str = r#"#!/usr/bin/env bash
set -euo pipefail
count="$(cat "$OKP_TEST_COUNTER" 2>/dev/null || printf '0')"
count=$((count + 1))
printf '%s\n' "$count" >"$OKP_TEST_COUNTER"
printf 'attempt=%s\n' "$count" >"$OKP_SMOKE_OUTPUT_DIR/session.log"
if (( count == 1 )); then
  exit 75
fi
printf 'pass\n' >"$OKP_SMOKE_OUTPUT_DIR/success.txt"
"#;

const ASSERTION_FAILURE: &str = r#"#!/usr/bin/env bash
set -euo pipefail
count="$(cat "$OKP_TEST_COUNTER" 2>/dev/null || printf '0')"
count=$((count + 1))
printf '%s\n' "$count" >"$OKP_TEST_COUNTER"
printf 'assertion failed\n' >"$OKP_SMOKE_OUTPUT_DIR/session.log"
exit 19
"#;

const PERSISTENCE_PROBE: &str = r#"#!/usr/bin/env bash
set -euo pipefail
history="$XDG_STATE_HOME/ok-player/history.json"
if [[ -e "$history" ]]; then
  printf 'shared history leaked into smoke: %s\n' "$history" >&2
  exit 31
fi
mkdir -p "$(dirname "$history")"
printf '{"position": 24}\n' >"$history"
printf 'config=%s\nstate=%s\ncache=%s\ndata=%s\n' \
  "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" \
  >"$OKP_SMOKE_OUTPUT_DIR/session.log"
"#;
