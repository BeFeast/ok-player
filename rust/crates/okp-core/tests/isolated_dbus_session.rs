#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use okp_test_fixtures::unique_temp_dir;

#[test]
fn independent_sessions_teardown_before_the_next_session_starts() {
    let root = unique_temp_dir("okp-isolated-dbus-sequence");
    let fixture = SessionFixture::new(root.path());

    let first = fixture.run("first");
    assert_success(&first);
    let second = fixture.run("second");
    assert_success(&second);

    let first_id = fixture.bus_id("first");
    let second_id = fixture.bus_id("second");
    assert_ne!(first_id, second_id, "each invocation needs a fresh bus");
    assert_clean_evidence(&fixture.evidence("first"));
    assert_clean_evidence(&fixture.evidence("second"));
}

#[test]
fn command_failure_still_waits_for_session_bus_teardown() {
    let root = unique_temp_dir("okp-isolated-dbus-failure");
    let script = session_script();
    let evidence = root.path().join("failure-evidence.txt");
    let output = Command::new("bash")
        .arg(script)
        .arg(&evidence)
        .arg("bash")
        .arg("-c")
        .arg("exit 23")
        .output()
        .expect("isolated session wrapper should run");

    assert_eq!(output.status.code(), Some(23));
    let evidence = fs::read_to_string(evidence).expect("failure evidence should exist");
    assert!(evidence.contains("command_status=23"));
    assert!(evidence.contains("session_bus_teardown=clean"));
    assert!(evidence.contains("session_process_teardown=clean"));
    assert!(evidence.contains("status=fail"));
}

#[test]
fn orphaned_session_child_is_reaped_before_the_next_session() {
    let root = unique_temp_dir("okp-isolated-dbus-orphan");
    let script = session_script();
    let evidence = root.path().join("orphan-evidence.txt");
    let pid_file = root.path().join("orphan-pid.txt");
    let output = Command::new("bash")
        .arg(script)
        .arg(&evidence)
        .arg("bash")
        .arg("-c")
        .arg("sleep 60 </dev/null >/dev/null 2>&1 & printf '%s\\n' \"$!\" >\"$1\"")
        .arg("bash")
        .arg(&pid_file)
        .output()
        .expect("isolated session wrapper should run");

    assert_success(&output);
    let pid = fs::read_to_string(pid_file).expect("orphan PID should be recorded");
    let status = Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .expect("process liveness probe should run");
    assert!(!status.success(), "isolated session child was not reaped");
    assert_clean_evidence(
        &fs::read_to_string(evidence).expect("orphan evidence should be captured"),
    );
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "isolated session should pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_clean_evidence(evidence: &str) {
    assert!(evidence.contains("session_bus_ready=true"));
    assert!(evidence.contains("command_status=0"));
    assert!(evidence.contains("session_bus_teardown=clean"));
    assert!(evidence.contains("session_process_teardown=clean"));
    assert!(evidence.contains("status=pass"));
}

fn session_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("scripts/run-linux-isolated-dbus-session.sh")
}

struct SessionFixture {
    root: PathBuf,
    script: PathBuf,
}

impl SessionFixture {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            script: session_script(),
        }
    }

    fn run(&self, name: &str) -> Output {
        Command::new("bash")
            .arg(&self.script)
            .arg(self.root.join(format!("{name}-evidence.txt")))
            .arg("bash")
            .arg("-c")
            .arg(
                "gdbus call --session --dest org.freedesktop.DBus \
                 --object-path /org/freedesktop/DBus \
                 --method org.freedesktop.DBus.GetId >\"$1\"",
            )
            .arg("bash")
            .arg(self.root.join(format!("{name}-bus-id.txt")))
            .output()
            .expect("isolated session wrapper should run")
    }

    fn bus_id(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(format!("{name}-bus-id.txt")))
            .expect("session ID should be captured")
    }

    fn evidence(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(format!("{name}-evidence.txt")))
            .expect("session evidence should be captured")
    }
}
