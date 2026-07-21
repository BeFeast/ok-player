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
