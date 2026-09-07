//! Portable lifecycle for copying finalized online media to a user-owned path.
//!
//! The downloader/cache layer supplies an immutable, pinned source file. Native
//! shells choose the destination before requesting a new download, run
//! [`export_saved_video`] off their UI thread, and persist [`SavedVideoIndex`]
//! only after this module reports a published file.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

pub const SAVED_VIDEO_INDEX_VERSION: u32 = 1;
const COPY_CHUNK_BYTES: usize = 256 * 1024;

/// Media available when the native destination chooser opens. A cache hit
/// carries its real completed suffix; an uncached explicit Save uses the shared
/// downloader's guaranteed Matroska finalization contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveMediaPlan {
    ReadyCache { source: PathBuf, extension: String },
    DownloadMatroska,
}

impl SaveMediaPlan {
    pub fn required_extension(&self) -> &str {
        match self {
            Self::ReadyCache { extension, .. } => extension,
            Self::DownloadMatroska => "mkv",
        }
    }
}

/// Everything captured at action time. The native callback owns this value, so
/// a later playback/source transition cannot retarget the operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveVideoSnapshot {
    pub original_url: String,
    pub title: String,
    pub format_selector: Option<String>,
    pub private_at_start: bool,
    pub media: SaveMediaPlan,
}

impl SaveVideoSnapshot {
    pub fn suggested_filename(&self) -> String {
        suggested_filename(&self.title, self.media.required_extension())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SavePickerOutcome {
    Selected(PathBuf),
    Cancelled,
    Failed(String),
}

/// Portable post-picker decision. Only the two accepted variants authorize
/// Save-owned background work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SavePickerDecision {
    NoWork,
    PickerFailed(String),
    ExtensionMismatch {
        target: PathBuf,
        required_extension: String,
    },
    ExportReady {
        request: SaveExportRequest,
        private_at_start: bool,
    },
    DownloadReady {
        original_url: String,
        target: PathBuf,
        format_selector: Option<String>,
        private_at_start: bool,
    },
}

pub fn decide_after_picker(
    snapshot: SaveVideoSnapshot,
    outcome: SavePickerOutcome,
) -> SavePickerDecision {
    let target = match outcome {
        SavePickerOutcome::Cancelled => return SavePickerDecision::NoWork,
        SavePickerOutcome::Failed(error) => return SavePickerDecision::PickerFailed(error),
        SavePickerOutcome::Selected(target) => target,
    };

    let required_extension = normalized_extension(snapshot.media.required_extension());
    if required_extension.is_empty()
        || target
            .extension()
            .and_then(|extension| extension.to_str())
            .map(normalized_extension)
            .as_deref()
            != Some(required_extension.as_str())
    {
        return SavePickerDecision::ExtensionMismatch {
            target,
            required_extension,
        };
    }

    match snapshot.media {
        SaveMediaPlan::ReadyCache { source, .. } => SavePickerDecision::ExportReady {
            request: SaveExportRequest::new(snapshot.original_url, source, target),
            private_at_start: snapshot.private_at_start,
        },
        SaveMediaPlan::DownloadMatroska => SavePickerDecision::DownloadReady {
            original_url: snapshot.original_url,
            target,
            format_selector: snapshot.format_selector,
            private_at_start: snapshot.private_at_start,
        },
    }
}

/// An explicit Save may finish in private mode, but its URL-to-path metadata is
/// suppressible session state. Privacy at either edge wins so a mid-job toggle
/// cannot leak what was saved.
pub const fn should_persist_saved_mapping(
    private_at_start: bool,
    private_at_completion: bool,
) -> bool {
    !private_at_start && !private_at_completion
}

pub fn suggested_filename(title: &str, extension: &str) -> String {
    let mut stem = String::with_capacity(title.len().min(120));
    let mut previous_was_space = false;
    for character in title.trim().chars() {
        if stem.chars().count() >= 120 {
            break;
        }
        let replacement = if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
            ' '
        } else {
            character
        };
        if replacement.is_whitespace() {
            if !previous_was_space && !stem.is_empty() {
                stem.push(' ');
            }
            previous_was_space = true;
        } else {
            stem.push(replacement);
            previous_was_space = false;
        }
    }
    let stem = stem.trim_matches([' ', '.']);
    let stem = if stem.is_empty() { "video" } else { stem };
    let extension = normalized_extension(extension);
    if extension.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem}.{extension}")
    }
}

