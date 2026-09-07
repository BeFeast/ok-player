use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

use okp_core::media_download::{
    DownloadContainer, DownloadJobId, DownloadPurpose, MediaDownloadEvent, MediaDownloadOutcome,
    MediaDownloadRequest,
};
use okp_core::replay_cache::{
    AcquiredReplay, DEFAULT_REPLAY_CACHE_CAPACITY_BYTES, ReplayCache, ReplayCacheStaging,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ReplayCacheStatusSnapshot {
    Idle,
    Disabled,
    Private,
    ToolUnavailable,
    Downloading { percent: Option<u8> },
    Ready,
    Error(String),
}

impl ReplayCacheStatusSnapshot {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Idle => "Ready for eligible public videos".to_owned(),
            Self::Disabled => "Replay cache is off".to_owned(),
            Self::Private => "Paused for private session".to_owned(),
            Self::ToolUnavailable => "yt-dlp is not installed".to_owned(),
            Self::Downloading {
                percent: Some(percent),
            } => format!("Saving replay — {percent}%"),
            Self::Downloading { percent: None } => "Saving replay…".to_owned(),
            Self::Ready => "Replay ready for History".to_owned(),
            Self::Error(message) => message.clone(),
        }
    }

    pub(crate) fn is_downloading(&self) -> bool {
        matches!(self, Self::Downloading { .. })
    }
}

#[derive(Clone, Debug)]
struct StreamCandidate {
    url: String,
    source_generation: u64,
    format_selector: Option<String>,
}

struct ActiveReplayDownload {
    job_id: DownloadJobId,
    staging: ReplayCacheStaging,
}

pub(crate) struct ReplayCacheRuntime {
    root: PathBuf,
    cache: Option<ReplayCache>,
    cache_open_attempted: bool,
    downloader: media_download::MediaDownloader,
    tool_available: bool,
    candidate: Option<StreamCandidate>,
    active: Option<ActiveReplayDownload>,
    staging_sequence: u64,
    status: ReplayCacheStatusSnapshot,
}

impl Default for ReplayCacheRuntime {
    fn default() -> Self {
        let executable = find_executable(youtube_open::YOUTUBE_RESOLVER);
        let tool_available = executable.is_some();
        let downloader = executable.map_or_else(
            media_download::MediaDownloader::new,
            media_download::MediaDownloader::with_executable,
        );
        Self {
            root: replay_cache_dir(),
            cache: None,
            cache_open_attempted: false,
            downloader,
            tool_available,
            candidate: None,
            active: None,
            staging_sequence: unix_now_u64(),
            status: if tool_available {
                ReplayCacheStatusSnapshot::Idle
            } else {
                ReplayCacheStatusSnapshot::ToolUnavailable
            },
        }
    }
}

impl ReplayCacheRuntime {
    pub(crate) fn status_snapshot(&self) -> ReplayCacheStatusSnapshot {
        self.status.clone()
    }

    pub(crate) fn offer_stream(
        &mut self,
        url: String,
        source_generation: u64,
        enabled: bool,
        private_session: bool,
        format_selector: Option<String>,
    ) {
        self.candidate = None;
        if !enabled {
            self.status = ReplayCacheStatusSnapshot::Disabled;
            return;
        }
        if private_session {
            self.status = ReplayCacheStatusSnapshot::Private;
            return;
        }
        if !self.tool_available {
            self.status = ReplayCacheStatusSnapshot::ToolUnavailable;
            return;
        }
        if self.active.is_none() {
            self.status = ReplayCacheStatusSnapshot::Idle;
        }
        self.candidate = Some(StreamCandidate {
            url,
            source_generation,
            format_selector,
        });
    }

    pub(crate) fn source_changed(&mut self) {
        self.candidate = None;
    }

    pub(crate) fn acquire(
        &mut self,
        source_url: &str,
        format_selector: Option<&str>,
    ) -> Option<AcquiredReplay> {
        let now = unix_now_i64();
        let cache = self.cache()?;
        match cache.acquire_for_format(source_url, format_selector, now) {
            Ok(replay) => replay,
            Err(error) => {
                eprintln!("Failed to read replay cache: {error}");
                self.status = ReplayCacheStatusSnapshot::Error(
                    "Replay cache could not be read — streaming instead".to_owned(),
                );
                None
            }
        }
    }

