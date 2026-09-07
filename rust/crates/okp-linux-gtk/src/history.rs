use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use okp_core::history::Preferences as PlaybackPreferences;
use okp_core::history::{
    History as HistoryFile, HistoryOpenIntent, HistoryOpenUpdate, HistoryProgressUpdate,
    HistoryRecordingSession, HistoryWriteMode, HistoryWriteResult,
};
use okp_core::nfo_metadata::HistoryTitleUpdate;
use okp_core::playlist::PlaylistItem;
use okp_core::recents_shelf::{HistoryItem, WelcomeShelf};

#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    data: HistoryFile,
    listable_paths: BTreeSet<String>,
    recording_session: HistoryRecordingSession,
    dirty: bool,
    read_failed: bool,
    cleared: bool,
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self::open()
    }
}

impl HistoryStore {
    pub fn open() -> Self {
        Self::open_path(history_path())
    }

    fn open_path(path: PathBuf) -> Self {
        let (data, read_failed) = match fs::read_to_string(&path) {
            Ok(json) => match HistoryFile::load(&json) {
                Some(data) => (data, false),
                None => (HistoryFile::default(), true),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (HistoryFile::default(), false)
            }
            Err(_) => (HistoryFile::default(), true),
        };
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
            read_failed,
            cleared: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn open_test(path: PathBuf) -> Self {
        Self::open_path(path)
    }

    pub fn read_failed(&self) -> bool {
        self.read_failed
    }

    pub fn was_cleared(&self) -> bool {
        self.cleared
    }

    pub fn retry_read(&mut self) {
        let recording_session = std::mem::take(&mut self.recording_session);
        *self = Self::open();
        self.recording_session = recording_session;
    }

    #[cfg(test)]
    pub fn record(&mut self, path: &Path, position: f64, duration: f64, finished: bool) {
        self.record_with_title(
            path,
            position,
            duration,
            finished,
            false,
            HistoryTitleUpdate::Preserve,
        );
    }

    #[cfg(test)]
    pub fn record_with_title(
        &mut self,
        path: &Path,
        position: f64,
        duration: f64,
        finished: bool,
        private_session: bool,
        title_update: HistoryTitleUpdate,
    ) {
        self.record_source_with_title(
            &PlaylistItem::Local(path.to_path_buf()),
            position,
            duration,
            finished,
            private_session,
            title_update,
        );
    }

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
                updated_at_unix: unix_now(),
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
                updated_at_unix: unix_now(),
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

    /// Keep lifecycle writes for `source` out of History for the rest of this playback
    /// session. Used after a successful Trash even when the following History save fails.
    pub fn suppress_source(&mut self, source: &PlaylistItem) {
        self.recording_session.suppress(source);
    }

    #[cfg(test)]
    pub fn is_source_suppressed(&self, source: &PlaylistItem) -> bool {
        self.recording_session.is_suppressed(source)
    }

    /// Mark `path` watched to the end, clearing its resume position so the next open
    /// starts at zero. Returns the entry's stored duration when a record was actually
    /// changed, so the caller can report the completion it just observed.
    #[cfg(test)]
    pub fn mark_finished(&mut self, path: &Path, private_session: bool) -> Option<f64> {
        self.mark_source_finished(&PlaylistItem::Local(path.to_path_buf()), private_session)
    }

    pub fn mark_source_finished(
        &mut self,
        source: &PlaylistItem,
        private_session: bool,
    ) -> Option<f64> {
        let key = source.history_key();
        let mode = self.write_mode(source, private_session);
        let result = self.data.mark_finished(&key, unix_now(), mode);
        if result != HistoryWriteResult::Changed {
            return None;
        }
        self.dirty = true;
        self.data.files.get(&key).map(|record| record.duration)
    }

    /// The user's saved position bookmarks for `path`, sorted (empty when none). Read by
    /// the side panel to render the Bookmarks section.
    pub fn bookmarks(&self, path: &Path) -> Vec<f64> {
        self.data
            .files
            .get(&history_key(path))
            .map(|record| record.bookmarks.clone())
            .unwrap_or_default()
    }

    /// Add a bookmark at `time` for `path`, with shared-core dedupe and
    /// private-session gating.
    pub fn add_bookmark(
        &mut self,
        path: &Path,
        time: f64,
        private_session: bool,
    ) -> HistoryWriteResult {
        let key = history_key(path);
        let source = PlaylistItem::Local(path.to_path_buf());
        let mode = self.write_mode(&source, private_session);
        let result = self.data.add_bookmark(&key, time, unix_now(), mode);
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
        result
    }

    /// Remove the bookmark nearest `time` for `path` (via [`okp_core::bookmarks::remove`]).
    /// Returns `true` when a mark was dropped.
    pub fn remove_bookmark(&mut self, path: &Path, time: f64) -> bool {
        let key = history_key(path);
        let removed = self.data.remove_bookmark(&key, time);
        if removed {
            self.dirty = true;
        }
        removed
    }

    /// Add a bookmark at `time` for `path` and persist it in the same step, undoing the
    /// in-memory mark if the write fails. This keeps memory and the file on disk in
    /// lock-step: a caller that reports "bookmarked" only after `Changed` can never
    /// advertise a mark that a crash-on-exit would silently lose. `Unchanged` means a
    /// mark already sat there, `Suppressed` means the private session gated it, and `Err`
    /// means the save failed with the store restored exactly as it was found.
    pub fn add_bookmark_persisted(
        &mut self,
        path: &Path,
        time: f64,
        private_session: bool,
    ) -> io::Result<HistoryWriteResult> {
        let before = self.snapshot();
        let result = self.add_bookmark(path, time, private_session);
        if result != HistoryWriteResult::Changed {
            return Ok(result);
        }
        if let Err(error) = self.save() {
            self.restore(before);
            return Err(error);
        }
        Ok(result)
    }

    /// Remove the bookmark nearest `time` for `path` and persist the removal, restoring
    /// the prior store if the write fails. Without the rollback a failed save would drop the
    /// mark only in memory while it survives on disk, so it would reappear on the next
    /// launch. `Ok(false)` means nothing matched (no write attempted); `Err` means the
    /// save failed and the mark was put back.
    pub fn remove_bookmark_persisted(&mut self, path: &Path, time: f64) -> io::Result<bool> {
        let before = self.snapshot();
        if !self.remove_bookmark(path, time) {
            return Ok(false);
        }
        if let Err(error) = self.save() {
            self.restore(before);
            return Err(error);
        }
        Ok(true)
    }

    pub fn resume_position(&self, path: &Path) -> Option<f64> {
        self.resume_position_for_source(&PlaylistItem::Local(path.to_path_buf()))
    }

    pub fn resume_position_for_source(&self, source: &PlaylistItem) -> Option<f64> {
        self.data.resume_position(&source.history_key())
    }

    #[cfg(test)]
    pub fn record_preferences(
        &mut self,
        path: &Path,
        preferences: PlaybackPreferences,
        private_session: bool,
    ) {
        self.record_source_preferences(
            &PlaylistItem::Local(path.to_path_buf()),
            preferences,
            private_session,
        );
    }

    pub fn record_source_preferences(
        &mut self,
        source: &PlaylistItem,
        preferences: PlaybackPreferences,
        private_session: bool,
    ) {
        let key = source.history_key();
        let mode = self.write_mode(source, private_session);
        let result = self
            .data
            .record_preferences(&key, preferences, unix_now(), mode);
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
    }

    pub fn playback_preferences(&self, path: &Path) -> Option<PlaybackPreferences> {
        self.playback_preferences_for_source(&PlaylistItem::Local(path.to_path_buf()))
    }

    pub fn playback_preferences_for_source(
        &self,
        source: &PlaylistItem,
    ) -> Option<PlaybackPreferences> {
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

    pub fn clear(&mut self) {
        self.cleared = true;
        self.read_failed = false;
        if self.data.clear() > 0 {
            self.listable_paths.clear();
            self.dirty = true;
        }
    }

    pub fn clear_persisted(&mut self) -> io::Result<usize> {
        let before = self.snapshot();
        let removed = self.data.files.len();
        self.clear();
        if let Err(error) = self.save() {
            self.restore(before);
            return Err(error);
        }
        Ok(removed)
    }

    pub fn prune_older_than_persisted(&mut self, days: i64) -> io::Result<usize> {
        let before = self.snapshot();
        let removed = self.data.prune_older_than(unix_now(), days);
        if removed == 0 {
            return Ok(0);
        }
        self.listable_paths
            .retain(|path| self.data.files.contains_key(path));
        self.dirty = true;
        if let Err(error) = self.save() {
            self.restore(before);
            return Err(error);
        }
        Ok(removed)
    }

    pub fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_vec_pretty(&self.data).map_err(io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(tmp, &self.path)?;
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
            cleared: self.cleared,
        }
    }

    fn restore(&mut self, snapshot: HistoryStoreSnapshot) {
        self.data = snapshot.data;
        self.listable_paths = snapshot.listable_paths;
        self.recording_session = snapshot.recording_session;
        self.dirty = snapshot.dirty;
        self.read_failed = snapshot.read_failed;
        self.cleared = snapshot.cleared;
    }
}

struct HistoryStoreSnapshot {
    data: HistoryFile,
    listable_paths: BTreeSet<String>,
    recording_session: HistoryRecordingSession,
    dirty: bool,
    read_failed: bool,
    cleared: bool,
}

fn history_path() -> PathBuf {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(state_home).join("ok-player/history.json");
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".local/state/ok-player/history.json");
    }

    PathBuf::from("ok-player-history.json")
}