fn normalized_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

/// An owned snapshot of the Save action. Later player source changes cannot
/// alter the original URL, pinned source, or destination carried by a job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveExportRequest {
    pub original_url: String,
    pub source: PathBuf,
    pub target: PathBuf,
}

impl SaveExportRequest {
    pub fn new(
        original_url: impl Into<String>,
        source: impl Into<PathBuf>,
        target: impl Into<PathBuf>,
    ) -> Self {
        Self {
            original_url: original_url.into(),
            source: source.into(),
            target: target.into(),
        }
    }
}

/// Cooperative cancellation shared between a native Cancel action and the
/// copy worker. It owns only the export copy; downloader consumer ownership is
/// deliberately outside this type.
#[derive(Clone, Debug, Default)]
pub struct SaveExportCancellation {
    cancelled: Arc<AtomicBool>,
}

impl SaveExportCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SaveExportProgress {
    pub copied_bytes: u64,
    pub total_bytes: u64,
}

impl SaveExportProgress {
    pub fn fraction(self) -> f64 {
        if self.total_bytes == 0 {
            1.0
        } else {
            (self.copied_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedVideo {
    pub original_url: String,
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum SaveExportError {
    InvalidRequest(&'static str),
    SourceOpen(io::Error),
    SourceMetadata(io::Error),
    SourceNotFile,
    DestinationInspect(io::Error),
    DestinationExists,
    StagingCreate(io::Error),
    SourceRead(io::Error),
    DestinationWrite(io::Error),
    DestinationFlush(io::Error),
    DestinationSync(io::Error),
    Publish(io::Error),
    Cancelled,
}

impl SaveExportError {
    pub const fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidRequest(_) => "Choose a valid local destination",
            Self::SourceOpen(_) | Self::SourceMetadata(_) | Self::SourceNotFile => {
                "The completed video is no longer available"
            }
            Self::DestinationInspect(_)
            | Self::StagingCreate(_)
            | Self::DestinationWrite(_)
            | Self::DestinationFlush(_)
            | Self::DestinationSync(_)
            | Self::Publish(_) => "Could not write the saved video",
            Self::DestinationExists => "A file already exists at that destination",
            Self::SourceRead(_) => "Could not read the completed video",
            Self::Cancelled => "Save canceled",
        }
    }
}

impl fmt::Display for SaveExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(reason) => write!(formatter, "invalid save request: {reason}"),
            Self::SourceOpen(error) => write!(formatter, "could not open source: {error}"),
            Self::SourceMetadata(error) => {
                write!(formatter, "could not inspect source: {error}")
            }
            Self::SourceNotFile => formatter.write_str("source is not a regular file"),
            Self::DestinationInspect(error) => {
                write!(formatter, "could not inspect destination: {error}")
            }
            Self::DestinationExists => formatter.write_str("destination already exists"),
            Self::StagingCreate(error) => {
                write!(formatter, "could not create private staging file: {error}")
            }
            Self::SourceRead(error) => write!(formatter, "could not read source: {error}"),
            Self::DestinationWrite(error) => {
                write!(formatter, "could not write destination: {error}")
            }
            Self::DestinationFlush(error) => {
                write!(formatter, "could not flush destination: {error}")
            }
            Self::DestinationSync(error) => {
                write!(formatter, "could not sync destination: {error}")
            }
            Self::Publish(error) => write!(formatter, "could not publish destination: {error}"),
            Self::Cancelled => formatter.write_str("save cancelled"),
        }
    }
}

impl Error for SaveExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SourceOpen(error)
            | Self::SourceMetadata(error)
            | Self::DestinationInspect(error)
            | Self::StagingCreate(error)
            | Self::SourceRead(error)
            | Self::DestinationWrite(error)
            | Self::DestinationFlush(error)
            | Self::DestinationSync(error)
            | Self::Publish(error) => Some(error),
            Self::InvalidRequest(_)
            | Self::SourceNotFile
            | Self::DestinationExists
            | Self::Cancelled => None,
        }
    }
}