    pub(crate) fn invalidate(&mut self, source_url: &str) {
        let Some(cache) = self.cache() else {
            return;
        };
        if let Err(error) = cache.invalidate(source_url) {
            eprintln!("Failed to invalidate replay cache entry: {error}");
        }
    }

    fn cache(&mut self) -> Option<&ReplayCache> {
        if self.cache.is_none() && !self.cache_open_attempted {
            self.cache_open_attempted = true;
            match ReplayCache::open(&self.root, DEFAULT_REPLAY_CACHE_CAPACITY_BYTES) {
                Ok(cache) => {
                    if let Err(error) = cache.recover_abandoned(cache_owner_alive) {
                        eprintln!("Failed to reclaim abandoned replay files: {error}");
                    }
                    self.cache = Some(cache);
                }
                Err(error) => {
                    eprintln!("Failed to open replay cache: {error}");
                    self.status = ReplayCacheStatusSnapshot::Error(
                        "Replay cache storage is unavailable".to_owned(),
                    );
                }
            }
        }
        self.cache.as_ref()
    }

    fn poll(
        &mut self,
        source_generation: u64,
        current_url: Option<&str>,
        duration: Option<f64>,
        playing: bool,
        enabled: bool,
        private_session: bool,
    ) -> Vec<String> {
        if !enabled || private_session {
            self.candidate = None;
            self.cancel_active();
            self.status = if private_session {
                ReplayCacheStatusSnapshot::Private
            } else {
                ReplayCacheStatusSnapshot::Disabled
            };
        } else if self.tool_available {
            self.try_start(source_generation, current_url, duration, playing);
        }

        let notices = self.drain_events(enabled && !private_session);
        // Policy remains authoritative after queued progress/terminal events.
        if !enabled || private_session {
            self.status = if private_session {
                ReplayCacheStatusSnapshot::Private
            } else {
                ReplayCacheStatusSnapshot::Disabled
            };
        }
        notices
    }

    fn try_start(
        &mut self,
        source_generation: u64,
        current_url: Option<&str>,
        duration: Option<f64>,
        playing: bool,
    ) {
        if self.active.is_some() || !playing {
            return;
        }
        let Some(duration) = duration.filter(|duration| duration.is_finite() && *duration > 0.0)
        else {
            return;
        };
        let Some(candidate) = self.candidate.clone() else {
            return;
        };
        if candidate.source_generation != source_generation
            || current_url != Some(candidate.url.as_str())
        {
            self.candidate = None;
            return;
        }

        // A duration is the shell's first non-live proof. The native adapter performs
        // the authoritative playlist/live probe before writing any media bytes.
        let _ = duration;
        if self
            .acquire(&candidate.url, candidate.format_selector.as_deref())
            .is_some()
        {
            self.candidate = None;
            self.status = ReplayCacheStatusSnapshot::Ready;
            return;
        }

        self.staging_sequence = self.staging_sequence.wrapping_add(1).max(1);
        let staging_id = DownloadJobId(self.staging_sequence);
        let Some(cache) = self.cache() else {
            self.candidate = None;
            return;
        };
        let staging = match cache.begin_download_for_format(
            &candidate.url,
            staging_id,
            candidate.format_selector.as_deref(),
        ) {
            Ok(staging) => staging,
            Err(error) => {
                eprintln!("Failed to prepare replay download: {error}");
                self.status = ReplayCacheStatusSnapshot::Error(
                    "Replay cache could not prepare a download".to_owned(),
                );
                self.candidate = None;
                return;
            }
        };
        let mut request = match MediaDownloadRequest::public_vod(
            candidate.url.clone(),
            staging.download_target(),
            DownloadPurpose::ReplayCache,
            DownloadContainer::Preserve,
            staging.max_bytes(),
        ) {
            Ok(request) => request,
            Err(error) => {
                if let Some(cache) = self.cache()
                    && let Err(cleanup_error) = cache.abandon_download(staging)
                {
                    eprintln!("Failed to abandon rejected replay staging: {cleanup_error}");
                }
                eprintln!("Replay download request was rejected: {error}");
                self.candidate = None;
                return;
            }
        };
        request.format_selector = candidate.format_selector.clone();
        match self.downloader.request(request) {
            Ok(job_id) => {
                self.active = Some(ActiveReplayDownload { job_id, staging });
                self.candidate = None;
                self.status = ReplayCacheStatusSnapshot::Downloading { percent: None };
            }
            Err(error) => {
                if let Some(cache) = self.cache()
                    && let Err(cleanup_error) = cache.abandon_download(staging)
                {
                    eprintln!("Failed to abandon busy replay staging: {cleanup_error}");
                }
                eprintln!("Replay downloader was busy: {error}");
                self.candidate = None;
            }
        }
    }

