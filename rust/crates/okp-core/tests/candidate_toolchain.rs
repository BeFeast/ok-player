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
fn portable_package_list_excludes_dotnet_tools() {
    let fixture = unique_temp_dir("okp-portable-package-list");
    let manifest = fixture.path().join("toolchain.manifest");
    fs::write(
        &manifest,
        concat!(
            "command|cargo|cargo|cargo\n",
            "command-or-dotnet-tool|vpk|vpk|dotnet-sdk-9.0\n",
            "pkg-config|libass|libass|libass-dev\n",
        ),
    )
    .expect("fake manifest");

    let host = Command::new("/bin/bash")
        .arg(toolchain_script())
        .arg("--print-ubuntu-packages")
        .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
        .output()
        .expect("host package list should run");
    assert!(host.status.success());
    assert!(String::from_utf8_lossy(&host.stdout).contains("dotnet-sdk-9.0"));

    let portable = Command::new("/bin/bash")
        .arg(toolchain_script())
        .arg("--print-portable-ubuntu-packages")
        .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
        .output()
        .expect("portable package list should run");
    assert!(portable.status.success());
    let packages = String::from_utf8_lossy(&portable.stdout);
    assert!(packages.contains("cargo"));
    assert!(packages.contains("libass-dev"));
    assert!(!packages.contains("dotnet-sdk-9.0"));
}

#[test]
fn package_entry_points_scope_their_preflight_gate_scripts() {
    let root = repository_root();
    for (script, required, excluded) in [
        (
            "scripts/package-linux-deb.sh",
            "$ROOT/scripts/package-linux-deb.sh",
            "$ROOT/scripts/package-linux-velopack.sh",
        ),
        (
            "scripts/package-linux-velopack.sh",
            "$ROOT/scripts/package-linux-velopack.sh",
            "$ROOT/scripts/package-linux-deb.sh",
        ),
    ] {
        let entry_point = fs::read_to_string(root.join(script)).expect("package entry point");
        assert!(entry_point.contains("export OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS="));
        assert!(entry_point.contains(required));
        assert!(entry_point.contains("$ROOT/scripts/collect-linux-bundled-mpv-runtime.sh"));
        assert!(entry_point.contains("$ROOT/scripts/verify-linux-bundled-mpv.sh"));
        assert!(!entry_point.contains(excluded));
        assert!(!entry_point.contains("$ROOT/scripts/verify-linux-package-portability.sh"));
    }
}

