fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
    println!("cargo:rerun-if-env-changed=OKP_REQUIRE_SYSTEM_MPV");
    let library = pkg_config::Config::new()
        .atleast_version("1.109")
        .probe("mpv")
        .expect("libmpv development files are required; install libmpv-dev");
    if std::env::var_os("OKP_REQUIRE_SYSTEM_MPV").is_some() {
        let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("okp-mpv lives under the Rust workspace");
        for path in library.link_paths.iter().chain(&library.include_paths) {
            assert!(
                !path.starts_with(source_root),
                "native RPM builds must link Fedora's system mpv, not a source-tree copy: {}",
                path.display()
            );
        }
    }
    println!("cargo:rustc-env=OKP_LINKED_MPV_VERSION={}", library.version);
    println!("cargo:rustc-link-lib=GL");
    println!("cargo:rustc-link-lib=EGL");
}
