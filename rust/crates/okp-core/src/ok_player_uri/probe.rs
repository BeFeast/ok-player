//! The filesystem question the policy asks about a local target, behind a trait so the
//! policy is testable without a filesystem and the real check stays bounded in time.

use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

/// What the filesystem says about a path (symlinks followed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsKind {
    RegularFile,
    Directory,
    /// Exists but is not a regular file or directory: FIFO, socket, character or block
    /// device.
    Special,
    Missing,
    /// The check failed (permissions, I/O error, no capacity to run it).
    Unavailable,
    /// The check did not answer in time (for example a hung network mount).
    TimedOut,
}

/// Answers "what is at this path?" for [`super::evaluate`].
pub trait FsProbe {
    fn probe(&self, path: &Path) -> FsKind;
}

/// The real probe: `std::fs::metadata` on a worker thread, bounded by a timeout.
///
/// A call blocks the calling thread for at most the timeout, so it belongs off the UI
/// thread; where it runs is the shell's decision. A stat stuck in the kernel (a dead NFS
/// mount) cannot be cancelled: its worker thread is left to finish on its own, and at most
/// `max_in_flight` such workers exist per probe; beyond that the probe answers
/// [`FsKind::Unavailable`] at once instead of spawning more.
///
/// Clones share one in-flight count. A shell uses [`StdFsProbe::shared`], the one probe of
/// the process, so a burst of links at a hung mount cannot each build a fresh probe and
/// bypass the cap.
#[derive(Clone, Debug)]
pub struct StdFsProbe {
    timeout: Duration,
    max_in_flight: usize,
    in_flight: Arc<AtomicUsize>,
}

impl StdFsProbe {
    /// Long enough for a spun-down NAS disk to wake and answer. The caller is off the UI
    /// thread, so waiting this long never freezes the window.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
    pub const DEFAULT_MAX_IN_FLIGHT: usize = 4;

    /// The process-wide probe, with the default timeout and in-flight cap.
    pub fn shared() -> &'static StdFsProbe {
        static SHARED: OnceLock<StdFsProbe> = OnceLock::new();
        SHARED.get_or_init(StdFsProbe::default)
    }

    pub fn new(timeout: Duration, max_in_flight: usize) -> Self {
        Self {
            timeout,
            max_in_flight,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Run `check` on a worker thread and wait for it at most the probe's timeout.
    fn bounded(&self, check: impl FnOnce() -> FsKind + Send + 'static) -> FsKind {
        let mut count = self.in_flight.load(Ordering::Acquire);
        loop {
            if count >= self.max_in_flight {
                return FsKind::Unavailable;
            }
            match self.in_flight.compare_exchange_weak(
                count,
                count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => count = actual,
            }
        }

        let slot = InFlightSlot(Arc::clone(&self.in_flight));
        let (sender, receiver) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("okp-link-fs-probe".to_owned())
            .spawn(move || {
                let kind = check();
                // Free the slot before answering, so a caller that got its answer never
                // sees its own worker still counted.
                drop(slot);
                let _ = sender.send(kind);
            });
        if spawned.is_err() {
            // The closure (and with it the slot) was dropped, so the count is back.
            return FsKind::Unavailable;
        }
        match receiver.recv_timeout(self.timeout) {
            Ok(kind) => kind,
            Err(RecvTimeoutError::Timeout) => FsKind::TimedOut,
            // The worker ended without answering (the check panicked).
            Err(RecvTimeoutError::Disconnected) => FsKind::Unavailable,
        }
    }
}

impl Default for StdFsProbe {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TIMEOUT, Self::DEFAULT_MAX_IN_FLIGHT)
    }
}

impl FsProbe for StdFsProbe {
    fn probe(&self, path: &Path) -> FsKind {
        let path = path.to_path_buf();
        self.bounded(move || kind_of(&path))
    }
}

/// Releases one in-flight slot when the worker ends, even if the check panicked.
struct InFlightSlot(Arc<AtomicUsize>);

