#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "updater/unix.rs"]
mod unix;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    if let Err(error) = unix::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("Self-update is unsupported on this platform.");
    std::process::exit(1);
}