/// Copy a finalized source through one exclusively-created sibling staging
/// file and atomically publish it without replacing an existing target.
///
/// The caller must keep its cache/download pin alive for this call. A native
/// shell should invoke this function on a worker thread and forward progress
/// to its main loop.
pub fn export_saved_video(
    request: SaveExportRequest,
    cancellation: &SaveExportCancellation,
    mut report_progress: impl FnMut(SaveExportProgress),
) -> Result<SavedVideo, SaveExportError> {
    validate_request(&request)?;
    if cancellation.is_cancelled() {
        return Err(SaveExportError::Cancelled);
    }

    // Open exactly once. The cache pin supplied by the caller protects this
    // completed inode while the open handle prevents a path retarget during copy.
    let mut source = File::open(&request.source).map_err(SaveExportError::SourceOpen)?;
    let metadata = source.metadata().map_err(SaveExportError::SourceMetadata)?;
    if !metadata.is_file() {
        return Err(SaveExportError::SourceNotFile);
    }

    export_from_reader(
        request,
        &mut source,
        metadata.len(),
        cancellation,
        &mut report_progress,
    )
}

fn export_from_reader(
    request: SaveExportRequest,
    source: &mut impl Read,
    total_bytes: u64,
    cancellation: &SaveExportCancellation,
    report_progress: &mut impl FnMut(SaveExportProgress),
) -> Result<SavedVideo, SaveExportError> {
    reject_existing_destination(&request.target)?;
    let parent = destination_parent(&request.target);
    let prefix = staging_prefix(&request.target);
    let mut staging = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(".partial")
        .tempfile_in(parent)
        .map_err(SaveExportError::StagingCreate)?;

    let mut copied_bytes = 0_u64;
    report_progress(SaveExportProgress {
        copied_bytes,
        total_bytes,
    });

    let mut chunk = vec![0_u8; COPY_CHUNK_BYTES];
    loop {
        if cancellation.is_cancelled() {
            return Err(SaveExportError::Cancelled);
        }
        let read = source
            .read(&mut chunk)
            .map_err(SaveExportError::SourceRead)?;
        if read == 0 {
            break;
        }
        staging
            .as_file_mut()
            .write_all(&chunk[..read])
            .map_err(SaveExportError::DestinationWrite)?;
        copied_bytes = copied_bytes.saturating_add(read as u64);
        report_progress(SaveExportProgress {
            copied_bytes,
            total_bytes,
        });
    }

    if cancellation.is_cancelled() {
        return Err(SaveExportError::Cancelled);
    }
    staging
        .as_file_mut()
        .flush()
        .map_err(SaveExportError::DestinationFlush)?;
    staging
        .as_file()
        .sync_all()
        .map_err(SaveExportError::DestinationSync)?;
    if cancellation.is_cancelled() {
        return Err(SaveExportError::Cancelled);
    }

    match staging.persist_noclobber(&request.target) {
        Ok(file) => drop(file),
        Err(error) => {
            let tempfile::PersistError { error, file } = error;
            drop(file);
            return if error.kind() == io::ErrorKind::AlreadyExists {
                Err(SaveExportError::DestinationExists)
            } else {
                Err(SaveExportError::Publish(error))
            };
        }
    }

    Ok(SavedVideo {
        original_url: request.original_url,
        path: request.target,
        bytes: copied_bytes,
    })
}

fn validate_request(request: &SaveExportRequest) -> Result<(), SaveExportError> {
    if request.original_url.trim().is_empty() {
        return Err(SaveExportError::InvalidRequest("original URL is empty"));
    }
    if request.source.as_os_str().is_empty() {
        return Err(SaveExportError::InvalidRequest("source path is empty"));
    }
    if request.target.as_os_str().is_empty() || request.target.file_name().is_none() {
        return Err(SaveExportError::InvalidRequest(
            "destination has no file name",
        ));
    }
    Ok(())
}

