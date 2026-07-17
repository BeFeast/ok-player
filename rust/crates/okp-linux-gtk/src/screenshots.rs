use std::env;
use std::ffi::CString;
use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use okp_core::screenshot::{
    SavedCaptureContext, SavedCaptureValidity, candidate_filename, saved_capture_validity,
};
use okp_core::settings::ScreenshotFormat;

const MAX_COLLISION_ATTEMPTS: u32 = 1_000;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct SavedCaptureTarget {
    pub temp_path: PathBuf,
    pub include_subtitles: bool,
    pub request_context: SavedCaptureContext,
    directory: PathBuf,
    media_path: Option<PathBuf>,
    timestamp_millis: u128,
    format: ScreenshotFormat,
}

#[derive(Debug)]
pub enum PendingCapture {
    Saved(SavedCaptureTarget),
    Clipboard(PathBuf),
}

#[derive(Debug)]
pub enum ScreenshotJobResult {
    SavedPrepared(Result<SavedCaptureTarget, String>),
    ClipboardPrepared(Result<PathBuf, String>),
    SavedPublished(Result<PathBuf, String>),
}

#[derive(Debug)]
pub struct ScreenshotJobs {
    sender: mpsc::Sender<ScreenshotJobResult>,
    receiver: mpsc::Receiver<ScreenshotJobResult>,
    pending: std::collections::HashMap<u64, PendingCapture>,
}

impl Default for ScreenshotJobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            pending: std::collections::HashMap::new(),
        }
    }
}

impl ScreenshotJobs {
    pub fn prepare_saved(
        &self,
        directory: PathBuf,
        media_path: Option<PathBuf>,
        request_context: SavedCaptureContext,
        format: ScreenshotFormat,
        include_subtitles: bool,
    ) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let error_directory = directory.clone();
            let result = prepare_saved_capture(
                directory,
                media_path,
                request_context,
                format,
                include_subtitles,
            )
            .map_err(|error| destination_error(&error_directory, error));
            let _ = sender.send(ScreenshotJobResult::SavedPrepared(result));
        });
    }

    pub fn prepare_clipboard(&self) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = prepare_clipboard_capture().map_err(|error| error.to_string());
            let _ = sender.send(ScreenshotJobResult::ClipboardPrepared(result));
        });
    }

    pub fn publish_saved(&self, target: SavedCaptureTarget) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let error_directory = target.directory.clone();
            let result = publish_saved_capture(target)
                .map_err(|error| destination_error(&error_directory, error));
            let _ = sender.send(ScreenshotJobResult::SavedPublished(result));
        });
    }

    pub fn drain(&self) -> Vec<ScreenshotJobResult> {
        self.receiver.try_iter().collect()
    }

    pub fn insert_pending(&mut self, request_id: u64, capture: PendingCapture) {
        self.pending.insert(request_id, capture);
    }

    pub fn take_pending(&mut self, request_id: u64) -> Option<PendingCapture> {
        self.pending.remove(&request_id)
    }
}

pub fn prepare_saved_capture(
    directory: PathBuf,
    media_path: Option<PathBuf>,
    request_context: SavedCaptureContext,
    format: ScreenshotFormat,
    include_subtitles: bool,
) -> io::Result<SavedCaptureTarget> {
    fs::create_dir_all(&directory)?;
    if !directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "screenshot destination is not a directory",
        ));
    }
    verify_directory_writable(&directory)?;

    let timestamp_millis = unix_millis();
    let temp_path = unique_temp_path(&directory, "saved-frame", format.extension())?;
    Ok(SavedCaptureTarget {
        temp_path,
        include_subtitles,
        request_context,
        directory,
        media_path,
        timestamp_millis,
        format,
    })
}

pub fn publish_saved_capture(target: SavedCaptureTarget) -> io::Result<PathBuf> {
    if let Err(error) = validate_capture_output(&target.temp_path) {
        remove_temporary_capture(&target.temp_path);
        return Err(error);
    }

    for suffix in 0..MAX_COLLISION_ATTEMPTS {
        let filename = candidate_filename(
            target.media_path.as_deref(),
            target.request_context.position,
            target.timestamp_millis,
            target.format.extension(),
            suffix,
        );
        let destination = target.directory.join(filename);
        match rename_noreplace(&target.temp_path, &destination) {
            Ok(()) => return Ok(destination),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                remove_temporary_capture(&target.temp_path);
                return Err(error);
            }
        }
    }

    remove_temporary_capture(&target.temp_path);
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a collision-free screenshot filename",
    ))
}

