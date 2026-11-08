//! The watch-history file: the okp-core history document in one JSON file under
//! the shell's data directory.
//!
//! Lifted from the GTK shell's `HistoryStore` together with its tests. The file
//! keeps the Linux schema (`okp_core::history::History`, pretty-printed), so a
//! history written by either build loads in the other. Writes go to a temporary
//! file that is flushed and then renamed over the real one, so an interrupted
//! write leaves the previous file intact.
//!
//! Bookmarks, clearing and retention pruning stay in the GTK store until their
//! macOS issues lift them.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use okp_core::history::{
    History, HistoryOpenIntent, HistoryOpenUpdate, HistoryProgressUpdate, HistoryRecordingSession,
    HistoryWriteMode, HistoryWriteResult, Preferences,
};
use okp_core::nfo_metadata::HistoryTitleUpdate;
use okp_core::playlist::PlaylistItem;
use okp_core::recents_shelf::{HistoryItem, WelcomeShelf};

/// File name of the history document inside the data directory, as on Linux.
pub const HISTORY_FILE_NAME: &str = "history.json";

/// The history document, the set of entries worth listing, and the per-session
/// write guards. Every mutation marks the store dirty; [`HistoryStore::save`]
/// writes it.
#[derive(Debug)]
pub struct HistoryStore {
    /// Where the document lives; `None` keeps it in memory only.
    path: Option<PathBuf>,
    data: History,
    listable_paths: BTreeSet<String>,
    recording_session: HistoryRecordingSession,
    dirty: bool,
    read_failed: bool,
    /// Unix seconds now; a parameter so tests can pin the clock.
    clock: fn() -> i64,
}

impl HistoryStore {
    /// Open the history in `data_dir`. A missing file is an empty history; an
    /// unreadable or unrecognised one is an empty history with
    /// [`HistoryStore::read_failed`] set, and is replaced by the next save.
    pub fn open(data_dir: &Path) -> Self {
        Self::open_path(data_dir.join(HISTORY_FILE_NAME))
    }

    /// Open the history document at `path`.
    pub fn open_path(path: PathBuf) -> Self {
        Self::open_path_with_clock(path, unix_now)
    }

