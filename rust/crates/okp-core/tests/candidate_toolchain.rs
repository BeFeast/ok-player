#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use okp_test_fixtures::unique_temp_dir;

#[test]
fn preflight_reports_every_missing_dependency_on_one_line() {
    let fixture = unique_temp_dir("okp-candidate-toolchain-missing");
    let bin = fixture.path().join("bin");
    fs::create_dir_all(&bin).expect("fake bin directory");
    write_executable(&bin.join("meson"), "#!/bin/sh\nexit 0\n");
    write_executable(
        &bin.join("pkg-config"),
        "#!/bin/sh\ncase \"${2:-}\" in libass|libplacebo) exit 1;; *) exit 0;; esac\n",
    );
    let manifest = fixture.path().join("toolchain.manifest");
    fs::write(
        &manifest,
        concat!(
            "command|meson|meson|meson\n",
            "command|ninja|ninja|ninja-build\n",
            "command|pkg-config|pkg-config|pkg-config\n",
            "pkg-config|libass|libass|libass-dev\n",
            "pkg-config|libplacebo|libplacebo|libplacebo-dev\n",
        ),
    )
    .expect("fake manifest");
    let build_script = fixture.path().join("build-local-mpv.sh");
    fs::write(
        &build_script,
        "okp_candidate_tool meson setup\nokp_candidate_tool ninja --version\n",
    )
    .expect("fake build script");

    let output = Command::new("/bin/bash")
        .arg(toolchain_script())
        .env("PATH", &bin)
        .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
        .env("OKP_CANDIDATE_TOOLCHAIN_BUILD_SCRIPT", &build_script)
        .output()
        .expect("preflight should run");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "candidate build failed at gate bundled-mpv; missing dependencies: ninja [ninja-build], pkg-config:libass [libass-dev], pkg-config:libplacebo [libplacebo-dev]"
    );
}

#[test]
fn undeclared_build_tool_fails_the_manifest_contract_check() {
    let fixture = unique_temp_dir("okp-candidate-toolchain-drift");
    let manifest = fixture.path().join("toolchain.manifest");
    fs::write(&manifest, "command|meson|meson|meson\n").expect("fake manifest");
    let build_script = fixture.path().join("build-local-mpv.sh");
    fs::write(
        &build_script,
        "okp_candidate_tool meson setup\nokp_candidate_tool cmake --build .\n",
    )
    .expect("fake build script");

    let output = Command::new("/bin/bash")
        .arg(toolchain_script())
        .arg("--check-build-script")
        .arg(&build_script)
        .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
        .output()
        .expect("contract check should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "candidate build tool is not declared in linux-candidate-toolchain.manifest: cmake"
    ));
}

#[test]
fn workflow_and_operator_guide_consume_the_canonical_manifest() {
    let root = repository_root();
    let workflow = fs::read_to_string(root.join(".github/workflows/release-linux-candidate.yml"))
        .expect("candidate workflow");
    let preflight = workflow
        .find("Preflight bundled-mpv toolchain")
        .expect("preflight step");
    let lock = workflow
        .find("Build lock coordinator")
        .expect("build lock step");
    assert!(
        preflight < lock,
        "preflight must run before lock acquisition"
    );
    assert!(workflow.contains("./scripts/linux-candidate-toolchain.sh"));

    let docs = fs::read_to_string(root.join("docs/linux-candidate-builder.md"))
        .expect("candidate builder guide");
    assert!(docs.contains("scripts/linux-candidate-toolchain.manifest"));
    assert!(docs.contains("--print-ubuntu-packages"));

    let output = Command::new("/bin/bash")
        .arg(toolchain_script())
        .arg("--check-build-script")
        .arg(root.join("scripts/build-local-mpv.sh"))
        .output()
        .expect("repository manifest contract should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn toolchain_script() -> PathBuf {
    repository_root().join("scripts/linux-candidate-toolchain.sh")
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("fake executable");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable permissions");
}
