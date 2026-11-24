use super::*;
use std::ffi::CString;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub const PLATFORM: Platform = Platform::Mac;
const APP_NAME: &str = "OK Player.app";
const BUNDLE_ID: &str = "uk.oklabs.ok-player";
const EXECUTABLE: &str = "Contents/MacOS/ok-player";

pub fn watch_process(pid: libc::pid_t) -> io::Result<OwnedFd> {
    let fd = unsafe { libc::kqueue() };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let queue = unsafe { OwnedFd::from_raw_fd(fd) };
    let subscription = libc::kevent {
        ident: pid as libc::uintptr_t,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0
        || unsafe {
            libc::kevent(
                fd,
                &subscription,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(queue)
}

pub fn wait_for_exit(player: &OwnedFd) -> io::Result<bool> {
    let timeout = libc::timespec {
        tv_sec: 45,
        tv_nsec: 0,
    };
    let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
    let count = unsafe {
        libc::kevent(
            player.as_raw_fd(),
            std::ptr::null(),
            0,
            event.as_mut_ptr(),
            1,
            &timeout,
        )
    };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(count == 1 && unsafe { event.assume_init() }.fflags & libc::NOTE_EXIT != 0)
}

pub struct Installation {
    target: PathBuf,
    directory: PathBuf,
    inode: (u64, u64),
    requirement: String,
}

pub fn validate_fixture(target: &Path, key: &Path) -> Result<()> {
    require_fixture_target(target, key)
}

pub fn installation(
    target: &Path,
    build: &str,
    source: &str,
    fixture: bool,
) -> Result<Installation> {
    let directory = target
        .parent()
        .ok_or("The app has no installation directory.")?
        .canonicalize()?;
    if !fixture {
        let installed = PathBuf::from(env::var_os("HOME").ok_or("No user home directory.")?)
            .canonicalize()?
            .join("Applications");
        if directory != installed {
            return Err(
                "Move OK Player.app to ~/Applications to install updates in the app.".into(),
            );
        }
    }
    require_owned_directory(&directory)?;
    if target.file_name().is_none_or(|name| name != APP_NAME) {
        return Err("The downloaded player has an unexpected bundle name.".into());
    }
    let target = directory.join(APP_NAME);
    require_owned_directory(&target)?;
    validate_bundle(&target, build, source)?;
    let requirement = run(Command::new("/usr/bin/codesign")
        .args(["-d", "-r-"])
        .arg(&target))?
    .lines()
    .find_map(|line| line.strip_prefix("designated => "))
    .filter(|line| {
        line.contains("identifier \"uk.oklabs.ok-player\"") && line.contains("certificate")
    })
    .ok_or("The installed app has no persistent code-signing identity.")?
    .to_owned();
    validate_signature(&target, &requirement)?;
    Ok(Installation {
        inode: identity(&target)?,
        target,
        directory,
        requirement,
    })
}

impl Installation {
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn prepare(&self, payload: &Path, release: &Release) -> Result<PathBuf> {
        check_zip(payload)?;
        let stage = payload
            .parent()
            .ok_or("The ZIP has no staging directory.")?;
        run(Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(payload)
            .arg(stage))?;
        let candidate = stage.join(APP_NAME);
        require_owned_directory(&candidate)?;
        validate_bundle(&candidate, &release.build.to_string(), &release.source)?;
        validate_signature(&candidate, &self.requirement).map_err(|error| {
            format!("The update does not satisfy the installed code-signing requirement: {error}")
        })?;

        // Keep the quarantine decision for the user's installed bundle. Fresh
        // update downloads are admitted by the detached signature and codesign.
        let quarantine = Command::new("/usr/bin/xattr")
            .args(["-px", "com.apple.quarantine"])
            .arg(&self.target)
            .output()?;
        if quarantine.status.success() {
            run(Command::new("/usr/bin/xattr")
                .args(["-wx", "com.apple.quarantine"])
                .arg(
                    String::from_utf8(quarantine.stdout)?
                        .split_whitespace()
                        .collect::<String>(),
                )
                .arg(&candidate))?;
        }
        Ok(candidate)
    }

    pub fn apply(
        &self,
        candidate: PathBuf,
        release: &Release,
        staging: tempfile::TempDir,
    ) -> Result<()> {
        let _lock = install_lock(&self.directory.join(".ok-player-update.lock"))?;
        if identity(&self.target)? != self.inode {
            return Err("The installed app changed during the update.".into());
        }
        validate_signature(&self.target, &self.requirement)?;
        validate_signature(&candidate, &self.requirement)?;
        swap(&candidate, &self.target)?;
        let receipt = staging.path().join("started");
        let started = launch(&self.target, Some(&receipt))
            .and_then(|child| await_startup(child, &receipt, release));
        if let Err(error) = started {
            if let Err(rollback) = swap(&candidate, &self.target) {
                // Skip automatic staging cleanup so it cannot delete the
                // previous bundle if the system refused the reverse exchange.
                let backup = staging.keep().join(APP_NAME);
                return Err(format!(
                    "{error} Rollback failed: {rollback}. Previous bundle is at {}.",
                    backup.display(),
                )
                .into());
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn reopen_after_failure(&self, error: &str) -> Result<()> {
        if identity(&self.target)? == self.inode {
            Command::new(self.target.join(EXECUTABLE))
                .env("OKP_UPDATE_RESTART_ERROR", error)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }
        Ok(())
    }
}

fn check_zip(payload: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(payload)?)?;
    let mut size = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let path = entry
            .enclosed_name()
            .ok_or("The update ZIP contains an unsafe path.")?;
        if !(path.starts_with(APP_NAME) || path.starts_with("__MACOSX"))
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o040000 | 0o100000))
        {
            return Err("The update ZIP contains an unexpected file or link.".into());
        }
        size = size
            .checked_add(entry.size())
            .ok_or("The expanded ZIP is too large.")?;
        if size > 2 * 1024 * 1024 * 1024 {
            return Err("The expanded ZIP is too large.".into());
        }
    }
    Ok(())
}

fn validate_bundle(app: &Path, build: &str, source: &str) -> Result<()> {
    for (key, expected) in [
        ("CFBundleIdentifier", BUNDLE_ID),
        ("CFBundleVersion", build),
        ("OKPSourceSHA", source),
    ] {
        let value = run(Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist")))?;
        if value.trim() != expected {
            return Err(format!("The app bundle does not match its expected {key}.").into());
        }
    }
    Ok(())
}

fn validate_signature(app: &Path, requirement: &str) -> Result<()> {
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={requirement}"))
        .arg(app))?;
    Ok(())
}

fn identity(path: &Path) -> Result<(u64, u64)> {
    let info = path.symlink_metadata()?;
    Ok((info.dev(), info.ino()))
}

fn swap(left: &Path, right: &Path) -> Result<()> {
    let left = CString::new(left.as_os_str().as_bytes())?;
    let right = CString::new(right.as_os_str().as_bytes())?;
    if unsafe { libc::renamex_np(left.as_ptr(), right.as_ptr(), libc::RENAME_SWAP) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

fn launch(app: &Path, receipt: Option<&Path>) -> Result<Child> {
    let mut command = Command::new(app.join(EXECUTABLE));
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(receipt) = receipt {
        command.env("OKP_UPDATE_RESTART_RECEIPT", receipt);
    }
    Ok(command.spawn()?)
}

fn run(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "{} failed: {stderr}",
            command.get_program().to_string_lossy()
        )
        .into());
    }
    Ok(format!(
        "{}{stderr}",
        String::from_utf8_lossy(&output.stdout)
    ))
}
