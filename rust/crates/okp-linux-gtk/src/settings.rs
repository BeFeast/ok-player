use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use okp_core::gapless::{GaplessPlaybackCapability, effective_gapless_enabled};
use okp_core::settings::{
    AppearanceTheme, ScreenshotFormat, Settings, SkippedUpdateVersions, UpdateChannel,
};
use okp_core::subtitle_style::{self, SubtitleStyle};

const DEFAULT_VOLUME: f64 = 100.0;
const MAX_VOLUME: f64 = 130.0;
const DEFAULT_RESUME: bool = true;
const DEFAULT_AUTO_ADVANCE: bool = true;
const DEFAULT_SHUFFLE: bool = false;
const DEFAULT_GAPLESS: bool = false;
const REPEAT_OFF: &str = "off";
const REPEAT_ONE: &str = "one";
const REPEAT_ALL: &str = "all";
const DEFAULT_AUDIO_NORMALIZATION: bool = false;
const DEFAULT_DOWNMIX_SURROUND_TO_STEREO: bool = false;
const DEFAULT_AUDIO_DEVICE: &str = "auto";
const HWDEC_OFF: &str = "no";
const HWDEC_AUTO_SAFE: &str = "auto-safe";
const DEFAULT_VIDEO_ADJUSTMENT: f64 = 0.0;
const MIN_VIDEO_ADJUSTMENT: f64 = -100.0;
const MAX_VIDEO_ADJUSTMENT: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoAdjustments {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub gamma: f64,
}

