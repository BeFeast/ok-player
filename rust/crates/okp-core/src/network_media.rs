//! Media loading and live-stream UI state — the pure model the Linux shell renders for
//! loading, buffering, unknown-duration, and error states. Shell-free so the
//! classification is unit-testable; the shell only projects it onto widgets.
//!
//! No parsing, state-machine, schema, or business logic belongs in a shell
//! (freeze-boundary), so the load-state machine, the live/unknown-duration predicate,
//! and the failure-action model live here. The Linux shell renders the model today; the
//! Windows shell renders the same model once its port lands.

use std::path::{Component, Path, PathBuf};

/// The transport-surface state for the loaded source, derived from what the shell has
/// observed from the engine. The shell transitions this on `load_url`/`load_file`, the
/// engine's `FileLoaded` lifecycle event, and a reported load failure (`EndFile::Error`
/// or a load command returning `Err`). It is the single source of truth the loading,
/// buffering, and error surfaces read from, so they never drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaLoadState {
    /// Nothing loaded, or the loaded media was closed.
    #[default]
    Idle,
    /// A source was handed to the engine but no frame has arrived yet — the loading /
    /// buffering surface shows while the network file opens.
    Loading,
    /// The engine fired `FileLoaded` — a frame is up and the source is playing.
    Playing,
    /// The engine reported a load failure for the source.
    Failed,
}

/// True when the duration is known (finite and positive). A live stream or a
/// not-yet-resolved network file reports no duration, so the transport readout falls
/// back to the `--:--` sentinel (see [`crate::time_code::format_duration`]) instead of
/// broken timeline math.
pub fn duration_is_known(duration: Option<f64>) -> bool {
    matches!(duration, Some(value) if value.is_finite() && value > 0.0)
}

/// True for a live / unknown-duration source: a URL whose duration has not resolved. A
/// local file with no observed duration yet is just *loading*, not live, so `is_url`
/// gates this — only network sources ever read as live. Lets the shell switch the
/// timeline to the progress-only / live readout without inspecting mpv properties
/// itself.
pub fn is_live_or_unknown_duration(is_url: bool, duration: Option<f64>) -> bool {
    is_url && !duration_is_known(duration)
}

/// Format the transport's duration total, gating the live `--:--` sentinel on the
/// live/unknown predicate. Only a URL whose duration has not resolved renders the
/// sentinel; a local file that has not reported a duration yet is just *loading*,
/// so it renders the padded `00:00` clock (via [`crate::time_code::format_clock`])
/// instead of the live-stream sentinel. A known duration (URL or local) renders the
/// padded clock. Pure core so the Linux and Windows shells render the same total.
pub fn format_duration_total(is_url: bool, duration: Option<f64>) -> String {
    if is_live_or_unknown_duration(is_url, duration) {
        "--:--".to_owned()
    } else {
        crate::time_code::format_clock(duration.unwrap_or(0.0))
    }
}

/// Format the trailing transport readout as time remaining while preserving the
/// local-loading versus live-URL distinction used by [`format_duration_total`].
pub fn format_remaining_total(is_url: bool, position: f64, duration: Option<f64>) -> String {
    if is_live_or_unknown_duration(is_url, duration) {
        "--:--".to_owned()
    } else if duration_is_known(duration) {
        crate::time_code::format_remaining(position, duration)
    } else {
        "-00:00".to_owned()
    }
}

/// Classify the transport-surface state for a source from what the shell has observed.
/// `is_loaded` is whether a source is currently loaded (a file or URL was handed to the
/// engine); `file_loaded` is whether the engine fired `FileLoaded`; `load_error` is a
/// reported failure. A failure wins over every other signal so a source that errors
/// mid-load reads as `Failed`, not `Loading`.
pub fn classify_load_state(is_loaded: bool, file_loaded: bool, load_error: bool) -> MediaLoadState {
    if load_error {
        MediaLoadState::Failed
    } else if !is_loaded {
        MediaLoadState::Idle
    } else if file_loaded {
        MediaLoadState::Playing
    } else {
        MediaLoadState::Loading
    }
}

/// The source that can be retried from a failed load card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadFailureSource {
    /// A local media file.
    Local(PathBuf),
    /// A network stream or URL.
    Url(String),
}

