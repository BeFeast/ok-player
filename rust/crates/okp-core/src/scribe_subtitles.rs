//! Reserved state model for the Scribe "Generate subtitles" flow.
//!
//! The transport contract is not implemented yet. This module only models the UI-visible states
//! and the eventual SRT sidecar handoff so shells can reserve the IA without making network calls.

use std::path::{Path, PathBuf};

use crate::srt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScribeSubtitleConfig {
    pub backend: ScribeSubtitleBackend,
}

impl Default for ScribeSubtitleConfig {
    fn default() -> Self {
        Self {
            backend: ScribeSubtitleBackend::Unconfigured,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScribeSubtitleBackend {
    Unconfigured,
    Unsupported { endpoint: String },
    Supported { endpoint: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScribeSubtitleRequest {
    pub media_path: PathBuf,
    pub sidecar_path: PathBuf,
}

impl ScribeSubtitleRequest {
    pub fn for_media_path(media_path: impl Into<PathBuf>) -> Option<Self> {
        let media_path = media_path.into();
        let sidecar_path = generated_srt_sidecar_path(&media_path)?;
        Some(Self {
            media_path,
            sidecar_path,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSrtTrack {
    pub path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScribeSubtitleJob {
    Queued {
        request: ScribeSubtitleRequest,
    },
    InProgress {
        request: ScribeSubtitleRequest,
        progress_percent: Option<u8>,
    },
    Ready {
        track: GeneratedSrtTrack,
    },
    Failed {
        message: String,
    },
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScribeSubtitleBeginError {
    Unconfigured,
    UnsupportedBackend,
    NoLocalMedia,
}

impl ScribeSubtitleBeginError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Unconfigured => "Scribe subtitles are coming soon",
            Self::UnsupportedBackend => "Configured Scribe backend is not supported yet",
            Self::NoLocalMedia => "Generate subtitles needs a local media file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScribeSubtitleState {
    config: ScribeSubtitleConfig,
    job: Option<ScribeSubtitleJob>,
}

impl ScribeSubtitleState {
    pub fn new(config: ScribeSubtitleConfig) -> Self {
        Self { config, job: None }
    }

    pub fn config(&self) -> &ScribeSubtitleConfig {
        &self.config
    }

    pub fn job(&self) -> Option<&ScribeSubtitleJob> {
        self.job.as_ref()
    }

    pub fn begin(
        &mut self,
        request: Option<ScribeSubtitleRequest>,
    ) -> Result<&ScribeSubtitleJob, ScribeSubtitleBeginError> {
        match &self.config.backend {
            ScribeSubtitleBackend::Unconfigured => {
                return Err(ScribeSubtitleBeginError::Unconfigured);
            }
            ScribeSubtitleBackend::Unsupported { .. } => {
                return Err(ScribeSubtitleBeginError::UnsupportedBackend);
            }
            ScribeSubtitleBackend::Supported { .. } => {}
        }

        let Some(request) = request else {
            return Err(ScribeSubtitleBeginError::NoLocalMedia);
        };
        self.job = Some(ScribeSubtitleJob::Queued { request });
        Ok(self.job.as_ref().expect("queued job"))
    }

    pub fn mark_in_progress(&mut self, progress_percent: Option<u8>) -> Option<&ScribeSubtitleJob> {
        let request = self.current_request()?.clone();
        self.job = Some(ScribeSubtitleJob::InProgress {
            request,
            progress_percent: progress_percent.map(|value| value.min(100)),
        });
        self.job.as_ref()
    }

    pub fn cancel(&mut self) {
        self.job = Some(ScribeSubtitleJob::Canceled);
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.job = Some(ScribeSubtitleJob::Failed {
            message: message.into(),
        });
    }

    pub fn complete_with_srt(
        &mut self,
        contents: impl Into<String>,
    ) -> Result<&GeneratedSrtTrack, ScribeSubtitleCompleteError> {
        let request = self
            .current_request()
            .cloned()
            .ok_or(ScribeSubtitleCompleteError::NoActiveRequest)?;
        let contents = contents.into();
        if srt::parse(Some(&contents)).is_empty() {
            self.fail("Scribe returned no subtitle cues");
            return Err(ScribeSubtitleCompleteError::EmptySrt);
        }

        self.job = Some(ScribeSubtitleJob::Ready {
            track: GeneratedSrtTrack {
                path: request.sidecar_path,
                contents,
            },
        });
        match self.job.as_ref().expect("ready job") {
            ScribeSubtitleJob::Ready { track } => Ok(track),
            _ => unreachable!("job was just set to ready"),
        }
    }

    fn current_request(&self) -> Option<&ScribeSubtitleRequest> {
        match self.job.as_ref()? {
            ScribeSubtitleJob::Queued { request }
            | ScribeSubtitleJob::InProgress { request, .. } => Some(request),
            ScribeSubtitleJob::Ready { .. }
            | ScribeSubtitleJob::Failed { .. }
            | ScribeSubtitleJob::Canceled => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScribeSubtitleCompleteError {
    NoActiveRequest,
    EmptySrt,
}

pub fn generated_srt_sidecar_path(media_path: &Path) -> Option<PathBuf> {
    if media_path.as_os_str().is_empty() || media_path.to_string_lossy().contains("://") {
        return None;
    }
    media_path.file_name()?;

    let mut candidate = media_path.to_path_buf();
    candidate.set_extension("scribe.srt");
    Some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_state() -> ScribeSubtitleState {
        ScribeSubtitleState::new(ScribeSubtitleConfig {
            backend: ScribeSubtitleBackend::Supported {
                endpoint: "https://scribe.example.test".to_owned(),
            },
        })
    }

    fn request() -> ScribeSubtitleRequest {
        ScribeSubtitleRequest::for_media_path("/media/Movie.mkv").expect("local media request")
    }

    #[test]
    fn default_state_is_disabled_without_network_side_effects() {
        let mut state = ScribeSubtitleState::default();

        assert_eq!(
            state.begin(Some(request())),
            Err(ScribeSubtitleBeginError::Unconfigured)
        );
        assert_eq!(state.job(), None);
    }

    #[test]
    fn unsupported_config_is_unavailable() {
        let mut state = ScribeSubtitleState::new(ScribeSubtitleConfig {
            backend: ScribeSubtitleBackend::Unsupported {
                endpoint: "https://scribe.oklabs.uk".to_owned(),
            },
        });

        assert_eq!(
            state.begin(Some(request())),
            Err(ScribeSubtitleBeginError::UnsupportedBackend)
        );
        assert_eq!(state.job(), None);
    }

    #[test]
    fn supported_backend_enters_queued_then_progress_placeholder() {
        let mut state = supported_state();
        let request = request();

        assert!(matches!(
            state.begin(Some(request.clone())),
            Ok(ScribeSubtitleJob::Queued { request: queued }) if queued == &request
        ));
        assert!(matches!(
            state.mark_in_progress(Some(125)),
            Some(ScribeSubtitleJob::InProgress {
                request: progress_request,
                progress_percent: Some(100),
            }) if progress_request == &request
        ));
    }

    #[test]
    fn cancel_records_cancelled_state() {
        let mut state = supported_state();
        state.begin(Some(request())).expect("queue job");

        state.cancel();

        assert_eq!(state.job(), Some(&ScribeSubtitleJob::Canceled));
        assert_eq!(
            state.complete_with_srt("1\n00:00:01,000 --> 00:00:02,000\nHello\n"),
            Err(ScribeSubtitleCompleteError::NoActiveRequest)
        );
    }

    #[test]
    fn successful_srt_becomes_external_track_payload() {
        let mut state = supported_state();
        state.begin(Some(request())).expect("queue job");
        state.mark_in_progress(None);

        let track = state
            .complete_with_srt("1\n00:00:01,000 --> 00:00:02,000\nHello\n")
            .expect("valid srt");

        assert_eq!(track.path, PathBuf::from("/media/Movie.scribe.srt"));
        assert!(track.contents.contains("Hello"));
        assert!(matches!(state.job(), Some(ScribeSubtitleJob::Ready { .. })));
    }

    #[test]
    fn empty_srt_records_error_state() {
        let mut state = supported_state();
        state.begin(Some(request())).expect("queue job");

        assert_eq!(
            state.complete_with_srt("not a subtitle file"),
            Err(ScribeSubtitleCompleteError::EmptySrt)
        );
        assert_eq!(
            state.job(),
            Some(&ScribeSubtitleJob::Failed {
                message: "Scribe returned no subtitle cues".to_owned()
            })
        );
    }

    #[test]
    fn sidecar_path_uses_normal_srt_extension_pipeline() {
        assert_eq!(
            generated_srt_sidecar_path(Path::new("/media/Movie.mkv")),
            Some(PathBuf::from("/media/Movie.scribe.srt"))
        );
        assert_eq!(
            generated_srt_sidecar_path(Path::new("https://example.test/movie.mkv")),
            None
        );
    }
}
