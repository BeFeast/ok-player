use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=OKP_BUILD_VERSION");
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../../.git/refs/heads/main");

    let version = env::var("OKP_BUILD_VERSION")
        .or_else(|_| env::var("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| "0.0.0-dev".to_owned());
    println!("cargo:rustc-env=OKP_BUILD_VERSION={version}");

    let sha = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=OKP_BUILD_SHA={sha}");
}
