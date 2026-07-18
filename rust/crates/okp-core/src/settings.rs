//! Cross-platform persistence schema for application settings (EPIC #134, B9).
//!
//! This is the shared "truth" both shells converge on: the Linux GTK shell and the
//! future WinUI consumer (over the C ABI) read and write the same canonical document.
//! Only the *schema* and the *migration* live here — path resolution and file IO stay
//! behind a shell seam (XDG on Linux, `%APPDATA%` on Windows), so this module never
//! touches the filesystem.
//!
//! The canonical form is snake_case, sectioned, and versioned ([`SETTINGS_VERSION`]).
//! It is a superset of both current on-disk dialects, so migration never drops a
//! platform's state:
//!
//! - **Linux alpha dialect** — the snake_case document the GTK shell shipped
//!   (`{ "version": 1, "playback": {…}, "audio": {…}, … }`). It is a structural
//!   subset of the canonical form, so it upgrades in place (the new sections and
//!   Windows-only fields default to absent).
//! - **Windows dialect** — the PascalCase, flat document `OkPlayer.Core.AppSettings`
//!   serializes with System.Text.Json (`{ "Theme": …, "DefaultVolume": …,
//!   "SchemaVersion": 1 }`). It is remapped field by field.
//!
//! See `docs/core-compatibility.md` for the full migration story and the field map.

use serde::{Deserialize, Serialize};

/// Version stamped into the canonical document. Bumped from the Linux alpha `1` to
/// mark the unified cross-platform schema; a loaded `1` document upgrades to this.
pub const SETTINGS_VERSION: u32 = 2;

const HWDEC_OFF: &str = "no";
const HWDEC_AUTO_SAFE: &str = "auto-safe";

/// The canonical settings document. Every field a shell does not yet understand is
/// carried through untouched on save, so the shared schema can grow without either
/// shell losing the other's state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    #[serde(default)]
    pub playback: PlaybackSettings,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub video: VideoSettings,
    #[serde(default, skip_serializing_if = "SubtitleSettings::is_empty")]
    pub subtitles: SubtitleSettings,
    #[serde(default, skip_serializing_if = "AppearanceSettings::is_empty")]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub advanced: AdvancedSettings,
    #[serde(default, skip_serializing_if = "ScreenshotSettings::is_empty")]
    pub screenshots: ScreenshotSettings,
    #[serde(default, skip_serializing_if = "PrivacySettings::is_empty")]
    pub privacy: PrivacySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            playback: PlaybackSettings::default(),
            audio: AudioSettings::default(),
            video: VideoSettings::default(),
            subtitles: SubtitleSettings::default(),
            appearance: AppearanceSettings::default(),
            updates: UpdateSettings::default(),
            advanced: AdvancedSettings::default(),
            screenshots: ScreenshotSettings::default(),
            privacy: PrivacySettings::default(),
        }
    }
}

impl Settings {
    /// Load a settings document from raw JSON, migrating whichever on-disk dialect it
    /// is. Returns `None` for input that matches no known dialect (an unrecognized or
    /// unreadable file) so the shell can fall back to defaults, exactly as both shells
    /// already treat a corrupt file.
    pub fn load(raw: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        let object = value.as_object()?;
        // A lowercase `version` key is the Linux alpha / canonical marker; the Windows
        // document has no such key (its counterpart is PascalCase `SchemaVersion`).
        let is_native = object.contains_key("version");
        let is_windows = object.contains_key("SchemaVersion");

        if is_native {
            let mut settings: Settings = serde_json::from_value(value).ok()?;
            // Reject a document newer than we understand rather than silently
            // downgrading it; a `0` version is a malformed stamp.
            if settings.version == 0 || settings.version > SETTINGS_VERSION {
                return None;
            }
            settings.version = SETTINGS_VERSION;
            Some(settings)
        } else if is_windows {
            let windows: WindowsSettings = serde_json::from_value(value).ok()?;
            Some(windows.into_canonical())
        } else {
            None
        }
    }
}

