fn main() {
    pkg_config::Config::new()
        .atleast_version("1.109")
        .probe("mpv")
        .expect("libmpv development files are required; install libmpv-dev");
    println!("cargo:rustc-link-lib=GL");
    println!("cargo:rustc-link-lib=EGL");
}
