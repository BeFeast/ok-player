//! Media loading and live-stream UI state — the pure model the Linux shell renders for
//! loading, buffering, unknown-duration, and error states. Shell-free so the
//! classification is unit-testable; the shell only projects it onto widgets.
//!
//! No parsing, state-machine, schema, or business logic belongs in a shell
//! (freeze-boundary), so the load-state machine, the live/unknown-duration predicate,
//! and the failure-action model live here. The Linux shell renders the model today; the
//! Windows shell renders the same model once its port lands.

use std::path::{Path, PathBuf};

/// The measured X/Twitter workaround: prefer a combined progressive HTTPS format, then
/// leave selection to the extractor when that format is absent. Keeping the fallback in
/// the selector means an X post without a progressive rendition still opens normally.
pub const X_TWITTER_YTDL_FORMAT: &str = "best[protocol=https]/bestvideo+bestaudio/best";

/// Per-URL options for handing a network source to the playback engine. This value is
/// rebuilt for every load; it deliberately carries no state from the preceding source.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UrlLoadOptions {
    ytdl_format: Option<String>,
}

impl UrlLoadOptions {
    /// A file-local `ytdl-format` value, or `None` to inherit the engine/user default.
    pub fn ytdl_format(&self) -> Option<&str> {
        self.ytdl_format.as_deref()
    }
}

/// Resolve the file-local options for one URL load.
///
/// X/Twitter page URLs receive the measured combined-HTTPS preference unless the user
/// explicitly configured `ytdl-format` in the Advanced mpv options. Other sources inherit
/// existing selection unchanged. Option names match mpv's case-sensitive configuration boundary.
pub fn url_load_options<'a>(
    url: &str,
    configured_options: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> UrlLoadOptions {
    // Reapply the last explicit value per load: mpv restores the option that existed
    // before the preceding file, which can otherwise erase an Advanced live edit.
    let mut user_selected_format = None;
    for (name, value) in configured_options {
        if name.trim().strip_prefix("--").unwrap_or(name.trim()) == "ytdl-format" {
            user_selected_format = Some(value.to_owned());
        }
    }
    UrlLoadOptions {
        ytdl_format: user_selected_format
            .or_else(|| is_x_twitter_url(url).then(|| X_TWITTER_YTDL_FORMAT.to_owned())),
    }
}

/// True only for HTTP(S) URLs on the X/Twitter hosts accepted by yt-dlp: the base domains
/// and its documented `www`, `m`, and `mobile` variants. Recognition is by the authority's
/// host rather than a substring: an X name in a path or userinfo cannot match a different host.
pub fn is_x_twitter_url(url: &str) -> bool {
    const ROOT_HOSTS: &[&str] = &[
        "x.com",
        "twitter.com",
        "twitter3e4tixl4xyajtrzo62zg5vztmjuricljdp2c5kshju4avyoid.onion",
    ];
    const ALLOWED_PREFIXES: &[&str] = &["", "www.", "m.", "mobile."];

    let Some(host) = http_url_host(url.trim()) else {
        return false;
    };
    let host = host.to_ascii_lowercase();

    ROOT_HOSTS.iter().any(|root| {
        host.strip_suffix(root)
            .is_some_and(|prefix| ALLOWED_PREFIXES.contains(&prefix))
    })
}

/// Recover a conventional hostname from an HTTP(S) URL without pulling URL parsing into
/// the core crate. This is intentionally stricter than the generic playable-URL check:
/// malformed authorities and non-HTTP schemes must never acquire a site-specific policy.
fn http_url_host(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }

    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.is_empty() || authority.contains('\\') {
        return None;
    }
    let host_and_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_and_port.starts_with('[') {
        return None;
    }
    let (host, port) = host_and_port
        .split_once(':')
        .map_or((host_and_port, None), |(host, port)| (host, Some(port)));
    if host.is_empty()
        || host.contains(':')
        || port
            .is_some_and(|port| port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }

    Some(host)
}

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

