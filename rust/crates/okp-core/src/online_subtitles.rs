use std::path::{Path, PathBuf};

use crate::subtitle_import::ExternalSubtitleImport;

/// Inputs that decide whether an online subtitle lookup may leave the machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OnlineSubtitleSearchContext {
    pub enabled: bool,
    pub provider_configured: bool,
    pub implementation_available: bool,
    pub private_session: bool,
    pub media_available: bool,
}

impl OnlineSubtitleSearchContext {
    /// Current reserved implementation: visible in the UI, explicitly off, and
    /// incapable of authorizing a network request.
    #[must_use]
    pub const fn reserved(private_session: bool, media_available: bool) -> Self {
        Self {
            enabled: false,
            provider_configured: false,
            implementation_available: false,
            private_session,
            media_available,
        }
    }

    #[must_use]
    pub const fn state(self) -> OnlineSubtitleSearchState {
        if !self.enabled {
            OnlineSubtitleSearchState::Disabled
        } else if self.private_session {
            OnlineSubtitleSearchState::PrivateSession
        } else if !self.media_available {
            OnlineSubtitleSearchState::MediaUnavailable
        } else if !self.provider_configured {
            OnlineSubtitleSearchState::ProviderNotConfigured
        } else if !self.implementation_available {
            OnlineSubtitleSearchState::Unavailable
        } else {
            OnlineSubtitleSearchState::Available
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnlineSubtitleSearchState {
    Disabled,
    PrivateSession,
    MediaUnavailable,
    ProviderNotConfigured,
    Unavailable,
    Available,
}

impl OnlineSubtitleSearchState {
    /// The UI badge is deliberately short enough for the compact track popover.
    #[must_use]
    pub const fn badge(self) -> &'static str {
        match self {
            Self::Disabled => "SOON",
            Self::PrivateSession => "PRIVATE",
            Self::MediaUnavailable => "NO MEDIA",
            Self::ProviderNotConfigured => "NO PROVIDER",
            Self::Unavailable => "UNAVAILABLE",
            Self::Available => "READY",
        }
    }

    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Disabled => {
                "Online subtitle search is coming later. No network request will be made."
            }
            Self::PrivateSession => "Online subtitle search is unavailable in a private session.",
            Self::MediaUnavailable => "Open media before searching for subtitles online.",
            Self::ProviderNotConfigured => {
                "No online subtitle provider is configured. No network request will be made."
            }
            Self::Unavailable => {
                "Online subtitle search is currently unavailable. No network request will be made."
            }
            Self::Available => "Search the configured online subtitle provider.",
        }
    }

    /// Future provider code must pass this gate immediately before any lookup.
    #[must_use]
    pub const fn allows_network(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Provider metadata retained while a future result is downloaded locally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnlineSubtitleResult {
    provider: String,
    result_id: String,
    language: Option<String>,
    release_name: Option<String>,
}

impl OnlineSubtitleResult {
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        result_id: impl Into<String>,
        language: Option<String>,
        release_name: Option<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            result_id: result_id.into(),
            language,
            release_name,
        }
    }

    #[must_use]
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    #[must_use]
    pub fn release_name(&self) -> Option<&str> {
        self.release_name.as_deref()
    }

    /// Convert a downloaded SRT into the same import type used by a local file.
    /// This does not download, parse, or load anything by itself.
    pub fn import_downloaded_srt(
        &self,
        path: impl Into<PathBuf>,
    ) -> Result<ExternalSubtitleImport, OnlineSubtitleImportError> {
        let path = path.into();
        if !is_srt_path(&path) {
            return Err(OnlineSubtitleImportError::NotSrt);
        }

        Ok(ExternalSubtitleImport::online_search(
            path,
            self.provider.clone(),
            self.result_id.clone(),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnlineSubtitleImportError {
    NotSrt,
}

fn is_srt_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("srt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitle_import::ExternalSubtitleOrigin;

    fn context() -> OnlineSubtitleSearchContext {
        OnlineSubtitleSearchContext {
            enabled: true,
            provider_configured: true,
            implementation_available: true,
            private_session: false,
            media_available: true,
        }
    }

    #[test]
    fn reserved_state_is_disabled_and_never_allows_network() {
        for private_session in [false, true] {
            for media_available in [false, true] {
                let state =
                    OnlineSubtitleSearchContext::reserved(private_session, media_available).state();

                assert_eq!(state, OnlineSubtitleSearchState::Disabled);
                assert_eq!(state.badge(), "SOON");
                assert!(!state.allows_network());
            }
        }
    }

    #[test]
    fn enabled_search_without_a_provider_reports_provider_missing() {
        let state = OnlineSubtitleSearchContext {
            provider_configured: false,
            ..context()
        }
        .state();

        assert_eq!(state, OnlineSubtitleSearchState::ProviderNotConfigured);
        assert_eq!(state.badge(), "NO PROVIDER");
        assert!(!state.allows_network());
    }

    #[test]
    fn unavailable_and_private_states_remain_network_safe() {
        let unavailable = OnlineSubtitleSearchContext {
            implementation_available: false,
            ..context()
        }
        .state();
        let private = OnlineSubtitleSearchContext {
            private_session: true,
            ..context()
        }
        .state();

        assert_eq!(unavailable, OnlineSubtitleSearchState::Unavailable);
        assert_eq!(private, OnlineSubtitleSearchState::PrivateSession);
        assert!(!unavailable.allows_network());
        assert!(!private.allows_network());
        assert!(context().state().allows_network());
    }

    #[test]
    fn enabled_search_without_media_reports_media_unavailable() {
        let state = OnlineSubtitleSearchContext {
            media_available: false,
            ..context()
        }
        .state();

        assert_eq!(state, OnlineSubtitleSearchState::MediaUnavailable);
        assert_eq!(state.badge(), "NO MEDIA");
        assert!(!state.allows_network());
    }

    #[test]
    fn downloaded_result_becomes_the_shared_external_subtitle_import() {
        let result = OnlineSubtitleResult::new(
            "example-provider",
            "result-42",
            Some("eng".to_owned()),
            Some("Example release".to_owned()),
        );
        let import = result
            .import_downloaded_srt("cache/Example.en.SRT")
            .expect("downloaded SRT should become an external import");

        assert_eq!(import.path(), Path::new("cache/Example.en.SRT"));
        assert_eq!(result.language(), Some("eng"));
        assert_eq!(result.release_name(), Some("Example release"));
        assert_eq!(
            import.origin(),
            &ExternalSubtitleOrigin::OnlineSearch {
                provider: "example-provider".to_owned(),
                result_id: "result-42".to_owned(),
            }
        );
        assert_eq!(
            result.import_downloaded_srt("cache/Example.ass"),
            Err(OnlineSubtitleImportError::NotSrt)
        );
    }
}
