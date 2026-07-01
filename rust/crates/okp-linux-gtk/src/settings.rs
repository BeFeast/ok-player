use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const SETTINGS_VERSION: u32 = 1;
const DEFAULT_VOLUME: f64 = 100.0;
const MAX_VOLUME: f64 = 130.0;

#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    data: SettingsFile,
    dirty: bool,
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::open()
    }
}

impl SettingsStore {
    pub fn open() -> Self {
        let path = settings_path();
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str::<SettingsFile>(&json).ok())
            .filter(|data| data.version == SETTINGS_VERSION)
            .unwrap_or_else(|| SettingsFile {
                version: SETTINGS_VERSION,
                playback: PlaybackSettings::default(),
            });

        Self {
            path,
            data,
            dirty: false,
        }
    }

    pub fn volume(&self) -> f64 {
        normalized_volume(self.data.playback.volume).unwrap_or(DEFAULT_VOLUME)
    }

    pub fn set_volume(&mut self, volume: f64) {
        let Some(volume) = normalized_volume(Some(volume)) else {
            return;
        };

        if !same_volume(self.data.playback.volume, volume) {
            self.data.playback.volume = Some(volume);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    #[serde(default)]
    playback: PlaybackSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct PlaybackSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    volume: Option<f64>,
}

fn settings_path() -> PathBuf {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(config_home).join("ok-player/settings.json");
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".config/ok-player/settings.json");
    }

    PathBuf::from("ok-player-settings.json")
}

fn normalized_volume(volume: Option<f64>) -> Option<f64> {
    volume
        .filter(|volume| volume.is_finite())
        .map(|volume| volume.clamp(0.0, MAX_VOLUME))
}

fn same_volume(current: Option<f64>, updated: f64) -> bool {
    current
        .and_then(|volume| normalized_volume(Some(volume)))
        .is_some_and(|volume| (volume - updated).abs() < 0.005)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SettingsStore {
        SettingsStore {
            path: PathBuf::from("unused.json"),
            data: SettingsFile {
                version: SETTINGS_VERSION,
                playback: PlaybackSettings::default(),
            },
            dirty: false,
        }
    }

    #[test]
    fn volume_defaults_to_one_hundred() {
        assert_eq!(store().volume(), 100.0);
    }

    #[test]
    fn stores_clamped_finite_volume() {
        let mut settings = store();

        settings.set_volume(140.0);

        assert_eq!(settings.volume(), 130.0);
        assert!(settings.dirty);
    }

    #[test]
    fn ignores_non_finite_volume() {
        let mut settings = store();

        settings.set_volume(f64::NAN);

        assert_eq!(settings.volume(), 100.0);
        assert!(!settings.dirty);
    }

    #[test]
    fn unchanged_volume_does_not_mark_dirty() {
        let mut settings = store();

        settings.set_volume(100.0);
        settings.dirty = false;
        settings.set_volume(100.002);

        assert!(!settings.dirty);
    }
}