    pub(crate) fn open_path_with_clock(path: PathBuf, clock: fn() -> i64) -> Self {
        let (data, read_failed) = match fs::read_to_string(&path) {
            Ok(json) => match History::load(&json) {
                Some(data) => (data, false),
                None => (History::default(), true),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => (History::default(), false),
            Err(_) => (History::default(), true),
        };
        let mut store = Self::with_data(Some(path), data, clock);
        store.read_failed = read_failed;
        store
    }

    /// A history that is never written anywhere.
    pub fn in_memory() -> Self {
        Self::with_data(None, History::default(), unix_now)
    }

    fn with_data(path: Option<PathBuf>, data: History, clock: fn() -> i64) -> Self {
        let listable_paths = data
            .files
            .keys()
            .filter(|path| is_history_path_listable(path))
            .cloned()
            .collect();
        Self {
            path,
            data,
            listable_paths,
            recording_session: HistoryRecordingSession::default(),
            dirty: false,
            read_failed: false,
            clock,
        }
    }

    /// The file could not be read or parsed when the store was opened.
    pub fn read_failed(&self) -> bool {
        self.read_failed
    }

    /// Changes not yet written.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Unix seconds now, by the store's clock.
    pub fn now_unix(&self) -> i64 {
        (self.clock)()
    }

    /// Record a progress sample for `source`.
    pub fn record_source_with_title(
        &mut self,
        source: &PlaylistItem,
        position: f64,
        duration: f64,
        finished: bool,
        private_session: bool,
        title_update: HistoryTitleUpdate,
    ) {
        let key = source.history_key();
        let mode = self.write_mode(source, private_session);
        let result = self.data.record_progress(
            &key,
            HistoryProgressUpdate {
                position,
                duration,
                finished,
                updated_at_unix: self.now_unix(),
                title: title_update,
            },
            mode,
        );
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
    }

    /// Record the engine's successful-open lifecycle edge. Unlike progress samples,
    /// this remains valid for live sources whose duration is unknown.
    pub fn record_source_opened(
        &mut self,
        source: &PlaylistItem,
        duration: Option<f64>,
        private_session: bool,
        title_update: HistoryTitleUpdate,
    ) {
        let key = source.history_key();
        let mode = self.write_mode(source, private_session);
        let result = self.data.record_opened(
            &key,
            HistoryOpenUpdate {
                duration,
                updated_at_unix: self.now_unix(),
                title: title_update,
            },
            mode,
        );
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
    }

    /// Release a per-source removal guard only for a user-initiated reopen.
    pub fn begin_source_open(&mut self, source: &PlaylistItem, intent: HistoryOpenIntent) {
        self.recording_session.source_opened(source, intent);
    }

    #[cfg(test)]
    pub(crate) fn is_source_suppressed(&self, source: &PlaylistItem) -> bool {
        self.recording_session.is_suppressed(source)
    }

    /// Mark `source` watched to the end, clearing its resume position so the next
    /// open starts at zero. Returns the entry's stored duration when a record was
    /// actually changed.
    pub fn mark_source_finished(
        &mut self,
        source: &PlaylistItem,
        private_session: bool,
    ) -> Option<f64> {
        let key = source.history_key();
        let mode = self.write_mode(source, private_session);
        let result = self.data.mark_finished(&key, self.now_unix(), mode);
        if result != HistoryWriteResult::Changed {
            return None;
        }
        self.dirty = true;
        self.data.files.get(&key).map(|record| record.duration)
    }

    /// Where reopening `source` should start, by the core resume rule.
    pub fn resume_position_for_source(&self, source: &PlaylistItem) -> Option<f64> {
        self.data.resume_position(&source.history_key())
    }

    /// Merge per-file playback preferences for `source`.
    pub fn record_source_preferences(
        &mut self,
        source: &PlaylistItem,
        preferences: Preferences,
        private_session: bool,
    ) {
        let key = source.history_key();
        let mode = self.write_mode(source, private_session);
        let result = self
            .data
            .record_preferences(&key, preferences, self.now_unix(), mode);
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
    }

    /// The preferences remembered for `source`, if any.
    pub fn playback_preferences_for_source(&self, source: &PlaylistItem) -> Option<Preferences> {
        self.data
            .files
            .get(&source.history_key())
            .map(|record| record.preferences.clone())
            .filter(|preferences| !preferences.is_empty())
    }

    /// Ranked model for the idle Continue Watching shelf. Private sessions gate
    /// writes only; existing records remain readable.
    pub fn welcome_shelf(&self, limit: usize) -> WelcomeShelf {
        okp_core::recents_shelf::select_where(&self.data, limit, |path| {
            self.listable_paths.contains(path)
        })
    }

    /// Newest-first rows for the explicit History surface, filtered in shared core.
    pub fn search(&self, query: &str) -> Vec<HistoryItem> {
        okp_core::recents_shelf::search_where(&self.data, query, |path| {
            self.listable_paths.contains(path)
        })
    }

    /// Remove one exact local-or-URL identity and persist it atomically. If `active` is true,
    /// every later incidental write for that source is suppressed until an explicit reopen.
    /// A failed save restores both the record and the session guard.
    pub fn remove_source_persisted(
        &mut self,
        source: &PlaylistItem,
        active: bool,
    ) -> io::Result<bool> {
        let before = self.snapshot();
        let key = source.history_key();
        if !self.data.remove(&key) {
            return Ok(false);
        }
        self.listable_paths.remove(&key);
        if active {
            self.recording_session.suppress(source);
        }
        self.dirty = true;
        if let Err(error) = self.save() {
            self.restore(before);
            return Err(error);
        }
        Ok(true)
    }

    /// Write the document if anything changed since the last save.
    pub fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(path) = &self.path {
            let json = serde_json::to_vec_pretty(&self.data).map_err(io::Error::other)?;
            write_atomically(path, &json, |file, bytes| file.write_all(bytes))?;
        }
        self.dirty = false;
        self.read_failed = false;
        Ok(())
    }

