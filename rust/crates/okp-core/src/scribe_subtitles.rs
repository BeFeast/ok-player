//! Reserved state model for the Scribe "Generate subtitles" flow.
//!
//! The service contract and transport do not exist in OK Player yet. This module owns the
//! portable availability/job transitions and the eventual SRT sidecar handoff so shells can
//! reserve the permanent UI entry without issuing a network request or inventing shell-local
//! business logic.

use std::path::{Path, PathBuf};

use crate::{srt, subtitle_import::ExternalSubtitleImport};

/// Optional Scribe backend configuration.
///
/// The shipped reservation uses [`Self::default`], which has no endpoint. A future transport may
/// construct a supported backend only after its service contract is implemented.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScribeSubtitleConfig {
    backend: Option<ScribeSubtitleBackend>,
}

impl ScribeSubtitleConfig {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            backend: Some(ScribeSubtitleBackend::Unsupported),
        }
    }

    /// Construct configuration for a transport whose contract this build supports.
    ///
    /// This does not perform a request. Future transport code must still obtain authorization
    /// from [`ScribeSubtitleState::network_endpoint`] immediately before using the endpoint.
    #[must_use]
    pub fn supported(endpoint: impl Into<String>) -> Self {
        let endpoint = endpoint.into().trim().to_owned();
        Self {
            backend: Some(ScribeSubtitleBackend::Supported { endpoint }),
        }
    }

    fn backend(&self) -> Option<&ScribeSubtitleBackend> {
        self.backend.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScribeSubtitleBackend {
    /// A backend was selected, but this build has no compatible service contract.
    Unsupported,
    /// A future build has an implemented, compatible transport for this endpoint.
    Supported { endpoint: String },
}

/// Local inputs that decide whether generation may start and how its permanent UI row renders.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScribeSubtitleContext {
    pub private_session: bool,
    pub request: Option<ScribeSubtitleRequest>,
}

/// The local media and canonical sidecar destination retained for a generation job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScribeSubtitleRequest {
    media_path: PathBuf,
    sidecar_path: PathBuf,
}

impl ScribeSubtitleRequest {
    #[must_use]
    pub fn for_media_path(media_path: impl Into<PathBuf>) -> Option<Self> {
        let media_path = media_path.into();
        let sidecar_path = generated_srt_sidecar_path(&media_path)?;
        Some(Self {
            media_path,
            sidecar_path,
        })
    }

    #[must_use]
    pub fn media_path(&self) -> &Path {
        &self.media_path
    }

    #[must_use]
    pub fn sidecar_path(&self) -> &Path {
        &self.sidecar_path
    }
}

/// A completed SRT payload ready to be persisted next to the media.
///
/// Once the caller saves [`Self::contents`] to [`Self::path`], [`Self::external_import`] feeds the
/// result into the same external-subtitle loader used by local and downloaded subtitle files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedSrtTrack {
    path: PathBuf,
    contents: String,
}

impl GeneratedSrtTrack {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn contents(&self) -> &str {
        &self.contents
    }

