//! Process coordination for the rolling Linux candidate builder.
//!
//! The lock file descriptor is explicitly close-on-exec. A package or
//! headless smoke may leave a child process running after its direct parent
//! returns, but that child must never retain the build/publish lock.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

static OWNER_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// The critical-section phase recorded for lock diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateLockPhase {
    Build,
    Publish,
    Promote,
    BuildAndPublish,
}

impl fmt::Display for CandidateLockPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Build => "build",
            Self::Publish => "publish",
            Self::Promote => "promote",
            Self::BuildAndPublish => "build-and-publish",
        };
        formatter.write_str(value)
    }
}

/// Human-readable owner metadata written beside the coordination lock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CandidateLockOwner {
    pub owner_id: String,
    pub phase: CandidateLockPhase,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
}

impl CandidateLockOwner {
    /// Create metadata for the current process without recording host-specific
    /// paths or machine identity.
    #[must_use]
    pub fn current(
        phase: CandidateLockPhase,
        workflow_run_id: Option<String>,
        source_sha: Option<String>,
    ) -> Self {
        let pid = std::process::id();
        let sequence = OWNER_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self {
            owner_id: format!("{pid}-{sequence}"),
            phase,
            pid,
            workflow_run_id: workflow_run_id.filter(|value| !value.trim().is_empty()),
            source_sha: source_sha.filter(|value| !value.trim().is_empty()),
        }
    }

    /// Compact diagnostic safe for workflow logs.
    #[must_use]
    pub fn diagnostic(&self) -> String {
        let mut fields = vec![format!("phase={}", self.phase), format!("pid={}", self.pid)];
        if let Some(run_id) = &self.workflow_run_id {
            fields.push(format!("workflow_run_id={run_id}"));
        }
        if let Some(source_sha) = &self.source_sha {
            fields.push(format!("source_sha={source_sha}"));
        }
        fields.join(" ")
    }
}

/// Failure to acquire or maintain the candidate coordination lock.
#[derive(Debug)]
pub enum CandidateLockError {
    Busy(Option<CandidateLockOwner>),
    Io(String),
    Unsupported,
}

impl fmt::Display for CandidateLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(Some(owner)) => write!(
                formatter,
                "candidate build/publish lock is held ({})",
                owner.diagnostic()
            ),
            Self::Busy(None) => formatter
                .write_str("candidate build/publish lock is held (owner metadata unavailable)"),
            Self::Io(error) => formatter.write_str(error),
            Self::Unsupported => {
                formatter.write_str("candidate file locking is supported only on Unix")
            }
        }
    }
}

impl std::error::Error for CandidateLockError {}

/// Held candidate lock. Dropping it clears this owner's diagnostic record and
/// releases the kernel lock.
#[derive(Debug)]
pub struct CandidateLock {
    file: File,
    owner_path: PathBuf,
    owner: CandidateLockOwner,
}

impl CandidateLock {
    /// Acquire an exclusive non-blocking lock and publish owner diagnostics.
    pub fn try_acquire(
        lock_path: &Path,
        owner_path: &Path,
        owner: CandidateLockOwner,
    ) -> Result<Self, CandidateLockError> {
        let parent = lock_path.parent().ok_or_else(|| {
            CandidateLockError::Io(format!("{} has no parent directory", lock_path.display()))
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create lock directory", parent, error))?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|error| io_error("open candidate lock", lock_path, error))?;

        acquire_file_lock(&file).map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                CandidateLockError::Busy(read_owner(owner_path))
            } else {
                io_error("acquire candidate lock", lock_path, error)
            }
        })?;

        if let Err(error) = write_owner(owner_path, &owner) {
            let _ = release_file_lock(&file);
            return Err(error);
        }

        Ok(Self {
            file,
            owner_path: owner_path.to_owned(),
            owner,
        })
    }

    #[must_use]
    pub fn owner(&self) -> &CandidateLockOwner {
        &self.owner
    }
}

impl Drop for CandidateLock {
    fn drop(&mut self) {
        if read_owner(&self.owner_path).as_ref() == Some(&self.owner) {
            let _ = fs::remove_file(&self.owner_path);
        }
        let _ = release_file_lock(&self.file);
    }
}

