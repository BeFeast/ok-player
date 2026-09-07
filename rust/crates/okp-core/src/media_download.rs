//! Portable contract for complete online-media downloads.
//!
//! This module deliberately performs no process or filesystem work. A native shell owns the
//! `yt-dlp`/`ffmpeg` child process and reports its progress through these values. Replay-cache
//! downloads and explicit user-owned saves share this contract without sharing ownership: only
//! the replay-cache caller may promote a completed result into cache storage.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stable identity assigned by the native downloader to one accepted request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DownloadJobId(pub u64);

/// Who owns the completed bytes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadPurpose {
    /// Temporary output intended for [`crate::replay_cache::ReplayCache`] promotion.
    ReplayCache,
    /// Temporary output that a Save caller will move to a user-selected destination.
    UserSave,
}

/// Container policy requested from the native adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadContainer {
    /// Keep the real container selected by the extractor and report its real suffix.
    Preserve,
    /// Remux to Matroska so a destination selected may truthfully use `.mkv`.
    Matroska,
}

/// An owned working directory and safe output stem supplied to the native adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DownloadTarget {
    pub staging_directory: PathBuf,
    pub file_stem: String,
}

impl DownloadTarget {
    /// Construct a target whose file stem cannot escape its staging directory.
    pub fn new(
        staging_directory: impl Into<PathBuf>,
        file_stem: impl Into<String>,
    ) -> Result<Self, DownloadRejection> {
        let target = Self {
            staging_directory: staging_directory.into(),
            file_stem: file_stem.into(),
        };
        target.validate()?;
        Ok(target)
    }

    pub fn validate(&self) -> Result<(), DownloadRejection> {
        if self.staging_directory.as_os_str().is_empty() || !safe_file_stem(&self.file_stem) {
            return Err(DownloadRejection::UnsafeTarget);
        }
        Ok(())
    }
}

/// A request for one complete public on-demand item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MediaDownloadRequest {
    pub source_url: String,
    pub target: DownloadTarget,
    pub purpose: DownloadPurpose,
    pub container: DownloadContainer,
    /// Hard upper bound for bytes owned by this request while it is staged.
    pub max_bytes: u64,
    #[serde(default)]
    pub format_selector: Option<String>,
}

impl MediaDownloadRequest {
    /// Validate the inputs that can be established before the native extractor runs.
    ///
    /// Live and playlist classification remains a native probe result and is represented by
    /// [`DownloadRejection::LiveStream`] and [`DownloadRejection::Playlist`] in the terminal
    /// outcome. No cookies, browser profile, or authentication input exists in this contract.
    pub fn public_vod(
        source_url: impl Into<String>,
        target: DownloadTarget,
        purpose: DownloadPurpose,
        container: DownloadContainer,
        max_bytes: u64,
    ) -> Result<Self, DownloadRejection> {
        let source_url = source_url.into();
        validate_public_http_url(&source_url)?;
        target.validate()?;
        if max_bytes == 0 {
            return Err(DownloadRejection::ZeroByteLimit);
        }
        Ok(Self {
            source_url,
            target,
            purpose,
            container,
            max_bytes,
            format_selector: None,
        })
    }
}

/// A request the native adapter must not download.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadRejection {
    /// The URL has no usable authority or contains whitespace/embedded credentials.
    InvalidUrl,
    /// Only public HTTP(S) sources are in scope.
    UnsupportedScheme,
    /// The staging directory or stem is unsafe.
    UnsafeTarget,
    /// A bounded request must reserve at least one byte.
    ZeroByteLimit,
    /// The extractor classified the item as live.
    LiveStream,
    /// The extractor classified the input as a playlist rather than one item.
    Playlist,
}

impl fmt::Display for DownloadRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidUrl => "the source is not a public URL",
            Self::UnsupportedScheme => "only HTTP and HTTPS downloads are supported",
            Self::UnsafeTarget => "the download target is unsafe",
            Self::ZeroByteLimit => "the download byte limit must be greater than zero",
            Self::LiveStream => "live streams are not downloaded",
            Self::Playlist => "playlists are not downloaded",
        })
    }
}

impl std::error::Error for DownloadRejection {}

/// Downloader admission failure when another request owns the single native worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadBusy {
    pub active_job_id: DownloadJobId,
}

impl fmt::Display for DownloadBusy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "media download job {} is already active",
            self.active_job_id.0
        )
    }
}

impl std::error::Error for DownloadBusy {}

/// Monotonic byte progress for an accepted job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MediaDownloadProgress {
    pub job_id: DownloadJobId,
    pub downloaded_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