fn reject_existing_destination(target: &Path) -> Result<(), SaveExportError> {
    match fs::symlink_metadata(target) {
        Ok(_) => Err(SaveExportError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SaveExportError::DestinationInspect(error)),
    }
}

fn destination_parent(target: &Path) -> &Path {
    target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn staging_prefix(target: &Path) -> String {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("video");
    format!(".{name}.ok-player-")
}

/// Minimal human-readable state linking an original public URL to the latest
/// completed user-owned file. The URL remains the map key; the saved path is
/// never substituted into watch History identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedVideoIndex {
    pub version: u32,
    #[serde(default)]
    pub videos: BTreeMap<String, SavedVideoRecord>,
}

impl Default for SavedVideoIndex {
    fn default() -> Self {
        Self {
            version: SAVED_VIDEO_INDEX_VERSION,
            videos: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedVideoRecord {
    pub path: PathBuf,
    pub saved_at_unix: i64,
}

impl SavedVideoIndex {
    pub fn load(raw: &str) -> Option<Self> {
        let index: Self = serde_json::from_str(raw).ok()?;
        (index.version == SAVED_VIDEO_INDEX_VERSION).then_some(index)
    }

    pub fn record(&mut self, saved: &SavedVideo, saved_at_unix: i64) {
        self.videos.insert(
            saved.original_url.clone(),
            SavedVideoRecord {
                path: saved.path.clone(),
                saved_at_unix,
            },
        );
    }

    pub fn saved_path(&self, original_url: &str) -> Option<&Path> {
        self.videos
            .get(original_url)
            .map(|record| record.path.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partials(parent: &Path) -> Vec<PathBuf> {
        fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".partial"))
            })
            .collect()
    }

    #[test]
    fn completed_copy_is_independent_of_cache_source_and_records_original_url() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cache-ready.webm");
        let target = root.path().join("chosen.webm");
        let payload = vec![0x5a; COPY_CHUNK_BYTES + 31];
        fs::write(&source, &payload).unwrap();
        let original_url = "https://video.example.test/watch/794";
        let request = SaveExportRequest::new(original_url, &source, &target);
        let mut progress = Vec::new();

        let saved = export_saved_video(request, &SaveExportCancellation::default(), |sample| {
            progress.push(sample)
        })
        .unwrap();
        fs::remove_file(&source).unwrap();

        assert_eq!(fs::read(&target).unwrap(), payload);
        assert_eq!(saved.original_url, original_url);
        assert_eq!(saved.path, target);
        assert_eq!(saved.bytes, (COPY_CHUNK_BYTES + 31) as u64);
        assert_eq!(progress.first().unwrap().copied_bytes, 0);
        assert_eq!(progress.last().unwrap().fraction(), 1.0);

        let mut index = SavedVideoIndex::default();
        index.record(&saved, 1_700_000_794);
        let json = serde_json::to_string_pretty(&index).unwrap();
        let loaded = SavedVideoIndex::load(&json).unwrap();
        assert_eq!(loaded.saved_path(original_url), Some(target.as_path()));
        assert!(!loaded.videos.contains_key(target.to_str().unwrap()));
    }

    #[test]
    fn cancellation_removes_only_the_jobs_private_staging_file() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cache-ready.mkv");
        let target = root.path().join("chosen.mkv");
        let unrelated = root.path().join(".chosen.mkv.other.partial");
        fs::write(&source, vec![7; COPY_CHUNK_BYTES * 2]).unwrap();
        fs::write(&unrelated, b"keep me").unwrap();
        let cancellation = SaveExportCancellation::default();
        let cancel_from_progress = cancellation.clone();

        let result = export_saved_video(
            SaveExportRequest::new("https://example.test/video", &source, &target),
            &cancellation,
            move |sample| {
                if sample.copied_bytes > 0 {
                    cancel_from_progress.cancel();
                }
            },
        );

