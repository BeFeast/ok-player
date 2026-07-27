use std::cell::Cell;
use std::collections::HashMap;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gtk::prelude::FileExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRevealOutcome {
    /// `org.freedesktop.FileManager1.ShowItems` selected the file itself.
    ExactFile,
    /// The desktop portal opened the containing directory for the file.
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

/// Identifies the toast generation a reveal request was started for.
///
/// A reveal runs off the main context, so its result can arrive after a newer toast has already
/// replaced the one the user acted on. Results carry the generation they were started under and
/// are discarded when that generation is no longer displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileRevealTicket {
    generation: u64,
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

fn session_proxy(
    destination: &'static str,
    object_path: &'static str,
    interface: &'static str,
) -> Result<zbus::blocking::Proxy<'static>, String> {
    let connection = zbus::blocking::connection::Builder::session()
        .map_err(|error| error.to_string())?
        .method_timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    zbus::blocking::Proxy::new(&connection, destination, object_path, interface)
        .map_err(|error| error.to_string())
}

impl FileRevealLauncher for DesktopFileRevealLauncher {
    fn reveal_exact(&self, path: &Path) -> Result<(), String> {
        let uri = file_uri(path);
        let proxy = session_proxy(
            "org.freedesktop.FileManager1",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1",
        )?;
        let _: () = proxy
            .call("ShowItems", &(vec![uri], ""))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn reveal_via_portal(&self, path: &Path) -> Result<(), String> {
        // `org.freedesktop.FileManager1` is not activatable on every desktop, while
        // `xdg-desktop-portal` usually is. `OpenDirectory` takes the file itself and opens the
        // directory that contains it, highlighting the file where the backend supports it.
        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let proxy = session_proxy(
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.OpenURI",
        )?;
        let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        let _: zbus::zvariant::OwnedObjectPath = proxy
            .call(
                "OpenDirectory",
                &("", zbus::zvariant::Fd::from(file.as_fd()), options),
            )
            .map_err(|error| error.to_string())?;
        Ok(())
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
    generation: Cell<u64>,
}

impl Default for FileRevealJobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            generation: Cell::new(0),
        }
    }
}

impl FileRevealJobs {
    pub(crate) fn request(&self, path: PathBuf, purpose: FileRevealPurpose) {
        drop(self.request_with(path, purpose, DesktopFileRevealLauncher));
    }

    /// Starts a reveal on a worker thread and returns its join handle.
    ///
    /// The handle lets tests order a slow reveal against a newer toast deterministically; the
    /// player itself drops it and collects results through [`FileRevealJobs::drain`].
    pub(crate) fn request_with<L>(
        &self,
        path: PathBuf,
        purpose: FileRevealPurpose,
        launcher: L,
    ) -> thread::JoinHandle<()>
    where
        L: FileRevealLauncher + Send + 'static,
    {
        let ticket = FileRevealTicket {
            generation: self.generation.get(),
            purpose,
        };
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = reveal_file_with(&path, &launcher);
            let _ = sender.send(FileRevealJobResult { ticket, result });
        })
    }

    /// Marks every in-flight reveal as belonging to a superseded toast.
    ///
    /// Called whenever a new toast replaces the visible one, so a late completion can no longer
    /// hide a newer saved-screenshot action.
    pub(crate) fn invalidate(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    pub(crate) fn drain(&self) -> Vec<FileRevealJobResult> {
        let current = self.generation.get();
        self.receiver
            .try_iter()
            .filter(|job| job.ticket.generation == current)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
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
    fn losing_both_reveal_routes_opens_the_containing_folder() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher {
            exact_result: Err("not activatable".to_owned()),
            portal_result: Err("no portal".to_owned()),
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
        let (_first_directory, first) = existing_file("frame-a.png");
        let (_second_directory, second) = existing_file("frame-b.png");
        let (release, gate) = mpsc::channel();

        // The user reveals screenshot A; the launcher has not returned yet.
        let pending = jobs.request_with(
            first,
            FileRevealPurpose::Screenshot,
            GatedLauncher {
                gate: Mutex::new(gate),
            },
        );

        // Screenshot B is saved and takes over the toast, then the user reveals it too.
        jobs.invalidate();
        jobs.request_with(second, FileRevealPurpose::MediaLocation, ImmediateLauncher)
            .join()
            .expect("newer reveal worker");

        let current = jobs.drain();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].purpose(), FileRevealPurpose::MediaLocation);

        // A's reveal finishes only now, after B owns the toast.
        release.send(()).expect("release the pending reveal");
        pending.join().expect("stale reveal worker");

        assert!(
            jobs.drain().is_empty(),
            "a reveal started for a replaced toast must not produce feedback"
        );
    }
}