pub fn cancel_saved_capture_if_stale(
    target: &SavedCaptureTarget,
    current_context: SavedCaptureContext,
) -> Option<SavedCaptureValidity> {
    let validity = saved_capture_validity(target.request_context, current_context);
    if validity == SavedCaptureValidity::Valid {
        return None;
    }

    remove_temporary_capture(&target.temp_path);
    Some(validity)
}

pub fn prepare_clipboard_capture() -> io::Result<PathBuf> {
    let directory = env::temp_dir().join("ok-player");
    fs::create_dir_all(&directory)?;
    unique_temp_path(&directory, "clipboard-frame", "png")
}

pub fn default_screenshot_dir() -> PathBuf {
    xdg_pictures_dir()
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join("Pictures")))
        .unwrap_or_else(env::temp_dir)
        .join("OK Player")
}

pub fn remove_temporary_capture(path: &Path) {
    let _ = fs::remove_file(path);
}

fn unique_temp_path(directory: &Path, purpose: &str, extension: &str) -> io::Result<PathBuf> {
    let process = std::process::id();
    let timestamp = unix_millis();
    for _ in 0..MAX_COLLISION_ATTEMPTS {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".ok-player-{purpose}-{process}-{timestamp}-{sequence}.{extension}"
        ));
        if !path.exists() {
            return Ok(path);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary screenshot path",
    ))
}

fn verify_directory_writable(directory: &Path) -> io::Result<()> {
    let probe = unique_temp_path(directory, "write-test", "tmp")?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    fs::remove_file(probe)
}

fn validate_capture_output(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "libmpv did not produce a regular screenshot file",
        ));
    }
    if metadata.len() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "libmpv produced an empty screenshot file",
        ));
    }

    let mut file = fs::File::open(path)?;
    let mut first_byte = [0_u8; 1];
    file.read_exact(&mut first_byte)?;
    Ok(())
}

fn destination_error(directory: &Path, error: io::Error) -> String {
    format!(
        "Couldn't save screenshot to {}: {error}",
        directory.display()
    )
}

fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    let source_c = CString::new(source.as_os_str().as_bytes())?;
    let destination_c = CString::new(destination.as_os_str().as_bytes())?;

    // SAFETY: both C strings are NUL-terminated and remain alive for the call.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source_c.as_ptr(),
            libc::AT_FDCWD,
            destination_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    if renameat2_is_unsupported(&error) {
        fs::hard_link(source, destination)?;
        fs::remove_file(source)?;
        return Ok(());
    }
    Err(error)
}

fn renameat2_is_unsupported(error: &io::Error) -> bool {
    error.raw_os_error().is_some_and(|code| {
        code == libc::ENOSYS
            || code == libc::EINVAL
            || code == libc::EOPNOTSUPP
            || code == libc::ENOTSUP
    })
}

fn xdg_pictures_dir() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let user_dirs = fs::read_to_string(config_home.join("user-dirs.dirs")).ok()?;
    parse_xdg_pictures_dir(&home, &user_dirs)
}

