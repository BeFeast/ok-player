#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use okp_test_fixtures::unique_temp_dir;

#[test]
fn independent_xvfb_processes_are_reaped_before_the_next_session() {
    let root = unique_temp_dir("okp-isolated-xvfb-sequence");
    let fixture = XvfbFixture::new(root.path());

    assert_success(&fixture.run("first"));
    assert_success(&fixture.run("second"));

    let pids = fs::read_to_string(root.path().join("xvfb-pids"))
        .expect("fake Xvfb process IDs should be recorded");
    let pids = pids.lines().collect::<Vec<_>>();
    assert_eq!(pids.len(), 2);
    assert_ne!(pids[0], pids[1], "each invocation needs a fresh process");
    for pid in pids {
        let status = Command::new("kill")
            .args(["-0", pid])
            .stderr(Stdio::null())
            .status()
            .expect("process liveness probe should run");
        assert!(!status.success(), "Xvfb process {pid} was not reaped");
    }

    assert_clean_evidence(&fixture.evidence("first"));
    assert_clean_evidence(&fixture.evidence("second"));
}

#[test]
fn main_window_fit_session_has_one_multiscreen_manager_and_two_supervisors() {
    let script = include_str!("../../../../scripts/smoke-linux-main-window.sh");
    assert_eq!(
        script.matches("xfwm4 --sm-client-disable").count(),
        2,
        "the script needs one Xfwm for idle smoke and one for the fit session"
    );
    assert!(script.contains("wait_for_window_manager \"$PRIMARY_DISPLAY\" primary"));
    assert!(script.contains("wait_for_window_manager \"$SECONDARY_DISPLAY\" secondary"));
    assert!(script.contains("run-linux-isolated-xvfb-session.sh"));
    assert!(script.contains("run-linux-isolated-dbus-session.sh"));
    assert!(
        script
            .contains("__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json")
    );
    assert!(!script.contains("-extension GLX"));
    assert!(script.contains("XDG_CACHE_HOME=\"$OUT_DIR/fit-cache\""));
    assert!(script.contains("XDG_RUNTIME_DIR=\"$OUT_DIR/fit-runtime\""));
    assert!(script.contains("xdg_runtime_mode=%s\\naccessibility_disabled=true"));
    assert!(script.contains("org.a11y.Bus"));
    assert!(script.contains("org.a11y.atspi.Registry"));
    assert!(script.contains("assert_logged_fit_containment"));
    assert!(script.contains("logged_monitor_workarea_containment=pass"));

    let close = script
        .split_once("close_app() {")
        .and_then(|(_, tail)| tail.split_once("\n}\n\nquit_app()"))
        .map(|(body, _)| body)
        .expect("main-window close helper");
    assert!(close.contains("route=ewmh-close-window"));
    assert!(close.contains("\"$X11_CLOSE_REQUEST\" \"$window_id\""));
    assert!(close.contains("result=already-gone"));
    assert!(close.contains("x11_window_state \"$window_id\""));
    assert!(close.contains("unqueryable)"));
    assert!(!close.contains("xdotool key"));
    assert!(!close.contains("xdotool click"));
    assert!(!close.contains("xdotool windowclose"));

    assert!(script.contains("scripts/send-x11-close-request.c"));
    assert!(script.contains("pkg-config --cflags --libs x11"));
    assert!(script.contains("\"$CC_BIN\" -Wall -Wextra -Werror \"$X11_CLOSE_REQUEST_SOURCE\""));

    let close_request = include_str!("../../../../scripts/send-x11-close-request.c");
    assert!(close_request.contains("_NET_CLOSE_WINDOW"));
    assert!(close_request.contains("RootWindowOfScreen(attributes.screen)"));
    assert!(close_request.contains("SubstructureRedirectMask | SubstructureNotifyMask"));
    assert!(close_request.contains("XSendEvent"));
}

