//! Local sidecar lyrics orchestration for [`crate::Session`].
//!
//! Sidecar discovery, bounded UTF-8 loading, LRC parsing, fallback copy, and
//! active-line timing all live in [`okp_core::lyrics`] and [`okp_core::lrc`].
//! This module owns the asynchronous off-thread sidecar load, source-generation
//! stale-track guards, and the typed [`LyricsView`] query projection so shells
//! never read `.lrc` files or parse timestamps themselves.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use okp_core::lyrics::{LYRICS_HEADER, LocalLyrics, LocalLyricsStatus, load_sidecar};
use serde::{Deserialize, Serialize};

use crate::protocol::{MediaSource, SourceKind};

/// Resolution state of local lyrics for the current track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsState {
    /// No loaded audio-only track (idle, opening, error, or video).
    Unavailable,
    /// Local sidecar lookup is running on a background thread.
    Loading,
    /// Synced `.lrc` sheet with timestamps; highlight, auto-scroll, and seek are active.
    Synced,
    /// Plain untimed `.lrc` sheet; static readable scroll without seek.
    Plain,
    /// No local `.lrc` sidecar exists for this track.
    Missing,
    /// A sidecar file exists but is unreadable, oversized, invalid UTF-8, or contains no lines.
    Invalid,
}

/// One renderable lyric line projected for a shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LyricsLineView {
    /// Timestamp in seconds (`0.0` for plain untimed lyrics).
    pub time_seconds: f64,
    /// Renderable line text (`"♪"` for an intentional instrumental gap in a synced sheet).
    pub text: String,
    /// True when this row represents an empty/instrumental gap line.
    pub is_gap: bool,
}

/// What a shell renders for local lyrics of the current track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LyricsView {
    /// The media source this view belongs to, or `None` when `state` is `unavailable`.
    /// Shells compare this against their active track so stale lyrics never render.
    pub source: Option<MediaSource>,
    /// Monotonic generation incremented on every accepted `open` or `close`.
    pub generation: u64,
    /// Canonical header (`"LYRICS"` or `"LYRICS · NOT SYNCED"`).
    pub header: String,
    pub state: LyricsState,
    /// Honest fallback message when `state` is `missing` or `invalid`.
    pub message: Option<String>,
    /// Renderable lines; empty for `unavailable`, `loading`, `missing`, and `invalid`.
    pub lines: Vec<LyricsLineView>,
    /// Index of the active line at the projected playback position (synced only).
    pub active_index: Option<usize>,
}

pub(crate) trait LyricsLoader: Send + Sync {
    fn load(&self, path: &Path) -> LocalLyrics;
}

struct HostLyricsLoader;

impl LyricsLoader for HostLyricsLoader {
    fn load(&self, path: &Path) -> LocalLyrics {
        load_sidecar(path)
    }
}

struct FinishedLoad {
    generation: u64,
    source: MediaSource,
    lyrics: LocalLyrics,
}

enum LoadedData {
    Unavailable,
    Loading,
    Ready(LocalLyrics),
}

/// Session-owned local sidecar lyrics state with background loading and
/// strict source-generation rejection of stale results.
pub(crate) struct SessionLyrics {
    generation: u64,
    requested_source: Option<MediaSource>,
    loaded_source: Option<MediaSource>,
    data: LoadedData,
    cancel: Arc<AtomicBool>,
    inbox: Arc<Mutex<Option<FinishedLoad>>>,
    waker: Arc<dyn Fn() + Send + Sync>,
    loader: Arc<dyn LyricsLoader>,
}

impl SessionLyrics {
    pub(crate) fn new(waker: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self::with_loader(waker, Arc::new(HostLyricsLoader))
    }

    pub(crate) fn with_loader(
        waker: Arc<dyn Fn() + Send + Sync>,
        loader: Arc<dyn LyricsLoader>,
    ) -> Self {
        Self {
            generation: 0,
            requested_source: None,
            loaded_source: None,
            data: LoadedData::Unavailable,
            cancel: Arc::new(AtomicBool::new(false)),
            inbox: Arc::new(Mutex::new(None)),
            waker,
            loader,
        }
    }