impl MediaDownloadProgress {
    /// A bounded percentage for presentation, when the extractor supplied a non-zero total.
    pub fn percent(&self) -> Option<u8> {
        let total = self.total_bytes.filter(|total| *total > 0)?;
        let percent = self.downloaded_bytes.saturating_mul(100) / total;
        Some(percent.min(100) as u8)
    }
}

/// A successfully downloaded, fully closed media file in caller-owned staging.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DownloadedMedia {
    pub path: PathBuf,
    pub extension: String,
    pub byte_len: u64,
}

impl DownloadedMedia {
    /// Build a completed result from its actual path and byte length.
    pub fn new(path: impl Into<PathBuf>, byte_len: u64) -> Result<Self, DownloadRejection> {
        let path = path.into();
        let extension = normalized_extension(&path).ok_or(DownloadRejection::UnsafeTarget)?;
        Ok(Self {
            path,
            extension,
            byte_len,
        })
    }

    /// Validate that the reported suffix is normalized and agrees with the actual path.
    pub fn validate(&self) -> Result<(), DownloadRejection> {
        if self.byte_len == 0 || self.path.file_name().is_none() {
            return Err(DownloadRejection::UnsafeTarget);
        }
        let Some(actual) = normalized_extension(&self.path) else {
            return Err(DownloadRejection::UnsafeTarget);
        };
        if actual != self.extension || !safe_extension(&self.extension) {
            return Err(DownloadRejection::UnsafeTarget);
        }
        Ok(())
    }
}

/// The single terminal result of an accepted request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", content = "detail", rename_all = "kebab-case")]
pub enum MediaDownloadOutcome {
    Completed(DownloadedMedia),
    Cancelled,
    Rejected(DownloadRejection),
    Failed { message: String },
}

/// An event emitted by the asynchronous native downloader.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum MediaDownloadEvent {
    Started {
        job_id: DownloadJobId,
    },
    Progress(MediaDownloadProgress),
    Terminal {
        job_id: DownloadJobId,
        outcome: MediaDownloadOutcome,
    },
}

/// Pure one-job lifecycle helper used by adapters to suppress stale or duplicate events.
#[derive(Clone, Debug, Default)]
pub struct MediaDownloadLifecycle {
    active: Option<ActiveDownload>,
}

#[derive(Clone, Debug)]
struct ActiveDownload {
    job_id: DownloadJobId,
    downloaded_bytes: u64,
}

impl MediaDownloadLifecycle {
    pub fn active_job_id(&self) -> Option<DownloadJobId> {
        self.active.as_ref().map(|active| active.job_id)
    }

    pub fn begin(&mut self, job_id: DownloadJobId) -> Result<MediaDownloadEvent, DownloadBusy> {
        if let Some(active) = &self.active {
            return Err(DownloadBusy {
                active_job_id: active.job_id,
            });
        }
        self.active = Some(ActiveDownload {
            job_id,
            downloaded_bytes: 0,
        });
        Ok(MediaDownloadEvent::Started { job_id })
    }

    /// Record progress for the active job. Stale IDs are ignored and byte counts never regress.
    pub fn progress(
        &mut self,
        job_id: DownloadJobId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    ) -> Option<MediaDownloadEvent> {
        let active = self
            .active
            .as_mut()
            .filter(|active| active.job_id == job_id)?;
        active.downloaded_bytes = active.downloaded_bytes.max(downloaded_bytes);
        Some(MediaDownloadEvent::Progress(MediaDownloadProgress {
            job_id,
            downloaded_bytes: active.downloaded_bytes,
            total_bytes,
        }))
    }

    /// Finish the active job exactly once. Stale and duplicate terminals are ignored.
    pub fn finish(
        &mut self,
        job_id: DownloadJobId,
        outcome: MediaDownloadOutcome,
    ) -> Option<MediaDownloadEvent> {
        if self.active.as_ref().map(|active| active.job_id) != Some(job_id) {
            return None;
        }
        self.active = None;
        Some(MediaDownloadEvent::Terminal { job_id, outcome })
    }
}

pub(crate) fn validate_public_http_url(url: &str) -> Result<(), DownloadRejection> {
    if url.is_empty() || url.trim() != url || url.chars().any(char::is_whitespace) {
        return Err(DownloadRejection::InvalidUrl);
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(DownloadRejection::InvalidUrl);
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(DownloadRejection::UnsupportedScheme);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return Err(DownloadRejection::InvalidUrl);
    }
    Ok(())
}

pub(crate) fn safe_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 16
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn normalized_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    safe_extension(&extension).then_some(extension)
}