    fn drain_events(&mut self, allow_promotion: bool) -> Vec<String> {
        let mut notices = Vec::new();
        for event in self.downloader.drain_events() {
            match event {
                MediaDownloadEvent::Started { .. } => {}
                MediaDownloadEvent::Progress(progress)
                    if self.active.as_ref().map(|active| active.job_id)
                        == Some(progress.job_id) =>
                {
                    self.status = ReplayCacheStatusSnapshot::Downloading {
                        percent: progress.percent(),
                    };
                }
                MediaDownloadEvent::Progress(_) => {}
                MediaDownloadEvent::Terminal { job_id, outcome }
                    if self.active.as_ref().map(|active| active.job_id) == Some(job_id) =>
                {
                    let active = self.active.take().expect("matching replay job");
                    match outcome {
                        MediaDownloadOutcome::Completed(media) => {
                            if !allow_promotion {
                                self.abandon(active.staging);
                                continue;
                            }
                            let completed = self.cache().and_then(|cache| {
                                match cache.complete_download(active.staging, media, unix_now_i64())
                                {
                                    Ok(_) => Some(()),
                                    Err(error) => {
                                        eprintln!("Failed to promote replay download: {error}");
                                        None
                                    }
                                }
                            });
                            if completed.is_some() {
                                self.status = ReplayCacheStatusSnapshot::Ready;
                                notices.push("Replay ready for History".to_owned());
                            } else {
                                self.status = ReplayCacheStatusSnapshot::Error(
                                    "Downloaded replay could not be stored".to_owned(),
                                );
                            }
                        }
                        MediaDownloadOutcome::Cancelled => {
                            self.abandon(active.staging);
                            self.status = ReplayCacheStatusSnapshot::Idle;
                            notices.push("Replay download canceled".to_owned());
                        }
                        MediaDownloadOutcome::Rejected(reason) => {
                            self.abandon(active.staging);
                            self.status = ReplayCacheStatusSnapshot::Idle;
                            eprintln!("Replay download excluded: {reason}");
                        }
                        MediaDownloadOutcome::Failed { message } => {
                            self.abandon(active.staging);
                            eprintln!("Replay download failed: {message}");
                            self.status = ReplayCacheStatusSnapshot::Error(
                                "Replay download did not complete".to_owned(),
                            );
                        }
                    }
                }
                MediaDownloadEvent::Terminal { .. } => {}
            }
        }
        notices
    }

    fn abandon(&mut self, staging: ReplayCacheStaging) {
        if let Some(cache) = self.cache()
            && let Err(error) = cache.abandon_download(staging)
        {
            eprintln!("Failed to clean replay staging: {error}");
        }
    }

    fn cancel_active(&mut self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| self.downloader.cancel(active.job_id))
    }

    fn clear(&mut self) -> Result<(usize, usize), String> {
        let Some(cache) = self.cache() else {
            return Err("Replay cache storage is unavailable".to_owned());
        };
        let recovered = cache
            .recover_abandoned(cache_owner_alive)
            .map_err(|error| error.to_string())?;
        cache
            .clear_unpinned()
            .map(|result| (result.removed + recovered, result.retained_pinned))
            .map_err(|error| error.to_string())
    }

    fn shutdown(&mut self) {
        self.candidate = None;
        self.cancel_active();
        self.downloader.shutdown();
        if let Some(active) = self.active.take() {
            self.abandon(active.staging);
        }
    }
}