impl Drop for InFlightSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn kind_of(path: &Path) -> FsKind {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => FsKind::RegularFile,
        Ok(metadata) if metadata.is_dir() => FsKind::Directory,
        Ok(_) => FsKind::Special,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            FsKind::Missing
        }
        Err(_) => FsKind::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn a_check_that_outlives_the_timeout_answers_timed_out() {
        let probe = StdFsProbe::new(Duration::from_millis(20), 4);
        let started = std::time::Instant::now();
        let kind = probe.bounded(|| {
            thread::sleep(Duration::from_secs(3));
            FsKind::RegularFile
        });
        assert_eq!(kind, FsKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_shared_probe_is_one_instance_whose_clones_share_the_cap() {
        let first = StdFsProbe::shared();
        let second = StdFsProbe::shared();
        assert!(std::ptr::eq(first, second));
        assert!(Arc::ptr_eq(&first.in_flight, &first.clone().in_flight));
        assert_eq!(first.timeout, StdFsProbe::DEFAULT_TIMEOUT);
        assert!(StdFsProbe::DEFAULT_TIMEOUT >= Duration::from_secs(10));
    }

    #[test]
    fn a_prompt_check_answers_and_frees_its_slot() {
        let probe = StdFsProbe::new(Duration::from_secs(5), 1);
        assert_eq!(probe.bounded(|| FsKind::Directory), FsKind::Directory);
        assert_eq!(probe.in_flight.load(Ordering::Acquire), 0);
        assert_eq!(probe.bounded(|| FsKind::Special), FsKind::Special);
    }

    #[test]
    fn stuck_checks_are_capped_and_the_cap_frees_when_they_finish() {
        let probe = StdFsProbe::new(Duration::from_millis(10), 2);
        let release = Arc::new(Barrier::new(3));
        for _ in 0..2 {
            let release = Arc::clone(&release);
            let kind = probe.bounded(move || {
                release.wait();
                FsKind::RegularFile
            });
            assert_eq!(kind, FsKind::TimedOut);
        }

        // Both workers are still stuck: the next check must not even start.
        let ran = Arc::new(AtomicUsize::new(0));
        let ran_in_check = Arc::clone(&ran);
        let kind = probe.bounded(move || {
            ran_in_check.fetch_add(1, Ordering::AcqRel);
            FsKind::RegularFile
        });
        assert_eq!(kind, FsKind::Unavailable);
        assert_eq!(ran.load(Ordering::Acquire), 0);

        release.wait();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while probe.in_flight.load(Ordering::Acquire) != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "workers never finished"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let probe = StdFsProbe {
            timeout: Duration::from_secs(5),
            ..probe
        };
        assert_eq!(probe.bounded(|| FsKind::RegularFile), FsKind::RegularFile);
    }

    #[cfg(unix)]
    #[test]
    fn the_real_probe_tells_files_directories_fifos_and_absence_apart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("clip.mkv");
        std::fs::write(&file, b"x").expect("write file");
        let fifo = dir.path().join("pipe.mkv");
        let fifo_c =
            std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(fifo.as_os_str()))
                .expect("fifo path has no NUL");
        // SAFETY: `fifo_c` is a valid NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let link = dir.path().join("link.mkv");
        std::os::unix::fs::symlink(&file, &link).expect("symlink");

        let probe = StdFsProbe::default();
        assert_eq!(probe.probe(&file), FsKind::RegularFile);
        assert_eq!(probe.probe(&link), FsKind::RegularFile);
        assert_eq!(probe.probe(dir.path()), FsKind::Directory);
        assert_eq!(probe.probe(&fifo), FsKind::Special);
        assert_eq!(probe.probe(Path::new("/dev/null")), FsKind::Special);
        assert_eq!(
            probe.probe(&dir.path().join("missing.mkv")),
            FsKind::Missing
        );
        assert_eq!(probe.probe(&file.join("below-a-file.mkv")), FsKind::Missing);
    }
}