#[test]
fn main_window_close_accepts_only_an_already_gone_retry_failure() {
    let script = include_str!("../../../../scripts/smoke-linux-main-window.sh");
    let window_state = script
        .split_once("x11_window_state() {")
        .and_then(|(_, tail)| tail.split_once("\n}\n\nclose_app()"))
        .map(|(body, _)| format!("x11_window_state() {{{body}\n}}"))
        .expect("X11 window-state helper");
    let close = script
        .split_once("close_app() {")
        .and_then(|(_, tail)| tail.split_once("\n}\n\nquit_app()"))
        .map(|(body, _)| format!("close_app() {{{body}\n}}"))
        .expect("main-window close helper");
    let root = unique_temp_dir("okp-close-dispatch-race");
    let probe = format!(
        r#"set -euo pipefail
{window_state}
{close}
OUT_DIR={out}
app_pid=123
X11_CLOSE_REQUEST=send_close
dispatches=0
send_close() {{
  dispatches=$((dispatches + 1))
  (( dispatches == 1 ))
}}
xwininfo() {{
  if [[ "$1" == "-root" ]]; then
    return 0
  fi
  (( dispatches < 2 ))
}}
finish_app_shutdown() {{ printf 'finish=%s\n' "$1"; }}
close_app 4194310
printf 'dispatches=%s\n' "$dispatches"
"#,
        window_state = window_state,
        out = root.path().display()
    );
    let output = Command::new("bash")
        .args(["-c", &probe])
        .output()
        .expect("close race probe should run");
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "finish=last_window_close\ndispatches=2\n"
    );

    let rejection_probe = format!(
        r#"set -euo pipefail
{window_state}
{close}
OUT_DIR={out}
app_pid=123
X11_CLOSE_REQUEST=send_close
send_close() {{ return 1; }}
xwininfo() {{
  [[ "$1" == "-id" ]] || return 2
  return 0
}}
finish_app_shutdown() {{ return 0; }}
if close_app 4194310; then
  exit 1
fi
printf 'live-window-failure=rejected\n'
"#,
        window_state = window_state,
        out = root.path().display()
    );
    let rejection = Command::new("bash")
        .args(["-c", &rejection_probe])
        .output()
        .expect("live-window rejection probe should run");
    assert_success(&rejection);
    assert_eq!(
        String::from_utf8_lossy(&rejection.stdout),
        "live-window-failure=rejected\n"
    );

    let display_failure_probe = format!(
        r#"set -euo pipefail
{window_state}
{close}
OUT_DIR={out}
app_pid=123
X11_CLOSE_REQUEST=send_close
send_close() {{ return 1; }}
xwininfo() {{ return 1; }}
finish_app_shutdown() {{ return 0; }}
if close_app 4194310; then
  exit 1
fi
printf 'display-failure=rejected\n'
"#,
        window_state = window_state,
        out = root.path().display()
    );
    let display_failure = Command::new("bash")
        .args(["-c", &display_failure_probe])
        .output()
        .expect("display failure probe should run");
    assert_success(&display_failure);
    assert_eq!(
        String::from_utf8_lossy(&display_failure.stdout),
        "display-failure=rejected\n"
    );
}

#[test]
fn portability_media_smokes_use_isolated_sessions_and_wait_for_xfwm() {
    for script in [
        include_str!("../../../../scripts/smoke-linux-narrow-width.sh"),
        include_str!("../../../../scripts/smoke-linux-fullscreen-chrome.sh"),
        include_str!("../../../../scripts/smoke-linux-compact-mode.sh"),
    ] {
        assert!(script.contains("run-linux-isolated-dbus-session.sh"));
        assert!(script.contains("run-linux-isolated-xvfb-session.sh"));
        assert!(
            script.find("run-linux-isolated-xvfb-session.sh")
                < script.find("run-linux-isolated-dbus-session.sh"),
            "Xvfb must establish DISPLAY before D-Bus activation inherits the environment"
        );
        assert!(script.contains("_NET_SUPPORTING_WM_CHECK"));
        assert!(script.contains("xfwm4-ready.txt"));
        assert!(script.contains("OKP_SESSION_INFRA_EXIT_CODE:-75"));
        assert!(
            script.contains(
                "__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json"
            )
        );
        assert!(!script.contains("-extension GLX"));
        assert!(!script.contains("xvfb-run "));
        assert!(!script.contains("dbus-run-session -- bash"));
    }
}