/// Playback preferences. The first five fields are the Linux alpha set, `gapless` is
/// the shared reserved preference, and the last three are Windows-only defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_advance: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shuffle: Option<bool>,
    /// User preference reserved for an engine-managed playlist path. Shells must
    /// capability-gate this value before applying or exposing it as enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gapless: Option<bool>,
    /// Windows `DefaultSpeed` — the speed a newly opened file starts at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_speed: Option<f64>,
    /// Windows `SkipStep` — seconds the arrow keys seek.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_step_seconds: Option<i64>,
    /// Windows `HideControlsWhenPaused`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_controls_when_paused: Option<bool>,
}

/// Audio preferences.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalization: Option<bool>,
    /// Force multichannel sources through mpv's stereo channel layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downmix_surround_to_stereo: Option<bool>,
    /// mpv output device id; absent means the platform default (`auto`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

/// Video preferences. `hwdec` holds the mpv option string (`no` / `auto-safe`); the
/// four adjustments are the Linux-only picture controls.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hwdec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamma: Option<f64>,
}

/// Default-subtitle presentation shared by both desktop shells. Linux additionally stores
/// per-file scale overrides in history; these values remain the defaults for new media and own
/// global position/style.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SubtitleSettings {
    /// Windows `SubtitleScale` — size multiplier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Windows `SubtitlePosition` — `sub-pos` (100 = bottom, lower = higher).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    /// Windows `SubtitleStyle` — appearance preset key (see [`crate::subtitle_style`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

impl SubtitleSettings {
    fn is_empty(&self) -> bool {
        self.scale.is_none() && self.position.is_none() && self.style.is_none()
    }
}

/// Appearance preferences shared by the desktop shells.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    /// Desktop theme — `Light` / `Dark` / `Auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Windows `AccentSource` — `System` / `OkTeal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_source: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AppearanceTheme {
    Light,
    Dark,
    #[default]
    Auto,
}

impl AppearanceTheme {
    pub fn from_setting(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("Light") => Self::Light,
            Some(value) if value.eq_ignore_ascii_case("Dark") => Self::Dark,
            _ => Self::Auto,
        }
    }

    pub const fn as_setting(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Auto => "Auto",
        }
    }
}

impl AppearanceSettings {
    fn is_empty(&self) -> bool {
        self.theme.is_none() && self.accent_source.is_none()
    }
}

/// Which update channel an install discovers builds from. `Public` is the
/// default: it discovers the public beta/stable feed and its user behavior is
/// unchanged. `Candidate` is an *explicit* opt-in for QA installs that follow the
/// rolling Linux candidate channel (issue #339); a default install never selects
/// it, so the public feed stays byte-for-byte the fleet's only source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannel {
    #[default]
    Public,
    Candidate,
}

impl UpdateChannel {
    /// Parses the cross-shell string encoding, defaulting to `Public` for any
    /// unknown or absent value so a typo never silently enrolls an install in
    /// the candidate channel.
    pub fn from_setting(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("candidate") => Self::Candidate,
            _ => Self::Public,
        }
    }

    /// The persisted string encoding.
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Candidate => "candidate",
        }
    }
}

/// Update-check preference. Always serialized (matching both shells), default on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default = "default_auto_check")]
    pub auto_check: bool,
    /// Discovery channel; absent in older files, which load as `Public` so no
    /// existing install is silently moved off the public feed.
    #[serde(default)]
    pub channel: UpdateChannel,
    /// Exact versions the user skipped, isolated by discovery channel. A skip
    /// suppresses only the matching version; a newer version is offered
    /// normally. Empty channel entries stay out of the human-readable JSON.
    #[serde(default, skip_serializing_if = "SkippedUpdateVersions::is_empty")]
    pub skipped_versions: SkippedUpdateVersions,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check: default_auto_check(),
            channel: UpdateChannel::default(),
            skipped_versions: SkippedUpdateVersions::default(),
        }
    }
}

/// Per-channel exact-version update suppression. Public and rolling candidate
/// installs never share a skip slot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SkippedUpdateVersions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<String>,
}

impl SkippedUpdateVersions {
    pub fn version(&self, channel: UpdateChannel) -> Option<&str> {
        match channel {
            UpdateChannel::Public => self.public.as_deref(),
            UpdateChannel::Candidate => self.candidate.as_deref(),
        }
    }