fn safe_file_stem(stem: &str) -> bool {
    if stem.is_empty() || stem == "." || stem == ".." {
        return false;
    }
    let mut components = Path::new(stem).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> DownloadTarget {
        DownloadTarget::new("/tmp/ok-player-job", "media").expect("safe target")
    }

    #[test]
    fn public_vod_accepts_only_credential_free_http_urls_and_a_bounded_target() {
        for url in [
            "https://example.test/watch?v=one#chapter",
            "http://127.0.0.1:8080/video.mp4",
        ] {
            assert!(
                MediaDownloadRequest::public_vod(
                    url,
                    target(),
                    DownloadPurpose::ReplayCache,
                    DownloadContainer::Preserve,
                    1024,
                )
                .is_ok(),
                "{url}"
            );
        }

        let cases = [
            (
                "file:///tmp/movie.mp4",
                DownloadRejection::UnsupportedScheme,
            ),
            ("smb://nas/movie.mkv", DownloadRejection::UnsupportedScheme),
            (
                "https://user:secret@example.test/video",
                DownloadRejection::InvalidUrl,
            ),
            ("https://example.test/a b", DownloadRejection::InvalidUrl),
            ("not-a-url", DownloadRejection::InvalidUrl),
        ];
        for (url, expected) in cases {
            assert_eq!(
                MediaDownloadRequest::public_vod(
                    url,
                    target(),
                    DownloadPurpose::ReplayCache,
                    DownloadContainer::Preserve,
                    1024,
                ),
                Err(expected),
                "{url}"
            );
        }
    }

    #[test]
    fn targets_cannot_escape_the_owned_staging_directory() {
        for stem in ["", ".", "..", "../media", "folder/media", "media.mkv"] {
            assert_eq!(
                DownloadTarget::new("/tmp/job", stem),
                Err(DownloadRejection::UnsafeTarget),
                "{stem}"
            );
        }
        assert_eq!(
            MediaDownloadRequest::public_vod(
                "https://example.test/video",
                target(),
                DownloadPurpose::ReplayCache,
                DownloadContainer::Preserve,
                0,
            ),
            Err(DownloadRejection::ZeroByteLimit)
        );
    }

    #[test]
    fn downloaded_media_reports_the_truthful_normalized_suffix() {
        let media = DownloadedMedia::new("/tmp/job/media.MP4", 42).expect("valid result");
        assert_eq!(media.extension, "mp4");
        assert!(media.validate().is_ok());

        let mut renamed_only = media;
        renamed_only.extension = "mkv".to_owned();
        assert_eq!(
            renamed_only.validate(),
            Err(DownloadRejection::UnsafeTarget)
        );
        assert!(DownloadedMedia::new("/tmp/job/media", 42).is_err());
    }

    #[test]
    fn lifecycle_allows_one_job_monotonic_progress_and_one_terminal() {
        let mut lifecycle = MediaDownloadLifecycle::default();
        assert_eq!(
            lifecycle.begin(DownloadJobId(7)),
            Ok(MediaDownloadEvent::Started {
                job_id: DownloadJobId(7)
            })
        );
        assert_eq!(
            lifecycle.begin(DownloadJobId(8)),
            Err(DownloadBusy {
                active_job_id: DownloadJobId(7)
            })
        );
        assert!(
            lifecycle
                .progress(DownloadJobId(99), 10, Some(100))
                .is_none()
        );
        assert_eq!(
            lifecycle.progress(DownloadJobId(7), 60, Some(100)),
            Some(MediaDownloadEvent::Progress(MediaDownloadProgress {
                job_id: DownloadJobId(7),
                downloaded_bytes: 60,
                total_bytes: Some(100),
            }))
        );
        assert_eq!(
            lifecycle.progress(DownloadJobId(7), 20, Some(100)),
            Some(MediaDownloadEvent::Progress(MediaDownloadProgress {
                job_id: DownloadJobId(7),
                downloaded_bytes: 60,
                total_bytes: Some(100),
            }))
        );
        let terminal = lifecycle.finish(DownloadJobId(7), MediaDownloadOutcome::Cancelled);
        assert!(matches!(
            terminal,
            Some(MediaDownloadEvent::Terminal { .. })
        ));
        assert!(
            lifecycle
                .finish(DownloadJobId(7), MediaDownloadOutcome::Cancelled)
                .is_none()
        );
        assert_eq!(lifecycle.active_job_id(), None);
    }

    #[test]
    fn progress_percent_is_bounded_and_requires_a_known_total() {
        let progress = |downloaded_bytes, total_bytes| MediaDownloadProgress {
            job_id: DownloadJobId(1),
            downloaded_bytes,
            total_bytes,
        };
        assert_eq!(progress(25, Some(100)).percent(), Some(25));
        assert_eq!(progress(200, Some(100)).percent(), Some(100));
        assert_eq!(progress(1, Some(0)).percent(), None);
        assert_eq!(progress(1, None).percent(), None);
    }
}