pub(crate) fn poll_replay_cache(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let snapshot = {
        let state = state.borrow();
        (
            state.source_generation,
            state.current_url.clone(),
            state
                .mpv
                .as_ref()
                .and_then(|mpv| mpv.observed_playback_state().duration),
            state.media_load_state == network_media::MediaLoadState::Playing,
            state.settings.replay_cache_enabled(),
            state.private_session,
        )
    };
    let notices = state.borrow_mut().replay_cache.poll(
        snapshot.0,
        snapshot.1.as_deref(),
        snapshot.2,
        snapshot.3,
        snapshot.4,
        snapshot.5,
    );
    for notice in notices {
        status_toast.show(&notice);
    }
}

pub(crate) fn set_replay_cache_enabled(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    enabled: bool,
) {
    let mut state = state.borrow_mut();
    state.settings.set_replay_cache_enabled(enabled);
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save replay-cache setting: {error}");
        status_toast.show("Could not save replay-cache setting");
        return;
    }
    if enabled {
        state.replay_cache.status = if state.replay_cache.tool_available {
            ReplayCacheStatusSnapshot::Idle
        } else {
            ReplayCacheStatusSnapshot::ToolUnavailable
        };
        status_toast.show("Replay cache on");
    } else {
        state.replay_cache.candidate = None;
        state.replay_cache.cancel_active();
        state.replay_cache.status = ReplayCacheStatusSnapshot::Disabled;
        status_toast.show("Replay cache off");
    }
}

pub(crate) fn cancel_replay_cache_download(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) -> bool {
    let cancelled = state.borrow_mut().replay_cache.cancel_active();
    if cancelled {
        status_toast.show("Canceling replay download…");
    }
    cancelled
}

pub(crate) fn clear_replay_cache(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    match state.borrow_mut().replay_cache.clear() {
        Ok((0, 0)) => status_toast.show("Replay cache was already empty"),
        Ok((removed, 0)) => status_toast.show(&format!(
            "Cleared {removed} cached replay{}",
            if removed == 1 { "" } else { "s" }
        )),
        Ok((removed, pinned)) => status_toast.show(&format!(
            "Cleared {removed}; kept {pinned} active replay{}",
            if pinned == 1 { "" } else { "s" }
        )),
        Err(error) => {
            eprintln!("Failed to clear replay cache: {error}");
            status_toast.show("Could not clear replay cache");
        }
    }
}

pub(crate) fn suspend_replay_cache_for_private_session(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.replay_cache.candidate = None;
    state.replay_cache.cancel_active();
    state.replay_cache.status = ReplayCacheStatusSnapshot::Private;
}

pub(crate) fn shutdown_replay_cache(state: &Rc<RefCell<PlayerState>>) {
    state.borrow_mut().replay_cache.shutdown();
}

fn replay_cache_dir() -> PathBuf {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(cache_home).join("ok-player/replay-cache");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".cache/ok-player/replay-cache");
    }
    env::temp_dir().join("ok-player/replay-cache")
}

fn unix_now_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn unix_now_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(1)
}

