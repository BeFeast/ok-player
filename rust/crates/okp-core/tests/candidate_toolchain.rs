#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use okp_test_fixtures::unique_temp_dir;

const TEST_SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

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
        .env("OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS", "")
        .output()
        .expect("preflight should run");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "candidate build failed at gate bundled-mpv; missing dependencies: ninja [ninja-build], pkg-config:libass [libass-dev], pkg-config:libplacebo [libplacebo-dev]"
    );
}

#[test]
fn undeclared_gate_tool_fails_the_manifest_contract_check() {
    let fixture = unique_temp_dir("okp-candidate-gate-tool-drift");
    let manifest = fixture.path().join("toolchain.manifest");
    fs::write(&manifest, "command|dpkg-deb|dpkg-deb|dpkg\n").expect("fake manifest");
    let gate_script = fixture.path().join("package-linux-deb.sh");
    fs::write(
        &gate_script,
        "#!/bin/sh\n# candidate-required-tools: dpkg-deb objdump\n",
    )
    .expect("fake gate script");

    let output = Command::new("/bin/bash")
        .arg(toolchain_script())
        .arg("--check-gate-script")
        .arg(&gate_script)
        .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
        .output()
        .expect("gate contract check should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "candidate gate tool is not declared in linux-candidate-toolchain.manifest: objdump"
    ));
}

#[test]
fn preflight_reports_a_missing_declared_gate_tool_before_building() {
    let fixture = unique_temp_dir("okp-candidate-gate-tool-missing");
    let bin = fixture.path().join("bin");
    fs::create_dir_all(&bin).expect("fake bin directory");
    write_executable(&bin.join("meson"), "#!/bin/sh\nexit 0\n");
    let manifest = fixture.path().join("toolchain.manifest");
    fs::write(
        &manifest,
        "command|meson|meson|meson\ncommand|dpkg-deb|dpkg-deb|dpkg\n",
    )
    .expect("fake manifest");
    let build_script = fixture.path().join("build-local-mpv.sh");
    fs::write(&build_script, "okp_candidate_tool meson setup\n").expect("fake build script");
    let gate_script = fixture.path().join("package-linux-deb.sh");
    fs::write(
        &gate_script,
        "#!/bin/sh\n# candidate-required-tools: dpkg-deb\n",
    )
    .expect("fake gate script");

    let output = Command::new("/bin/bash")
        .arg(toolchain_script())
        .env("PATH", &bin)
        .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
        .env("OKP_CANDIDATE_TOOLCHAIN_BUILD_SCRIPT", &build_script)
        .env("OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS", &gate_script)
        .output()
        .expect("preflight should run");

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "candidate build failed at gate bundled-mpv; missing dependencies: dpkg-deb [dpkg]"
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

    let release_workflow = fs::read_to_string(root.join(".github/workflows/release-linux.yml"))
        .expect("release workflow");
    assert!(release_workflow.contains("OKP_PORTABILITY_CONTAINER_MODE: required"));
    assert!(release_workflow.contains("OKP_PORTABILITY_REQUIRED_MODE=foreign-container"));

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

    for script in [
        "scripts/package-linux-deb.sh",
        "scripts/package-linux-velopack.sh",
        "scripts/collect-linux-bundled-mpv-runtime.sh",
        "scripts/verify-linux-bundled-mpv.sh",
        "scripts/verify-linux-package-portability.sh",
    ] {
        let output = Command::new("/bin/bash")
            .arg(toolchain_script())
            .arg("--check-gate-script")
            .arg(root.join(script))
            .output()
            .expect("repository gate manifest contract should run");
        assert!(
            output.status.success(),
            "{script}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn native_portability_gate_accepts_only_declared_host_dependencies() {
    let fixture = unique_temp_dir("okp-native-portability-pass");
    let deb = build_test_deb(fixture.path(), "libc6");
    let appimage = build_test_appimage(fixture.path());
    let report = fixture.path().join("portability-report.json");

    let output = Command::new("/bin/bash")
        .arg(portability_script())
        .arg(&deb)
        .arg(&appimage)
        .arg(&report)
        .arg(TEST_SOURCE_SHA)
        .env("PATH", "/usr/bin:/bin")
        .env("OKP_PORTABILITY_CONTAINER_MODE", "skip")
        .output()
        .expect("native portability check should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = fs::read_to_string(report).expect("portability report");
    assert!(report.contains(r#""schema_version": 2"#));
    assert!(report.contains(r#""verification_mode": "native-equivalence""#));
    assert!(report.contains(&format!(r#""source_sha": "{TEST_SOURCE_SHA}""#)));
    assert!(report.contains(r#""build_marker": "0123456""#));
}

#[test]
fn native_portability_gate_rejects_undeclared_host_dependencies() {
    let fixture = unique_temp_dir("okp-native-portability-fail");
    let deb = build_test_deb(fixture.path(), "coreutils");
    let appimage = build_test_appimage(fixture.path());

    let output = Command::new("/bin/bash")
        .arg(portability_script())
        .arg(&deb)
        .arg(&appimage)
        .arg(fixture.path().join("portability-report.json"))
        .arg(TEST_SOURCE_SHA)
        .env("PATH", "/usr/bin:/bin")
        .env("OKP_PORTABILITY_CONTAINER_MODE", "skip")
        .output()
        .expect("native portability check should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("portability dependency is not declared by the Debian package")
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn toolchain_script() -> PathBuf {
    repository_root().join("scripts/linux-candidate-toolchain.sh")
}

fn portability_script() -> PathBuf {
    repository_root().join("scripts/verify-linux-package-portability.sh")
}

fn build_test_deb(root: &Path, depends: &str) -> PathBuf {
    let package_root = root.join("package");
    fs::create_dir_all(package_root.join("DEBIAN")).expect("control directory");
    fs::create_dir_all(package_root.join("usr/lib/ok-player")).expect("private lib directory");
    fs::copy(
        "/bin/true",
        package_root.join("usr/lib/ok-player/ok-player"),
    )
    .expect("test ELF");
    fs::write(
        package_root.join("DEBIAN/control"),
        format!(
            "Package: ok-player-portability-test\nVersion: 1.0.0\nArchitecture: amd64\nDepends: {depends}\nMaintainer: Test <test@example.invalid>\nDescription: Portability fixture\n"
        ),
    )
    .expect("test control");
    let deb = root.join("ok-player-test_1.0.0_amd64.deb");
    let output = Command::new("dpkg-deb")
        .arg("--root-owner-group")
        .arg("--build")
        .arg(&package_root)
        .arg(&deb)
        .output()
        .expect("dpkg-deb should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    deb
}

fn build_test_appimage(root: &Path) -> PathBuf {
    let appimage = root.join("OK-Player-test-x86_64.AppImage");
    write_executable(
        &appimage,
        "#!/bin/sh\nset -eu\nmkdir -p squashfs-root/usr/bin\ncp /bin/true squashfs-root/usr/bin/ok-player\n",
    );
    appimage
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("fake executable");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable permissions");
}