fn history_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

    fn store() -> HistoryStore {
        HistoryStore {
            path: PathBuf::from("unused.json"),
            data: HistoryFile::default(),
            listable_paths: BTreeSet::new(),
            recording_session: HistoryRecordingSession::default(),
            dirty: false,
            read_failed: false,
            cleared: false,
        }
    }

    /// A store whose path cannot be written: `/dev/null` exists but is not a directory,
    /// so `create_dir_all` on the parent fails and every `save()` errors deterministically
    /// without touching the real filesystem.
    fn unwritable_store() -> HistoryStore {
        HistoryStore {
            path: PathBuf::from("/dev/null/history.json"),
            data: HistoryFile::default(),
            listable_paths: BTreeSet::new(),
            recording_session: HistoryRecordingSession::default(),
            dirty: false,
            read_failed: false,
            cleared: false,
        }
    }

    #[test]
    fn returns_resume_position_for_middle_of_file() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record(path, 120.0, 600.0, false);

        assert_eq!(history.resume_position(path), Some(120.0));
    }

    #[test]
    fn skips_resume_in_first_five_percent() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record(path, 30.0, 600.0, false);

        assert_eq!(history.resume_position(path), None);
    }

    #[test]
    fn skips_resume_in_completion_window() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record(
            path,
            okp_core::recents_shelf::completion_start(600.0),
            600.0,
            false,
        );

        assert_eq!(history.resume_position(path), None);
    }

    #[test]
    fn marking_finished_clears_the_resume_position_without_a_final_sample() {
        // The shell reaches end of file with no position to report; the store must still
        // stop the next open from resuming, and must persist that.
        let mut history = store();
        let path = Path::new("/media/movie.mkv");
        history.record(path, 120.0, 600.0, false);

        let duration = history.mark_finished(path, false);

        assert_eq!(duration, Some(600.0));
        assert_eq!(history.resume_position(path), None);
        assert!(history.dirty);
    }

    #[test]
    fn marking_finished_reports_nothing_for_an_unknown_file() {
        let mut history = store();

        assert_eq!(
            history.mark_finished(Path::new("/media/never-seen.mkv"), false),
            None
        );
    }

    #[test]
    fn a_private_session_marks_nothing_finished() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");
        history.record(path, 120.0, 600.0, false);

        assert_eq!(history.mark_finished(path, true), None);
        assert_eq!(history.resume_position(path), Some(120.0));
    }

    #[test]
    fn skips_resume_after_finished_file() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record(path, 599.0, 600.0, true);

        assert_eq!(history.resume_position(path), None);
    }

    #[test]
    fn preserves_preferences_when_progress_is_updated() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record_preferences(
            path,
            PlaybackPreferences {
                subtitle_enabled: Some(true),
                subtitle_track_id: Some(3),
                subtitle_delay: Some(0.25),
                ..PlaybackPreferences::default()
            },
            false,
        );
        history.record(path, 120.0, 600.0, false);

        assert_eq!(
            history.playback_preferences(path),
            Some(PlaybackPreferences {
                subtitle_enabled: Some(true),
                subtitle_track_id: Some(3),
                subtitle_delay: Some(0.25),
                ..PlaybackPreferences::default()
            })
        );
    }

    #[test]
    fn resolved_nfo_title_flows_to_recents_and_completed_miss_restores_fallback() {
        let mut history = store();
        let path = Path::new("/media/Movie.mkv");

        history.record_with_title(
            path,
            120.0,
            600.0,
            false,
            false,
            HistoryTitleUpdate::Set("Curated Movie Title".to_owned()),
        );
        assert_eq!(history.search("")[0].title, "Curated Movie Title");

        // A save while the next read is still pending preserves the last known title.
        history.record_with_title(
            path,
            130.0,
            600.0,
            false,
            false,
            HistoryTitleUpdate::Preserve,
        );
        assert_eq!(history.search("")[0].title, "Curated Movie Title");

        // Once discovery completes with no usable sidecar, recents return to the
        // existing filename-stem fallback instead of retaining stale metadata.
        history.record_with_title(path, 140.0, 600.0, false, false, HistoryTitleUpdate::Clear);
        assert_eq!(history.search("")[0].title, "Movie");
    }

    #[test]
    fn merges_preference_updates_without_clearing_unrelated_fields() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record_preferences(
            path,
            PlaybackPreferences {
                audio_enabled: Some(true),
                audio_track_id: Some(1),
                secondary_subtitle_enabled: Some(true),
                secondary_subtitle_track_id: Some(4),
                subtitle_delay: Some(0.25),
                speed: Some(0.75),
                ..PlaybackPreferences::default()
            },
            false,
        );
        history.record_preferences(
            path,
            PlaybackPreferences {
                subtitle_enabled: Some(false),
                subtitle_scale: Some(1.2),
                ..PlaybackPreferences::default()
            },
            false,
        );

        assert_eq!(
            history.playback_preferences(path),
            Some(PlaybackPreferences {
                audio_enabled: Some(true),
                audio_track_id: Some(1),
                subtitle_enabled: Some(false),
                secondary_subtitle_enabled: Some(true),
                secondary_subtitle_track_id: Some(4),
                subtitle_delay: Some(0.25),
                subtitle_scale: Some(1.2),
                speed: Some(0.75),
                ..PlaybackPreferences::default()
            })
        );
    }

    #[test]
    fn per_file_video_geometry_persists_through_the_shared_history_path() {
        use okp_core::video_geometry::{VideoAspect, VideoGeometry};

        let mut history = store();
        let path = Path::new("/media/movie.mkv");
        let geometry = VideoGeometry {
            aspect: VideoAspect::Cinema,
            zoom: 1.5,
            pan_x: -0.2,
            pan_y: 0.1,
            rotation_degrees: 90,
            fill_screen: true,
            deinterlace: true,
        };

        history.record_preferences(
            path,
            PlaybackPreferences {
                video_geometry: Some(geometry),
                ..PlaybackPreferences::default()
            },
            false,
        );
        history.record(path, 120.0, 600.0, false);

        assert_eq!(
            history
                .playback_preferences(path)
                .and_then(|preferences| preferences.video_geometry),
            Some(geometry)
        );
    }

    #[test]
    fn add_and_remove_bookmarks_round_trip_and_sort() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        assert_eq!(
            history.add_bookmark(path, 100.0, false),
            HistoryWriteResult::Changed
        );
        assert_eq!(
            history.add_bookmark(path, 10.0, false),
            HistoryWriteResult::Changed
        );
        // A near-duplicate within half a second is refused.
        assert_eq!(
            history.add_bookmark(path, 100.2, false),
            HistoryWriteResult::Unchanged
        );
        assert_eq!(history.bookmarks(path), vec![10.0, 100.0]);

        assert!(history.remove_bookmark(path, 10.0));
        assert!(!history.remove_bookmark(path, 555.0));
        assert_eq!(history.bookmarks(path), vec![100.0]);
    }

    #[test]
    fn add_bookmark_persisted_rolls_back_when_the_save_fails() {
        let mut history = unwritable_store();
        let path = Path::new("/media/movie.mkv");

        let error = history
            .add_bookmark_persisted(path, 42.0, false)
            .expect_err("save must fail on an unwritable path");
        assert!(!error.to_string().is_empty());
        // The mark must not linger in memory once the write that would have persisted it
        // failed — otherwise the UI reports success for a change that vanishes on restart.
        assert!(history.bookmarks(path).is_empty());
    }

    #[test]
    fn remove_bookmark_persisted_rolls_back_when_the_save_fails() {
        let mut history = unwritable_store();
        let path = Path::new("/media/movie.mkv");
        // Seed a mark directly (no save) so we can exercise the failing removal.
        assert_eq!(
            history.add_bookmark(path, 42.0, false),
            HistoryWriteResult::Changed
        );

        history
            .remove_bookmark_persisted(path, 42.0)
            .expect_err("save must fail on an unwritable path");
        // The mark survives on disk, so it must survive in memory too; dropping it only
        // in memory would make it reappear on the next launch.
        assert_eq!(history.bookmarks(path), vec![42.0]);
    }

    #[test]
    fn persisted_bookmark_helpers_skip_the_save_when_nothing_changes() {
        let mut history = unwritable_store();
        let path = Path::new("/media/movie.mkv");
        assert_eq!(
            history.add_bookmark(path, 42.0, false),
            HistoryWriteResult::Changed
        );

        // A duplicate add and a no-match remove change nothing, so no save is attempted
        // and the unwritable path is never reached.
        assert_eq!(
            history.add_bookmark_persisted(path, 42.2, false).ok(),
            Some(HistoryWriteResult::Unchanged)
        );
        assert_eq!(
            history.remove_bookmark_persisted(path, 900.0).ok(),
            Some(false)
        );
        assert_eq!(history.bookmarks(path), vec![42.0]);
    }

    #[test]
    fn recording_progress_preserves_bookmarks() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.add_bookmark(path, 42.0, false);
        // A progress save must not wipe the bookmark the way the old
        // `..HistoryRecord::default()` reset did.
        history.record(path, 120.0, 600.0, false);

        assert_eq!(history.bookmarks(path), vec![42.0]);
        assert_eq!(history.resume_position(path), Some(120.0));
    }

    #[test]
    fn clear_removes_open_file_progress_preferences_bookmarks_and_chapters() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record(path, 120.0, 600.0, false);
        history.record_preferences(
            path,
            PlaybackPreferences {
                speed: Some(1.25),
                ..PlaybackPreferences::default()
            },
            false,
        );
        assert_eq!(
            history.add_bookmark(path, 42.0, false),
            HistoryWriteResult::Changed
        );
        history
            .data
            .files
            .get_mut(&history_key(path))
            .expect("current file entry")
            .chapters
            .push(okp_core::history::ChapterMark {
                time: 75.0,
                title: "Scene".to_owned(),
            });
        history.clear();

        assert_eq!(history.resume_position(path), None);
        assert_eq!(history.playback_preferences(path), None);
        assert!(history.bookmarks(path).is_empty());
        assert!(!history.data.files.contains_key(&history_key(path)));
        assert!(history.dirty);
        assert!(history.was_cleared());
        assert!(!history.read_failed());
    }

    #[test]
    fn persisted_clear_rolls_back_all_open_file_state_when_the_save_fails() {
        let mut history = unwritable_store();
        let path = Path::new("/media/movie.mkv");
        history.record(path, 120.0, 600.0, false);
        assert_eq!(
            history.add_bookmark(path, 42.0, false),
            HistoryWriteResult::Changed
        );

        history
            .clear_persisted()
            .expect_err("save must fail on an unwritable path");

        assert_eq!(history.resume_position(path), Some(120.0));
        assert_eq!(history.bookmarks(path), vec![42.0]);
        assert!(!history.was_cleared());
    }

    #[test]
    fn persisted_retention_rolls_back_when_the_save_fails() {
        let mut history = unwritable_store();
        let path = Path::new("/media/old.mkv");
        history.record(path, 120.0, 600.0, false);
        history
            .data
            .files
            .get_mut(&history_key(path))
            .expect("old entry")
            .updated_at_unix = 1;

        history
            .prune_older_than_persisted(7)
            .expect_err("save must fail on an unwritable path");

        assert_eq!(history.resume_position(path), Some(120.0));
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
        let root = tempfile::tempdir().expect("temporary history directory");
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

        let mut reloaded = HistoryStore::open_path(history_path.clone());
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

        assert_eq!(
            reloaded.clear_persisted().expect("clear persisted history"),
            3
        );
        assert!(HistoryStore::open_path(history_path).search("").is_empty());
    }

    #[test]
    fn local_and_url_removal_persist_without_touching_local_media() {
        let root = tempfile::tempdir().expect("temporary history directory");
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
        let root = tempfile::tempdir().expect("temporary history directory");
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
            PlaybackPreferences {
                speed: Some(1.25),
                ..PlaybackPreferences::default()
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
}