fn cache_owner_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // Signal zero performs a liveness check without signalling the process.
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryStore;
    use okp_core::media_download::DownloadedMedia;

    #[test]
    fn switched_stream_remains_eligible_when_previous_download_finishes() {
        let root = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(root.path(), 1024).unwrap();
        let url = "https://example.com/current";
        let completed = cache.begin_download(url, DownloadJobId(1)).unwrap();
        let file = completed
            .download_target()
            .staging_directory
            .join("media.mp4");
        fs::write(&file, b"complete").unwrap();
        cache
            .complete_download(completed, DownloadedMedia::new(file, 8).unwrap(), 1)
            .unwrap();
        let previous = cache
            .begin_download("https://example.com/previous", DownloadJobId(2))
            .unwrap();
        let mut runtime = ReplayCacheRuntime {
            cache: Some(cache),
            cache_open_attempted: true,
            tool_available: true,
            active: Some(ActiveReplayDownload {
                job_id: DownloadJobId(2),
                staging: previous,
            }),
            ..ReplayCacheRuntime::default()
        };
        runtime.source_changed();
        runtime.offer_stream(url.into(), 7, true, false, None);
        runtime.try_start(7, Some(url), Some(30.0), true);
        assert_eq!(runtime.active.as_ref().unwrap().job_id, DownloadJobId(2));
        let previous = runtime.active.take().unwrap();
        runtime.abandon(previous.staging);
        runtime.poll(7, Some(url), Some(30.0), true, true, false);
        assert_eq!(runtime.status_snapshot(), ReplayCacheStatusSnapshot::Ready);
        assert!(runtime.candidate.is_none());
    }

    #[test]
    fn disabled_and_private_policy_survive_queued_download_terminal() {
        for private in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut runtime = ReplayCacheRuntime {
                root: root.path().to_owned(),
                tool_available: true,
                downloader: media_download::MediaDownloader::with_executable(PathBuf::from(
                    "/bin/false",
                )),
                ..ReplayCacheRuntime::default()
            };
            let url = "https://example.com/video";
            runtime.offer_stream(url.into(), 1, true, false, None);
            runtime.try_start(1, Some(url), Some(30.0), true);
            assert!(runtime.active.is_some());
            runtime.downloader.shutdown();
            runtime.poll(1, Some(url), Some(30.0), true, false, private);
            assert!(runtime.active.is_none());
            assert_eq!(
                runtime.status_snapshot(),
                if private {
                    ReplayCacheStatusSnapshot::Private
                } else {
                    ReplayCacheStatusSnapshot::Disabled
                }
            );
        }
    }

    #[test]
    fn cached_fallback_preserves_history_title_and_current_decoder_notices() {
        let root = tempfile::tempdir().unwrap();
        let url = "https://example.com/episode";
        let physical = root.path().join("cached.mp4");
        let mut history = HistoryStore::open_test(root.path().join("history.json"));
        history.record_source_opened(
            &PlaylistItem::Url(url.into()),
            Some(30.0),
            false,
            okp_core::nfo_metadata::HistoryTitleUpdate::Set("Original episode title".into()),
        );
        let state = Rc::new(RefCell::new(PlayerState {
            history,
            current_url: Some(url.into()),
            replay_engine_path: Some(physical.clone()),
            url_history_load_confirmed: true,
            mpv: Some(Mpv::new().expect("libmpv is required by the existing GTK suite")),
            replay_cache: ReplayCacheRuntime {
                root: root.path().join("cache"),
                ..ReplayCacheRuntime::default()
            },
            ..PlayerState::default()
        }));
        let messages = ["vd: Failed to initialize a decoder for codec h264".to_owned()];
        assert!(runtime_decoder_notice(&state, physical.to_str(), &messages).is_some());
        assert!(runtime_decoder_notice(&state, Some("/stale.mp4"), &messages).is_none());
        assert!(!fallback_cached_replay_to_url(&state, Some("/stale.mp4")));
        assert!(fallback_cached_replay_to_url(&state, physical.to_str()));
        assert_eq!(state.borrow().current_url.as_deref(), Some(url));
        assert!(state.borrow().replay_engine_path.is_none());
        let persisted = fs::read_to_string(root.path().join("history.json")).unwrap();
        assert!(persisted.contains("Original episode title"));
    }

    #[test]
    fn cached_fallback_does_not_restore_an_explicitly_removed_history_row() {
        let root = tempfile::tempdir().unwrap();
        let url = "https://example.com/removed";
        let source = PlaylistItem::Url(url.into());
        let mut history = HistoryStore::open_test(root.path().join("history.json"));
        history.record_source_opened(
            &source,
            Some(30.0),
            false,
            okp_core::nfo_metadata::HistoryTitleUpdate::Set("Removed".into()),
        );
        history.remove_source_persisted(&source, true).unwrap();
        let physical = root.path().join("cached.mp4");
        let state = Rc::new(RefCell::new(PlayerState {
            history,
            current_url: Some(url.into()),
            replay_engine_path: Some(physical.clone()),
            replay_cache: ReplayCacheRuntime {
                root: root.path().join("cache"),
                ..ReplayCacheRuntime::default()
            },
            ..PlayerState::default()
        }));
        assert!(fallback_cached_replay_to_url(&state, physical.to_str()));
        state.borrow_mut().media_load_state = network_media::MediaLoadState::Playing;
        record_successful_url_open(&state);
        assert!(state.borrow().history.is_source_suppressed(&source));
        assert!(state.borrow().history.search("").is_empty());
    }

    #[test]
    fn history_replay_keeps_url_title_and_path_bound_poster_after_another_source() {
        let root = tempfile::tempdir().unwrap();
        let cache = ReplayCache::open(root.path().join("replay"), 1024).unwrap();
        let url = "https://example.com/episode";
        let staging = cache.begin_download(url, DownloadJobId(1)).unwrap();
        let media = staging
            .download_target()
            .staging_directory
            .join("media.mp4");
        fs::write(&media, b"complete").unwrap();
        let path = cache
            .complete_download(staging, DownloadedMedia::new(media, 8).unwrap(), 1)
            .unwrap()
            .path;
        let history_path = root.path().join("history.json");
        let mut history = HistoryStore::open_test(history_path.clone());
        history.record_source_opened(
            &PlaylistItem::Url(url.into()),
            Some(100.0),
            false,
            okp_core::nfo_metadata::HistoryTitleUpdate::Set("Original episode".into()),
        );
        history.save().unwrap();
        let state = Rc::new(RefCell::new(PlayerState {
            history,
            replay_cache: ReplayCacheRuntime {
                root: root.path().join("replay"),
                cache: Some(cache),
                cache_open_attempted: true,
                ..ReplayCacheRuntime::default()
            },
            screenshot_jobs: screenshots::ScreenshotJobs::with_poster_directory(
                root.path().join("posters"),
            ),
            ..PlayerState::default()
        }));
        remember_loaded_url(&state, "https://example.com/previous".into());
        assert!(load_history_url(&state, url.into()));
        assert_eq!(state.borrow().current_url.as_deref(), Some(url));
        assert!(state.borrow().current_file.is_none());
        assert_eq!(state.borrow().replay_engine_path.as_ref(), Some(&path));
        assert!(!current_engine_path_matches(
            &state,
            "https://example.com/previous"
        ));
        assert!(current_engine_path_matches(&state, path.to_str().unwrap()));
        state.borrow_mut().media_load_state = network_media::MediaLoadState::Playing;
        record_successful_url_open(&state);
        record_ready_url_poster(&state, Some("https://example.com/previous"));
        assert_eq!(state.borrow().screenshot_jobs.poster_request_count(), 0);
        record_ready_url_poster(&state, path.to_str());
        assert_eq!(state.borrow().screenshot_jobs.poster_request_count(), 1);
        let history = fs::read_to_string(history_path).unwrap();
        assert!(history.contains(url));
        assert!(history.contains("Original episode"));
        assert!(!history.contains(path.to_str().unwrap()));
        fs::remove_file(path).unwrap();
        assert!(load_history_url(&state, url.into()));
        assert!(state.borrow().replay_engine_path.is_none());
        assert_eq!(state.borrow().current_url.as_deref(), Some(url));
    }

    #[test]
    fn private_history_open_does_not_acquire_or_start_persistent_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache_root = root.path().join("replay");
        let state = Rc::new(RefCell::new(PlayerState {
            private_session: true,
            history: HistoryStore::open_test(root.path().join("history.json")),
            replay_cache: ReplayCacheRuntime {
                root: cache_root.clone(),
                ..ReplayCacheRuntime::default()
            },
            ..PlayerState::default()
        }));
        assert!(load_history_url(
            &state,
            "https://example.com/private-session".into()
        ));
        let mut state = state.borrow_mut();
        assert!(state.replay_engine_path.is_none());
        state.replay_cache.poll(
            1,
            Some("https://example.com/private-session"),
            Some(30.0),
            true,
            true,
            true,
        );
        assert!(state.replay_cache.active.is_none());
        assert!(!state.replay_cache.cache_open_attempted);
        assert!(!cache_root.exists());
    }
}