    /// Start a new media generation on every accepted `open` or `close`.
    /// Cancels any in-flight sidecar read and immediately discards the previous
    /// track's lyrics so stale data can never follow a track switch. Returns
    /// `true` when visible lyrics state changed (was not already unavailable).
    pub(crate) fn begin_source(&mut self) -> bool {
        let changed = self.requested_source.is_some()
            || self.loaded_source.is_some()
            || !matches!(self.data, LoadedData::Unavailable);
        self.generation = self.generation.wrapping_add(1);
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.requested_source = None;
        self.loaded_source = None;
        self.data = LoadedData::Unavailable;
        self.clear_inbox();
        changed
    }

    /// Reconcile against the current loaded media. Returns `true` when the
    /// projected lyrics state changed (so the session should emit `lyrics_changed`).
    pub(crate) fn observe(&mut self, source: Option<&MediaSource>, is_audio: bool) -> bool {
        let Some(source) = source.filter(|_| is_audio) else {
            return self.clear_to_unavailable();
        };

        if self.requested_source.as_ref() == Some(source) {
            return false;
        }

        // Source changed without a prior `begin_source` (e.g., engine-driven change).
        self.generation = self.generation.wrapping_add(1);
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.clear_inbox();
        self.requested_source = Some(source.clone());

        if source.kind == SourceKind::Url {
            self.loaded_source = Some(source.clone());
            self.data = LoadedData::Ready(LocalLyrics::missing());
            return true;
        }

        self.loaded_source = None;
        self.data = LoadedData::Loading;
        self.spawn_load(self.generation, source.clone());
        true
    }

    /// Apply a completed background sidecar load if it still matches the active
    /// generation and source; drop stale completions silently.
    pub(crate) fn poll(&mut self) -> bool {
        let finished = self
            .inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let Some(finished) = finished else {
            return false;
        };
        if finished.generation != self.generation
            || self.requested_source.as_ref() != Some(&finished.source)
        {
            return false;
        }
        self.loaded_source = Some(finished.source);
        self.data = LoadedData::Ready(finished.lyrics);
        true
    }

    /// Project the current lyrics state at `position_seconds`.
    #[must_use]
    pub(crate) fn view(&self, position_seconds: Option<f64>) -> LyricsView {
        match &self.data {
            LoadedData::Unavailable => LyricsView {
                source: None,
                generation: self.generation,
                header: LYRICS_HEADER.to_owned(),
                state: LyricsState::Unavailable,
                message: None,
                lines: Vec::new(),
                active_index: None,
            },
            LoadedData::Loading => LyricsView {
                source: self.requested_source.clone(),
                generation: self.generation,
                header: LYRICS_HEADER.to_owned(),
                state: LyricsState::Loading,
                message: None,
                lines: Vec::new(),
                active_index: None,
            },
            LoadedData::Ready(local) => project_local(
                self.loaded_source.clone(),
                self.generation,
                local,
                position_seconds,
            ),
        }
    }

    fn clear_to_unavailable(&mut self) -> bool {
        if self.requested_source.is_none()
            && self.loaded_source.is_none()
            && matches!(self.data, LoadedData::Unavailable)
        {
            return false;
        }
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.requested_source = None;
        self.loaded_source = None;
        self.data = LoadedData::Unavailable;
        self.clear_inbox();
        true
    }