        assert!(matches!(result, Err(SaveExportError::Cancelled)));
        assert!(!target.exists());
        assert_eq!(fs::read(&unrelated).unwrap(), b"keep me");
        assert_eq!(partials(root.path()), vec![unrelated]);
        assert!(source.exists());
    }

    #[test]
    fn existing_destination_is_never_replaced_or_removed() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cache.mp4");
        let target = root.path().join("movie.mp4");
        fs::write(&source, b"new video").unwrap();
        fs::write(&target, b"original file").unwrap();

        let result = export_saved_video(
            SaveExportRequest::new("https://example.test/video", &source, &target),
            &SaveExportCancellation::default(),
            |_| {},
        );

        assert!(matches!(result, Err(SaveExportError::DestinationExists)));
        assert_eq!(fs::read(&target).unwrap(), b"original file");
        assert!(partials(root.path()).is_empty());
    }

    #[test]
    fn destination_appearing_after_copy_wins_without_being_replaced() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cache.mp4");
        let target = root.path().join("movie.mp4");
        fs::write(&source, b"new video").unwrap();
        let racing_target = target.clone();

        let result = export_saved_video(
            SaveExportRequest::new("https://example.test/video", &source, &target),
            &SaveExportCancellation::default(),
            move |sample| {
                if sample.total_bytes > 0
                    && sample.copied_bytes == sample.total_bytes
                    && !racing_target.exists()
                {
                    fs::write(&racing_target, b"racing file").unwrap();
                }
            },
        );

        assert!(matches!(result, Err(SaveExportError::DestinationExists)));
        assert_eq!(fs::read(&target).unwrap(), b"racing file");
        assert!(partials(root.path()).is_empty());
        assert!(source.exists());
    }

    #[test]
    fn source_directory_is_rejected_without_touching_other_files() {
        let root = tempfile::tempdir().unwrap();
        let source_directory = root.path().join("not-media");
        let target = root.path().join("movie.mkv");
        let unrelated = root.path().join("notes.txt");
        fs::create_dir(&source_directory).unwrap();
        fs::write(&unrelated, b"keep me").unwrap();

        let result = export_saved_video(
            SaveExportRequest::new("https://example.test/video", &source_directory, &target),
            &SaveExportCancellation::default(),
            |_| {},
        );

        assert!(matches!(result, Err(SaveExportError::SourceNotFile)));
        assert!(!target.exists());
        assert_eq!(fs::read(&unrelated).unwrap(), b"keep me");
        assert!(partials(root.path()).is_empty());
    }

    #[test]
    fn copy_read_failure_cleans_owned_staging_and_keeps_unrelated_files() {
        struct FailingReader {
            delivered_first_chunk: bool,
        }

        impl Read for FailingReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.delivered_first_chunk {
                    return Err(io::Error::other("injected read failure"));
                }
                self.delivered_first_chunk = true;
                buffer[..4].copy_from_slice(b"data");
                Ok(4)
            }
        }

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("movie.webm");
        let unrelated = root.path().join("notes.txt");
        fs::write(&unrelated, b"keep me").unwrap();
        let mut reader = FailingReader {
            delivered_first_chunk: false,
        };
        let mut ignore_progress = |_| {};

        let result = export_from_reader(
            SaveExportRequest::new("https://example.test/video", "pinned.webm", &target),
            &mut reader,
            8,
            &SaveExportCancellation::default(),
            &mut ignore_progress,
        );

        assert!(matches!(result, Err(SaveExportError::SourceRead(_))));
        assert!(!target.exists());
        assert_eq!(fs::read(&unrelated).unwrap(), b"keep me");
        assert!(partials(root.path()).is_empty());
    }

    #[test]
    fn request_snapshot_cannot_be_retargeted_by_later_source_state() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.webm");
        let second = root.path().join("second.webm");
        let target = root.path().join("saved.webm");
        fs::write(&first, b"first snapshot").unwrap();
        fs::write(&second, b"later playback").unwrap();
        let current_url = String::from("https://example.test/first");
        let request = SaveExportRequest::new(current_url.clone(), &first, &target);

        let _later_player_source = (String::from("https://example.test/second"), second);
        let saved =
            export_saved_video(request, &SaveExportCancellation::default(), |_| {}).unwrap();

        assert_eq!(saved.original_url, current_url);
        assert_eq!(fs::read(target).unwrap(), b"first snapshot");
    }

    #[test]
    fn saved_index_rejects_unknown_versions() {
        assert!(SavedVideoIndex::load(r#"{"version":99,"videos":{}}"#).is_none());
    }

    #[test]
    fn picker_cancel_and_failure_authorize_no_save_work() {
        let snapshot = SaveVideoSnapshot {
            original_url: "https://example.test/video".to_owned(),
            title: "Video".to_owned(),
            format_selector: None,
            private_at_start: false,
            media: SaveMediaPlan::DownloadMatroska,
        };

        assert_eq!(
            decide_after_picker(snapshot.clone(), SavePickerOutcome::Cancelled),
            SavePickerDecision::NoWork
        );
        assert_eq!(
            decide_after_picker(
                snapshot,
                SavePickerOutcome::Failed("portal unavailable".to_owned())
            ),
            SavePickerDecision::PickerFailed("portal unavailable".to_owned())
        );
    }

    #[test]
    fn ready_cache_is_exported_without_authorizing_a_download() {
        let snapshot = SaveVideoSnapshot {
            original_url: "https://example.test/video".to_owned(),
            title: "A / B".to_owned(),
            format_selector: Some("best[height<=1080]".to_owned()),
            private_at_start: false,
            media: SaveMediaPlan::ReadyCache {
                source: PathBuf::from("/cache/ready.webm"),
                extension: "WEBM".to_owned(),
            },
        };
        assert_eq!(snapshot.suggested_filename(), "A B.webm");

        assert_eq!(
            decide_after_picker(
                snapshot,
                SavePickerOutcome::Selected(PathBuf::from("/videos/chosen.WeBm"))
            ),
            SavePickerDecision::ExportReady {
                request: SaveExportRequest::new(
                    "https://example.test/video",
                    "/cache/ready.webm",
                    "/videos/chosen.WeBm"
                ),
                private_at_start: false
            }
        );
    }

    #[test]
    fn uncached_save_requests_matroska_only_after_picker_accepts() {
        let snapshot = SaveVideoSnapshot {
            original_url: "https://example.test/video".to_owned(),
            title: "Public: video?".to_owned(),
            format_selector: Some("bestvideo*+bestaudio/best".to_owned()),
            private_at_start: true,
            media: SaveMediaPlan::DownloadMatroska,
        };
        assert_eq!(snapshot.suggested_filename(), "Public video.mkv");

        assert_eq!(
            decide_after_picker(
                snapshot,
                SavePickerOutcome::Selected(PathBuf::from("/videos/public.mkv"))
            ),
            SavePickerDecision::DownloadReady {
                original_url: "https://example.test/video".to_owned(),
                target: PathBuf::from("/videos/public.mkv"),
                format_selector: Some("bestvideo*+bestaudio/best".to_owned()),
                private_at_start: true
            }
        );
    }

    #[test]
    fn picker_extension_must_match_the_finalized_container() {
        let snapshot = SaveVideoSnapshot {
            original_url: "https://example.test/video".to_owned(),
            title: "Video".to_owned(),
            format_selector: None,
            private_at_start: false,
            media: SaveMediaPlan::DownloadMatroska,
        };

        assert_eq!(
            decide_after_picker(
                snapshot,
                SavePickerOutcome::Selected(PathBuf::from("/videos/not-really.mp4"))
            ),
            SavePickerDecision::ExtensionMismatch {
                target: PathBuf::from("/videos/not-really.mp4"),
                required_extension: "mkv".to_owned()
            }
        );
    }

    #[test]
    fn privacy_at_either_job_edge_suppresses_only_saved_path_metadata() {
        assert!(should_persist_saved_mapping(false, false));
        assert!(!should_persist_saved_mapping(true, false));
        assert!(!should_persist_saved_mapping(false, true));
        assert!(!should_persist_saved_mapping(true, true));
    }
}
