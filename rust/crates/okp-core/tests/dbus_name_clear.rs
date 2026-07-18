#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;

#[test]
fn waits_for_application_and_mpris_names_to_disappear() {
    let root = unique_temp_dir("okp-dbus-name-clear");
    let fixture = NameFixture::new(root.path(), "present-then-clear");
    let output = fixture.run(3);

    assert_success(&output);
    let diagnostics = fixture.diagnostics();
    assert_eq!(diagnostics.matches("attempt=").count(), 2);
    assert!(diagnostics.contains("name=com.befeast.okplayer present=true"));
    assert!(diagnostics.contains("name=org.mpris.MediaPlayer2.okplayer present=true"));
    assert!(diagnostics.contains("clear=true"));
}

#[test]
fn list_failure_is_not_mistaken_for_clean_registration_state() {
    let root = unique_temp_dir("okp-dbus-name-error");
    let fixture = NameFixture::new(root.path(), "list-error");
    let output = fixture.run(2);

    assert!(!output.status.success());
    let diagnostics = fixture.diagnostics();
    assert_eq!(diagnostics.matches("attempt=").count(), 2);
    assert!(diagnostics.contains("list_status=1"));
    assert!(!diagnostics.contains("clear=true"));
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "D-Bus name waiter should pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct NameFixture {
    root: PathBuf,
    fake_bin: PathBuf,
    scenario: &'static str,
    script: PathBuf,
}

impl NameFixture {
    fn new(root: &Path, scenario: &'static str) -> Self {
        let root = root.to_path_buf();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin should be created");
        write_executable(&fake_bin.join("gdbus"), FAKE_GDBUS);
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("scripts/wait-for-dbus-names-clear.sh");
        Self {
            root,
            fake_bin,
            scenario,
            script,
        }
    }

    fn run(&self, attempts: usize) -> Output {
        Command::new("bash")
            .arg(&self.script)
            .arg(self.root.join("dbus-lifecycle.log"))
            .arg("com.befeast.okplayer")
            .arg("org.mpris.MediaPlayer2.okplayer")
            .env("FAKE_SCENARIO", self.scenario)
            .env("OKP_DBUS_NAME_CLEAR_ATTEMPTS", attempts.to_string())
            .env("OKP_DBUS_NAME_CLEAR_INTERVAL", "0")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").expect("PATH should be set")
                ),
            )
            .output()
            .expect("D-Bus name waiter fixture should run")
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(self.root.join("dbus-lifecycle.log"))
            .expect("D-Bus diagnostics should be preserved")
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

const FAKE_GDBUS: &str = r#"#!/usr/bin/env bash
set -euo pipefail

case "$FAKE_SCENARIO" in
  present-then-clear)
    if (( OKP_DBUS_NAME_CLEAR_ATTEMPT == 1 )); then
      printf "(['org.freedesktop.DBus', 'com.befeast.okplayer', 'org.mpris.MediaPlayer2.okplayer'],)\n"
    else
      printf "(['org.freedesktop.DBus'],)\n"
    fi
    ;;
  list-error)
    echo 'session bus unavailable' >&2
    exit 1
    ;;
esac
"#;