#[test]
fn player_window_drag_smoke_covers_survival_cancel_and_recovery() {
    let script = include_str!("../../../../scripts/smoke-linux-window-drag.sh");
    assert!(script.contains("run-linux-isolated-xvfb-session.sh"));
    assert!(script.contains("run-linux-isolated-dbus-session.sh"));
    assert!(
        script
            .contains("__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json")
    );
    assert!(script.contains("_NET_SUPPORTING_WM_CHECK"));
    assert!(script.contains("video-surface-drag"));
    assert!(script.contains("compositor-cancel"));
    assert!(script.contains("post-cancel-drag"));
    assert!(script.contains("idle-canvas-drag"));
    assert!(script.contains("wait_for_new_drag_handoff"));
    assert!(script.contains("drag_handoff_count"));
    assert!(script.contains("video_surface_drag_handoff=observed"));
    assert!(script.contains("compositor_cancel_drag_handoff=observed"));
    assert!(script.contains("post_cancel_drag_handoff=observed"));
    assert!(script.contains("fresh_drag_begin_boundaries=observed"));
    assert!(script.contains("gtk_completion_edge=observed"));
    assert!(script.contains("idle_canvas_drag_handoff=observed"));
    assert!(script.contains("kill -0 \"$app_pid\""));
    assert!(script.contains("expected all three playback-surface move handoffs"));
    assert!(script.contains("panicked at|fatal runtime error|Aborted|core dumped"));
}

#[test]
fn window_regression_runner_dispatches_drag_and_fit_with_bound_evidence() {
    let script = include_str!("../../../../scripts/run-linux-window-regression-smokes.sh");
    assert!(script.contains("smoke-linux-window-drag.sh"));
    assert!(script.contains("run-linux-window-fit-series.sh"));
    assert!(script.contains("non_osc_window_drag"));
    assert!(script.contains("single_monitor_window_fit"));
    assert!(script.contains("window-drag/results.txt"));
    assert!(script.contains("window-fit/series-evidence.txt"));
    assert!(script.contains("compositor_cancel_survival=pass"));
    assert!(script.contains("fatal_diagnostics=absent"));
    assert!(script.contains("completed_consecutive_runs=3"));
    assert!(script.contains("logged_monitor_workarea_containment=pass"));
    assert!(script.contains("window-fit/run-{1,2,3}/fit-xvfb-evidence.txt"));
    assert!(script.contains("OKP_WINDOW_REGRESSION_SOURCE_SHA must be"));
    assert!(script.contains("source_sha=$SOURCE_SHA"));
    assert!(script.contains("Output directory already exists"));
    assert!(script.contains("sha256sum"));
    assert!(script.contains("if (( failed != 0 ))"));
}

#[test]
fn night_gui_runs_headless_window_regressions_before_the_live_seat_gate() {
    let host = include_str!("../../../../scripts/ok-player-night-gui-host.sh");
    let candidate = host
        .find("run_hook A candidate_install")
        .expect("candidate preparation should be present");
    let headless = host
        .find("run_action A headless_window_regressions")
        .expect("headless window regressions should be present");
    let seat = host
        .find("\nrun_seat_check\n")
        .expect("the live graphical seat gate should be present");
    assert!(candidate < headless && headless < seat);
    assert!(host.contains("probe-host"));

    let controller = include_str!("../../../../scripts/ok-player-night-gui-qa.sh");
    assert!(controller.contains("probe-host"));
    assert!(controller.contains("local LC_ALL=C"));
    assert!(controller.contains("${value,,}"));
    assert!(controller.contains("[[ -v OKP_QA_HOSTS ]]"));
    assert!(controller.contains("OKP_QA_HOSTS must contain at least one host alias"));

    let lease = include_str!("../../../../scripts/ok-player-qa-lease.sh");
    assert!(lease.contains("local LC_ALL=C"));
    assert!(lease.contains("${value,,}"));
}

