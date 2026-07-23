use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gtk::prelude::FileExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileRevealOutcome {
    ExactFile,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileRevealJobResult {
    pub purpose: FileRevealPurpose,
    pub result: Result<FileRevealOutcome, FileRevealError>,
}

pub(crate) trait FileRevealLauncher {
    fn reveal_exact(&self, path: &Path) -> Result<(), String>;
    fn open_folder(&self, path: &Path) -> Result<(), String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DesktopFileRevealLauncher;

impl FileRevealLauncher for DesktopFileRevealLauncher {
    fn reveal_exact(&self, path: &Path) -> Result<(), String> {
        let uri = gtk::gio::File::for_path(path).uri().to_string();
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

    fn open_folder(&self, path: &Path) -> Result<(), String> {
        let uri = gtk::gio::File::for_path(path).uri();
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
}

impl Default for FileRevealJobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }
}

impl FileRevealJobs {
    pub(crate) fn request(&self, path: PathBuf, purpose: FileRevealPurpose) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = reveal_file_with(&path, &DesktopFileRevealLauncher);
            let _ = sender.send(FileRevealJobResult { purpose, result });
        });
    }

    pub(crate) fn drain(&self) -> Vec<FileRevealJobResult> {
        self.receiver.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;

    #[derive(Debug)]
    struct FakeLauncher {
        exact_result: Result<(), String>,
        folder_result: Result<(), String>,
        exact_paths: RefCell<Vec<PathBuf>>,
        folder_paths: RefCell<Vec<PathBuf>>,
    }

    impl Default for FakeLauncher {
        fn default() -> Self {
            Self {
                exact_result: Ok(()),
                folder_result: Ok(()),
                exact_paths: RefCell::new(Vec::new()),
                folder_paths: RefCell::new(Vec::new()),
            }
        }
    }

    impl FileRevealLauncher for FakeLauncher {
        fn reveal_exact(&self, path: &Path) -> Result<(), String> {
            self.exact_paths.borrow_mut().push(path.to_owned());
            self.exact_result.clone()
        }

        fn open_folder(&self, path: &Path) -> Result<(), String> {
            self.folder_paths.borrow_mut().push(path.to_owned());
            self.folder_result.clone()
        }
    }

    fn existing_file(name: impl AsRef<Path>) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temporary reveal directory");
        let path = directory.path().join(name);
        fs::write(&path, b"frame").expect("test screenshot");
        (directory, path)
    }

    #[test]
    fn exact_reveal_stops_before_folder_fallback() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher::default();

        assert_eq!(
            reveal_file_with(&path, &launcher),
            Ok(FileRevealOutcome::ExactFile)
        );
        assert_eq!(launcher.exact_paths.borrow().as_slice(), [path]);
        assert!(launcher.folder_paths.borrow().is_empty());
    }

    #[test]
    fn unsupported_exact_reveal_opens_the_containing_folder() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher {
            exact_result: Err("unsupported".to_owned()),
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
        assert!(launcher.folder_paths.borrow().is_empty());
    }

    #[test]
    fn launch_failure_is_reported_after_the_fallback() {
        let (_directory, path) = existing_file("frame.png");
        let launcher = FakeLauncher {
            exact_result: Err("unsupported".to_owned()),
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
}