impl LoadFailureSource {
    /// Build a local-file retry source.
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self::Local(path.into())
    }

    /// Build a URL retry source.
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// True when the failed source was a URL.
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }

    /// True when the engine's ended path identifies this source.
    pub fn matches_engine_path(&self, ended_path: &str) -> bool {
        match self {
            Self::Local(path) => engine_path_matches_local(path, ended_path),
            Self::Url(url) => url == ended_path,
        }
    }
}

fn engine_path_matches_local(path: &Path, ended_path: &str) -> bool {
    normalize_path_lexically(path) == normalize_path_lexically(Path::new(ended_path))
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

/// The recoverable actions offered when a load fails (PRD §2.1, retry / copy details).
/// The ordered set is exposed via [`LOAD_FAILURE_ACTIONS`] so the shell renders a
/// consistent action row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailureAction {
    /// Retry the same source.
    Retry,
    /// Open a different source.
    OpenAnother,
    /// Copy a short, copyable diagnostic (not raw internal logs).
    CopyDetails,
}

/// The ordered failure actions the shell renders, left to right.
pub const LOAD_FAILURE_ACTIONS: &[LoadFailureAction] = &[
    LoadFailureAction::Retry,
    LoadFailureAction::OpenAnother,
    LoadFailureAction::CopyDetails,
];

impl LoadFailureAction {
    /// The button label for this action.
    pub fn label(self) -> &'static str {
        match self {
            Self::Retry => "Retry",
            Self::OpenAnother => "Open another",
            Self::CopyDetails => "Copy details",
        }
    }
}

