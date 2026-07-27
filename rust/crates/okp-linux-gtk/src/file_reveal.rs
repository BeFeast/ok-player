use std::cell::Cell;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::future::Future;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use futures_lite::StreamExt;
use gtk::prelude::FileExt;

/// How long the whole desktop-portal exchange may take.
///
/// `OpenDirectory` answers asynchronously through the request's `Response` signal. Waiting for that
/// is what makes the portal route honest: without it a rejection would be reported as success and
/// the containing-folder fallback would never run. Connecting, the method call, and the response
/// can each stall on a degraded portal, so the deadline covers all of them — otherwise the click
/// would strand with no feedback and no fallback.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(10);

/// Serial for the portal `handle_token`, which must be unique per request.
static PORTAL_REQUEST_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRevealOutcome {
    /// `org.freedesktop.FileManager1.ShowItems` selected the file itself.
    ExactFile,
    /// The desktop portal opened the directory containing the file.
    PortalDirectory,
    /// The default handler opened the containing directory.
    ContainingFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRevealError {
    MissingFile,
    MissingParent,
    LaunchFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRevealPurpose {
    Screenshot,
    MediaLocation,
}

/// Identifies one reveal request.
///
/// A reveal runs off the main context, so its result can arrive after the user has started a newer
/// reveal or after a newer toast has replaced the one they acted on. Results carry the request they
/// belong to and are discarded unless that request is still the one being awaited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileRevealTicket {
    id: u64,
    purpose: FileRevealPurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileRevealJobResult {
    ticket: FileRevealTicket,
    pub result: Result<FileRevealOutcome, FileRevealError>,
}

impl FileRevealJobResult {
    pub(crate) fn purpose(&self) -> FileRevealPurpose {
        self.ticket.purpose
    }
}

pub(crate) trait FileRevealLauncher {
    /// Select the file itself through the session-bus file manager interface.
    fn reveal_exact(&self, path: &Path) -> Result<(), String>;
    /// Open the file's directory through `xdg-desktop-portal`.
    fn reveal_via_portal(&self, path: &Path) -> Result<(), String>;
    /// Open a directory with the desktop default handler.
    fn open_folder(&self, path: &Path) -> Result<(), String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DesktopFileRevealLauncher;

/// Percent-encoded `file://` URI for a local path.
///
/// Paths with spaces, non-ASCII characters, or non-UTF-8 bytes are routine on Linux, and every
/// launcher below hands the location to another process as a URI.
pub(crate) fn file_uri(path: &Path) -> String {
    gtk::gio::File::for_path(path).uri().to_string()
}

/// Opens `path` for the portal without ever blocking on a peer.
///
/// The reveal target is whatever the user pointed the player at. A FIFO or a device node opened
/// the ordinary way waits for the other end, and that wait happens before the portal deadline
/// starts, so it would strand the click with no feedback and no folder fallback.
pub(crate) fn open_for_portal(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// Runs `work`, giving up with an error once `deadline` elapses.
async fn with_deadline<T>(
    deadline: Duration,
    work: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    futures_lite::future::or(work, async move {
        async_io::Timer::after(deadline).await;
        Err("the portal did not answer the reveal request".to_owned())
    })
    .await
}

/// Object path the portal will answer on, derived from our unique name and `token`.
///
/// Deriving it lets the response be subscribed to before the method call, so an immediate answer
/// cannot be missed.
fn portal_request_path(unique_name: &str, token: &str) -> String {
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    format!("/org/freedesktop/portal/desktop/request/{sender}/{token}")
}

impl FileRevealLauncher for DesktopFileRevealLauncher {
    fn reveal_exact(&self, path: &Path) -> Result<(), String> {
        let uri = file_uri(path);
        let connection = zbus::blocking::connection::Builder::session()
            .map_err(|error| error.to_string())?
            .method_timeout(Duration::from_secs(2))
            .build()
            .map_err(|error| error.to_string())?;
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.FileManager1",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1",
        )
        .map_err(|error| error.to_string())?;
        let _: () = proxy
            .call("ShowItems", &(vec![uri], ""))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn reveal_via_portal(&self, path: &Path) -> Result<(), String> {
        // `org.freedesktop.FileManager1` is not activatable on every desktop, while
        // `xdg-desktop-portal` usually is. `OpenDirectory` takes the file itself and opens the
        // directory that contains it, highlighting the file where the backend supports it.
        let file = open_for_portal(path).map_err(|error| error.to_string())?;
        zbus::block_on(with_deadline(PORTAL_TIMEOUT, async move {
            let connection = zbus::connection::Builder::session()
                .map_err(|error| error.to_string())?
                .build()
                .await
                .map_err(|error| error.to_string())?;
            let unique_name = connection
                .unique_name()
                .ok_or_else(|| "the session bus assigned no unique name".to_owned())?
                .to_string();
            let token = format!(
                "okplayer_{}_{}",
                std::process::id(),
                PORTAL_REQUEST_SERIAL.fetch_add(1, Ordering::Relaxed)
            );

            let request_path = portal_request_path(&unique_name, &token);
            let request = zbus::Proxy::new(
                &connection,
                "org.freedesktop.portal.Desktop",
                request_path.clone(),
                "org.freedesktop.portal.Request",
            )
            .await
            .map_err(|error| error.to_string())?;
            let mut responses = request
                .receive_signal("Response")
                .await
                .map_err(|error| error.to_string())?;

            let open_uri = zbus::Proxy::new(
                &connection,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.OpenURI",
            )
            .await
            .map_err(|error| error.to_string())?;
            let mut options = HashMap::new();
            options.insert("handle_token", zbus::zvariant::Value::from(token.as_str()));
            let handle: zbus::zvariant::OwnedObjectPath = open_uri
                .call(
                    "OpenDirectory",
                    &("", zbus::zvariant::Fd::from(file.as_fd()), options),
                )
                .await
                .map_err(|error| error.to_string())?;
            if handle.as_str() != request_path {
                // A portal that ignored `handle_token` will answer somewhere we are not
                // listening. Give up now instead of stalling until the timeout.
                return Err(format!(
                    "the portal answers on {handle} instead of the requested handle"
                ));
            }

            let response = responses
                .next()
                .await
                .ok_or_else(|| "the portal closed the reveal request".to_owned())?;

            let (code, _details): (u32, HashMap<String, zbus::zvariant::OwnedValue>) = response
                .body()
                .deserialize()
                .map_err(|error| error.to_string())?;
            if code == 0 {
                Ok(())
            } else {
                Err(format!("the portal refused the reveal request: {code}"))
            }
        }))
    }

    fn open_folder(&self, path: &Path) -> Result<(), String> {
        let uri = file_uri(path);
        gtk::gio::AppInfo::launch_default_for_uri(uri.as_str(), None::<&gtk::gio::AppLaunchContext>)
            .map_err(|error| error.to_string())
    }
}

pub(crate) fn reveal_file_with(
    path: &Path,
    launcher: &impl FileRevealLauncher,
) -> Result<FileRevealOutcome, FileRevealError> {
    match path.try_exists() {
        Ok(true) => {}
        Ok(false) | Err(_) => return Err(FileRevealError::MissingFile),
    }

    if launcher.reveal_exact(path).is_ok() {
        return Ok(FileRevealOutcome::ExactFile);
    }

    if launcher.reveal_via_portal(path).is_ok() {
        return Ok(FileRevealOutcome::PortalDirectory);
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(FileRevealError::MissingParent)?;
    launcher
        .open_folder(parent)
        .map(|()| FileRevealOutcome::ContainingFolder)
        .map_err(|_| FileRevealError::LaunchFailed)
}

#[derive(Debug)]
pub(crate) struct FileRevealJobs {
    sender: mpsc::Sender<FileRevealJobResult>,
    receiver: mpsc::Receiver<FileRevealJobResult>,
    next_id: Cell<u64>,
    awaited: Cell<Option<u64>>,
}

impl Default for FileRevealJobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            next_id: Cell::new(0),
            awaited: Cell::new(None),
        }
    }
}

impl FileRevealJobs {
    pub(crate) fn request(&self, path: PathBuf, purpose: FileRevealPurpose) {
        drop(self.request_with(path, purpose, DesktopFileRevealLauncher));
    }

    /// Starts a reveal on a worker thread and returns its join handle.
    ///
    /// Starting a reveal also makes it the only one whose result will be delivered: whichever
    /// action the user took last owns the feedback, and anything still in flight from before is
    /// dropped. The handle lets tests order a slow reveal against a newer one deterministically;
    /// the player drops it and collects results through [`FileRevealJobs::drain`].
    pub(crate) fn request_with<L>(
        &self,
        path: PathBuf,
        purpose: FileRevealPurpose,
        launcher: L,
    ) -> thread::JoinHandle<()>
    where
        L: FileRevealLauncher + Send + 'static,
    {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.awaited.set(Some(id));
        let ticket = FileRevealTicket { id, purpose };
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = reveal_file_with(&path, &launcher);
            let _ = sender.send(FileRevealJobResult { ticket, result });
        })
    }

    /// Drops every in-flight reveal, because the toast that would show its feedback is gone.
    pub(crate) fn invalidate(&self) {
        self.awaited.set(None);
    }

    pub(crate) fn drain(&self) -> Vec<FileRevealJobResult> {
        let awaited = self.awaited.get();
        let delivered: Vec<FileRevealJobResult> = self
            .receiver
            .try_iter()
            .filter(|job| Some(job.ticket.id) == awaited)
            .collect();
        if !delivered.is_empty() {
            self.awaited.set(None);
        }
        delivered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct FakeLauncher {
        exact_result: Result<(), String>,
        portal_result: Result<(), String>,
        folder_result: Result<(), String>,
        exact_paths: RefCell<Vec<PathBuf>>,
        portal_paths: RefCell<Vec<PathBuf>>,
        folder_paths: RefCell<Vec<PathBuf>>,
    }

    impl Default for FakeLauncher {
        fn default() -> Self {
            Self {
                exact_result: Ok(()),
                portal_result: Ok(()),
                folder_result: Ok(()),
                exact_paths: RefCell::new(Vec::new()),
                portal_paths: RefCell::new(Vec::new()),
                folder_paths: RefCell::new(Vec::new()),
            }
        }
    }

    impl FileRevealLauncher for FakeLauncher {
        fn reveal_exact(&self, path: &Path) -> Result<(), String> {
            self.exact_paths.borrow_mut().push(path.to_owned());
            self.exact_result.clone()
        }

        fn reveal_via_portal(&self, path: &Path) -> Result<(), String> {
            self.portal_paths.borrow_mut().push(path.to_owned());
            self.portal_result.clone()
        }

        fn open_folder(&self, path: &Path) -> Result<(), String> {
            self.folder_paths.borrow_mut().push(path.to_owned());
            self.folder_result.clone()
        }
    }

    /// A launcher that succeeds only once the test releases it.
    struct GatedLauncher {
        gate: Mutex<mpsc::Receiver<()>>,
    }

    impl FileRevealLauncher for GatedLauncher {
        fn reveal_exact(&self, _path: &Path) -> Result<(), String> {
            let _ = self.gate.lock().expect("reveal gate").recv();
            Ok(())
        }

        fn reveal_via_portal(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }

        fn open_folder(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }
    }

    struct ImmediateLauncher;

    impl FileRevealLauncher for ImmediateLauncher {
        fn reveal_exact(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }

        fn reveal_via_portal(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }

        fn open_folder(&self, _path: &Path) -> Result<(), String> {
            Ok(())
        }
    }

    fn existing_file(name: impl AsRef<Path>) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary reveal directory");
        let path = directory.path().join(name);
        fs::write(&path, b"frame").expect("test screenshot");
        (directory, path)
    }

    fn gated() -> (mpsc::Sender<()>, GatedLauncher) {
        let (release, gate) = mpsc::channel();
        (
            release,
            GatedLauncher {
                gate: Mutex::new(gate),
            },
        )
    }

    #[test]
    fn exact_reveal_stops_before_the_portal_and_folder_fallbacks() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher::default();

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Ok(FileRevealOutcome::ExactFile)
        );
        assert_eq!(launcher.exact_paths.borrow().as_slice(), [path]);
        assert!(launcher.portal_paths.borrow().is_empty());
        assert!(launcher.folder_paths.borrow().is_empty());
    }

    #[test]
    fn an_unactivatable_file_manager_falls_back_to_the_desktop_portal() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher {
            exact_result: Err("not activatable".to_owned()),
            ..FakeLauncher::default()
        };

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Ok(FileRevealOutcome::PortalDirectory)
        );
        assert_eq!(
            launcher.portal_paths.borrow().as_slice(),
            std::slice::from_ref(&path)
        );
        assert!(
            launcher.folder_paths.borrow().is_empty(),
            "a successful portal reveal must not also open the containing folder"
        );
    }

    #[test]
    fn a_refused_portal_request_still_opens_the_containing_folder() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher {
            exact_result: Err("not activatable".to_owned()),
            portal_result: Err("the portal refused the reveal request: 2".to_owned()),
            ..FakeLauncher::default()
        };

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Ok(FileRevealOutcome::ContainingFolder)
        );
        assert_eq!(
            launcher.exact_paths.borrow().as_slice(),
            std::slice::from_ref(&path)
        );
        assert_eq!(
            launcher.portal_paths.borrow().as_slice(),
            std::slice::from_ref(&path)
        );
        assert_eq!(
            launcher.folder_paths.borrow().as_slice(),
            [path.parent().expect("test parent").to_owned()]
        );
    }

    #[test]
    fn opening_a_fifo_for_the_portal_does_not_wait_for_a_writer() {
        let directory = tempfile::tempdir().expect("temporary reveal directory");
        let path = directory.path().join("stream.mkv");
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("fifo path");
        assert_eq!(
            unsafe { libc::mkfifo(name.as_ptr(), 0o600) },
            0,
            "could not create the test fifo"
        );

        let (finished, outcome) = mpsc::channel();
        let probe = path.clone();
        thread::spawn(move || {
            let _ = finished.send(open_for_portal(&probe).is_ok());
        });

        let opened = outcome
            .recv_timeout(Duration::from_secs(5))
            .expect("opening a fifo with no writer must not block the reveal");
        assert!(opened, "the fifo should still open for the portal");
    }

    #[test]
    fn a_silent_portal_cannot_hang_the_reveal() {
        let (finished, outcome) = mpsc::channel();
        thread::spawn(move || {
            let result = zbus::block_on(with_deadline(
                Duration::from_millis(50),
                std::future::pending::<Result<(), String>>(),
            ));
            let _ = finished.send(result);
        });

        let result = outcome
            .recv_timeout(Duration::from_secs(5))
            .expect("a portal that never answers must not hold the reveal open");
        assert!(result.is_err(), "unexpected success: {result:?}");
    }

    #[test]
    fn the_portal_answers_on_a_request_path_derived_from_our_unique_name() {
        assert_eq!(
            portal_request_path(":1.271", "okplayer_4213_0"),
            "/org/freedesktop/portal/desktop/request/1_271/okplayer_4213_0"
        );
    }

    #[test]
    fn missing_file_does_not_invoke_a_launcher() {
        let directory = tempfile::tempdir().expect("temporary reveal directory");
        let path = directory.path().join("removed.png");
        let launcher = FakeLauncher::default();

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Err(FileRevealError::MissingFile)
        );
        assert!(launcher.exact_paths.borrow().is_empty());
        assert!(launcher.portal_paths.borrow().is_empty());
        assert!(launcher.folder_paths.borrow().is_empty());
    }

    #[test]
    fn a_file_deleted_after_the_toast_appears_reports_a_missing_file() {
        let (_directory, path) = existing_file("frame with spaces.png");
        let launcher = FakeLauncher::default();
        assert_eq!(
            reveal_file_with(&path, &launcher),
            Ok(FileRevealOutcome::ExactFile)
        );

        fs::remove_file(&path).expect("delete the saved screenshot");

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Err(FileRevealError::MissingFile)
        );
        assert_eq!(
            launcher.exact_paths.borrow().len(),
            1,
            "a deleted screenshot must not reach a launcher a second time"
        );
    }

    #[test]
    fn launch_failure_is_reported_after_every_fallback() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher {
            exact_result: Err("not activatable".to_owned()),
            portal_result: Err("no portal".to_owned()),
            folder_result: Err("no handler".to_owned()),
            ..FakeLauncher::default()
        };

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Err(FileRevealError::LaunchFailed)
        );
    }

    #[test]
    fn non_utf8_paths_reach_the_launcher_without_lossy_conversion() {
        let name = OsString::from_vec(b"frame-\xff.png".to_vec());
        let (_directory, path) = existing_file(PathBuf::from(name));
        let launcher = FakeLauncher::default();

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Ok(FileRevealOutcome::ExactFile)
        );
        assert_eq!(launcher.exact_paths.borrow().as_slice(), [path]);
    }

    #[test]
    fn launcher_uris_percent_encode_spaces_and_non_ascii_paths() {
        let uri = file_uri(Path::new("/screens/OK Player/кадр 01.png"));

        assert!(
            uri.starts_with("file:///screens/OK%20Player/"),
            "unexpected uri: {uri}"
        );
        assert!(!uri.contains(' '), "unexpected unescaped space: {uri}");
        assert!(
            !uri.contains('к'),
            "non-ASCII path segments must be percent-encoded: {uri}"
        );
        assert!(uri.ends_with("%2001.png"), "unexpected uri: {uri}");
    }

    #[test]
    fn launcher_uris_escape_reserved_characters_in_file_names() {
        let uri = file_uri(Path::new("/screens/a#b?c/frame&1.png"));

        assert!(!uri.contains('#'), "unexpected fragment marker: {uri}");
        assert!(!uri.contains('?'), "unexpected query marker: {uri}");
        assert!(uri.ends_with("/frame&1.png"), "unexpected uri: {uri}");
    }

    #[test]
    fn a_completion_for_the_displayed_toast_is_delivered() {
        let jobs = FileRevealJobs::default();
        let (_directory, path) = existing_file("frame.png");

        jobs.request_with(path, FileRevealPurpose::Screenshot, ImmediateLauncher)
            .join()
            .expect("reveal worker");

        let delivered = jobs.drain();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].purpose(), FileRevealPurpose::Screenshot);
        assert_eq!(delivered[0].result, Ok(FileRevealOutcome::ExactFile));
    }

    #[test]
    fn a_stale_reveal_completion_cannot_replace_a_newer_toast() {
        let jobs = FileRevealJobs::default();
        let (_directory, path) = existing_file("frame-a.png");
        let (release, launcher) = gated();

        // The user reveals a screenshot; the launcher has not returned yet.
        let pending = jobs.request_with(path, FileRevealPurpose::Screenshot, launcher);

        // A newer screenshot is saved and takes over the toast.
        jobs.invalidate();

        // The reveal finishes only now, after the toast it belonged to is gone.
        release.send(()).expect("release the pending reveal");
        pending.join().expect("stale reveal worker");

        assert!(
            jobs.drain().is_empty(),
            "a reveal started for a replaced toast must not produce feedback"
        );
    }

    #[test]
    fn a_newer_reveal_request_owns_the_feedback() {
        let jobs = FileRevealJobs::default();
        let (_first_directory, first) = existing_file("frame-a.png");
        let (_second_directory, second) = existing_file("frame-b.png");
        let (release, launcher) = gated();

        // The saved-path button starts a reveal that has not returned yet.
        let pending = jobs.request_with(first, FileRevealPurpose::Screenshot, launcher);

        // The user then invokes the media-location command, without any toast change in between.
        jobs.request_with(second, FileRevealPurpose::MediaLocation, ImmediateLauncher)
            .join()
            .expect("newer reveal worker");

        release.send(()).expect("release the pending reveal");
        pending.join().expect("stale reveal worker");

        let delivered = jobs.drain();
        assert_eq!(
            delivered.len(),
            1,
            "only the reveal the user started last may report back"
        );
        assert_eq!(delivered[0].purpose(), FileRevealPurpose::MediaLocation);
    }
}