fn read_owner(path: &Path) -> Option<CandidateLockOwner> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_owner(path: &Path, owner: &CandidateLockOwner) -> Result<(), CandidateLockError> {
    let parent = path.parent().ok_or_else(|| {
        CandidateLockError::Io(format!("{} has no parent directory", path.display()))
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create lock-owner directory", parent, error))?;
    let temp = parent.join(format!(".candidate-lock-owner-{}.tmp", owner.owner_id));
    let mut bytes = serde_json::to_vec_pretty(owner).map_err(|error| {
        CandidateLockError::Io(format!("serialize candidate lock owner: {error}"))
    })?;
    bytes.push(b'\n');
    fs::write(&temp, bytes).map_err(|error| io_error("write lock owner", &temp, error))?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(io_error("publish lock owner", path, error));
    }
    Ok(())
}

fn io_error(action: &str, path: &Path, error: io::Error) -> CandidateLockError {
    CandidateLockError::Io(format!("{action} {}: {error}", path.display()))
}

#[cfg(unix)]
fn acquire_file_lock(file: &File) -> io::Result<()> {
    let fd = file.as_raw_fd();
    // Rust opens files close-on-exec on Unix; set the flag explicitly because
    // retaining this descriptor in a delayed smoke child is the regression
    // this lock exists to prevent.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn acquire_file_lock(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        CandidateLockError::Unsupported,
    ))
}

#[cfg(unix)]
fn release_file_lock(file: &File) -> io::Result<()> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn release_file_lock(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::process::Command;

    use okp_test_fixtures::unique_temp_dir;

    use super::*;

    fn owner(phase: CandidateLockPhase) -> CandidateLockOwner {
        CandidateLockOwner::current(phase, Some("29598738722".to_owned()), None)
    }

    #[test]
    fn concurrent_acquisition_reports_owner_and_phase() {
        let root = unique_temp_dir("okp-candidate-lock-owner");
        let lock_path = root.path().join("build.lock");
        let owner_path = root.path().join("build.lock.owner.json");
        let first_owner = owner(CandidateLockPhase::Build);
        let _first = CandidateLock::try_acquire(&lock_path, &owner_path, first_owner.clone())
            .expect("first lock should succeed");

        let error =
            CandidateLock::try_acquire(&lock_path, &owner_path, owner(CandidateLockPhase::Publish))
                .expect_err("overlapping lock should fail");

        assert!(matches!(
            &error,
            CandidateLockError::Busy(Some(actual)) if actual == &first_owner
        ));
        assert!(error.to_string().contains("phase=build"));
        assert!(error.to_string().contains("workflow_run_id=29598738722"));
    }

    #[test]
    fn inheritable_descriptor_fixture_reproduces_the_old_handoff_failure() {
        let root = unique_temp_dir("okp-candidate-lock-inherited");
        let lock_path = root.path().join("build.lock");
        let owner_path = root.path().join("build.lock.owner.json");
        let inherited = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .expect("old-style lock should open");
        let fd = inherited.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert_ne!(flags, -1, "descriptor flags should be readable");
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
            0,
            "fixture descriptor should be inheritable"
        );
        assert_eq!(
            unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) },
            0,
            "fixture lock should succeed"
        );

        let status = Command::new("sh")
            .args(["-c", "sleep 2 &"])
            .status()
            .expect("delayed child fixture should launch");
        assert!(status.success());
        drop(inherited);

        assert!(matches!(
            CandidateLock::try_acquire(&lock_path, &owner_path, owner(CandidateLockPhase::Publish)),
            Err(CandidateLockError::Busy(None))
        ));
    }

    #[test]
    fn delayed_child_cannot_retain_the_lock_descriptor() {
        let root = unique_temp_dir("okp-candidate-lock-cloexec");
        let lock_path = root.path().join("build.lock");
        let owner_path = root.path().join("build.lock.owner.json");
        let first =
            CandidateLock::try_acquire(&lock_path, &owner_path, owner(CandidateLockPhase::Build))
                .expect("build lock should succeed");

        let status = Command::new("sh")
            .args(["-c", "sleep 2 &"])
            .status()
            .expect("delayed child fixture should launch");
        assert!(status.success());
        drop(first);

        let _publish =
            CandidateLock::try_acquire(&lock_path, &owner_path, owner(CandidateLockPhase::Publish))
                .expect("publish must acquire immediately after build returns");
    }

    #[test]
    fn repeated_build_to_publish_handoffs_are_deterministic() {
        let root = unique_temp_dir("okp-candidate-lock-handoff");
        let lock_path = root.path().join("build.lock");
        let owner_path = root.path().join("build.lock.owner.json");

        for _ in 0..8 {
            let build = CandidateLock::try_acquire(
                &lock_path,
                &owner_path,
                owner(CandidateLockPhase::Build),
            )
            .expect("build lock should succeed");
            let status = Command::new("sh")
                .args(["-c", "sleep 2 &"])
                .status()
                .expect("delayed child fixture should launch");
            assert!(status.success());
            drop(build);

            let publish = CandidateLock::try_acquire(
                &lock_path,
                &owner_path,
                owner(CandidateLockPhase::Publish),
            )
            .expect("publish lock should succeed without a timing retry");
            drop(publish);
        }
    }
}
