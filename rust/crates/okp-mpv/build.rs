fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
    let library = pkg_config::Config::new()
        .atleast_version("1.109")
        .probe("mpv")
        .expect("libmpv development files are required; install libmpv-dev");
    println!("cargo:rustc-env=OKP_LINKED_MPV_VERSION={}", library.version);
    println!("cargo:rustc-link-lib=GL");
    println!("cargo:rustc-link-lib=EGL");
}