/// Build the copyable diagnostic line for a failed load — a short, stable summary
/// (source + reason) rather than the raw internal log, so the primary UI never dumps
/// engine trace into the clipboard. `reason` is the short human-readable cause the
/// shell already produced (e.g. `libmpv error 412`), not a verbatim log buffer; an
/// empty reason is omitted so a transient failure still copies a clean line.
pub fn failure_detail(source: &LoadFailureSource, reason: &str) -> String {
    let reason = reason.trim();
    match source {
        LoadFailureSource::Url(url) if reason.is_empty() => {
            format!("OK Player could not open the stream.\nURL: {url}")
        }
        LoadFailureSource::Url(url) => {
            format!("OK Player could not open the stream.\nURL: {url}\nReason: {reason}")
        }
        LoadFailureSource::Local(path) if reason.is_empty() => {
            format!(
                "OK Player could not open the media.\nPath: {}",
                path.display()
            )
        }
        LoadFailureSource::Local(path) => format!(
            "OK Player could not open the media.\nPath: {}\nReason: {reason}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_is_known_only_for_finite_positive() {
        assert!(duration_is_known(Some(120.0)));
        assert!(duration_is_known(Some(0.5)));
        // Unknown, zero, negative, and non-finite all read as unknown — the live sentinel.
        assert!(!duration_is_known(None));
        assert!(!duration_is_known(Some(0.0)));
        assert!(!duration_is_known(Some(-3.0)));
        assert!(!duration_is_known(Some(f64::NAN)));
        assert!(!duration_is_known(Some(f64::INFINITY)));
    }

    #[test]
    fn is_live_or_unknown_duration_only_for_urls_without_duration() {
        // A URL with no resolved duration is live-style.
        assert!(is_live_or_unknown_duration(true, None));
        assert!(is_live_or_unknown_duration(true, Some(0.0)));
        // Once the URL's duration resolves it is no longer live-style.
        assert!(!is_live_or_unknown_duration(true, Some(120.0)));
        // A local file with no observed duration is just loading, not live.
        assert!(!is_live_or_unknown_duration(false, None));
    }

    #[test]
    fn format_duration_total_sentinel_only_for_urls_with_unknown_duration() {
        // A URL whose duration has not resolved renders the live sentinel.
        assert_eq!(format_duration_total(true, None), "--:--");
        assert_eq!(format_duration_total(true, Some(0.0)), "--:--");
        assert_eq!(format_duration_total(true, Some(f64::NAN)), "--:--");
        // Once the URL's duration resolves, the padded clock renders.
        assert_eq!(format_duration_total(true, Some(90.0)), "01:30");
        // A local file that has not reported a duration yet is just loading, so it
        // renders `00:00`, not the live sentinel.
        assert_eq!(format_duration_total(false, None), "00:00");
        assert_eq!(format_duration_total(false, Some(0.0)), "00:00");
        assert_eq!(format_duration_total(false, Some(5025.0)), "01:23:45");
    }

    #[test]
    fn format_remaining_total_preserves_live_and_local_loading_states() {
        assert_eq!(format_remaining_total(true, 30.0, None), "--:--");
        assert_eq!(format_remaining_total(false, 30.0, None), "-00:00");
        assert_eq!(format_remaining_total(false, 30.0, Some(90.0)), "-01:00");
        assert_eq!(format_remaining_total(true, 95.0, Some(90.0)), "-00:00");
    }

    #[test]
    fn classify_load_state_priority() {
        // A failure wins over every other signal.
        assert_eq!(
            classify_load_state(true, true, true),
            MediaLoadState::Failed
        );
        assert_eq!(
            classify_load_state(true, false, true),
            MediaLoadState::Failed
        );
        // Nothing loaded -> Idle, even if a stale file_loaded flag lingers.
        assert_eq!(
            classify_load_state(false, true, false),
            MediaLoadState::Idle
        );
        assert_eq!(
            classify_load_state(false, false, false),
            MediaLoadState::Idle
        );
        // Loaded but no frame yet -> Loading.
        assert_eq!(
            classify_load_state(true, false, false),
            MediaLoadState::Loading
        );
        // Loaded and a frame is up -> Playing.
        assert_eq!(
            classify_load_state(true, true, false),
            MediaLoadState::Playing
        );
    }

    #[test]
    fn media_load_state_default_is_idle() {
        assert_eq!(MediaLoadState::default(), MediaLoadState::Idle);
    }

    #[test]
    fn load_failure_actions_are_ordered_with_stable_labels() {
        let labels: Vec<&'static str> = LOAD_FAILURE_ACTIONS
            .iter()
            .copied()
            .map(LoadFailureAction::label)
            .collect();
        assert_eq!(labels, ["Retry", "Open another", "Copy details"]);
        // The ordered set is the contract the shell renders, so each action appears once.
        assert_eq!(LOAD_FAILURE_ACTIONS.len(), 3);
    }

    #[test]
    fn load_failure_source_matches_engine_path() {
        assert!(
            LoadFailureSource::local("/media/movie.mkv").matches_engine_path("/media/movie.mkv")
        );
        assert!(
            !LoadFailureSource::local("/media/movie.mkv").matches_engine_path("/media/other.mkv")
        );
        assert!(
            LoadFailureSource::url("https://example.com/live.m3u8")
                .matches_engine_path("https://example.com/live.m3u8")
        );
        assert!(
            !LoadFailureSource::url("https://example.com/live.m3u8")
                .matches_engine_path("https://example.com/other.m3u8")
        );
        assert!(
            LoadFailureSource::local("/workspace/rust/crate/../../media/movie.mkv")
                .matches_engine_path("/workspace/media/movie.mkv")
        );
        assert!(LoadFailureSource::url("https://example.com/live.m3u8").is_url());
        assert!(!LoadFailureSource::local("/media/movie.mkv").is_url());
    }

    #[test]
    fn failure_detail_includes_source_and_reason_without_raw_logs() {
        let url = LoadFailureSource::url("https://example.com/live.m3u8");
        assert_eq!(
            failure_detail(&url, "libmpv error 412"),
            "OK Player could not open the stream.\nURL: https://example.com/live.m3u8\nReason: libmpv error 412"
        );
        // An empty reason is omitted so a transient failure still copies a clean line.
        assert_eq!(
            failure_detail(&url, ""),
            "OK Player could not open the stream.\nURL: https://example.com/live.m3u8"
        );
        // Whitespace-only reason is treated as empty.
        assert_eq!(
            failure_detail(&url, "   "),
            "OK Player could not open the stream.\nURL: https://example.com/live.m3u8"
        );

        let local = LoadFailureSource::local("/media/movie.mkv");
        assert_eq!(
            failure_detail(&local, "libmpv error 7"),
            "OK Player could not open the media.\nPath: /media/movie.mkv\nReason: libmpv error 7"
        );
        assert_eq!(
            failure_detail(&local, ""),
            "OK Player could not open the media.\nPath: /media/movie.mkv"
        );
    }
}