    #[must_use]
    pub fn external_import(&self) -> ExternalSubtitleImport {
        ExternalSubtitleImport::scribe(self.path.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
        request: ScribeSubtitleRequest,
        message: String,
    },
    Canceled {
        request: ScribeSubtitleRequest,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScribeSubtitleBeginError {
    NotConfigured,
    UnsupportedBackend,
    PrivateSession,
    NoLocalMedia,
    AlreadyActive,
}

impl ScribeSubtitleBeginError {
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotConfigured => {
                "Scribe subtitle generation is coming later. No network request will be made."
            }
            Self::UnsupportedBackend => {
                "The configured Scribe backend is not supported. No network request will be made."
            }
            Self::PrivateSession => {
                "Scribe subtitle generation is unavailable in a private session."
            }
            Self::NoLocalMedia => "Open a local media file before generating subtitles.",
            Self::AlreadyActive => "Subtitle generation is already running.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScribeSubtitleCompleteError {
    NoActiveRequest,
    EmptySrt,
}

/// Shell-facing projection for the permanent Generate subtitles row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScribeSubtitlePresentation {
    pub label: String,
    pub badge: String,
    pub message: String,
    pub can_generate: bool,
    pub can_cancel: bool,
    pub show_progress: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScribeSubtitleState {
    config: ScribeSubtitleConfig,
    job: Option<ScribeSubtitleJob>,
}

impl ScribeSubtitleState {
    #[must_use]
    pub const fn new(config: ScribeSubtitleConfig) -> Self {
        Self { config, job: None }
    }

    #[must_use]
    pub fn job(&self) -> Option<&ScribeSubtitleJob> {
        self.job.as_ref()
    }

    /// Start a local placeholder job. No transport is invoked here.
    pub fn begin(
        &mut self,
        context: ScribeSubtitleContext,
    ) -> Result<&ScribeSubtitleJob, ScribeSubtitleBeginError> {
        if self.active_request().is_some() {
            return Err(ScribeSubtitleBeginError::AlreadyActive);
        }
        self.validate_context(&context)?;
        let request = context
            .request
            .ok_or(ScribeSubtitleBeginError::NoLocalMedia)?;
        self.job = Some(ScribeSubtitleJob::Queued { request });
        Ok(self.job.as_ref().expect("queued Scribe job"))
    }

    /// Future transport code must call this immediately before issuing a request.
    ///
    /// An endpoint is returned only for a configured supported backend, an active job, and a
    /// non-private session. The reserved Linux shell never receives one with its default config.
    #[must_use]
    pub fn network_endpoint(&self, private_session: bool) -> Option<&str> {
        if private_session || self.active_request().is_none() {
            return None;
        }
        match self.config.backend()? {
            ScribeSubtitleBackend::Supported { endpoint } if !endpoint.trim().is_empty() => {
                Some(endpoint)
            }
            ScribeSubtitleBackend::Unsupported | ScribeSubtitleBackend::Supported { .. } => None,
        }
    }

    pub fn mark_in_progress(&mut self, progress_percent: Option<u8>) -> bool {
        let Some(request) = self.active_request().cloned() else {
            return false;
        };
        self.job = Some(ScribeSubtitleJob::InProgress {
            request,
            progress_percent: progress_percent.map(|value| value.min(100)),
        });
        true
    }

    pub fn cancel(&mut self) -> bool {
        let Some(request) = self.active_request().cloned() else {
            return false;
        };
        self.job = Some(ScribeSubtitleJob::Canceled { request });
        true
    }

    pub fn fail(&mut self, message: impl Into<String>) -> bool {
        let Some(request) = self.active_request().cloned() else {
            return false;
        };
        self.job = Some(ScribeSubtitleJob::Failed {
            request,
            message: message.into(),
        });
        true
    }

    pub fn complete_with_srt(
        &mut self,
        contents: impl Into<String>,
    ) -> Result<&GeneratedSrtTrack, ScribeSubtitleCompleteError> {
        let request = self
            .active_request()
            .cloned()
            .ok_or(ScribeSubtitleCompleteError::NoActiveRequest)?;
        let contents = contents.into();
        if srt::parse(Some(&contents)).is_empty() {
            self.job = Some(ScribeSubtitleJob::Failed {
                request,
                message: "Scribe returned no subtitle cues.".to_owned(),
            });
            return Err(ScribeSubtitleCompleteError::EmptySrt);
        }

        self.job = Some(ScribeSubtitleJob::Ready {
            track: GeneratedSrtTrack {
                path: request.sidecar_path,
                contents,
            },
        });
        match self.job.as_ref().expect("completed Scribe job") {
            ScribeSubtitleJob::Ready { track } => Ok(track),
            _ => unreachable!("Scribe job was just completed"),
        }
    }

    /// Remove a completed result for the caller to persist and load through the shared external
    /// subtitle pipeline. Non-ready states are left unchanged.
    pub fn take_ready_track(&mut self) -> Option<GeneratedSrtTrack> {
        let job = self.job.take()?;
        match job {
            ScribeSubtitleJob::Ready { track } => Some(track),
            other => {
                self.job = Some(other);
                None
            }
        }
    }

    #[must_use]
    pub fn presentation(&self, context: &ScribeSubtitleContext) -> ScribeSubtitlePresentation {
        match self.job.as_ref() {
            Some(ScribeSubtitleJob::Queued { .. }) => ScribeSubtitlePresentation {
                label: "Generating subtitles…".to_owned(),
                badge: "QUEUED".to_owned(),
                message: "Scribe subtitle generation is queued.".to_owned(),
                can_generate: false,
                can_cancel: true,
                show_progress: true,
            },
            Some(ScribeSubtitleJob::InProgress {
                progress_percent, ..
            }) => {
                let badge = progress_percent
                    .map(|percent| format!("{percent}%"))
                    .unwrap_or_else(|| "WORKING".to_owned());
                let message = progress_percent.map_or_else(
                    || "Scribe subtitle generation is in progress.".to_owned(),
                    |percent| format!("Scribe subtitle generation is {percent}% complete."),
                );
                ScribeSubtitlePresentation {
                    label: "Generating subtitles…".to_owned(),
                    badge,
                    message,
                    can_generate: false,
                    can_cancel: true,
                    show_progress: true,
                }
            }
            Some(ScribeSubtitleJob::Ready { .. }) => ScribeSubtitlePresentation {
                label: "Generated subtitles".to_owned(),
                badge: "READY".to_owned(),
                message: "Generated subtitles are ready to save and load.".to_owned(),
                can_generate: false,
                can_cancel: false,
                show_progress: false,
            },
            Some(ScribeSubtitleJob::Failed { message, .. }) => ScribeSubtitlePresentation {
                label: "Generate subtitles…".to_owned(),
                badge: "ERROR".to_owned(),
                message: message.clone(),
                can_generate: self.validate_context(context).is_ok(),
                can_cancel: false,
                show_progress: false,
            },
            Some(ScribeSubtitleJob::Canceled { .. }) => ScribeSubtitlePresentation {
                label: "Generate subtitles…".to_owned(),
                badge: "CANCELED".to_owned(),
                message: "Scribe subtitle generation was canceled.".to_owned(),
                can_generate: self.validate_context(context).is_ok(),
                can_cancel: false,
                show_progress: false,
            },
            None => match self.validate_context(context) {
                Ok(()) => ScribeSubtitlePresentation {
                    label: "Generate subtitles…".to_owned(),
                    badge: "READY".to_owned(),
                    message: "Generate subtitles with the configured Scribe backend.".to_owned(),
                    can_generate: true,
                    can_cancel: false,
                    show_progress: false,
                },
                Err(error) => ScribeSubtitlePresentation {
                    label: "Generate subtitles…".to_owned(),
                    badge: unavailable_badge(error).to_owned(),
                    message: error.message().to_owned(),
                    can_generate: false,
                    can_cancel: false,
                    show_progress: false,
                },
            },
        }
    }

    fn validate_context(
        &self,
        context: &ScribeSubtitleContext,
    ) -> Result<(), ScribeSubtitleBeginError> {
        match self.config.backend() {
            None => return Err(ScribeSubtitleBeginError::NotConfigured),
            Some(ScribeSubtitleBackend::Unsupported) => {
                return Err(ScribeSubtitleBeginError::UnsupportedBackend);
            }
            Some(ScribeSubtitleBackend::Supported { endpoint }) if endpoint.trim().is_empty() => {
                return Err(ScribeSubtitleBeginError::NotConfigured);
            }
            Some(ScribeSubtitleBackend::Supported { .. }) => {}
        }
        if context.private_session {
            return Err(ScribeSubtitleBeginError::PrivateSession);
        }
        if context.request.is_none() {
            return Err(ScribeSubtitleBeginError::NoLocalMedia);
        }
        Ok(())
    }

    fn active_request(&self) -> Option<&ScribeSubtitleRequest> {
        match self.job.as_ref()? {
            ScribeSubtitleJob::Queued { request }
            | ScribeSubtitleJob::InProgress { request, .. } => Some(request),
            ScribeSubtitleJob::Ready { .. }
            | ScribeSubtitleJob::Failed { .. }
            | ScribeSubtitleJob::Canceled { .. } => None,
        }
    }
}

const fn unavailable_badge(error: ScribeSubtitleBeginError) -> &'static str {
    match error {
        ScribeSubtitleBeginError::NotConfigured => "SOON",
        ScribeSubtitleBeginError::UnsupportedBackend => "UNAVAILABLE",
        ScribeSubtitleBeginError::PrivateSession => "PRIVATE",
        ScribeSubtitleBeginError::NoLocalMedia => "NO MEDIA",
        ScribeSubtitleBeginError::AlreadyActive => "WORKING",
    }
}

/// Canonical generated subtitle sidecar: `Movie.mkv` -> `Movie.scribe.srt`.
#[must_use]
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
    use crate::subtitle_import::ExternalSubtitleOrigin;

    fn request() -> ScribeSubtitleRequest {
        ScribeSubtitleRequest::for_media_path("/media/Movie.mkv").expect("local media request")
    }

    fn context() -> ScribeSubtitleContext {
        ScribeSubtitleContext {
            private_session: false,
            request: Some(request()),
        }
    }

    fn supported_state() -> ScribeSubtitleState {
        ScribeSubtitleState::new(ScribeSubtitleConfig::supported(
            "  https://scribe.example.invalid  ",
        ))
    }

    #[test]
    fn default_state_is_an_honest_disabled_reservation() {
        let mut state = ScribeSubtitleState::default();
        let presentation = state.presentation(&context());

        assert_eq!(presentation.label, "Generate subtitles…");
        assert_eq!(presentation.badge, "SOON");
        assert!(!presentation.can_generate);
        assert!(!presentation.can_cancel);
        assert!(!presentation.show_progress);
        assert!(presentation.message.contains("No network request"));
        assert_eq!(
            state.begin(context()),
            Err(ScribeSubtitleBeginError::NotConfigured)
        );
        assert_eq!(state.network_endpoint(false), None);
        assert_eq!(state.job(), None);
    }

    #[test]
    fn unsupported_empty_private_and_media_missing_states_never_authorize_network() {
        let mut unsupported = ScribeSubtitleState::new(ScribeSubtitleConfig::unsupported());
        assert_eq!(
            unsupported.begin(context()),
            Err(ScribeSubtitleBeginError::UnsupportedBackend)
        );
        assert_eq!(unsupported.network_endpoint(false), None);

        let mut empty = ScribeSubtitleState::new(ScribeSubtitleConfig::supported("  "));
        assert_eq!(
            empty.begin(context()),
            Err(ScribeSubtitleBeginError::NotConfigured)
        );
        assert_eq!(empty.network_endpoint(false), None);

        let mut private = supported_state();
        assert_eq!(
            private.begin(ScribeSubtitleContext {
                private_session: true,
                request: Some(request()),
            }),
            Err(ScribeSubtitleBeginError::PrivateSession)
        );
        assert_eq!(private.network_endpoint(true), None);

        let mut no_media = supported_state();
        assert_eq!(
            no_media.begin(ScribeSubtitleContext::default()),
            Err(ScribeSubtitleBeginError::NoLocalMedia)
        );
        assert_eq!(no_media.network_endpoint(false), None);
    }

    #[test]
    fn supported_backend_enters_queued_then_progress_placeholder() {
        let mut state = supported_state();
        let request = request();

        assert!(matches!(
            state.begin(context()),
            Ok(ScribeSubtitleJob::Queued { request: queued }) if queued == &request
        ));
        assert_eq!(
            state.network_endpoint(false),
            Some("https://scribe.example.invalid")
        );
        assert_eq!(state.network_endpoint(true), None);
        assert!(state.mark_in_progress(Some(125)));
        assert!(matches!(
            state.job(),
            Some(ScribeSubtitleJob::InProgress {
                progress_percent: Some(100),
                ..
            })
        ));
        let presentation = state.presentation(&context());
        assert_eq!(presentation.label, "Generating subtitles…");
        assert_eq!(presentation.badge, "100%");
        assert!(presentation.can_cancel);
        assert!(presentation.show_progress);
    }

    #[test]
    fn begin_never_replaces_an_active_request() {
        let mut state = supported_state();
        let first = request();
        state.begin(context()).expect("queue first job");

        let second = ScribeSubtitleContext {
            private_session: false,
            request: ScribeSubtitleRequest::for_media_path("/media/Other.mkv"),
        };
        assert_eq!(
            state.begin(second),
            Err(ScribeSubtitleBeginError::AlreadyActive)
        );
        assert_eq!(
            state.job(),
            Some(&ScribeSubtitleJob::Queued { request: first })
        );
    }

    #[test]
    fn cancel_is_terminal_and_revokes_network_authorization() {
        let mut state = supported_state();
        state.begin(context()).expect("queue job");

        assert!(state.cancel());
        assert!(matches!(
            state.job(),
            Some(ScribeSubtitleJob::Canceled { .. })
        ));
        assert_eq!(state.network_endpoint(false), None);
        assert!(!state.cancel());
        assert!(!state.mark_in_progress(Some(50)));
        assert_eq!(
            state.complete_with_srt("1\n00:00:01,000 --> 00:00:02,000\nHello\n"),
            Err(ScribeSubtitleCompleteError::NoActiveRequest)
        );
    }

    #[test]
    fn valid_result_uses_the_shared_external_subtitle_track_pipeline() {
        let mut state = supported_state();
        state.begin(context()).expect("queue job");
        state.mark_in_progress(None);

        state
            .complete_with_srt("1\n00:00:01,000 --> 00:00:02,000\nHello\n")
            .expect("valid SRT");
        assert!(matches!(state.job(), Some(ScribeSubtitleJob::Ready { .. })));
        assert!(!state.presentation(&context()).can_generate);

        let track = state.take_ready_track().expect("completed track handoff");
        let import = track.external_import();
        assert_eq!(track.path(), Path::new("/media/Movie.scribe.srt"));
        assert!(track.contents().contains("Hello"));
        assert_eq!(import.path(), track.path());
        assert_eq!(import.origin(), &ExternalSubtitleOrigin::Scribe);
        assert_eq!(state.network_endpoint(false), None);
        assert_eq!(state.job(), None);
    }

    #[test]
    fn invalid_result_records_a_retryable_error_state() {
        let mut state = supported_state();
        state.begin(context()).expect("queue job");

        assert_eq!(
            state.complete_with_srt("not a subtitle file"),
            Err(ScribeSubtitleCompleteError::EmptySrt)
        );
        assert!(matches!(
            state.job(),
            Some(ScribeSubtitleJob::Failed { message, .. })
                if message == "Scribe returned no subtitle cues."
        ));
        let presentation = state.presentation(&context());
        assert_eq!(presentation.badge, "ERROR");
        assert!(presentation.can_generate);
        assert_eq!(state.network_endpoint(false), None);
    }

    #[test]
    fn sidecar_path_is_local_and_uses_an_srt_extension() {
        assert_eq!(
            generated_srt_sidecar_path(Path::new("/media/Movie.mkv")),
            Some(PathBuf::from("/media/Movie.scribe.srt"))
        );
        assert_eq!(
            generated_srt_sidecar_path(Path::new("/media/Movie")),
            Some(PathBuf::from("/media/Movie.scribe.srt"))
        );
        assert_eq!(
            generated_srt_sidecar_path(Path::new("https://example.invalid/Movie.mkv")),
            None
        );
        assert_eq!(generated_srt_sidecar_path(Path::new("")), None);
        assert_eq!(generated_srt_sidecar_path(Path::new("/")), None);
    }
}