/// Local final saves remain eligible after stop/error transitions. URL history
/// requires a confirmed load in the current source generation. Confirmation survives
/// stop/error UI transitions but resets on every new open, including the same URL.
pub fn history_progress_is_eligible(is_url: bool, load_confirmed: bool) -> bool {
    !is_url || load_confirmed
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
    path.to_string_lossy() == ended_path
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
    fn x_twitter_load_prefers_combined_https_with_extractor_fallback() {
        let options = url_load_options(
            "https://x.com/0xCodez/status/2095987472328958435/video/1",
            std::iter::empty(),
        );

        assert_eq!(
            options.ytdl_format(),
            Some("best[protocol=https]/bestvideo+bestaudio/best")
        );
        let (preferred, fallback) = options
            .ytdl_format()
            .and_then(|selector| selector.split_once('/'))
            .expect("X selector should include an extractor-supported fallback");
        assert_eq!(preferred, "best[protocol=https]");
        assert_eq!(fallback, "bestvideo+bestaudio/best");
    }

    #[test]
    fn x_twitter_recognition_matches_only_supported_host_boundaries() {
        for url in [
            "https://x.com/user/status/1",
            "http://www.x.com/user/status/1",
            "https://m.x.com/user/status/1",
            "https://mobile.x.com/user/status/1",
            "https://twitter.com/user/status/1",
            "https://www.twitter.com/user/status/1",
            "https://m.twitter.com/user/status/1",
            "https://mobile.twitter.com/user/status/1",
            "https://twitter3e4tixl4xyajtrzo62zg5vztmjuricljdp2c5kshju4avyoid.onion/user/status/1",
            "https://X.COM:443/user/status/1",
        ] {
            assert!(is_x_twitter_url(url), "supported X/Twitter URL: {url}");
        }

        for url in [
            "https://notx.com/status/1",
            "https://x.com.example.test/status/1",
            "https://news.x.com/status/1",
            "https://mobile.news.x.com/status/1",
            "https://example.test/x.com/status/1",
            "https://x.com@evil.test/status/1",
            "https://x.com.evil.test@twitter.invalid/status/1",
            "https://x.com./status/1",
            "https://x.com:bad/status/1",
            "ftp://x.com/status/1",
            "not a URL mentioning twitter.com",
        ] {
            assert!(!is_x_twitter_url(url), "non-X/Twitter URL: {url}");
        }
    }

    #[test]
    fn explicit_user_ytdl_format_overrides_x_policy() {
        for configured_names in [
            vec![("ytdl-format", "worst")],
            vec![
                ("cache", "yes"),
                ("ytdl-format", "worst"),
                ("profile", "fast"),
            ],
            vec![("--ytdl-format", "worst")],
        ] {
            assert_eq!(
                url_load_options("https://www.x.com/user/status/1", configured_names).ytdl_format(),
                Some("worst")
            );
        }
    }

    #[test]
    fn invalid_uppercase_option_cannot_override_the_x_policy() {
        assert_eq!(
            url_load_options("https://x.com/user/status/1", [("YTDL-FORMAT", "worst")])
                .ytdl_format(),
            Some(X_TWITTER_YTDL_FORMAT)
        );
        assert_eq!(
            url_load_options("https://example.test/video", [("YTDL-FORMAT", "worst")])
                .ytdl_format(),
            None
        );
        assert!(is_x_twitter_url("https://user@x.com/user/status/1"));
        assert!(!is_x_twitter_url("https://x.com@other.test/user/status/1"));
    }

    #[test]
    fn non_x_sources_keep_existing_format_selection() {
        for url in [
            "https://www.youtube.com/watch?v=0O91lY-CoeE",
            "https://youtu.be/0O91lY-CoeE",
            "https://vimeo.com/1234",
            "https://example.test/video.mp4",
            "rtsp://example.test/live",
        ] {
            assert_eq!(
                url_load_options(url, [("cache", "yes"), ("profile", "fast")]).ytdl_format(),
                None,
                "non-X source must inherit existing selection: {url}"
            );
        }
    }

    #[test]
    fn format_policy_is_recomputed_for_each_source() {
        let x = url_load_options("https://x.com/user/status/1", std::iter::empty());
        let youtube = url_load_options(
            "https://www.youtube.com/watch?v=0O91lY-CoeE",
            std::iter::empty(),
        );
        let direct = url_load_options("https://example.test/movie.mp4", std::iter::empty());

        assert_eq!(x.ytdl_format(), Some(X_TWITTER_YTDL_FORMAT));
        assert_eq!(youtube.ytdl_format(), None);
        assert_eq!(direct.ytdl_format(), None);
    }

    #[test]
    fn local_final_saves_remain_eligible_but_unsuccessful_urls_do_not() {
        use crate::history::{History, HistoryProgressUpdate, HistoryWriteMode};
        use crate::nfo_metadata::HistoryTitleUpdate;

        for state in [
            MediaLoadState::Idle,
            MediaLoadState::Loading,
            MediaLoadState::Failed,
            MediaLoadState::Playing,
        ] {
            let mut history = History::default();
            for (key, is_url) in [
                ("/media/movie.mp4", false),
                ("https://example.com/video", true),
            ] {
                if history_progress_is_eligible(is_url, state == MediaLoadState::Playing) {
                    history.record_progress(
                        key,
                        HistoryProgressUpdate {
                            position: 120.0,
                            duration: 600.0,
                            finished: false,
                            updated_at_unix: 10,
                            title: HistoryTitleUpdate::Preserve,
                        },
                        HistoryWriteMode::Record,
                    );
                }
            }
            assert_eq!(
                history.resume_position("/media/movie.mp4"),
                Some(120.0),
                "local final save in {state:?}"
            );
            assert_eq!(
                history.files.contains_key("https://example.com/video"),
                state == MediaLoadState::Playing
            );
        }
    }

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