    fn clear_inbox(&self) {
        *self
            .inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    fn spawn_load(&self, generation: u64, source: MediaSource) {
        let path = PathBuf::from(&source.location);
        let cancel = Arc::clone(&self.cancel);
        let inbox = Arc::clone(&self.inbox);
        let waker = Arc::clone(&self.waker);
        let loader = Arc::clone(&self.loader);
        let _ = thread::Builder::new()
            .name("okp-session-lyrics".to_owned())
            .spawn(move || {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let lyrics = loader.load(&path);
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                *inbox
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(FinishedLoad {
                    generation,
                    source,
                    lyrics,
                });
                waker();
            });
    }
}

pub(crate) fn project_local(
    source: Option<MediaSource>,
    generation: u64,
    local: &LocalLyrics,
    position_seconds: Option<f64>,
) -> LyricsView {
    let state = match local.status {
        LocalLyricsStatus::Synced => LyricsState::Synced,
        LocalLyricsStatus::Plain => LyricsState::Plain,
        LocalLyricsStatus::Missing => LyricsState::Missing,
        LocalLyricsStatus::Invalid => LyricsState::Invalid,
    };
    let lines = match local.status {
        LocalLyricsStatus::Synced | LocalLyricsStatus::Plain => local
            .document
            .lines
            .iter()
            .map(|line| {
                let (text, is_gap) = LocalLyrics::display_line_text(line);
                LyricsLineView {
                    time_seconds: line.time_seconds,
                    text: text.to_owned(),
                    is_gap,
                }
            })
            .collect(),
        LocalLyricsStatus::Missing | LocalLyricsStatus::Invalid => Vec::new(),
    };
    let active_index = position_seconds.and_then(|pos| local.active_index(pos));
    LyricsView {
        source,
        generation,
        header: local.header().to_owned(),
        state,
        message: local.fallback_message().map(str::to_owned),
        lines,
        active_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_core::lrc;
    use std::fs;
    use std::sync::Condvar;
    use std::time::{Duration, Instant};

    fn local_source(path: &Path) -> MediaSource {
        MediaSource {
            kind: SourceKind::Local,
            location: path.to_string_lossy().into_owned(),
        }
    }

    fn wait_for_poll(lyrics: &mut SessionLyrics) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if lyrics.poll() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for background lyrics load");
    }

    #[test]
    fn loads_synced_plain_missing_and_invalid_sidecars_and_computes_active_index() {
        let dir = okp_test_fixtures::unique_temp_dir("okp-session-lyrics");

        let synced_path = dir.path().join("track-a.mp3");
        fs::write(&synced_path, b"audio").unwrap();
        fs::write(
            dir.path().join("track-a.lrc"),
            "[00:01.00]Line one\n[00:03.50]\n[00:05.00]Line three\n",
        )
        .unwrap();

        let plain_path = dir.path().join("track-b.mp3");
        fs::write(&plain_path, b"audio").unwrap();
        fs::write(dir.path().join("track-b.lrc"), "Plain one\nPlain two\n").unwrap();

        let invalid_path = dir.path().join("track-c.mp3");
        fs::write(&invalid_path, b"audio").unwrap();
        fs::write(dir.path().join("track-c.lrc"), [0xff, 0xfe, 0x00, 0x01]).unwrap();

        let missing_path = dir.path().join("track-d.mp3");
        fs::write(&missing_path, b"audio").unwrap();

        let mut lyrics = SessionLyrics::new(Arc::new(|| {}));
        assert_eq!(lyrics.view(Some(0.0)).state, LyricsState::Unavailable);

        // Synced track
        lyrics.begin_source();
        let src_a = local_source(&synced_path);
        assert!(lyrics.observe(Some(&src_a), true));
        assert_eq!(lyrics.view(Some(2.0)).state, LyricsState::Loading);
        wait_for_poll(&mut lyrics);

        let before = lyrics.view(Some(0.5));
        assert_eq!(before.state, LyricsState::Synced);
        assert_eq!(before.source.as_ref(), Some(&src_a));
        assert_eq!(before.header, "LYRICS");
        assert_eq!(before.lines.len(), 3);
        assert_eq!(before.active_index, None);
        assert!(before.lines[1].is_gap);
        assert_eq!(before.lines[1].text, "♪");

        assert_eq!(lyrics.view(Some(1.5)).active_index, Some(0));
        assert_eq!(lyrics.view(Some(4.0)).active_index, Some(1));
        assert_eq!(lyrics.view(Some(6.0)).active_index, Some(2));
        // Backward seek moves active_index back cleanly.
        assert_eq!(lyrics.view(Some(1.2)).active_index, Some(0));

        // Switch to plain track: immediately drops Track A before Track B finishes.
        lyrics.begin_source();
        let cleared = lyrics.view(Some(1.2));
        assert_eq!(cleared.state, LyricsState::Unavailable);
        assert!(cleared.lines.is_empty());

        let src_b = local_source(&plain_path);
        assert!(lyrics.observe(Some(&src_b), true));
        wait_for_poll(&mut lyrics);
        let plain = lyrics.view(Some(2.0));
        assert_eq!(plain.state, LyricsState::Plain);
        assert_eq!(plain.header, "LYRICS · NOT SYNCED");
        assert_eq!(plain.active_index, None);
        assert_eq!(plain.lines.len(), 2);

        // Invalid sidecar track.
        lyrics.begin_source();
        let src_c = local_source(&invalid_path);
        assert!(lyrics.observe(Some(&src_c), true));
        wait_for_poll(&mut lyrics);
        let invalid = lyrics.view(Some(2.0));
        assert_eq!(invalid.state, LyricsState::Invalid);
        assert!(invalid.lines.is_empty());
        assert_eq!(
            invalid.message.as_deref(),
            Some(okp_core::lyrics::LYRICS_INVALID_MESSAGE)
        );

        // Missing sidecar track.
        lyrics.begin_source();
        let src_d = local_source(&missing_path);
        assert!(lyrics.observe(Some(&src_d), true));
        wait_for_poll(&mut lyrics);
        let missing = lyrics.view(Some(2.0));
        assert_eq!(missing.state, LyricsState::Missing);
        assert!(missing.lines.is_empty());
        assert_eq!(
            missing.message.as_deref(),
            Some(okp_core::lyrics::LYRICS_EMPTY_MESSAGE)
        );

        // Video or non-audio clears to Unavailable.
        assert!(lyrics.observe(Some(&src_a), false));
        assert_eq!(lyrics.view(Some(2.0)).state, LyricsState::Unavailable);
    }

    #[test]
    fn slow_previous_track_load_never_overwrites_a_newer_track() {
        struct GateLoader {
            entered: (Mutex<bool>, Condvar),
            release: (Mutex<bool>, Condvar),
        }

        impl LyricsLoader for GateLoader {
            fn load(&self, path: &Path) -> LocalLyrics {
                if path.ends_with("slow-a.mp3") {
                    let (lock, cv) = &self.entered;
                    *lock.lock().unwrap() = true;
                    cv.notify_all();
                    let (rel_lock, rel_cv) = &self.release;
                    let mut released = rel_lock.lock().unwrap();
                    while !*released {
                        released = rel_cv.wait(released).unwrap();
                    }
                    return LocalLyrics::from_document(lrc::parse(Some("[00:01.00]Stale track A")));
                }
                LocalLyrics::missing()
            }
        }

        let loader = Arc::new(GateLoader {
            entered: (Mutex::new(false), Condvar::new()),
            release: (Mutex::new(false), Condvar::new()),
        });
        let mut lyrics = SessionLyrics::with_loader(Arc::new(|| {}), loader.clone());

        // Start Track A load and wait until its background thread is inside `load`.
        lyrics.begin_source();
        let src_a = MediaSource {
            kind: SourceKind::Local,
            location: "/music/slow-a.mp3".to_owned(),
        };
        assert!(lyrics.observe(Some(&src_a), true));
        {
            let (lock, cv) = &loader.entered;
            let mut entered = lock.lock().unwrap();
            while !*entered {
                entered = cv.wait(entered).unwrap();
            }
        }

        // Switch to Track B while Track A is still loading.
        lyrics.begin_source();
        let src_b = MediaSource {
            kind: SourceKind::Local,
            location: "/music/fast-b.mp3".to_owned(),
        };
        assert!(lyrics.observe(Some(&src_b), true));
        wait_for_poll(&mut lyrics);
        assert_eq!(lyrics.view(Some(2.0)).state, LyricsState::Missing);
        assert_eq!(lyrics.view(Some(2.0)).source.as_ref(), Some(&src_b));

        // Now release Track A's stalled load thread and give it time to finish.
        {
            let (lock, cv) = &loader.release;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        thread::sleep(Duration::from_millis(40));
        assert!(!lyrics.poll(), "stale Track A completion must be dropped");

        let view = lyrics.view(Some(2.0));
        assert_eq!(view.source.as_ref(), Some(&src_b));
        assert_eq!(view.state, LyricsState::Missing);
        assert!(view.lines.is_empty());
    }
}