    pub fn set(&mut self, channel: UpdateChannel, version: Option<String>) {
        match channel {
            UpdateChannel::Public => self.public = version,
            UpdateChannel::Candidate => self.candidate = version,
        }
    }

    pub fn is_skipped(&self, channel: UpdateChannel, version: &str) -> bool {
        self.version(channel) == Some(version)
    }

    fn is_empty(&self) -> bool {
        self.public.is_none() && self.candidate.is_none()
    }
}

/// Power-user escape hatches. `mpv_conf` is the raw mpv.conf text; on Windows this
/// lives in a separate `mpv.conf` file, so migrating Windows settings leaves it empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AdvancedSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpv_conf: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keybindings: Option<String>,
}

/// Screenshot capture preferences shared by desktop shells. The directory is a
/// platform path string; each shell resolves and validates it before filesystem IO.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ScreenshotFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

impl ScreenshotSettings {
    fn is_empty(&self) -> bool {
        self.format.is_none() && self.directory.is_none()
    }
}

/// Formats supported by mpv screenshot capture and the GTK clipboard loader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    Jpeg,
    Webp,
    #[default]
    #[serde(other)]
    Png,
}

impl ScreenshotFormat {
    pub const ALL: [Self; 3] = [Self::Png, Self::Jpeg, Self::Webp];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
        }
    }
}

/// Privacy, a Windows-only section.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// Windows `HistoryRetentionDays` — prune history older than N days; 0 = keep forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_retention_days: Option<i64>,
}

impl PrivacySettings {
    fn is_empty(&self) -> bool {
        self.history_retention_days.is_none()
    }
}

fn default_auto_check() -> bool {
    true
}

/// Deserialization shape for the Windows `AppSettings` document: flat, PascalCase, all
/// fields optional so a partial or older file still loads. Converted to the canonical
/// [`Settings`] by [`WindowsSettings::into_canonical`].
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct WindowsSettings {
    theme: Option<String>,
    accent_source: Option<String>,
    resume_playback: Option<bool>,
    hide_controls_when_paused: Option<bool>,
    default_speed: Option<f64>,
    skip_step: Option<i64>,
    hardware_decoding: Option<bool>,
    default_volume: Option<i64>,
    audio_normalization: Option<bool>,
    audio_device: Option<String>,
    subtitle_scale: Option<f64>,
    subtitle_position: Option<i64>,
    subtitle_style: Option<String>,
    history_retention_days: Option<i64>,
    auto_check_updates: Option<bool>,
}

impl WindowsSettings {
    fn into_canonical(self) -> Settings {
        Settings {
            version: SETTINGS_VERSION,
            playback: PlaybackSettings {
                volume: self.default_volume.map(|volume| volume as f64),
                resume: self.resume_playback,
                auto_advance: None,
                repeat: None,
                shuffle: None,
                gapless: None,
                default_speed: self.default_speed,
                skip_step_seconds: self.skip_step,
                hide_controls_when_paused: self.hide_controls_when_paused,
            },
            audio: AudioSettings {
                normalization: self.audio_normalization,
                downmix_surround_to_stereo: None,
                // Windows uses "" for "device not remembered"; the canonical form uses
                // absent, matching the Linux `auto` convention.
                device: self.audio_device.filter(|device| !device.is_empty()),
            },
            video: VideoSettings {
                hwdec: self.hardware_decoding.map(|on| hwdec_option(on).to_owned()),
                brightness: None,
                contrast: None,
                saturation: None,
                gamma: None,
            },
            subtitles: SubtitleSettings {
                scale: self.subtitle_scale,
                position: self.subtitle_position,
                style: self.subtitle_style,
            },
            appearance: AppearanceSettings {
                theme: self.theme,
                accent_source: self.accent_source,
            },
            updates: UpdateSettings {
                auto_check: self.auto_check_updates.unwrap_or_else(default_auto_check),
                // Windows has no candidate channel; migrated installs stay public.
                channel: UpdateChannel::Public,
                skipped_versions: SkippedUpdateVersions::default(),
            },
            advanced: AdvancedSettings::default(),
            screenshots: ScreenshotSettings::default(),
            privacy: PrivacySettings {
                history_retention_days: self.history_retention_days,
            },
        }
    }
}