    fn write_mode(&self, source: &PlaylistItem, private_session: bool) -> HistoryWriteMode {
        self.recording_session.write_mode(source, private_session)
    }

    fn snapshot(&self) -> HistoryStoreSnapshot {
        HistoryStoreSnapshot {
            data: self.data.clone(),
            listable_paths: self.listable_paths.clone(),
            recording_session: self.recording_session.clone(),
            dirty: self.dirty,
            read_failed: self.read_failed,
        }
    }

    fn restore(&mut self, snapshot: HistoryStoreSnapshot) {
        self.data = snapshot.data;
        self.listable_paths = snapshot.listable_paths;
        self.recording_session = snapshot.recording_session;
        self.dirty = snapshot.dirty;
        self.read_failed = snapshot.read_failed;
    }
}

struct HistoryStoreSnapshot {
    data: History,
    listable_paths: BTreeSet<String>,
    recording_session: HistoryRecordingSession,
    dirty: bool,
    read_failed: bool,
}

/// Replace `path` with `bytes` so that a reader sees either the old file or the
/// new one, never a part of either: `write` fills a temporary file next to it,
/// which is flushed to disk and then renamed over `path`. On failure the
/// temporary file is removed and `path` is untouched.
fn write_atomically(
    path: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let result = File::create(&temporary).and_then(|mut file| {
        write(&mut file, bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn is_history_path_listable(path: &str) -> bool {
    path.contains("://")
        || okp_core::network_path::is_network(path, |_| None)
        || Path::new(path).is_file()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;

    fn local(path: &str) -> PlaylistItem {
        PlaylistItem::Local(PathBuf::from(path))
    }

    fn store() -> HistoryStore {
        HistoryStore::in_memory()
    }

    /// A store whose path cannot be written: `/dev/null` exists but is not a directory,
    /// so `create_dir_all` on the parent fails and every `save()` errors deterministically
    /// without touching the real filesystem.
    fn unwritable_store() -> HistoryStore {
        HistoryStore::with_data(
            Some(PathBuf::from("/dev/null/history.json")),
            History::default(),
            unix_now,
        )
    }

    fn record(
        history: &mut HistoryStore,
        path: &str,
        position: f64,
        duration: f64,
        finished: bool,
    ) {
        history.record_source_with_title(
            &local(path),
            position,
            duration,
            finished,
            false,
            HistoryTitleUpdate::Preserve,
        );
    }

    #[test]
    fn returns_resume_position_for_middle_of_file() {
        let mut history = store();
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);
        assert_eq!(
            history.resume_position_for_source(&local("/media/movie.mkv")),
            Some(120.0)
        );
    }

    #[test]
    fn skips_resume_in_first_five_percent() {
        let mut history = store();
        record(&mut history, "/media/movie.mkv", 30.0, 600.0, false);
        assert_eq!(
            history.resume_position_for_source(&local("/media/movie.mkv")),
            None
        );
    }

    #[test]
    fn skips_resume_in_completion_window() {
        let mut history = store();
        record(
            &mut history,
            "/media/movie.mkv",
            okp_core::recents_shelf::completion_start(600.0),
            600.0,
            false,
        );
        assert_eq!(
            history.resume_position_for_source(&local("/media/movie.mkv")),
            None
        );
    }

    #[test]
    fn marking_finished_clears_the_resume_position_without_a_final_sample() {
        // The shell reaches end of file with no position to report; the store must still
        // stop the next open from resuming, and must persist that.
        let mut history = store();
        let source = local("/media/movie.mkv");
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);
        history.dirty = false;

        assert_eq!(history.mark_source_finished(&source, false), Some(600.0));
        assert_eq!(history.resume_position_for_source(&source), None);
        assert!(history.is_dirty());
    }

    #[test]
    fn marking_finished_reports_nothing_for_an_unknown_file() {
        let mut history = store();
        assert_eq!(
            history.mark_source_finished(&local("/media/never-seen.mkv"), false),
            None
        );
    }

    #[test]
    fn a_private_session_marks_nothing_finished() {
        let mut history = store();
        let source = local("/media/movie.mkv");
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);

        assert_eq!(history.mark_source_finished(&source, true), None);
        assert_eq!(history.resume_position_for_source(&source), Some(120.0));
    }

    #[test]
    fn skips_resume_after_finished_file() {
        let mut history = store();
        record(&mut history, "/media/movie.mkv", 599.0, 600.0, true);
        assert_eq!(
            history.resume_position_for_source(&local("/media/movie.mkv")),
            None
        );
    }

    #[test]
    fn preserves_preferences_when_progress_is_updated() {
        let mut history = store();
        let source = local("/media/movie.mkv");
        let preferences = Preferences {
            subtitle_enabled: Some(true),
            subtitle_track_id: Some(3),
            subtitle_delay: Some(0.25),
            ..Preferences::default()
        };

        history.record_source_preferences(&source, preferences.clone(), false);
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);

        assert_eq!(
            history.playback_preferences_for_source(&source),
            Some(preferences)
        );
    }

    #[test]
    fn resolved_nfo_title_flows_to_recents_and_completed_miss_restores_fallback() {
        let mut history = store();
        let source = local("/media/Movie.mkv");
        let mut sample = |position: f64, title: HistoryTitleUpdate| {
            history.record_source_with_title(&source, position, 600.0, false, false, title);
            history.search("")[0].title.clone()
        };

        assert_eq!(
            sample(
                120.0,
                HistoryTitleUpdate::Set("Curated Movie Title".to_owned())
            ),
            "Curated Movie Title"
        );
        // A save while the next read is still pending preserves the last known title.
        assert_eq!(
            sample(130.0, HistoryTitleUpdate::Preserve),
            "Curated Movie Title"
        );
        // Once discovery completes with no usable sidecar, recents return to the
        // existing filename-stem fallback instead of retaining stale metadata.
        assert_eq!(sample(140.0, HistoryTitleUpdate::Clear), "Movie");
    }

    #[test]
    fn merges_preference_updates_without_clearing_unrelated_fields() {
        let mut history = store();
        let source = local("/media/movie.mkv");

        history.record_source_preferences(
            &source,
            Preferences {
                audio_enabled: Some(true),
                audio_track_id: Some(1),
                secondary_subtitle_enabled: Some(true),
                secondary_subtitle_track_id: Some(4),
                subtitle_delay: Some(0.25),
                speed: Some(0.75),
                volume: Some(60.0),
                ..Preferences::default()
            },
            false,
        );
        history.record_source_preferences(
            &source,
            Preferences {
                subtitle_enabled: Some(false),
                subtitle_scale: Some(1.2),
                ..Preferences::default()
            },
            false,
        );

        assert_eq!(
            history.playback_preferences_for_source(&source),
            Some(Preferences {
                audio_enabled: Some(true),
                audio_track_id: Some(1),
                subtitle_enabled: Some(false),
                secondary_subtitle_enabled: Some(true),
                secondary_subtitle_track_id: Some(4),
                subtitle_delay: Some(0.25),
                subtitle_scale: Some(1.2),
                speed: Some(0.75),
                volume: Some(60.0),
                ..Preferences::default()
            })
        );
    }

    #[test]
    fn per_file_video_geometry_persists_through_the_shared_history_path() {
        use okp_core::video_geometry::{VideoAspect, VideoGeometry};

        let mut history = store();
        let source = local("/media/movie.mkv");
        let geometry = VideoGeometry {
            aspect: VideoAspect::Cinema,
            zoom: 1.5,
            pan_x: -0.2,
            pan_y: 0.1,
            rotation_degrees: 90,
            fill_screen: true,
            deinterlace: true,
        };

        history.record_source_preferences(
            &source,
            Preferences {
                video_geometry: Some(geometry),
                ..Preferences::default()
            },
            false,
        );
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);

        assert_eq!(
            history
                .playback_preferences_for_source(&source)
                .and_then(|preferences| preferences.video_geometry),
            Some(geometry)
        );
    }

    #[test]
    fn recording_progress_preserves_bookmarks() {
        let mut history = store();
        let key = "/media/movie.mkv";
        // A bookmark the Linux build wrote; this store does not add them yet.
        history
            .data
            .files
            .entry(key.to_owned())
            .or_default()
            .bookmarks
            .push(42.0);

        record(&mut history, key, 120.0, 600.0, false);

        assert_eq!(history.data.files[key].bookmarks, vec![42.0]);
        assert_eq!(history.resume_position_for_source(&local(key)), Some(120.0));
    }

    #[test]
    fn history_path_listability_hides_missing_local_files_but_keeps_remote_media() {
        let existing = std::env::current_exe().expect("test executable path");

        assert!(is_history_path_listable(&existing.to_string_lossy()));
        assert!(!is_history_path_listable(
            "/definitely/missing/ok-player-history-test.mkv"
        ));
        assert!(is_history_path_listable("https://example.com/movie.mkv"));
        assert!(is_history_path_listable(r"\\server\share\movie.mkv"));
    }

    #[test]
    fn url_history_records_persists_lists_and_restores_original_load_identity() {
        let root = unique_temp_dir("okp-session-history-url");
        let history_path = root.path().join("history.json");
        let local_path = root.path().join("local-regression.mkv");
        fs::write(&local_path, b"fixture").expect("local history fixture");

        let finite_url = "https://example.com/watch?v=stable-id&token=required%2Fvalue#chapter";
        let resolved_cdn_url = "https://cdn.example.net/expiring/stream.m3u8?expires=1";
        let finite_source = PlaylistItem::Url(finite_url.to_owned());
        let live_source = PlaylistItem::Url("https://example.com/live/channel".to_owned());
        let private_source = PlaylistItem::Url("https://example.com/private".to_owned());
        let local_source = PlaylistItem::Local(local_path.clone());
        let mut history = HistoryStore::open_path(history_path.clone());

        history.record_source_opened(
            &finite_source,
            Some(600.0),
            false,
            HistoryTitleUpdate::Set("Stable page title".to_owned()),
        );
        history.record_source_with_title(
            &finite_source,
            120.0,
            600.0,
            false,
            false,
            HistoryTitleUpdate::Preserve,
        );
        history.record_source_with_title(
            &finite_source,
            180.0,
            600.0,
            false,
            false,
            HistoryTitleUpdate::Set("Updated page title".to_owned()),
        );
        // A private replay may neither change the prior row nor add a new one.
        history.record_source_with_title(
            &finite_source,
            300.0,
            600.0,
            false,
            true,
            HistoryTitleUpdate::Set("Private title".to_owned()),
        );
        history.record_source_opened(
            &private_source,
            Some(300.0),
            true,
            HistoryTitleUpdate::Set("Private source".to_owned()),
        );
        history.record_source_opened(
            &live_source,
            None,
            false,
            HistoryTitleUpdate::Set("Live channel".to_owned()),
        );
        history.record_source_with_title(
            &local_source,
            60.0,
            300.0,
            false,
            false,
            HistoryTitleUpdate::Set("Local regression".to_owned()),
        );
        history.save().expect("persist URL history");

        let reloaded = HistoryStore::open_path(history_path);
        let rows = reloaded.search("");
        assert_eq!(
            rows.len(),
            3,
            "duplicate and private opens must not add rows"
        );

        let finite = rows
            .iter()
            .find(|item| item.path == finite_url)
            .expect("finite URL row after restart");
        assert_eq!(finite.source(), finite_source);
        assert_eq!(finite.title, "Updated page title");
        assert_eq!(finite.position, 180.0);
        assert_eq!(finite.duration, 600.0);
        assert_eq!(
            reloaded.resume_position_for_source(&finite.source()),
            Some(180.0)
        );
        assert_ne!(finite.path, resolved_cdn_url);

        let live = rows
            .iter()
            .find(|item| item.source() == live_source)
            .expect("duration-unknown URL row after restart");
        assert_eq!(live.duration, 0.0);
        assert_eq!(live.progress, 0.0);
        assert_eq!(live.state_label, "Duration unknown");
        assert_eq!(reloaded.resume_position_for_source(&live.source()), None);

        let local = rows
            .iter()
            .find(|item| item.source() == local_source)
            .expect("local history row after URL changes");
        assert_eq!(local.position, 60.0);
        assert_eq!(local.duration, 300.0);
        assert!(!rows.iter().any(|item| item.source() == private_source));
    }

    #[test]
    fn local_and_url_removal_persist_without_touching_local_media() {
        let root = unique_temp_dir("okp-session-history-remove");
        let history_path = root.path().join("history.json");
        let media_path = root.path().join("keep-media.mkv");
        fs::write(&media_path, b"media fixture").expect("local media fixture");
        let local = PlaylistItem::Local(media_path.clone());
        let url = PlaylistItem::Url("https://example.test/watch?id=keep".to_owned());
        let mut history = HistoryStore::open_path(history_path.clone());
        history.record_source_with_title(
            &local,
            60.0,
            300.0,
            false,
            false,
            HistoryTitleUpdate::Preserve,
        );
        history.record_source_opened(&url, None, false, HistoryTitleUpdate::Preserve);
        history.save().expect("seed persisted history");

        assert!(
            history
                .remove_source_persisted(&local, false)
                .expect("persist local removal")
        );
        assert_eq!(
            fs::read(&media_path).ok().as_deref(),
            Some(b"media fixture".as_slice())
        );
        let reloaded = HistoryStore::open_path(history_path.clone());
        assert_eq!(
            reloaded
                .search("")
                .iter()
                .map(HistoryItem::source)
                .collect::<Vec<_>>(),
            vec![url.clone()]
        );

        assert!(
            history
                .remove_source_persisted(&url, false)
                .expect("persist URL removal")
        );
        assert!(HistoryStore::open_path(history_path).search("").is_empty());
        assert!(media_path.is_file());
    }

    #[test]
    fn failed_removal_save_restores_record_and_session_writes() {
        let mut history = unwritable_store();
        let source = PlaylistItem::Url("https://example.test/watch".to_owned());
        history.record_source_opened(&source, None, false, HistoryTitleUpdate::Preserve);

        history
            .remove_source_persisted(&source, true)
            .expect_err("save must fail on an unwritable path");

        assert_eq!(history.search("").len(), 1);
        assert!(!history.is_source_suppressed(&source));
        history.record_source_with_title(
            &source,
            30.0,
            300.0,
            false,
            false,
            HistoryTitleUpdate::Preserve,
        );
        assert_eq!(history.resume_position_for_source(&source), Some(30.0));
    }

    #[test]
    fn active_removal_blocks_every_late_write_until_explicit_reopen() {
        let root = unique_temp_dir("okp-session-history-active-removal");
        let history_path = root.path().join("history.json");
        let source = PlaylistItem::Url("https://example.test/live/channel".to_owned());
        let mut history = HistoryStore::open_path(history_path.clone());
        history.record_source_opened(&source, None, false, HistoryTitleUpdate::Preserve);
        history.save().expect("seed persisted history");

        assert!(
            history
                .remove_source_persisted(&source, true)
                .expect("persist active removal")
        );
        assert!(history.is_source_suppressed(&source));

        // These are the periodic unknown-duration open, progress/preferences, and EOF paths.
        history.record_source_opened(&source, None, false, HistoryTitleUpdate::Preserve);
        history.record_source_with_title(
            &source,
            30.0,
            300.0,
            false,
            false,
            HistoryTitleUpdate::Preserve,
        );
        history.record_source_preferences(
            &source,
            Preferences {
                speed: Some(1.25),
                ..Preferences::default()
            },
            false,
        );
        assert_eq!(history.mark_source_finished(&source, false), None);
        history.save().expect("exit save after removal");
        assert!(history.search("").is_empty());
        assert!(
            HistoryStore::open_path(history_path.clone())
                .search("")
                .is_empty()
        );

        history.begin_source_open(&source, HistoryOpenIntent::Automatic);
        history.record_source_opened(&source, None, false, HistoryTitleUpdate::Preserve);
        assert!(history.search("").is_empty());

        history.begin_source_open(&source, HistoryOpenIntent::Explicit);
        history.record_source_opened(&source, None, false, HistoryTitleUpdate::Preserve);
        history.save().expect("persist explicit reopen");
        assert_eq!(HistoryStore::open_path(history_path).search("").len(), 1);
    }

    #[test]
    fn an_interrupted_write_leaves_the_previous_file_intact() {
        let root = unique_temp_dir("okp-session-history-interrupted");
        let path = root.path().join("history.json");
        let mut history = HistoryStore::open_path(path.clone());
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);
        history.save().expect("the first save succeeds");
        let previous = fs::read(&path).expect("the first save wrote the file");

        // The process stops half way through writing the new document.
        record(&mut history, "/media/other.mkv", 200.0, 900.0, false);
        let json = serde_json::to_vec_pretty(&history.data).expect("history serialises");
        let error = write_atomically(&path, &json, |file, bytes| {
            file.write_all(&bytes[..bytes.len() / 2])?;
            Err(io::Error::new(io::ErrorKind::Interrupted, "power cut"))
        })
        .expect_err("the interrupted write fails");

        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(
            fs::read(&path).expect("the file is still there"),
            previous,
            "the previous history must survive byte for byte"
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the half-written temporary file is cleaned up"
        );
        let reopened = HistoryStore::open_path(path);
        assert!(!reopened.read_failed());
        assert_eq!(
            reopened.resume_position_for_source(&local("/media/movie.mkv")),
            Some(120.0)
        );
    }

    #[test]
    fn a_temporary_file_left_by_a_crash_is_ignored_and_replaced() {
        let root = unique_temp_dir("okp-session-history-stale-temporary");
        let path = root.path().join("history.json");
        let mut history = HistoryStore::open_path(path.clone());
        record(&mut history, "/media/movie.mkv", 120.0, 600.0, false);
        history.save().expect("the first save succeeds");
        fs::write(path.with_extension("json.tmp"), b"{\"version\":2,\"fi")
            .expect("a crash left half a document behind");

        let mut reopened = HistoryStore::open_path(path.clone());
        assert!(!reopened.read_failed());
        record(&mut reopened, "/media/movie.mkv", 150.0, 600.0, false);
        reopened.save().expect("saving over the leftover succeeds");

        assert_eq!(
            HistoryStore::open_path(path).resume_position_for_source(&local("/media/movie.mkv")),
            Some(150.0)
        );
    }

    #[test]
    fn an_unreadable_file_opens_empty_and_reports_it() {
        let root = unique_temp_dir("okp-session-history-corrupt");
        let path = root.path().join("history.json");
        fs::write(&path, b"not json").expect("corrupt fixture");

        let history = HistoryStore::open_path(path);
        assert!(history.read_failed());
        assert!(history.search("").is_empty());
    }
}
