use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use okp_core::history::Preferences as PlaybackPreferences;
use okp_core::history::{FileEntry as HistoryRecord, History as HistoryFile};

#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    data: HistoryFile,
    dirty: bool,
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self::open()
    }
}

impl HistoryStore {
    pub fn open() -> Self {
        let path = history_path();
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|json| HistoryFile::load(&json))
            .unwrap_or_default();

        Self {
            path,
            data,
            dirty: false,
        }
    }

    pub fn record(&mut self, path: &Path, position: f64, duration: f64, finished: bool) {
        if !duration.is_finite() || duration <= 0.0 || !position.is_finite() {
            return;
        }

        let key = history_key(path);
        let complete_at = completion_start(duration);
        let final_stretch = position >= complete_at;
        let stored_position = if finished || final_stretch {
            0.0
        } else {
            position.clamp(0.0, duration)
        };

        // Mutate the existing record in place so the progress fields refresh while every
        // other field carries through: the stored `preferences` as before, plus the
        // shared-schema extras (`bookmarks`, `chapters`, `title`, `poster_path`). Before
        // bookmarks existed this used `..HistoryRecord::default()`, which was harmless
        // only because Linux never wrote those extras; now that a bookmark lives here, a
        // progress save must not wipe it.
        let mut record = self.data.files.remove(&key).unwrap_or_default();
        let existing_finished = record.finished;
        record.position = stored_position;
        record.duration = duration;
        record.finished = finished || (existing_finished && final_stretch);
        record.updated_at_unix = unix_now();
        self.data.files.insert(key, record);
        self.dirty = true;
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

    /// Add a bookmark at `time` for `path`, deduping and sorting through
    /// [`okp_core::bookmarks::add`]. Returns `true` when a mark was added, `false` when
    /// one already sits at that spot (or the time is unusable).
    pub fn add_bookmark(&mut self, path: &Path, time: f64) -> bool {
        let key = history_key(path);
        let mut record = self
            .data
            .files
            .remove(&key)
            .unwrap_or_else(new_history_record);
        let added = okp_core::bookmarks::add(&mut record.bookmarks, time);
        if added {
            record.updated_at_unix = unix_now();
            self.dirty = true;
        }
        self.data.files.insert(key, record);
        added
    }

    /// Remove the bookmark nearest `time` for `path` (via [`okp_core::bookmarks::remove`]).
    /// Returns `true` when a mark was dropped.
    pub fn remove_bookmark(&mut self, path: &Path, time: f64) -> bool {
        let key = history_key(path);
        let Some(mut record) = self.data.files.remove(&key) else {
            return false;
        };
        let removed = okp_core::bookmarks::remove(&mut record.bookmarks, time);
        if removed {
            record.updated_at_unix = unix_now();
            self.dirty = true;
        }
        self.data.files.insert(key, record);
        removed
    }

    /// Add a bookmark at `time` for `path` and persist it in the same step, undoing the
    /// in-memory mark if the write fails. This keeps memory and the file on disk in
    /// lock-step: a caller that reports "bookmarked" only after `Ok(true)` can never
    /// advertise a mark that a crash-on-exit would silently lose. `Ok(false)` means a
    /// mark already sat there (no write attempted); `Err` means the save failed and the
    /// store was left exactly as it was found.
    pub fn add_bookmark_persisted(&mut self, path: &Path, time: f64) -> io::Result<bool> {
        if !self.add_bookmark(path, time) {
            return Ok(false);
        }
        if let Err(error) = self.save() {
            // The mark we just added is the only one within the remove window (an add
            // only succeeds when nothing sits within the wider dedupe window), so this
            // drops exactly it and restores the pre-add set.
            self.remove_bookmark(path, time);
            return Err(error);
        }
        Ok(true)
    }

    /// Remove the bookmark nearest `time` for `path` and persist the removal, re-adding
    /// the mark if the write fails. Without the rollback a failed save would drop the
    /// mark only in memory while it survives on disk, so it would reappear on the next
    /// launch. `Ok(false)` means nothing matched (no write attempted); `Err` means the
    /// save failed and the mark was put back.
    pub fn remove_bookmark_persisted(&mut self, path: &Path, time: f64) -> io::Result<bool> {
        if !self.remove_bookmark(path, time) {
            return Ok(false);
        }
        if let Err(error) = self.save() {
            self.add_bookmark(path, time);
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

    pub fn record_preferences(&mut self, path: &Path, preferences: PlaybackPreferences) {
        if preferences.is_empty() {
            return;
        }

        let key = history_key(path);
        let mut record = self
            .data
            .files
            .remove(&key)
            .unwrap_or_else(new_history_record);
        record.preferences.merge(preferences);
        record.updated_at_unix = unix_now();
        self.data.files.insert(key, record);
        self.dirty = true;
    }

    pub fn playback_preferences(&self, path: &Path) -> Option<PlaybackPreferences> {
        self.data
            .files
            .get(&history_key(path))
            .map(|record| record.preferences.clone())
            .filter(|preferences| !preferences.is_empty())
    }

    pub fn clear(&mut self) {
        if !self.data.files.is_empty() {
            self.data.files.clear();
            self.dirty = true;
        }
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
        Ok(())
    }
}

/// A fresh record stamped with the current time — the starting point when preferences
/// are recorded for a file that has no progress entry yet.
fn new_history_record() -> HistoryRecord {
    HistoryRecord {
        updated_at_unix: unix_now(),
        ..HistoryRecord::default()
    }
}

pub fn completion_start(duration: f64) -> f64 {
    (duration * 0.95).max(duration - 30.0)
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
            dirty: false,
        }
    }

    /// A store whose path cannot be written: `/dev/null` exists but is not a directory,
    /// so `create_dir_all` on the parent fails and every `save()` errors deterministically
    /// without touching the real filesystem.
    fn unwritable_store() -> HistoryStore {
        HistoryStore {
            path: PathBuf::from("/dev/null/history.json"),
            data: HistoryFile::default(),
            dirty: false,
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
        );
        history.record_preferences(
            path,
            PlaybackPreferences {
                subtitle_enabled: Some(false),
                subtitle_scale: Some(1.2),
                ..PlaybackPreferences::default()
            },
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
    fn add_and_remove_bookmarks_round_trip_and_sort() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        assert!(history.add_bookmark(path, 100.0));
        assert!(history.add_bookmark(path, 10.0));
        // A near-duplicate within half a second is refused.
        assert!(!history.add_bookmark(path, 100.2));
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
            .add_bookmark_persisted(path, 42.0)
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
        assert!(history.add_bookmark(path, 42.0));

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
        assert!(history.add_bookmark(path, 42.0));

        // A duplicate add and a no-match remove change nothing, so no save is attempted
        // and the unwritable path is never reached.
        assert_eq!(history.add_bookmark_persisted(path, 42.2).ok(), Some(false));
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

        history.add_bookmark(path, 42.0);
        // A progress save must not wipe the bookmark the way the old
        // `..HistoryRecord::default()` reset did.
        history.record(path, 120.0, 600.0, false);

        assert_eq!(history.bookmarks(path), vec![42.0]);
        assert_eq!(history.resume_position(path), Some(120.0));
    }

    #[test]
    fn clear_removes_progress_and_preferences() {
        let mut history = store();
        let path = Path::new("/media/movie.mkv");

        history.record(path, 120.0, 600.0, false);
        history.record_preferences(
            path,
            PlaybackPreferences {
                speed: Some(1.25),
                ..PlaybackPreferences::default()
            },
        );
        history.clear();

        assert_eq!(history.resume_position(path), None);
        assert_eq!(history.playback_preferences(path), None);
        assert!(history.dirty);
    }
}