#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    data: Settings,
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
            .and_then(|json| Settings::load(&json))
            .unwrap_or_default();

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

    pub fn gapless_enabled(&self, capability: GaplessPlaybackCapability) -> bool {
        effective_gapless_enabled(
            self.data.playback.gapless.unwrap_or(DEFAULT_GAPLESS),
            capability,
        )
    }

    pub fn audio_normalization_enabled(&self) -> bool {
        self.data
            .audio
            .normalization
            .unwrap_or(DEFAULT_AUDIO_NORMALIZATION)
    }

    pub fn downmix_surround_to_stereo_enabled(&self) -> bool {
        self.data
            .audio
            .downmix_surround_to_stereo
            .unwrap_or(DEFAULT_DOWNMIX_SURROUND_TO_STEREO)
    }

    pub fn audio_device(&self) -> &str {
        normalized_audio_device(self.data.audio.device.as_deref())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn auto_check_updates(&self) -> bool {
        self.data.updates.auto_check
    }

    /// The persisted update channel. `Public` for every default install;
    /// `Candidate` only when the operator has explicitly enrolled this QA
    /// install in the rolling candidate channel (issue #339).
    pub fn update_channel(&self) -> UpdateChannel {
        self.data.updates.channel
    }

    pub fn skipped_update_version(&self, channel: UpdateChannel) -> Option<&str> {
        self.data.updates.skipped_versions.version(channel)
    }

    pub fn skipped_update_versions(&self) -> &SkippedUpdateVersions {
        &self.data.updates.skipped_versions
    }

    pub fn appearance_theme(&self) -> AppearanceTheme {
        AppearanceTheme::from_setting(self.data.appearance.theme.as_deref())
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

    pub fn video_adjustments(&self) -> VideoAdjustments {
        VideoAdjustments {
            brightness: self.brightness(),
            contrast: self.contrast(),
            saturation: self.saturation(),
            gamma: self.gamma(),
        }
    }

    pub fn subtitle_scale(&self) -> f64 {
        subtitle_style::normalized_scale(self.data.subtitles.scale)
    }

    pub fn subtitle_position(&self) -> i64 {
        subtitle_style::normalized_position(self.data.subtitles.position)
    }

    pub fn subtitle_style(&self) -> &'static SubtitleStyle {
        subtitle_style::from_key(self.data.subtitles.style.as_deref())
    }

    pub fn brightness(&self) -> f64 {
        normalized_video_adjustment(self.data.video.brightness).unwrap_or(DEFAULT_VIDEO_ADJUSTMENT)
    }

    pub fn contrast(&self) -> f64 {
        normalized_video_adjustment(self.data.video.contrast).unwrap_or(DEFAULT_VIDEO_ADJUSTMENT)
    }

    pub fn saturation(&self) -> f64 {
        normalized_video_adjustment(self.data.video.saturation).unwrap_or(DEFAULT_VIDEO_ADJUSTMENT)
    }

    pub fn gamma(&self) -> f64 {
        normalized_video_adjustment(self.data.video.gamma).unwrap_or(DEFAULT_VIDEO_ADJUSTMENT)
    }

    pub fn raw_mpv_config(&self) -> &str {
        self.data.advanced.mpv_conf.as_deref().unwrap_or("")
    }

    pub fn raw_keybindings_config(&self) -> &str {
        self.data.advanced.keybindings.as_deref().unwrap_or("")
    }

    pub fn screenshot_format(&self) -> ScreenshotFormat {
        self.data.screenshots.format.unwrap_or_default()
    }

    pub fn screenshot_directory(&self) -> Option<PathBuf> {
        let directory = self.data.screenshots.directory.as_deref()?.trim();
        let path = PathBuf::from(directory);
        path.is_absolute().then_some(path)
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

    pub fn set_gapless_enabled(
        &mut self,
        capability: GaplessPlaybackCapability,
        enabled: bool,
    ) -> bool {
        if !capability.allows_enablement() {
            return false;
        }
        if self.gapless_enabled(capability) != enabled {
            self.data.playback.gapless = Some(enabled);
            self.dirty = true;
        }
        true
    }

    pub fn set_audio_normalization_enabled(&mut self, enabled: bool) {
        if self.audio_normalization_enabled() != enabled {
            self.data.audio.normalization = Some(enabled);
            self.dirty = true;
        }
    }

    pub fn set_downmix_surround_to_stereo_enabled(&mut self, enabled: bool) {
        if self.downmix_surround_to_stereo_enabled() != enabled {
            self.data.audio.downmix_surround_to_stereo = Some(enabled);
            self.dirty = true;
        }
    }

    pub fn set_audio_device(&mut self, device: &str) {
        let device = normalized_audio_device(Some(device));
        if self.audio_device() != device {
            self.data.audio.device = audio_device_setting(device);
            self.dirty = true;
        }
    }

    pub fn set_auto_check_updates(&mut self, enabled: bool) {
        if self.data.updates.auto_check != enabled {
            self.data.updates.auto_check = enabled;
            self.dirty = true;
        }
    }

    pub fn set_skipped_update_version(&mut self, channel: UpdateChannel, version: Option<String>) {
        if self.skipped_update_version(channel) != version.as_deref() {
            self.data.updates.skipped_versions.set(channel, version);
            self.dirty = true;
        }
    }

    pub fn set_appearance_theme(&mut self, theme: AppearanceTheme) {
        if self.appearance_theme() != theme {
            self.data.appearance.theme = Some(theme.as_setting().to_owned());
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

    pub fn set_brightness(&mut self, value: f64) {
        if let Some(value) = normalized_video_adjustment(Some(value))
            && !same_video_adjustment(self.data.video.brightness, value)
        {
            self.data.video.brightness = video_adjustment_setting(value);
            self.dirty = true;
        }
    }

    pub fn set_contrast(&mut self, value: f64) {
        if let Some(value) = normalized_video_adjustment(Some(value))
            && !same_video_adjustment(self.data.video.contrast, value)
        {
            self.data.video.contrast = video_adjustment_setting(value);
            self.dirty = true;
        }
    }

    pub fn set_saturation(&mut self, value: f64) {
        if let Some(value) = normalized_video_adjustment(Some(value))
            && !same_video_adjustment(self.data.video.saturation, value)
        {
            self.data.video.saturation = video_adjustment_setting(value);
            self.dirty = true;
        }
    }

    pub fn set_gamma(&mut self, value: f64) {
        if let Some(value) = normalized_video_adjustment(Some(value))
            && !same_video_adjustment(self.data.video.gamma, value)
        {
            self.data.video.gamma = video_adjustment_setting(value);
            self.dirty = true;
        }
    }

    pub fn set_subtitle_scale(&mut self, value: f64) {
        let value = subtitle_style::normalized_scale(Some(value));
        if (self.subtitle_scale() - value).abs() >= 0.005 {
            self.data.subtitles.scale = Some(value);
            self.dirty = true;
        }
    }

    pub fn set_subtitle_position(&mut self, value: i64) {
        let value = subtitle_style::normalized_position(Some(value));
        if self.subtitle_position() != value {
            self.data.subtitles.position = Some(value);
            self.dirty = true;
        }
    }

    pub fn set_subtitle_style(&mut self, key: &str) {
        let style = subtitle_style::from_key(Some(key));
        if self.subtitle_style().key != style.key {
            self.data.subtitles.style = Some(style.key.to_owned());
            self.dirty = true;
        }
    }

    pub fn set_raw_mpv_config(&mut self, text: &str) {
        let updated = raw_mpv_config_setting(text);
        if self.data.advanced.mpv_conf != updated {
            self.data.advanced.mpv_conf = updated;
            self.dirty = true;
        }
    }

    pub fn set_raw_keybindings_config(&mut self, text: &str) {
        let updated = raw_mpv_config_setting(text);
        if self.data.advanced.keybindings != updated {
            self.data.advanced.keybindings = updated;
            self.dirty = true;
        }
    }

    pub fn set_screenshot_format(&mut self, format: ScreenshotFormat) {
        if self.screenshot_format() != format {
            self.data.screenshots.format = Some(format);
            self.dirty = true;
        }
    }

    pub fn set_screenshot_directory(&mut self, directory: Option<&Path>) -> bool {
        let updated = match directory {
            None => None,
            Some(directory) if directory.is_absolute() => {
                let Some(directory) = directory.to_str().map(str::trim) else {
                    return false;
                };
                if directory.is_empty() {
                    return false;
                }
                Some(directory.to_owned())
            }
            Some(_) => return false,
        };

        if self.data.screenshots.directory != updated {
            self.data.screenshots.directory = updated;
            self.dirty = true;
        }
        true
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

fn normalized_video_adjustment(value: Option<f64>) -> Option<f64> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(MIN_VIDEO_ADJUSTMENT, MAX_VIDEO_ADJUSTMENT))
}

fn normalized_audio_device(device: Option<&str>) -> &str {
    device
        .map(str::trim)
        .filter(|device| !device.is_empty())
        .unwrap_or(DEFAULT_AUDIO_DEVICE)
}

fn audio_device_setting(device: &str) -> Option<String> {
    if device == DEFAULT_AUDIO_DEVICE {
        None
    } else {
        Some(device.to_owned())
    }
}

fn video_adjustment_setting(value: f64) -> Option<f64> {
    if (value - DEFAULT_VIDEO_ADJUSTMENT).abs() < 0.005 {
        None
    } else {
        Some(value)
    }
}

fn raw_mpv_config_setting(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

fn normalized_hwdec(hwdec: Option<&str>) -> &'static str {
    match hwdec {
        Some(HWDEC_AUTO_SAFE) => HWDEC_AUTO_SAFE,
        Some(HWDEC_OFF) => HWDEC_OFF,
        // Match mpv's practical desktop default on supported systems: a clean
        // install must not fall back to software decoding for 4K HEVC. Users
        // who explicitly disable hardware decoding keep the persisted `no`.
        None => HWDEC_AUTO_SAFE,
        Some(_) => HWDEC_OFF,
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

fn same_video_adjustment(current: Option<f64>, updated: f64) -> bool {
    normalized_video_adjustment(current).is_some_and(|current| (current - updated).abs() < 0.005)
        || (current.is_none() && (updated - DEFAULT_VIDEO_ADJUSTMENT).abs() < 0.005)
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;

    fn store() -> SettingsStore {
        SettingsStore {
            path: PathBuf::from("unused.json"),
            data: Settings::default(),
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
        assert!(!settings.gapless_enabled(GaplessPlaybackCapability::Available));
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
    fn appearance_theme_defaults_to_auto_and_persists_light() {
        let mut settings = store();
        assert_eq!(settings.appearance_theme(), AppearanceTheme::Auto);

        settings.set_appearance_theme(AppearanceTheme::Light);
        assert_eq!(settings.appearance_theme(), AppearanceTheme::Light);
        assert!(settings.dirty);
    }

    #[test]
    fn audio_normalization_defaults_off() {
        assert!(!store().audio_normalization_enabled());
    }

    #[test]
    fn audio_normalization_toggle_marks_dirty_once() {
        let mut settings = store();

        settings.set_audio_normalization_enabled(true);

        assert!(settings.audio_normalization_enabled());
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_audio_normalization_enabled(true);

        assert!(!settings.dirty);
    }

    #[test]
    fn surround_downmix_defaults_off_and_toggle_marks_dirty_once() {
        let mut settings = store();

        assert!(!settings.downmix_surround_to_stereo_enabled());
        settings.set_downmix_surround_to_stereo_enabled(true);
        assert!(settings.downmix_surround_to_stereo_enabled());
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_downmix_surround_to_stereo_enabled(true);
        assert!(!settings.dirty);
    }

    #[test]
    fn audio_device_defaults_to_auto() {
        assert_eq!(store().audio_device(), "auto");
    }

    #[test]
    fn audio_device_setting_trims_and_marks_dirty_once() {
        let mut settings = store();

        settings.set_audio_device(" pulse/device ");

        assert_eq!(settings.audio_device(), "pulse/device");
        assert_eq!(settings.data.audio.device.as_deref(), Some("pulse/device"));
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_audio_device("pulse/device");

        assert!(!settings.dirty);
    }

    #[test]
    fn audio_device_stores_auto_as_none() {
        let mut settings = store();

        settings.set_audio_device("pulse/device");
        settings.dirty = false;
        settings.set_audio_device("auto");

        assert_eq!(settings.audio_device(), "auto");
        assert_eq!(settings.data.audio.device, None);
        assert!(settings.dirty);
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
    fn skipped_update_versions_save_and_reload_per_channel() {
        let dir = unique_temp_dir("okp-update-skip-settings");
        let path = dir.path().join("settings.json");
        let mut settings = SettingsStore {
            path: path.clone(),
            data: Settings::default(),
            dirty: false,
        };

        settings
            .set_skipped_update_version(UpdateChannel::Public, Some("0.11.0-beta.2".to_owned()));
        settings.set_skipped_update_version(
            UpdateChannel::Candidate,
            Some("0.11.0-beta.2.41".to_owned()),
        );
        settings.save().expect("skip settings should save");

        let json = fs::read_to_string(&path).expect("settings should be readable");
        let reloaded = Settings::load(&json).expect("settings should reload");
        assert_eq!(
            reloaded
                .updates
                .skipped_versions
                .version(UpdateChannel::Public),
            Some("0.11.0-beta.2")
        );
        assert_eq!(
            reloaded
                .updates
                .skipped_versions
                .version(UpdateChannel::Candidate),
            Some("0.11.0-beta.2.41")
        );
        dir.close().expect("temp settings dir should be removed");
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
    fn gapless_setting_is_persisted_only_when_capability_allows_it() {
        let mut settings = store();

        assert!(!settings.set_gapless_enabled(GaplessPlaybackCapability::Deferred, true));
        assert_eq!(settings.data.playback.gapless, None);
        assert!(!settings.dirty);

        assert!(settings.set_gapless_enabled(GaplessPlaybackCapability::Available, true));
        assert!(settings.gapless_enabled(GaplessPlaybackCapability::Available));
        assert_eq!(settings.data.playback.gapless, Some(true));
        assert!(settings.dirty);

        assert!(!settings.gapless_enabled(GaplessPlaybackCapability::Deferred));
    }

    #[test]
    fn unknown_repeat_mode_falls_back_to_off() {
        let mut settings = store();
        settings.data.playback.repeat = Some("forever".to_owned());

        assert_eq!(settings.repeat_mode(), "off");
    }

    #[test]
    fn hardware_decode_defaults_on() {
        let settings = store();

        assert!(settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "auto-safe");
        assert_eq!(settings.hardware_decode_label(), "auto-safe");
    }

    #[test]
    fn hardware_decode_toggle_marks_dirty_once() {
        let mut settings = store();

        settings.set_hardware_decode_enabled(false);

        assert!(!settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "no");
        assert_eq!(settings.hardware_decode_label(), "off");
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_hardware_decode_enabled(false);

        assert!(!settings.dirty);

        settings.set_hardware_decode_enabled(true);
        assert!(settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "auto-safe");
        assert!(settings.dirty);
    }

    #[test]
    fn unknown_hardware_decode_value_falls_back_to_off() {
        let mut settings = store();
        settings.data.video.hwdec = Some("yes-please".to_owned());

        assert!(!settings.hardware_decode_enabled());
        assert_eq!(settings.hardware_decode_mpv_option(), "no");
    }

    #[test]
    fn video_adjustments_default_to_zero() {
        let settings = store();

        assert_eq!(
            settings.video_adjustments(),
            VideoAdjustments {
                brightness: 0.0,
                contrast: 0.0,
                saturation: 0.0,
                gamma: 0.0,
            }
        );
    }

    #[test]
    fn video_adjustments_clamp_and_mark_dirty() {
        let mut settings = store();

        settings.set_brightness(150.0);
        settings.set_contrast(-125.0);
        settings.set_saturation(25.0);
        settings.set_gamma(-10.0);

        assert_eq!(settings.brightness(), 100.0);
        assert_eq!(settings.contrast(), -100.0);
        assert_eq!(settings.saturation(), 25.0);
        assert_eq!(settings.gamma(), -10.0);
        assert!(settings.dirty);
    }

    #[test]
    fn video_adjustments_ignore_non_finite_values() {
        let mut settings = store();

        settings.set_brightness(f64::NAN);
        settings.set_contrast(f64::INFINITY);

        assert_eq!(settings.brightness(), 0.0);
        assert_eq!(settings.contrast(), 0.0);
        assert!(!settings.dirty);
    }

    #[test]
    fn video_adjustments_store_default_as_none() {
        let mut settings = store();

        settings.set_brightness(12.0);
        assert_eq!(settings.data.video.brightness, Some(12.0));
        settings.dirty = false;

        settings.set_brightness(0.0);

        assert_eq!(settings.brightness(), 0.0);
        assert_eq!(settings.data.video.brightness, None);
        assert!(settings.dirty);
    }

    #[test]
    fn subtitle_presentation_defaults_match_windows() {
        let settings = store();

        assert_eq!(settings.subtitle_scale(), 1.0);
        assert_eq!(settings.subtitle_position(), 100);
        assert_eq!(settings.subtitle_style().key, "Default");
    }

    #[test]
    fn subtitle_presentation_normalizes_and_marks_dirty_once() {
        let mut settings = store();

        settings.set_subtitle_scale(9.0);
        settings.set_subtitle_position(-10);
        settings.set_subtitle_style("contrast");

        assert_eq!(settings.subtitle_scale(), 4.0);
        assert_eq!(settings.subtitle_position(), 0);
        assert_eq!(settings.subtitle_style().key, "Contrast");
        assert_eq!(settings.data.subtitles.style.as_deref(), Some("Contrast"));
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_subtitle_scale(4.0);
        settings.set_subtitle_position(0);
        settings.set_subtitle_style("Contrast");
        assert!(!settings.dirty);
    }

    #[test]
    fn unknown_subtitle_style_falls_back_without_persisting_an_invalid_key() {
        let mut settings = store();
        settings.data.subtitles.style = Some("Cinema".to_owned());

        assert_eq!(settings.subtitle_style().key, "Default");
        settings.set_subtitle_style("unknown");
        assert_eq!(settings.data.subtitles.style.as_deref(), Some("Cinema"));
        assert!(!settings.dirty);

        settings.set_subtitle_style("Bold");
        assert_eq!(settings.data.subtitles.style.as_deref(), Some("Bold"));
        assert!(settings.dirty);
    }

    #[test]
    fn subtitle_presentation_survives_save_and_reload() {
        let root = unique_temp_dir("okp-subtitle-settings");
        let path = root.path().join("missing-parent/settings.json");
        let mut settings = SettingsStore {
            path: path.clone(),
            data: Settings::default(),
            dirty: false,
        };

        settings.set_subtitle_scale(1.4);
        settings.set_subtitle_position(90);
        settings.set_subtitle_style("Contrast");
        settings.save().expect("subtitle settings should save");

        let data = fs::read_to_string(&path).expect("saved settings should be readable");
        let reloaded = Settings::load(&data).expect("saved settings should reload");
        assert_eq!(reloaded.subtitles.scale, Some(1.4));
        assert_eq!(reloaded.subtitles.position, Some(90));
        assert_eq!(reloaded.subtitles.style.as_deref(), Some("Contrast"));

        root.close()
            .expect("temporary settings directory should be removed");
    }

    #[test]
    fn raw_mpv_config_defaults_empty() {
        assert_eq!(store().raw_mpv_config(), "");
    }

    #[test]
    fn raw_mpv_config_stores_text_and_marks_dirty_once() {
        let mut settings = store();

        settings.set_raw_mpv_config("scale=ewa_lanczossharp\nprofile=gpu-hq\n");

        assert_eq!(
            settings.raw_mpv_config(),
            "scale=ewa_lanczossharp\nprofile=gpu-hq\n"
        );
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_raw_mpv_config("scale=ewa_lanczossharp\nprofile=gpu-hq\n");

        assert!(!settings.dirty);
    }

    #[test]
    fn raw_mpv_config_stores_blank_as_none() {
        let mut settings = store();

        settings.set_raw_mpv_config("profile=gpu-hq");
        settings.dirty = false;
        settings.set_raw_mpv_config("   \n\t");

        assert_eq!(settings.raw_mpv_config(), "");
        assert_eq!(settings.data.advanced.mpv_conf, None);
        assert!(settings.dirty);
    }

    #[test]
    fn raw_keybindings_config_stores_text_and_marks_dirty_once() {
        let mut settings = store();

        settings.set_raw_keybindings_config("play-pause=P\ncopy-frame=Shift+C\n");

        assert_eq!(
            settings.raw_keybindings_config(),
            "play-pause=P\ncopy-frame=Shift+C\n"
        );
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_raw_keybindings_config("play-pause=P\ncopy-frame=Shift+C\n");

        assert!(!settings.dirty);
    }

    #[test]
    fn raw_keybindings_config_stores_blank_as_none() {
        let mut settings = store();

        settings.set_raw_keybindings_config("play-pause=P");
        settings.dirty = false;
        settings.set_raw_keybindings_config("\n ");

        assert_eq!(settings.raw_keybindings_config(), "");
        assert_eq!(settings.data.advanced.keybindings, None);
        assert!(settings.dirty);
    }

    #[test]
    fn screenshot_settings_default_validate_and_mark_dirty_once() {
        let mut settings = store();
        assert_eq!(settings.screenshot_format(), ScreenshotFormat::Png);
        assert_eq!(settings.screenshot_directory(), None);

        assert!(!settings.set_screenshot_directory(Some(Path::new("relative/captures"))));
        assert!(!settings.dirty);

        settings.set_screenshot_format(ScreenshotFormat::Jpeg);
        assert!(settings.set_screenshot_directory(Some(Path::new("/captures"))));
        assert_eq!(settings.screenshot_format(), ScreenshotFormat::Jpeg);
        assert_eq!(
            settings.screenshot_directory().as_deref(),
            Some(Path::new("/captures"))
        );
        assert!(settings.dirty);

        settings.dirty = false;
        settings.set_screenshot_format(ScreenshotFormat::Jpeg);
        assert!(settings.set_screenshot_directory(Some(Path::new("/captures"))));
        assert!(!settings.dirty);
    }

    #[test]
    fn screenshot_settings_persist_in_the_human_readable_document() {
        let directory = unique_temp_dir("okp-screenshot-settings");
        let path = directory.path().join("settings.json");
        let mut settings = SettingsStore {
            path: path.clone(),
            data: Settings::default(),
            dirty: false,
        };
        settings.set_screenshot_format(ScreenshotFormat::Webp);
        assert!(settings.set_screenshot_directory(Some(directory.path())));

        settings.save().expect("save settings");

        let json = fs::read_to_string(path).expect("read settings");
        assert!(json.contains("\"screenshots\""));
        assert!(json.contains("\"format\": \"webp\""));
        let restored = Settings::load(&json).expect("reload settings");
        assert_eq!(restored.screenshots.format, Some(ScreenshotFormat::Webp));
        assert_eq!(
            restored.screenshots.directory.as_deref(),
            directory.path().to_str()
        );
    }
}
