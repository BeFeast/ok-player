use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use okp_core::history::Preferences as PlaybackPreferences;
use okp_core::history::{
    History as HistoryFile, HistoryProgressUpdate, HistoryWriteMode, HistoryWriteResult,
};
use okp_core::nfo_metadata::HistoryTitleUpdate;
use okp_core::recents_shelf::{HistoryItem, WelcomeShelf};

#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    data: HistoryFile,
    listable_paths: BTreeSet<String>,
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
        let path = history_path();
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
            dirty: false,
            read_failed,
            cleared: false,
        }
    }

    pub fn read_failed(&self) -> bool {
        self.read_failed
    }

    pub fn was_cleared(&self) -> bool {
        self.cleared
    }

    pub fn retry_read(&mut self) {
        *self = Self::open();
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

    pub fn record_with_title(
        &mut self,
        path: &Path,
        position: f64,
        duration: f64,
        finished: bool,
        private_session: bool,
        title_update: HistoryTitleUpdate,
    ) {
        let key = history_key(path);
        let result = self.data.record_progress(
            &key,
            HistoryProgressUpdate {
                position,
                duration,
                finished,
                updated_at_unix: unix_now(),
                title: title_update,
            },
            HistoryWriteMode::from_private(private_session),
        );
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
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
        let result = self.data.add_bookmark(
            &key,
            time,
            unix_now(),
            HistoryWriteMode::from_private(private_session),
        );
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
        let record = self.data.files.get(&history_key(path))?;
        if record.finished
            || !record.duration.is_finite()
            || record.duration <= 0.0
            || !record.position.is_finite()
            || record.position <= record.duration * 0.05
            || record.position >= completion_start(record.duration)
        {
            return None;
        }

        Some(record.position)
    }

    pub fn record_preferences(
        &mut self,
        path: &Path,
        preferences: PlaybackPreferences,
        private_session: bool,
    ) {
        let key = history_key(path);
        let result = self.data.record_preferences(
            &key,
            preferences,
            unix_now(),
            HistoryWriteMode::from_private(private_session),
        );
        if result == HistoryWriteResult::Changed {
            self.listable_paths.insert(key);
            self.dirty = true;
        }
    }

    pub fn playback_preferences(&self, path: &Path) -> Option<PlaybackPreferences> {
        self.data
            .files
            .get(&history_key(path))
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

    fn snapshot(&self) -> HistoryStoreSnapshot {
        HistoryStoreSnapshot {
            data: self.data.clone(),
            listable_paths: self.listable_paths.clone(),
            dirty: self.dirty,
            read_failed: self.read_failed,
            cleared: self.cleared,
        }
    }

    fn restore(&mut self, snapshot: HistoryStoreSnapshot) {
        self.data = snapshot.data;
        self.listable_paths = snapshot.listable_paths;
        self.dirty = snapshot.dirty;
        self.read_failed = snapshot.read_failed;
        self.cleared = snapshot.cleared;
    }
}

struct HistoryStoreSnapshot {
    data: HistoryFile,
    listable_paths: BTreeSet<String>,
    dirty: bool,
    read_failed: bool,
    cleared: bool,
}

pub fn completion_start(duration: f64) -> f64 {
    okp_core::recents_shelf::completion_start(duration)
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

        history.record(path, completion_start(600.0), 600.0, false);

        assert_eq!(history.resume_position(path), None);
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
}