/// The mpv `hwdec` option string for a hardware-decoding toggle, the encoding both
/// shells persist (`auto-safe` on, `no` off).
fn hwdec_option(enabled: bool) -> &'static str {
    if enabled { HWDEC_AUTO_SAFE } else { HWDEC_OFF }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_stamps_the_current_version() {
        assert_eq!(Settings::default().version, SETTINGS_VERSION);
        assert!(Settings::default().updates.auto_check);
        // A default install is on the public channel; enrollment is explicit.
        assert_eq!(Settings::default().updates.channel, UpdateChannel::Public);
    }

    #[test]
    fn update_channel_parses_and_defaults_to_public() {
        assert_eq!(
            UpdateChannel::from_setting(Some("candidate")),
            UpdateChannel::Candidate
        );
        assert_eq!(
            UpdateChannel::from_setting(Some("Candidate")),
            UpdateChannel::Candidate
        );
        assert_eq!(
            UpdateChannel::from_setting(Some("public")),
            UpdateChannel::Public
        );
        // Unknown or absent never silently enrolls in the candidate channel.
        assert_eq!(
            UpdateChannel::from_setting(Some("beta")),
            UpdateChannel::Public
        );
        assert_eq!(UpdateChannel::from_setting(None), UpdateChannel::Public);
        assert_eq!(UpdateChannel::Candidate.as_setting(), "candidate");
        assert_eq!(UpdateChannel::Public.as_setting(), "public");
    }

    #[test]
    fn update_channel_absent_in_older_files_loads_as_public() {
        let updates: UpdateSettings =
            serde_json::from_str(r#"{"auto_check":true}"#).expect("older files omit channel");
        assert_eq!(updates.channel, UpdateChannel::Public);
        let enrolled: UpdateSettings =
            serde_json::from_str(r#"{"auto_check":true,"channel":"candidate"}"#)
                .expect("channel round-trips");
        assert_eq!(enrolled.channel, UpdateChannel::Candidate);
    }

    #[test]
    fn skipped_update_versions_round_trip_per_channel() {
        let mut settings = Settings::default();
        settings
            .updates
            .skipped_versions
            .set(UpdateChannel::Public, Some("0.11.0-beta.2".to_owned()));
        settings.updates.skipped_versions.set(
            UpdateChannel::Candidate,
            Some("0.11.0-beta.2.41".to_owned()),
        );

        let json = serde_json::to_string_pretty(&settings).expect("settings should serialize");
        let loaded = Settings::load(&json).expect("settings should reload");

        assert_eq!(
            loaded
                .updates
                .skipped_versions
                .version(UpdateChannel::Public),
            Some("0.11.0-beta.2")
        );
        assert_eq!(
            loaded
                .updates
                .skipped_versions
                .version(UpdateChannel::Candidate),
            Some("0.11.0-beta.2.41")
        );
    }

    #[test]
    fn older_settings_without_skip_state_load_with_empty_channel_slots() {
        let settings = Settings::load(
            r#"{
                "version": 2,
                "updates": { "auto_check": true, "channel": "candidate" }
            }"#,
        )
        .expect("older canonical settings should load");

        assert!(
            settings
                .updates
                .skipped_versions
                .version(UpdateChannel::Public)
                .is_none()
        );
        assert!(
            settings
                .updates
                .skipped_versions
                .version(UpdateChannel::Candidate)
                .is_none()
        );
    }

    #[test]
    fn appearance_theme_parses_cross_shell_values() {
        assert_eq!(
            AppearanceTheme::from_setting(Some("Light")),
            AppearanceTheme::Light
        );
        assert_eq!(
            AppearanceTheme::from_setting(Some("dark")),
            AppearanceTheme::Dark
        );
        assert_eq!(
            AppearanceTheme::from_setting(Some("Auto")),
            AppearanceTheme::Auto
        );
        assert_eq!(
            AppearanceTheme::from_setting(Some("unknown")),
            AppearanceTheme::Auto
        );
        assert_eq!(AppearanceTheme::from_setting(None), AppearanceTheme::Auto);
        assert_eq!(AppearanceTheme::Light.as_setting(), "Light");
        assert_eq!(AppearanceTheme::Auto.as_setting(), "Auto");
    }

    #[test]
    fn load_rejects_unrecognized_input() {
        assert!(Settings::load("not json").is_none());
        assert!(Settings::load("[]").is_none());
        assert!(Settings::load("{}").is_none());
        assert!(Settings::load("42").is_none());
    }

    // ---- Linux alpha (snake_case) dialect ----

    #[test]
    fn migrates_linux_alpha_document_in_place() {
        // A representative Linux alpha settings.json (version 1, snake_case sections).
        let raw = r#"{
            "version": 1,
            "playback": { "volume": 80.0, "resume": false, "repeat": "all", "shuffle": true },
            "audio": { "normalization": true, "device": "pulse/device" },
            "video": { "hwdec": "auto-safe", "brightness": 12.0 },
            "updates": { "auto_check": false },
            "advanced": { "mpv_conf": "profile=gpu-hq\n", "keybindings": "play-pause=P\n" }
        }"#;

        let settings = Settings::load(raw).expect("linux alpha document should load");

        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.playback.volume, Some(80.0));
        assert_eq!(settings.playback.resume, Some(false));
        assert_eq!(settings.playback.repeat.as_deref(), Some("all"));
        assert_eq!(settings.playback.shuffle, Some(true));
        assert_eq!(settings.audio.normalization, Some(true));
        assert_eq!(settings.audio.device.as_deref(), Some("pulse/device"));
        assert_eq!(settings.video.hwdec.as_deref(), Some("auto-safe"));
        assert_eq!(settings.video.brightness, Some(12.0));
        assert!(!settings.updates.auto_check);
        assert_eq!(
            settings.advanced.mpv_conf.as_deref(),
            Some("profile=gpu-hq\n")
        );
        assert_eq!(
            settings.advanced.keybindings.as_deref(),
            Some("play-pause=P\n")
        );
        // Sections the alpha document never carried default to absent.
        assert!(settings.subtitles.is_empty());
        assert!(settings.appearance.is_empty());
        assert!(settings.screenshots.is_empty());
        assert!(settings.privacy.is_empty());
    }

    #[test]
    fn a_canonical_document_round_trips() {
        let mut settings = Settings::default();
        settings.playback.volume = Some(55.0);
        settings.playback.gapless = Some(true);
        settings.appearance.theme = Some("Dark".to_owned());
        settings.screenshots.format = Some(ScreenshotFormat::Webp);
        settings.screenshots.directory = Some("/captures".to_owned());
        settings.privacy.history_retention_days = Some(30);

        let json = serde_json::to_string(&settings).expect("serialize");
        let restored = Settings::load(&json).expect("canonical document should load");

        assert_eq!(restored, settings);
    }

    #[test]
    fn gapless_preference_persists_in_the_canonical_playback_section() {
        let raw = r#"{
            "version": 2,
            "playback": { "gapless": true }
        }"#;

        let settings = Settings::load(raw).expect("gapless preference should load");
        assert_eq!(settings.playback.gapless, Some(true));

        let json = serde_json::to_string(&settings).expect("serialize");
        assert!(json.contains("\"gapless\":true"));
        assert_eq!(Settings::load(&json), Some(settings));
    }

    #[test]
    fn empty_windows_only_sections_are_omitted_from_a_linux_document() {
        // A Linux-shaped document never populates the Windows-only sections, so they
        // must not appear in the serialized output (keeps Linux files clean).
        let json = serde_json::to_string(&Settings::default()).expect("serialize");
        assert!(!json.contains("subtitles"));
        assert!(!json.contains("appearance"));
        assert!(!json.contains("screenshots"));
        assert!(!json.contains("privacy"));
    }

    #[test]
    fn subtitle_defaults_round_trip_in_the_canonical_document() {
        let raw = r#"{
            "version": 2,
            "subtitles": { "scale": 1.4, "position": 90, "style": "Contrast" }
        }"#;
        let settings = Settings::load(raw).expect("subtitle defaults should load");

        assert_eq!(settings.subtitles.scale, Some(1.4));
        assert_eq!(settings.subtitles.position, Some(90));
        assert_eq!(settings.subtitles.style.as_deref(), Some("Contrast"));

        let json = serde_json::to_string(&settings).expect("serialize");
        assert_eq!(Settings::load(&json), Some(settings));
    }

    #[test]
    fn screenshot_settings_round_trip_and_unknown_formats_fall_back_to_png() {
        let raw = r#"{
            "version": 2,
            "screenshots": { "format": "webp", "directory": "/captures" }
        }"#;
        let settings = Settings::load(raw).expect("screenshot settings should load");
        assert_eq!(settings.screenshots.format, Some(ScreenshotFormat::Webp));
        assert_eq!(settings.screenshots.directory.as_deref(), Some("/captures"));

        let unknown = Settings::load(r#"{ "version": 2, "screenshots": { "format": "tiff" } }"#)
            .expect("unknown screenshot formats should not corrupt all settings");
        assert_eq!(unknown.screenshots.format, Some(ScreenshotFormat::Png));
    }

    #[test]
    fn load_rejects_a_future_version() {
        let raw = r#"{ "version": 99, "playback": {} }"#;
        assert!(Settings::load(raw).is_none());
    }

    // ---- Windows (PascalCase) dialect ----

    #[test]
    fn migrates_windows_document_field_by_field() {
        // A representative Windows settings.json (PascalCase, flat, SchemaVersion 1).
        let raw = r#"{
            "Theme": "Light",
            "AccentSource": "System",
            "ResumePlayback": false,
            "HideControlsWhenPaused": false,
            "DefaultSpeed": 1.25,
            "SkipStep": 10,
            "HardwareDecoding": true,
            "DefaultVolume": 75,
            "AudioNormalization": true,
            "AudioDevice": "wasapi/headphones",
            "SubtitleScale": 1.4,
            "SubtitlePosition": 95,
            "SubtitleStyle": "Cinema",
            "HistoryRetentionDays": 30,
            "AutoCheckUpdates": false,
            "SchemaVersion": 1
        }"#;

        let settings = Settings::load(raw).expect("windows document should load");

        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.playback.volume, Some(75.0));
        assert_eq!(settings.playback.resume, Some(false));
        assert_eq!(settings.playback.default_speed, Some(1.25));
        assert_eq!(settings.playback.skip_step_seconds, Some(10));
        assert_eq!(settings.playback.hide_controls_when_paused, Some(false));
        assert_eq!(settings.audio.normalization, Some(true));
        assert_eq!(settings.audio.device.as_deref(), Some("wasapi/headphones"));
        // Windows stores a hardware-decoding bool; the canonical form is the mpv string.
        assert_eq!(settings.video.hwdec.as_deref(), Some("auto-safe"));
        assert_eq!(settings.subtitles.scale, Some(1.4));
        assert_eq!(settings.subtitles.position, Some(95));
        assert_eq!(settings.subtitles.style.as_deref(), Some("Cinema"));
        assert_eq!(settings.appearance.theme.as_deref(), Some("Light"));
        assert_eq!(settings.appearance.accent_source.as_deref(), Some("System"));
        assert!(!settings.updates.auto_check);
        assert_eq!(settings.privacy.history_retention_days, Some(30));
        // Windows keeps mpv.conf in a separate file, never in settings.json.
        assert_eq!(settings.advanced.mpv_conf, None);
    }

    #[test]
    fn windows_hardware_decoding_off_maps_to_the_mpv_off_string() {
        let raw = r#"{ "HardwareDecoding": false, "SchemaVersion": 1 }"#;
        let settings = Settings::load(raw).expect("windows document should load");
        assert_eq!(settings.video.hwdec.as_deref(), Some("no"));
    }

    #[test]
    fn windows_default_audio_device_becomes_absent() {
        // "" means "device not remembered" on Windows; canonicalize to absent.
        let raw = r#"{ "AudioDevice": "", "SchemaVersion": 1 }"#;
        let settings = Settings::load(raw).expect("windows document should load");
        assert_eq!(settings.audio.device, None);
    }

    #[test]
    fn a_minimal_windows_document_fills_defaults() {
        // Only the version marker: everything else falls back to canonical absence.
        let raw = r#"{ "SchemaVersion": 1 }"#;
        let settings = Settings::load(raw).expect("windows document should load");
        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.playback.volume, None);
        assert!(settings.updates.auto_check); // default-on when the key is absent
    }
}
