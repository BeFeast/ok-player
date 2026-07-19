#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use okp_test_fixtures::unique_temp_dir;

#[test]
fn prune_out_keeps_three_complete_generations_plus_the_pinned_bundle() {
    let root = unique_temp_dir("okp-candidate-out-retention");
    let out = root.path().join("out");
    fs::create_dir_all(&out).expect("out root");
    for build in 1..=5 {
        let bundle = out.join(build.to_string());
        fs::create_dir_all(bundle.join("artifacts")).expect("bundle artifacts");
        fs::write(bundle.join("candidate-build.json"), b"{}\n").expect("bundle marker");
    }
    fs::create_dir_all(out.join("6/artifacts")).expect("incomplete generation");
    fs::create_dir_all(out.join("operator-notes")).expect("unknown entry");
    let pointer = root.path().join("last-bundle.path");
    fs::write(&pointer, format!("{}\n", out.join("1").display())).expect("bundle pointer");

    let output = Command::new(env!("CARGO_BIN_EXE_okp-candidate"))
        .args(["prune-out", "--out-root"])
        .arg(&out)
        .arg("--last-bundle-path")
        .arg(&pointer)
        .args(["--keep", "3"])
        .output()
        .expect("retention command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(numeric_generations(&out), vec![1, 3, 4, 5]);
    assert!(out.join("operator-notes").is_dir());
}

#[test]
fn failed_install_smoke_reclaims_its_implicit_scratch_root() {
    let root = unique_temp_dir("okp-install-smoke-cleanup");
    let tmp = root.path().join("tmp");
    let bin = root.path().join("bin");
    fs::create_dir_all(&tmp).expect("temp root");
    fs::create_dir_all(&bin).expect("fake bin");
    write_executable(&bin.join("dpkg-deb"), "#!/bin/sh\nexit 23\n");
    let deb = root.path().join("candidate.deb");
    fs::write(&deb, b"fixture").expect("fake package");

    let output = Command::new("bash")
        .arg(repository_root().join("scripts/smoke-linux-install-upgrade.sh"))
        .arg(&deb)
        .env("TMPDIR", &tmp)
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")),
        )
        .output()
        .expect("failing smoke");
    assert!(!output.status.success());
    assert_eq!(fs::read_dir(&tmp).expect("temp listing").count(), 0);
}

#[test]
fn session_reclaimer_removes_only_the_named_sessions_scratch() {
    let root = unique_temp_dir("okp-session-reclaimer");
    let scratch = root.path().join("tmp");
    fs::create_dir_all(scratch.join("ok-player-worker-42-smoke.aaaaaa")).expect("owned scratch");
    fs::create_dir_all(scratch.join("okp-worker-42-package.bbbbbb")).expect("legacy scratch");
    fs::create_dir_all(scratch.join("ok-player-worker-43-smoke.cccccc")).expect("other scratch");

    let output = Command::new("bash")
        .arg(repository_root().join("scripts/reclaim-ok-player-scratch.sh"))
        .arg("worker-42")
        .env("OKP_SCRATCH_ROOT", &scratch)
        .output()
        .expect("session reclaimer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!scratch.join("ok-player-worker-42-smoke.aaaaaa").exists());
    assert!(!scratch.join("okp-worker-42-package.bbbbbb").exists());
    assert!(scratch.join("ok-player-worker-43-smoke.cccccc").exists());
}

#[test]
fn candidate_workflow_wires_exit_retention_and_session_reclaim() {
    let root = repository_root();
    let builder = fs::read_to_string(root.join("scripts/build-linux-candidate.sh"))
        .expect("candidate builder");
    assert!(builder.contains("trap prune_candidate_out EXIT"));
    assert!(builder.contains("prune-out"));
    assert!(builder.contains("--last-bundle-path \"$LAST_BUNDLE\""));

    let workflow = fs::read_to_string(root.join(".github/workflows/release-linux-candidate.yml"))
        .expect("candidate workflow");
    assert!(
        workflow.contains(
            "OKP_SCRATCH_SESSION: candidate-${{ github.run_id }}-${{ github.run_attempt }}"
        )
    );
    assert!(workflow.contains("if: always()\n        env:\n          OKP_SCRATCH_SESSION:"));
    assert!(workflow.contains("run: ./scripts/reclaim-ok-player-scratch.sh"));
}

fn numeric_generations(out: &Path) -> Vec<u64> {
    let mut generations = fs::read_dir(out)
        .expect("out listing")
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u64>().ok())
        .collect::<Vec<_>>();
    generations.sort_unstable();
    generations
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write executable");
    let mut permissions = fs::metadata(path)
        .expect("executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set executable permissions");
}