#[test]
fn debian_preflight_skips_dotnet_tools_but_other_lanes_require_them() {
    let fixture = unique_temp_dir("okp-debian-preflight-dotnet-boundary");
    let bin = fixture.path().join("bin");
    fs::create_dir_all(&bin).expect("fake bin directory");
    write_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    let manifest = fixture.path().join("toolchain.manifest");
    fs::write(
        &manifest,
        "command|cargo|cargo|cargo\ncommand-or-dotnet-tool|vpk|vpk|dotnet-sdk-9.0\n",
    )
    .expect("fake manifest");
    let build_script = fixture.path().join("build-local-mpv.sh");
    fs::write(&build_script, "okp_candidate_tool cargo --version\n").expect("fake build script");
    let gate_script = fixture.path().join("package-linux-deb.sh");
    fs::write(
        &gate_script,
        "#!/bin/sh\n# candidate-required-tools: cargo\n",
    )
    .expect("fake gate script");

    let run_preflight = |require_dotnet_tools: &str| {
        Command::new("/bin/bash")
            .arg(toolchain_script())
            .env("PATH", &bin)
            .env("HOME", fixture.path())
            .env("OKP_CANDIDATE_TOOLCHAIN_MANIFEST", &manifest)
            .env("OKP_CANDIDATE_TOOLCHAIN_BUILD_SCRIPT", &build_script)
            .env("OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS", &gate_script)
            .env(
                "OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS",
                require_dotnet_tools,
            )
            .output()
            .expect("preflight should run")
    };

    let debian = run_preflight("false");
    assert!(
        debian.status.success(),
        "{}",
        String::from_utf8_lossy(&debian.stderr)
    );

    let appimage = run_preflight("true");
    assert_eq!(appimage.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&appimage.stderr).contains("vpk [dotnet-sdk-9.0]"));
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

    let rpm_workflow =
        fs::read_to_string(root.join(".github/workflows/rpm.yml")).expect("Fedora RPM workflow");
    assert!(rpm_workflow.contains("CARGO_HTTP_MULTIPLEXING: 'false'"));
    assert!(rpm_workflow.contains("CARGO_NET_RETRY: '10'"));

    let docs = fs::read_to_string(root.join("docs/linux-candidate-builder.md"))
        .expect("candidate builder guide");
    assert!(docs.contains("scripts/linux-candidate-toolchain.manifest"));
    assert!(docs.contains("--print-ubuntu-packages"));
    assert!(docs.contains("--print-portable-ubuntu-packages"));
    assert!(docs.contains("dotnet tool install --global vpk --version 1.2.0"));
    assert!(docs.contains("scheduled native-builder preflight"));

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
fn portable_builder_falls_back_to_usable_podman() {
    let fixture = unique_temp_dir("okp-portable-builder-runtime-fallback");
    let bin = fixture.path().join("bin");
    let log = fixture.path().join("runtime.log");
    fs::create_dir_all(&bin).expect("fake runtime directory");
    write_executable(
        &bin.join("git"),
        &format!(
            "#!/bin/sh\nset -eu\n[ \"${{3:-}}\" = rev-parse ]\n[ \"${{4:-}}\" = --verify ]\n[ \"${{5:-}}\" = 'HEAD^{{commit}}' ]\nprintf '%s\\n' '{TEST_SOURCE_SHA}'\n"
        ),
    );
    write_executable(
        &bin.join("docker"),
        "#!/bin/sh\nset -eu\nprintf 'docker %s\\n' \"$*\" >> \"$OKP_RUNTIME_LOG\"\n[ \"${1:-}\" != info ]\n",
    );
    write_executable(
        &bin.join("podman"),
        "#!/bin/sh\nset -eu\nprintf 'podman %s\\n' \"$*\" >> \"$OKP_RUNTIME_LOG\"\nexit 0\n",
    );

    let output = Command::new("/bin/bash")
        .arg(portable_builder_script())
        .args(["deb", "1.0.0"])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("OKP_RUNTIME_LOG", &log)
        .output()
        .expect("portable package builder should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_to_string(log).expect("runtime invocation log");
    assert!(log.contains("docker info"));
    assert!(log.contains("podman info"));
    assert!(log.contains("podman build --tag"));
    assert!(log.contains("--target deb"));
    assert!(log.contains("podman run"));
    assert!(!log.contains("docker build"));
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
        .env("PATH", portability_test_path(fixture.path()))
        .env("OKP_PORTABILITY_CONTAINER_MODE", "skip")
        .output()
        .expect("native portability check should run");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("portability build marker: appimage PASS"));
    assert!(stdout.contains("portability build marker: debian PASS"));
    let report_contents = fs::read_to_string(&report).expect("portability report");
    assert!(report_contents.contains(r#""schema_version": 2"#));
    assert!(report_contents.contains(r#""verification_mode": "native-equivalence""#));
    assert!(report_contents.contains(&format!(r#""source_sha": "{TEST_SOURCE_SHA}""#)));
    assert!(report_contents.contains(r#""build_marker": "0123456""#));
    let output = Command::new("/bin/bash")
        .arg(portability_report_script())
        .arg(&report)
        .arg(&deb)
        .arg(&appimage)
        .arg(TEST_SOURCE_SHA)
        .output()
        .expect("portability report verification should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::metadata(&appimage)
            .expect("AppImage metadata")
            .permissions()
            .mode()
            & 0o111,
        0,
        "verification must not mutate a downloaded artifact's mode"
    );
}

#[test]
fn native_portability_gate_rejects_mismatched_build_marker() {
    let fixture = unique_temp_dir("okp-native-portability-marker-fail");
    let deb = build_test_deb(fixture.path(), "libc6");
    let appimage = build_test_appimage(fixture.path());

    let output = Command::new("/bin/bash")
        .arg(portability_script())
        .arg(&deb)
        .arg(&appimage)
        .arg(fixture.path().join("portability-report.json"))
        .arg("fedcba9876543210fedcba9876543210fedcba98")
        .env("PATH", portability_test_path(fixture.path()))
        .env("OKP_PORTABILITY_CONTAINER_MODE", "skip")
        .output()
        .expect("native portability check should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("packaged build marker mismatch: appimage expected fedcba9")
    );
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
        .env("PATH", portability_test_path(fixture.path()))
        .env("OKP_PORTABILITY_CONTAINER_MODE", "skip")
        .output()
        .expect("native portability check should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("portability dependency is not declared by the Debian package")
    );
    assert!(
        fs::read_to_string(portability_script())
            .expect("portability script")
            .contains("dependency_failures=1")
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

fn portability_report_script() -> PathBuf {
    repository_root().join("scripts/verify-linux-portability-report.sh")
}

fn portable_builder_script() -> PathBuf {
    repository_root().join("scripts/build-linux-portable-package.sh")
}

fn build_test_deb(root: &Path, depends: &str) -> PathBuf {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("fake tool directory");
    write_executable(
        &bin.join("dpkg-deb"),
        "#!/bin/sh\nset -eu\ncase \"$1\" in\n  -f) cat \"$2\" ;;\n  -x) mkdir -p \"$3/usr/lib/ok-player\"; cp /bin/true \"$3/usr/lib/ok-player/ok-player\"; printf '0123456\\n' >> \"$3/usr/lib/ok-player/ok-player\" ;;\n  *) exit 2 ;;\nesac\n",
    );
    write_executable(
        &bin.join("dpkg-query"),
        "#!/bin/sh\nset -eu\n[ \"$1\" = -S ]\nprintf 'libc6: %s\\n' \"$2\"\n",
    );
    let deb = root.join("ok-player-test_1.0.0_amd64.deb");
    fs::write(&deb, depends).expect("fake Debian metadata");
    deb
}

fn build_test_appimage(root: &Path) -> PathBuf {
    let appimage = root.join("OK-Player-test-x86_64.AppImage");
    fs::write(
        &appimage,
        "#!/bin/sh\nset -eu\nmkdir -p squashfs-root/usr/bin\ncp /bin/true squashfs-root/usr/bin/ok-player\nprintf '0123456\\n' >> squashfs-root/usr/bin/ok-player\n",
    )
    .expect("fake downloaded AppImage");
    appimage
}

fn portability_test_path(root: &Path) -> String {
    format!("{}:/usr/bin:/bin", root.join("bin").display())
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("fake executable");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable permissions");
}
