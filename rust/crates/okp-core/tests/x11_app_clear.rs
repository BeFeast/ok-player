#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;

#[test]
fn refuses_to_clear_while_the_previous_process_is_alive() {
    let root = unique_temp_dir("okp-x11-app-clear-process");
    let fixture = ClearFixture::new(root.path(), "empty");
    let mut child = Command::new("sleep")
        .arg("2")
        .spawn()
        .expect("short-lived fixture process should start");

    let output = fixture.run(&child.id().to_string(), 2, "0");
    assert!(!output.status.success());
    assert!(fixture.diagnostics().contains("process_alive=true"));

    child.kill().expect("fixture process should stop");
    child.wait().expect("fixture process should be reaped");
    let output = fixture.run(&child.id().to_string(), 1, "0");
    assert_success(&output);
    let diagnostics = fixture.diagnostics();
    assert!(diagnostics.contains("process_alive=false"));
    assert!(diagnostics.contains("clear=true"));
}

#[test]
fn waits_for_every_named_window_to_disappear() {
    let root = unique_temp_dir("okp-x11-app-clear-window");
    let fixture = ClearFixture::new(root.path(), "window-then-empty");
    let output = fixture.run("none", 4, "0");

    assert_success(&output);
    let diagnostics = fixture.diagnostics();
    assert!(diagnostics.contains("candidate=17 pid=4242 state=IsUnMapped width=1 height=1"));
    assert!(diagnostics.contains("clear=true"));
}

#[test]
fn timeout_preserves_pid_xid_map_state_and_geometry() {
    let root = unique_temp_dir("okp-x11-app-clear-timeout");
    let fixture = ClearFixture::new(root.path(), "window-never-clears");
    let output = fixture.run("none", 2, "0");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("previous OK Player process/window lifecycle"));
    assert!(stderr.contains("candidate=17 pid=4242 state=IsUnMapped width=1 height=1"));
    assert_eq!(fixture.diagnostics().matches("attempt=").count(), 2);
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "lifecycle waiter should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct ClearFixture {
    root: PathBuf,
    fake_bin: PathBuf,
    scenario: &'static str,
    script: PathBuf,
}

impl ClearFixture {
    fn new(root: &Path, scenario: &'static str) -> Self {
        let root = root.to_path_buf();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin should be created");
        write_executable(&fake_bin.join("xdotool"), FAKE_XDOTOOL);
        write_executable(&fake_bin.join("xwininfo"), FAKE_XWININFO);
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("scripts/wait-for-x11-app-clear.sh");
        Self {
            root,
            fake_bin,
            scenario,
            script,
        }
    }

    fn run(&self, expected_pid: &str, attempts: usize, interval: &str) -> Output {
        Command::new("bash")
            .arg(&self.script)
            .arg(expected_pid)
            .arg(self.root.join("lifecycle.log"))
            .env("FAKE_SCENARIO", self.scenario)
            .env("FAKE_STATE_DIR", &self.root)
            .env("OKP_X11_APP_CLEAR_ATTEMPTS", attempts.to_string())
            .env("OKP_X11_APP_CLEAR_INTERVAL", interval)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").expect("PATH should be set")
                ),
            )
            .output()
            .expect("lifecycle waiter fixture should run")
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(self.root.join("lifecycle.log"))
            .expect("lifecycle diagnostics should be preserved")
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("fake executable should be written");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable should be executable");
}

const FAKE_XDOTOOL: &str = r#"#!/usr/bin/env bash
set -euo pipefail

command="$1"
shift
case "$command" in
  search)
    count_file="$FAKE_STATE_DIR/search-count"
    count="$(cat "$count_file" 2>/dev/null || echo 0)"
    count=$((count + 1))
    printf '%s\n' "$count" >"$count_file"
    case "$FAKE_SCENARIO" in
      empty) ;;
      window-then-empty) if (( count < 3 )); then printf '17\n'; fi ;;
      window-never-clears) printf '17\n' ;;
    esac
    ;;
  getwindowpid)
    [[ "$1" == "17" ]]
    printf '4242\n'
    ;;
  *) exit 2 ;;
esac
"#;

const FAKE_XWININFO: &str = r#"#!/usr/bin/env bash
set -euo pipefail

cat <<EOF
xwininfo: Window id: $2 "OK Player"
  Width: 1
  Height: 1
  Map State: IsUnMapped
EOF
"#;
