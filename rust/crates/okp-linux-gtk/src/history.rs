use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const HISTORY_VERSION: u32 = 1;

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
            .and_then(|json| serde_json::from_str::<HistoryFile>(&json).ok())
            .filter(|data| data.version == HISTORY_VERSION)
            .unwrap_or_else(|| HistoryFile {
                version: HISTORY_VERSION,
                files: BTreeMap::new(),
            });

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

        self.data.files.insert(
            key,
            HistoryRecord {
                position: stored_position,
                duration,
                finished: finished || (existing_finished && final_stretch),
                updated_at_unix: unix_now(),
            },
        );
        self.dirty = true;
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

#[derive(Debug, Serialize, Deserialize)]
struct HistoryFile {
    version: u32,
    files: BTreeMap<String, HistoryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryRecord {
    position: f64,
    duration: f64,
    finished: bool,
    updated_at_unix: i64,
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
            data: HistoryFile {
                version: HISTORY_VERSION,
                files: BTreeMap::new(),
            },
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
}