#[test]
fn narrow_width_portability_capture_uses_a_long_lived_dark_fixture() {
    let narrow = include_str!("../../../../scripts/smoke-linux-narrow-width.sh");
    assert!(narrow.contains("FIXTURE=\"${3:-}\""));
    assert!(narrow.contains("color=c=0x101010:s=640x360:r=2:d=30"));
    let delayed_capture = narrow.find("sleep 6").expect("delayed capture wait");
    let screenshot = narrow.find("import -window").expect("window screenshot");
    assert!(delayed_capture < screenshot);

    let portability = include_str!("../../../../scripts/verify-linux-package-portability.sh");
    assert!(portability.contains("\"$scratch/dark.mkv\""));
    assert!(portability.contains("color=c=0x101010:s=640x360:r=2:d=30"));
}

#[test]
fn fullscreen_portability_capture_reads_the_application_toplevel() {
    let fullscreen = include_str!("../../../../scripts/smoke-linux-fullscreen-chrome.sh");
    assert_eq!(
        fullscreen.matches("import -window \"$window_id\"").count(),
        2
    );
    assert!(!fullscreen.contains("import -window root"));
    assert!(fullscreen.contains("FLATPAK_ID=com.befeast.okplayer"));
    assert!(fullscreen.contains("OKP_TEST_DRI_DEVICE_ROOT=\"$OUT_DIR/no-dri-devices\""));
    assert!(fullscreen.contains("mode=software-no-dri"));
    assert!(fullscreen.contains("backend=libmpv-software"));
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "isolated Xvfb session should pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_clean_evidence(evidence: &str) {
    assert!(evidence.contains("xvfb_ready=true"));
    assert!(evidence.contains("command_status=0"));
    assert!(evidence.contains("xvfb_alive_before_teardown=true"));
    assert!(evidence.contains("xvfb_teardown=clean"));
    assert!(evidence.contains("status=pass"));
}

struct XvfbFixture {
    root: PathBuf,
    fake_bin: PathBuf,
    script: PathBuf,
}

impl XvfbFixture {
    fn new(root: &Path) -> Self {
        let root = root.to_path_buf();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin should be created");
        write_executable(&fake_bin.join("Xvfb"), FAKE_XVFB);
        write_executable(&fake_bin.join("xauth"), "#!/usr/bin/env bash\nexit 0\n");
        write_executable(&fake_bin.join("xprop"), "#!/usr/bin/env bash\nexit 0\n");
        write_executable(
            &fake_bin.join("mcookie"),
            "#!/usr/bin/env bash\nprintf '0123456789abcdef0123456789abcdef\\n'\n",
        );
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("scripts/run-linux-isolated-xvfb-session.sh");
        Self {
            root,
            fake_bin,
            script,
        }
    }

    fn run(&self, name: &str) -> Output {
        let server_args = ["-screen", "0", "640x480x24", "-nolisten", "tcp"].join(" ");
        Command::new("bash")
            .arg(&self.script)
            .arg(self.root.join(format!("{name}-evidence.txt")))
            .arg(self.root.join(format!("{name}-xvfb.log")))
            .arg(server_args)
            .arg("bash")
            .arg("-c")
            .arg("exit 0")
            .env("FAKE_XVFB_PIDS", self.root.join("xvfb-pids"))
            .env("OKP_XVFB_FIRST_SERVER_NUM", "700")
            .env("OKP_XVFB_LAST_SERVER_NUM", "700")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").expect("PATH should be set")
                ),
            )
            .output()
            .expect("isolated Xvfb wrapper should run")
    }

    fn evidence(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(format!("{name}-evidence.txt")))
            .expect("Xvfb evidence should be captured")
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

const FAKE_XVFB: &str = r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$BASHPID" >>"$FAKE_XVFB_PIDS"
while :; do
  sleep 1
done
"#;
