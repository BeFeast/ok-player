use okp_core::signed_updates::{Channel, Feed, Platform, Release};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{self, Child};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
use linux as adapter;
#[cfg(target_os = "macos")]
#[path = "mac.rs"]
mod mac;
#[cfg(target_os = "macos")]
use mac as adapter;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run() -> Result<()> {
    // The first-generation launch wrapper passes its AppImage flock to the app.
    // The helper must release its own copy before it can take the exclusive lock.
    #[cfg(target_os = "linux")]
    if unsafe { libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 0_u32) } < 0 {
        return Err(io::Error::last_os_error().into());
    }

    let mut args = env::args().skip(1);
    let mode = args.next().ok_or("Missing updater operation.")?;
    let (operation, fixture) = match mode.as_str() {
        "check" => ("check", false),
        "install" => ("install", false),
        "fixture-check" => ("check", true),
        "fixture-install" => ("install", true),
        _ => return Err("Unknown updater operation.".into()),
    };
    let channel: Channel = args.next().ok_or("Missing release channel.")?.parse()?;
    let build = args.next().ok_or("Missing running build.")?;
    let source = args.next().ok_or("Missing running revision.")?;
    let target = PathBuf::from(args.next().ok_or("Missing installation path.")?);
    let feed = if fixture {
        let port = args.next().ok_or("Missing fixture port.")?.parse()?;
        let key = PathBuf::from(args.next().ok_or("Missing fixture key.")?);
        adapter::validate_fixture(&target, &key)?;
        Feed::fixture(port, &fs::read(key)?)?
    } else {
        Feed::production()?
    };
    let release = feed.check(adapter::PLATFORM, channel, &build, &source)?;
    if operation == "check" {
        if args.next().is_some() {
            return Err("Unexpected check arguments.".into());
        }
        println!("{}", serde_json::to_string(&release)?);
        return Ok(());
    }
    let expected = args.next().ok_or("Missing confirmed release digest.")?;
    let parent = args
        .next()
        .ok_or("Missing player process ID.")?
        .parse::<libc::pid_t>()?;
    if args.next().is_some() || parent <= 1 || parent == process::id() as libc::pid_t {
        return Err("Invalid restart request.".into());
    }
    let release = release.ok_or("This build is already current.")?;
    if release.sha256 != expected {
        return Err("The channel changed. Check for updates again.".into());
    }
    let old = adapter::installation(&target, &build, &source, fixture)?;
    let staging = tempfile::Builder::new()
        .prefix(".okp-update-")
        .tempdir_in(old.directory())?;
    let payload = feed.stage(&release, staging.path())?;
    let prepared = old.prepare(&payload, &release)?;
    println!("ready");
    io::stdout().flush()?;
    let mut confirmation = String::new();
    io::stdin().read_line(&mut confirmation)?;
    if confirmation.trim_end() != "relaunch" {
        return Ok(());
    }
    wait_for_exit(parent)?;
    if let Err(error) = old.apply(prepared, &release, staging.path()) {
        eprintln!("{error}");
        old.reopen_after_failure(&error.to_string(), staging.path())?;
        return Err(error);
    }
    Ok(())
}

fn wait_for_exit(pid: libc::pid_t) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        if unsafe { libc::kill(pid, 0) } < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(error.into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("OK Player did not quit in time. The update was left untouched.".into())
}

fn install_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let info = file.metadata()?;
    if !info.is_file() || info.nlink() != 1 || info.uid() != unsafe { libc::geteuid() } {
        return Err("The update lock has an unexpected owner or type.".into());
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::WouldBlock {
            return Err(error.into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("Another OK Player process is using the installation.".into())
}

fn await_startup(mut child: Child, receipt: &Path, release: &Release) -> Result<()> {
    let expected = format!("{}\n{}\n", release.build, release.source);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if fs::read_to_string(receipt).is_ok_and(|value| value == expected) {
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            return Err("The updated app exited before confirming startup.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    child.wait()?;
    Err("The updated app did not confirm startup.".into())
}

fn require_owned_directory(path: &Path) -> Result<()> {
    let info = path.symlink_metadata()?;
    if !info.is_dir() || info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o022 != 0 {
        return Err("The update directory must be a user-owned directory without group or world write access.".into());
    }
    Ok(())
}

fn require_fixture_target(target: &Path, key: &Path) -> Result<()> {
    let root = env::temp_dir().canonicalize()?;
    let dir = target
        .parent()
        .ok_or("Fixture target has no parent.")?
        .canonicalize()?;
    if dir == root || !dir.starts_with(&root) || !key.canonicalize()?.starts_with(&dir) {
        return Err(
            "The update fixture and key must be in the isolated temporary installation.".into(),
        );
    }
    require_owned_directory(&dir)
}
