use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rustc-link-lib=X11");
    if cfg!(target_os = "linux") {
        for library in ["wayland-client", "wayland-egl", "egl"] {
            pkg_config::Config::new()
                .probe(library)
                .unwrap_or_else(|_| panic!("{library} development files are required"));
        }
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"));
        let protocols = PathBuf::from(
            pkg_config::get_variable("wayland-protocols", "pkgdatadir")
                .expect("wayland-protocols development files are required"),
        );
        let viewporter_xml = protocols.join("stable/viewporter/viewporter.xml");
        let viewporter_header = out_dir.join("viewporter-client-protocol.h");
        let viewporter_code = out_dir.join("viewporter-protocol.c");
        generate_wayland_protocol(&viewporter_xml, &viewporter_header, &viewporter_code);
        let presentation_xml = protocols.join("stable/presentation-time/presentation-time.xml");
        let presentation_header = out_dir.join("presentation-time-client-protocol.h");
        let presentation_code = out_dir.join("presentation-time-protocol.c");
        generate_wayland_protocol(&presentation_xml, &presentation_header, &presentation_code);

        cc::Build::new()
            .file("src/native_wayland_video.c")
            .file(&viewporter_code)
            .file(&presentation_code)
            .include(&out_dir)
            .warnings(true)
            .compile("okp_native_wayland_video");
        // Cargo places pkg-config libraries before this package's static archive.
        // Repeat the direct Wayland dependencies at the end so --as-needed links
        // the archive and system libmpv references correctly in Fedora RPM builds.
        println!("cargo:rustc-link-arg=-Wl,-lwayland-egl,-lwayland-client");
        println!("cargo:rerun-if-changed=src/native_wayland_video.c");
        println!("cargo:rerun-if-changed={}", viewporter_xml.display());
        println!("cargo:rerun-if-changed={}", presentation_xml.display());
    }
    println!("cargo:rerun-if-env-changed=OKP_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=OKP_PACKAGE_KIND");
    println!("cargo:rerun-if-env-changed=OKP_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=OKP_FEDORA_RPM");
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../../.git/refs/heads/main");

    let version = env::var("OKP_BUILD_VERSION")
        .or_else(|_| env::var("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| "0.0.0-dev".to_owned());
    println!("cargo:rustc-env=OKP_BUILD_VERSION={version}");

    let package_kind = env::var("OKP_PACKAGE_KIND").unwrap_or_else(|_| "development".to_owned());
    assert!(
        matches!(
            package_kind.as_str(),
            "deb" | "appimage" | "rpm" | "development"
        ),
        "OKP_PACKAGE_KIND must be deb, appimage, rpm, or development"
    );
    println!("cargo:rustc-env=OKP_PACKAGE_KIND={package_kind}");

    let sha = env::var("OKP_BUILD_SHA")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=OKP_BUILD_SHA={sha}");
}

fn generate_wayland_protocol(xml: &Path, header: &Path, code: &Path) {
    for (mode, output) in [("client-header", header), ("private-code", code)] {
        let status = Command::new("wayland-scanner")
            .arg(mode)
            .arg(xml)
            .arg(output)
            .status()
            .unwrap_or_else(|error| panic!("running wayland-scanner failed: {error}"));
        assert!(status.success(), "wayland-scanner {mode} failed");
    }
}
