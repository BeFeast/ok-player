#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;

const EXPECTED_PID: &str = "4242";

#[test]
fn retries_when_xdotool_succeeds_with_empty_output() {
    let root = unique_temp_dir("okp-x11-window-empty");
    let fixture = WaitFixture::new(root.path(), "empty-then-ready");
    let output = fixture.run(3);

    assert_successful_selection(&output, "17");
    let diagnostics = fixture.diagnostics();
    assert!(diagnostics.contains("attempt=1 search_status=0"));
    assert!(diagnostics.contains("search_output: <empty>"));
    assert!(diagnostics.contains("selected=17"));
}

#[test]
fn rejects_invalid_xids_before_accepting_a_ready_window() {
    let root = unique_temp_dir("okp-x11-window-invalid");
    let fixture = WaitFixture::new(root.path(), "invalid-then-ready");
    let output = fixture.run(3);

    assert_successful_selection(&output, "17");
    assert!(
        fixture
            .diagnostics()
            .contains("candidate=99 rejected=invalid-xid")
    );
}

#[test]
fn ignores_stale_processes_and_selects_current_windows_deterministically() {
    let root = unique_temp_dir("okp-x11-window-stale");
    let fixture = WaitFixture::new(root.path(), "stale-and-multiple");
    let output = fixture.run(1);

    assert_successful_selection(&output, "31");
    let diagnostics = fixture.diagnostics();
    assert!(diagnostics.contains("candidate=40 rejected=pid expected=4242 actual=9999"));
    assert!(diagnostics.contains("candidate=17 accepted"));
    assert!(diagnostics.contains("candidate=31 accepted"));
    assert!(diagnostics.contains("policy=highest-viewable-xid-for-pid"));
}

#[test]
fn retries_unmapped_windows_until_they_are_viewable() {
    let root = unique_temp_dir("okp-x11-window-unmapped");
    let fixture = WaitFixture::new(root.path(), "unmapped-then-ready");
    let output = fixture.run(3);

    assert_successful_selection(&output, "17");
    let diagnostics = fixture.diagnostics();
    assert!(diagnostics.contains("candidate=17 rejected=map-state state=IsUnMapped"));
    assert!(diagnostics.contains("candidate=17 accepted"));
}

#[test]
fn timeout_preserves_every_readiness_diagnostic() {
    let root = unique_temp_dir("okp-x11-window-timeout");
    let fixture = WaitFixture::new(root.path(), "never-ready");
    let app_log = root.path().join("app.log");
    fs::write(
        &app_log,
        "mpv render context initialized before source load\nLaunch request: 1 item(s)\n",
    )
    .expect("app log fixture should be written");
    let output = fixture.run_with_app_log(2, &app_log);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Timed out waiting for a viewable OK Player window"));
    assert!(stderr.contains("Application log:"));
    assert!(stderr.contains("mpv render context initialized before source load"));
    assert!(stderr.contains("candidate=17 rejected=map-state state=IsUnMapped"));
    let diagnostics = fixture.diagnostics();
    assert_eq!(diagnostics.matches("attempt=").count(), 2);
    assert!(diagnostics.contains("search_output: <empty>"));
    assert!(diagnostics.contains("candidate=99 rejected=invalid-xid"));
    assert!(diagnostics.contains("candidate=40 rejected=pid expected=4242 actual=9999"));
    assert!(diagnostics.contains("candidate=17 rejected=map-state state=IsUnMapped"));
}

fn assert_successful_selection(output: &Output, expected: &str) {
    assert!(
        output.status.success(),
        "window waiter should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
}

struct WaitFixture {
    root: PathBuf,
    fake_bin: PathBuf,
    scenario: &'static str,
    script: PathBuf,
}

impl WaitFixture {
    fn new(root: &Path, scenario: &'static str) -> Self {
        let root = root.to_path_buf();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin should be created");
        write_executable(&fake_bin.join("xdotool"), FAKE_XDOTOOL);
        write_executable(&fake_bin.join("xwininfo"), FAKE_XWININFO);
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("scripts/wait-for-x11-window.sh");
        Self {
            root,
            fake_bin,
            scenario,
            script,
        }
    }

    fn run(&self, attempts: usize) -> Output {
        self.command(attempts)
            .output()
            .expect("window waiter fixture should run")
    }

    fn run_with_app_log(&self, attempts: usize, app_log: &Path) -> Output {
        self.command(attempts)
            .arg(app_log)
            .output()
            .expect("window waiter fixture should run")
    }

    fn command(&self, attempts: usize) -> Command {
        let mut command = Command::new("bash");
        command
            .arg(&self.script)
            .arg(EXPECTED_PID)
            .arg(self.root.join("window.ids"))
            .arg(self.root.join("window.readiness.log"))
            .env("FAKE_SCENARIO", self.scenario)
            .env("FAKE_STATE_DIR", &self.root)
            .env("OKP_X11_WINDOW_WAIT_ATTEMPTS", attempts.to_string())
            .env("OKP_X11_WINDOW_WAIT_INTERVAL", "0")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").expect("PATH should be set")
                ),
            );
        command
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(self.root.join("window.readiness.log"))
            .expect("diagnostics should be preserved")
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
      empty-then-ready)
        if (( count > 1 )); then printf '17\n'; fi
        ;;
      invalid-then-ready)
        if (( count == 1 )); then printf '99\n'; else printf '17\n'; fi
        ;;
      stale-and-multiple)
        printf '40\n17\n31\n'
        ;;
      unmapped-then-ready)
        printf '17\n'
        ;;
      never-ready)
        if (( count == 1 )); then printf '99\n40\n17\n'; fi
        ;;
    esac
    ;;
  getwindowpid)
    case "$1" in
      40) printf '9999\n' ;;
      17|31|99) printf '4242\n' ;;
      *) exit 1 ;;
    esac
    ;;
  *)
    exit 2
    ;;
esac
"#;

const FAKE_XWININFO: &str = r#"#!/usr/bin/env bash
set -euo pipefail

window_id="$2"
if [[ "$window_id" == "99" ]]; then
  echo 'xwininfo: error: No such window' >&2
  exit 1
fi

state='IsViewable'
if [[ "$FAKE_SCENARIO" == 'unmapped-then-ready' && "$(cat "$FAKE_STATE_DIR/search-count")" == '1' ]] ||
  [[ "$FAKE_SCENARIO" == 'never-ready' && "$window_id" == '17' ]]; then
  state='IsUnMapped'
fi

cat <<EOF
xwininfo: Window id: $window_id "OK Player"
  Width: 1120
  Height: 680
  Map State: $state
EOF
"#;