fn parse_xdg_pictures_dir(home: &Path, user_dirs: &str) -> Option<PathBuf> {
    for line in user_dirs.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("XDG_PICTURES_DIR=") {
            let value = value.trim_matches('"');
            let value = value.replace("$HOME", &home.to_string_lossy());
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }

    None
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;

    #[test]
    fn publish_saved_capture_never_overwrites_a_collision() {
        let directory = unique_temp_dir("okp-screenshot-collision");
        let first_name = candidate_filename(None, None, 1234, "png", 0);
        fs::write(directory.path().join(first_name), b"existing").expect("existing screenshot");
        let temp_path = directory.path().join(".capture.png");
        fs::write(&temp_path, b"new frame").expect("temporary screenshot");

        let published = publish_saved_capture(SavedCaptureTarget {
            temp_path,
            include_subtitles: false,
            request_context: SavedCaptureContext {
                source_generation: 1,
                seek_generation: 0,
                position: None,
            },
            directory: directory.path().to_owned(),
            media_path: None,
            timestamp_millis: 1234,
            format: ScreenshotFormat::Png,
        })
        .expect("publish screenshot");

        assert_eq!(published, directory.path().join("ok-player-1234-1.png"));
        assert_eq!(
            fs::read(directory.path().join("ok-player-1234.png")).unwrap(),
            b"existing"
        );
        assert_eq!(fs::read(published).unwrap(), b"new frame");
    }

    #[test]
    fn prepare_saved_capture_creates_a_missing_writable_destination() {
        let root = unique_temp_dir("okp-screenshot-create-destination");
        let directory = root.path().join("Pictures/OK Player");

        let target = prepare_saved_capture(
            directory.clone(),
            None,
            SavedCaptureContext {
                source_generation: 1,
                seek_generation: 0,
                position: Some(3.0),
            },
            ScreenshotFormat::Png,
            false,
        )
        .expect("missing screenshot destination should be created");

        assert!(directory.is_dir());
        assert!(!target.temp_path.exists());
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }

    #[test]
    fn prepare_saved_capture_rejects_a_file_as_the_destination() {
        let root = unique_temp_dir("okp-screenshot-invalid-destination");
        let destination = root.path().join("not-a-directory");
        fs::write(&destination, b"occupied").expect("destination fixture");

        let error = prepare_saved_capture(
            destination,
            None,
            SavedCaptureContext {
                source_generation: 1,
                seek_generation: 0,
                position: None,
            },
            ScreenshotFormat::Png,
            false,
        )
        .expect_err("a file cannot be used as a screenshot directory");

        assert!(matches!(
            error.kind(),
            io::ErrorKind::AlreadyExists | io::ErrorKind::NotADirectory
        ));
    }

    #[test]
    fn publish_rejects_missing_and_empty_libmpv_output() {
        let root = unique_temp_dir("okp-screenshot-output-validation");
        let request_context = SavedCaptureContext {
            source_generation: 1,
            seek_generation: 0,
            position: None,
        };
        let missing = root.path().join("missing.png");
        let missing_error = publish_saved_capture(SavedCaptureTarget {
            temp_path: missing,
            include_subtitles: false,
            request_context,
            directory: root.path().to_owned(),
            media_path: None,
            timestamp_millis: 1234,
            format: ScreenshotFormat::Png,
        })
        .expect_err("a successful command reply without output must not publish");
        assert_eq!(missing_error.kind(), io::ErrorKind::NotFound);

        let empty = root.path().join("empty.png");
        fs::write(&empty, []).expect("empty screenshot fixture");
        let empty_error = publish_saved_capture(SavedCaptureTarget {
            temp_path: empty.clone(),
            include_subtitles: false,
            request_context,
            directory: root.path().to_owned(),
            media_path: None,
            timestamp_millis: 1234,
            format: ScreenshotFormat::Png,
        })
        .expect_err("an empty screenshot must not publish");
        assert_eq!(empty_error.kind(), io::ErrorKind::UnexpectedEof);
        assert!(!empty.exists());
    }

    #[test]
    fn media_switch_cancels_prepared_capture_before_publication() {
        let root = unique_temp_dir("okp-screenshot-stale-source");
        let directory = root.path().join("missing-captures");
        let requested = SavedCaptureContext {
            source_generation: 7,
            seek_generation: 2,
            position: Some(42.0),
        };
        let target = prepare_saved_capture(
            directory.clone(),
            Some(PathBuf::from("old-media.mkv")),
            requested,
            ScreenshotFormat::Png,
            false,
        )
        .expect("prepared capture");
        fs::write(&target.temp_path, b"wrong frame").expect("temporary frame");

        let validity = cancel_saved_capture_if_stale(
            &target,
            SavedCaptureContext {
                source_generation: 8,
                ..requested
            },
        );

        assert_eq!(validity, Some(SavedCaptureValidity::SourceChanged));
        assert!(!target.temp_path.exists());
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }

    #[test]
    fn queued_seek_cancels_capture_before_observed_position_changes() {
        let root = unique_temp_dir("okp-screenshot-stale-seek");
        let directory = root.path().join("missing-captures");
        let requested = SavedCaptureContext {
            source_generation: 7,
            seek_generation: 2,
            position: Some(42.0),
        };
        let target = prepare_saved_capture(
            directory.clone(),
            Some(PathBuf::from("movie.mkv")),
            requested,
            ScreenshotFormat::Webp,
            true,
        )
        .expect("prepared capture");
        fs::write(&target.temp_path, b"wrong frame").expect("temporary frame");

        let validity = cancel_saved_capture_if_stale(
            &target,
            SavedCaptureContext {
                seek_generation: 3,
                ..requested
            },
        );

        assert_eq!(validity, Some(SavedCaptureValidity::Seeked));
        assert!(!target.temp_path.exists());
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }

    #[test]
    fn parse_xdg_pictures_dir_reads_standard_user_dirs_file() {
        let home = Path::new("/home/tester");
        let user_dirs = r#"
XDG_DESKTOP_DIR="$HOME/Desktop"
XDG_PICTURES_DIR="$HOME/Pictures"
"#;

        assert_eq!(
            parse_xdg_pictures_dir(home, user_dirs),
            Some(PathBuf::from("/home/tester/Pictures"))
        );
    }
}
