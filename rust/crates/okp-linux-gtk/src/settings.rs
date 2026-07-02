use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SETTINGS_VERSION: u32 = 1;
const DEFAULT_VOLUME: f64 = 100.0;
const MAX_VOLUME: f64 = 130.0;
const DEFAULT_RESUME: bool = true;
const DEFAULT_AUTO_ADVANCE: bool = true;
const DEFAULT_SHUFFLE: bool = false;
const REPEAT_OFF: &str = "off";
const REPEAT_ONE: &str = "one";
const REPEAT_ALL: &str = "all";
const DEFAULT_AUTO_CHECK_UPDATES: bool = true;
const HWDEC_OFF: &str = "no";
const HWDEC_AUTO_SAFE: &str = "auto-safe";

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
                video: VideoSettings::default(),
                updates: UpdateSettings::default(),
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

    pub fn resume_enabled(&self) -> bool {
        self.data.playback.resume.unwrap_or(DEFAULT_RESUME)
    }

    pub fn auto_advance_enabled(&self) -> bool {
        self.data
            .playback
            .auto_advance
            .unwrap_or(DEFAULT_AUTO_ADVANCE)
    }

    pub fn shuffle_enabled(&self) -> bool {
        self.data.playback.shuffle.unwrap_or(DEFAULT_SHUFFLE)
    }

    pub fn repeat_mode(&self) -> &'static str {
        normalized_repeat(self.data.playback.repeat.as_deref())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn auto_check_updates(&self) -> bool {
        self.data.updates.auto_check
    }

    pub fn hardware_decode_enabled(&self) -> bool {
        normalized_hwdec(self.data.video.hwdec.as_deref()) == HWDEC_AUTO_SAFE
    }

    pub fn hardware_decode_mpv_option(&self) -> &'static str {
        normalized_hwdec(self.data.video.hwdec.as_deref())
    }

    pub fn hardware_decode_label(&self) -> &'static str {
        if self.hardware_decode_enabled() {
            "auto-safe"
        } else {
            "off"
        }
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

    pub fn set_resume_enabled(&mut self, enabled: bool) {
        if self.resume_enabled() != enabled {
            self.data.playback.resume = Some(enabled);
            self.dirty = true;
        }
    }

    pub fn set_auto_advance_enabled(&mut self, enabled: bool) {
        if self.auto_advance_enabled() != enabled {
            self.data.playback.auto_advance = Some(enabled);
            self.dirty = true;
        }
    }

    pub fn set_shuffle_enabled(&mut self, enabled: bool) {
        if self.shuffle_enabled() != enabled {
            self.data.playback.shuffle = Some(enabled);
            self.dirty = true;
        }
    }

    pub fn set_repeat_mode(&mut self, repeat: &str) {
        let repeat = normalized_repeat(Some(repeat));
        if self.repeat_mode() != repeat {
            self.data.playback.repeat = Some(repeat.to_owned());
            self.dirty = true;
        }
    }

    pub fn set_auto_check_updates(&mut self, enabled: bool) {
        if self.data.updates.auto_check != enabled {
            self.data.updates.auto_check = enabled;
            self.dirty = true;
        }
    }

    pub fn set_hardware_decode_enabled(&mut self, enabled: bool) {
        let updated = if enabled { HWDEC_AUTO_SAFE } else { HWDEC_OFF };
        if normalized_hwdec(self.data.video.hwdec.as_deref()) != updated {
            self.data.video.hwdec = Some(updated.to_owned());
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
    #[serde(default)]
    video: VideoSettings,
    #[serde(default)]
    updates: UpdateSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct PlaybackSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resume: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auto_advance: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shuffle: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct VideoSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hwdec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct UpdateSettings {
    #[serde(default = "default_auto_check_updates")]
    auto_check: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check: DEFAULT_AUTO_CHECK_UPDATES,
        }
    }
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

fn normalized_hwdec(hwdec: Option<&str>) -> &'static str {
    match hwdec {
        Some(HWDEC_AUTO_SAFE) => HWDEC_AUTO_SAFE,
        _ => HWDEC_OFF,
    }
}

fn normalized_repeat(repeat: Option<&str>) -> &'static str {
    match repeat {
        Some(REPEAT_ONE) => REPEAT_ONE,
        Some(REPEAT_ALL) => REPEAT_ALL,
        _ => REPEAT_OFF,
    }
}

fn same_volume(current: Option<f64>, updated: f64) -> bool {
    current
        .and_then(|volume| normalized_volume(Some(volume)))
        .is_some_and(|volume| (volume - updated).abs() < 0.005)
}

fn default_auto_check_updates() -> bool {
    DEFAULT_AUTO_CHECK_UPDATES
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
                video: VideoSettings::default(),
                updates: UpdateSettings::default(),
            },
            dirty: false,
        }
    }

    #[test]
    fn volume_defaults_to_one_hundred() {
        assert_eq!(store().volume(), 100.0);
    }

    #[test]
    fn playback_defaults_match_player_modes() {
        let settings = store();

        assert!(settings.resume_enabled());
        assert!(settings.auto_advance_enabled());
        assert!(!settings.shuffle_enabled());
        assert_eq!(settings.repeat_mode(), "off");
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

    #[test]
    fn auto_update_checks_default_on() {
        assert!(store().auto_check_updates());
    }

    #[test]
    fn auto_update_toggle_marks_dirty_once() {
        let mut settings = store();

        settings.set_auto_check_updates(false);

        assert!(!settings.auto_check_updates());
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_auto_check_updates(false);

        assert!(!settings.dirty);
    }

    #[test]
    fn playback_toggles_mark_dirty_once() {
        let mut settings = store();

        settings.set_resume_enabled(false);
        settings.set_auto_advance_enabled(false);
        settings.set_shuffle_enabled(true);
        settings.set_repeat_mode("all");

        assert!(!settings.resume_enabled());
        assert!(!settings.auto_advance_enabled());
        assert!(settings.shuffle_enabled());
        assert_eq!(settings.repeat_mode(), "all");
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_resume_enabled(false);
        settings.set_auto_advance_enabled(false);
        settings.set_shuffle_enabled(true);
        settings.set_repeat_mode("all");

        assert!(!settings.dirty);
    }

    #[test]
    fn unknown_repeat_mode_falls_back_to_off() {
        let mut settings = store();
        settings.data.playback.repeat = Some("forever".to_owned());

        assert_eq!(settings.repeat_mode(), "off");
    }

    #[test]
    fn hardware_decode_defaults_off() {
        let settings = store();

        assert!(!settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "no");
        assert_eq!(settings.hardware_decode_label(), "off");
    }

    #[test]
    fn hardware_decode_toggle_marks_dirty_once() {
        let mut settings = store();

        settings.set_hardware_decode_enabled(true);

        assert!(settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "auto-safe");
        assert_eq!(settings.hardware_decode_label(), "auto-safe");
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_hardware_decode_enabled(true);

        assert!(!settings.dirty);
    }

    #[test]
    fn unknown_hardware_decode_value_falls_back_to_off() {
        let mut settings = store();
        settings.data.video.hwdec = Some("yes-please".to_owned());

        assert!(!settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "no");
    }
}
