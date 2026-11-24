use super::*;
use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub const PLATFORM: Platform = Platform::Linux;
const MANAGED_IMAGE: &str = "OK-Player.AppImage";

pub fn watch_process(pid: libc::pid_t) -> io::Result<OwnedFd> {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) } as libc::c_int;
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn wait_for_exit(player: &OwnedFd) -> io::Result<bool> {
    let mut event = libc::pollfd {
        fd: player.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    if unsafe { libc::poll(&mut event, 1, 45_000) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(event.revents & libc::POLLIN != 0)
}

pub fn managed_root() -> Result<PathBuf> {
    let data = env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    if !data.is_absolute() {
        return Err("The AppImage data directory must be absolute.".into());
    }
    Ok(data.join("ok-player"))
}

pub fn validate_fixture(target: &Path, key: &Path) -> Result<()> {
    require_fixture_target(target, key)
}

pub struct Installation {
    directory: PathBuf,
    target: PathBuf,
    previous: PathBuf,
    inode: (u64, u64),
}

pub fn installation(target: &Path, build: &str, _: &str, fixture: bool) -> Result<Installation> {
    let directory = target
        .parent()
        .ok_or("No managed AppImage installation directory.")?
        .canonicalize()?;
    if target.file_name().is_none_or(|name| name != MANAGED_IMAGE)
        || (!fixture && directory != managed_root()?.canonicalize()?)
    {
        return Err(
            "Use the managed OK Player AppImage launcher to install in-app updates.".into(),
        );
    }
    require_owned_directory(&directory)?;
    let target = directory.join(MANAGED_IMAGE);
    let previous = fs::read_link(&target)?;
    let expected = PathBuf::from(format!(
        "builds/{build}/OK-Player-0.11.0-beta.0.{build}-x86_64.AppImage"
    ));
    if previous != expected {
        return Err("The managed AppImage has changed since this instance was launched.".into());
    }
    let previous_image = directory.join(&previous);
    if previous_image.canonicalize()? != previous_image {
        return Err("The installed AppImage path contains an unexpected link.".into());
    }
    let inode = image_identity(&previous_image)?;
    Ok(Installation {
        directory,
        target,
        previous,
        inode,
    })
}

impl Installation {
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn prepare(&self, payload: &Path, _: &Release) -> Result<PathBuf> {
        let mut header = [0_u8; 20];
        File::open(payload)?.read_exact(&mut header)?;
        if &header[..4] != b"\x7fELF" || &header[8..11] != b"AI\x02" || header[18..20] != [0x3e, 0]
        {
            return Err("The update is not an x86-64 type-2 AppImage.".into());
        }
        fs::set_permissions(payload, fs::Permissions::from_mode(0o755))?;
        Ok(payload.to_path_buf())
    }

    pub fn apply(
        &self,
        payload: PathBuf,
        release: &Release,
        staging: tempfile::TempDir,
    ) -> Result<()> {
        let stage = staging.path();
        let lock = install_lock(&self.directory.join(".appimage.lock"))?;
        let builds = self.directory.join("builds");
        require_owned_directory(&builds)?;
        let archived = builds.join(release.build.to_string());
        let image = archived.join(&release.filename);
        if archived.exists() {
            require_owned_directory(&archived)?;
            image_identity(&image)?;
            if !okp_core::signed_updates::matches_payload(release, &image)? {
                return Err("Different bytes already exist for the update build.".into());
            }
        } else {
            let directory = stage.join("build");
            fs::create_dir(&directory)?;
            fs::rename(payload, directory.join(&release.filename))?;
            let mut provenance = File::create_new(directory.join("release.json"))?;
            serde_json::to_writer(&mut provenance, release)?;
            provenance.sync_all()?;
            File::open(&directory)?.sync_all()?;
            fs::rename(directory, &archived)?;
            File::open(builds)?.sync_all()?;
        }
        let relative = image.strip_prefix(&self.directory)?;
        if fs::read_link(&self.target)? != self.previous
            || image_identity(&self.directory.join(&self.previous))? != self.inode
        {
            return Err("The managed AppImage changed during the update.".into());
        }
        self.set_current(relative, stage)?;
        drop(lock);

        let receipt = stage.join("started");
        let entered = self
            .launch(Some(&receipt), None)
            .and_then(|child| await_startup(child, &receipt, release));
        if let Err(error) = entered {
            let _lock = install_lock(&self.directory.join(".appimage.lock"))?;
            if fs::read_link(&self.target)? == relative {
                self.set_current(&self.previous, stage)?;
            }
            return Err(error);
        }
        Ok(())
    }

    fn set_current(&self, relative: &Path, stage: &Path) -> Result<()> {
        let link = stage.join("current");
        symlink(relative, &link)?;
        fs::rename(link, &self.target)?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }

    pub fn reopen_after_failure(&self, error: &str) -> Result<()> {
        if fs::read_link(&self.target)? == self.previous {
            self.launch(None, Some(error))?;
        }
        Ok(())
    }

    fn launch(&self, receipt: Option<&Path>, failure: Option<&str>) -> Result<Child> {
        let lock = self.directory.join(".appimage.lock");
        let mut command = Command::new("/usr/bin/flock");
        command
            .process_group(0)
            .args(["--close", "--shared"])
            .arg(lock)
            .arg(&self.target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for name in [
            "APPIMAGE",
            "APPDIR",
            "ARGV0",
            "OWD",
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "OKP_UPDATE_RESTART_RECEIPT",
            "OKP_UPDATE_RESTART_ERROR",
        ] {
            command.env_remove(name);
        }
        command
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .env("OKP_MANAGED_APPIMAGE", "1")
            .env_remove("OKP_SKIP_UPDATE_CHECK");
        if let Some(path) = receipt {
            command.env("OKP_UPDATE_RESTART_RECEIPT", path);
        }
        if let Some(message) = failure {
            command.env("OKP_UPDATE_RESTART_ERROR", message);
        }
        Ok(command.spawn()?)
    }
}

fn image_identity(path: &Path) -> Result<(u64, u64)> {
    let info = path.symlink_metadata()?;
    if !info.is_file()
        || info.nlink() != 1
        || info.uid() != unsafe { libc::geteuid() }
        || info.mode() & 0o022 != 0
        || info.mode() & 0o100 == 0
    {
        return Err("The installed AppImage is not a private executable file.".into());
    }
    Ok((info.dev(), info.ino()))
}
