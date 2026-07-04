use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use okp_core::history::Preferences as PlaybackPreferences;
use okp_core::history::{FileEntry as HistoryRecord, History as HistoryFile};
use okp_core::media_formats;
pub use okp_core::recents_shelf::completion_start;
use okp_core::recents_shelf::{self, ContinueWatchingCard};

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
        let existing_finished = self
            .data
            .files
            .get(&key)
            .is_some_and(|record| record.finished);
        let stored_position = if finished || final_stretch {
            0.0
        } else {
            position.clamp(0.0, duration)
        };

        let preferences = self
            .data
            .files
            .get(&key)
            .map(|record| record.preferences.clone())
            .unwrap_or_default();
        self.data.files.insert(
            key,
            HistoryRecord {
                position: stored_position,
                duration,
                finished: finished || (existing_finished && final_stretch),
                updated_at_unix: unix_now(),
                preferences,
                ..HistoryRecord::default()
            },
        );
        self.dirty = true;
    }

    pub fn resume_position(&self, path: &Path) -> Option<f64> {
        let record = self.data.files.get(&history_key(path))?;
        if record.finished || !recents_shelf::is_resumable(record.position, record.duration) {
            return None;
        }

        Some(record.position)
    }

    /// The recents-forward "Continue watching" cards for the welcome shelf: every still-present
    /// resumable file, newest-opened first, projected by [`recents_shelf`]. `private` is threaded
    /// through so a private session yields an empty shelf (recents never leak); `max_cards` bounds
    /// the pool the shell draws from. Genuinely local-and-missing files are dropped so the shelf
    /// never shows a dead path, but playable stream URLs are kept (a flaky share shouldn't hide
    /// them) — the listability rule the Windows `HistoryService` applies, done here as shell IO.
    pub fn continue_watching(&self, private: bool, max_cards: usize) -> Vec<ContinueWatchingCard> {
        if private || max_cards == 0 {
            return Vec::new();
        }
        // Filter the cheap in-memory resumable predicate first so the filesystem `is_listable`
        // probe only touches the handful of genuinely resumable files, not the whole history —
        // this runs on the idle welcome-surface poll. `select_continue_watching` re-applies the
        // same resumable rule (its contract), so the pre-filter is an optimisation, not a divergence.
        let listable: Vec<(&str, &HistoryRecord)> = self
            .data
            .files
            .iter()
            .filter(|(_, record)| {
                !record.finished && recents_shelf::is_resumable(record.position, record.duration)
            })
            .filter(|(path, _)| is_listable(path))
            .map(|(path, record)| (path.as_str(), record))
            .collect();
        recents_shelf::select_continue_watching(listable, private, max_cards)
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

/// Whether a tracked path should still surface in the recents shelf, matching exactly what
/// [`open_recent_media`](crate::open_recent_media) can actually open. A playable stream URL is
/// always kept (a flaky share shouldn't hide it); everything else — a local path, or a non-playable
/// `file://`/malformed `://` string the URL loader would reject and the file loader would treat as a
/// literal path — is kept only while it exists on disk. So a deleted file, or an unopenable URI from
/// a migrated/shared history, never lingers as a dead card. Mirrors `HistoryService.IsListable` on
/// the Windows side, using `is_playable_url` (the same predicate the card click applies) instead of a
/// bare `://` check so listability and openability can't diverge.
fn is_listable(path: &str) -> bool {
    media_formats::is_playable_url(Some(path)) || Path::new(path).exists()
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
    fn continue_watching_lists_resumable_present_files_and_hides_missing_and_private() {
        use okp_test_fixtures::unique_temp_dir;

        let dir = unique_temp_dir("okp-gtk-continue-watching");
        fs::create_dir_all(&dir).expect("temp dir");
        let present = dir.join("present.mkv");
        fs::write(&present, b"video").expect("write present file");
        let missing = dir.join("missing.mkv");

        let mut history = store();
        history.record(&present, 120.0, 600.0, false); // resumable, still on disk
        history.record(&missing, 240.0, 600.0, false); // resumable, but deleted below
        fs::remove_file(&missing).ok();

        let cards = history.continue_watching(false, 10);
        let paths: Vec<&str> = cards.iter().map(|card| card.path.as_str()).collect();
        assert_eq!(paths, [present.to_string_lossy()]);

        // A private session yields an empty shelf even with resumable history present.
        assert!(history.continue_watching(true, 10).is_empty());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn continue_watching_keeps_playable_urls_and_drops_unopenable_uris() {
        let mut history = store();

        // A genuine stream URL: kept even without touching the filesystem, and `open_recent_media`
        // routes it through the URL loader, so it stays clickable.
        let url = Path::new("https://example.com/live.m3u8");
        history.record(url, 120.0, 600.0, false);

        // Both of these contain "://" yet fail `is_playable_url`, so a card click would send them to
        // the file loader as a literal (non-existent) path — a dead card. They must not render.
        history.record(Path::new("file:///home/user/gone.mkv"), 120.0, 600.0, false);
        history.record(Path::new("nope://"), 120.0, 600.0, false);

        let cards = history.continue_watching(false, 10);
        let paths: Vec<&str> = cards.iter().map(|card| card.path.as_str()).collect();
        assert_eq!(paths, ["https://example.com/live.m3u8"]);
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
