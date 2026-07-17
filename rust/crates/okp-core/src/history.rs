//! Cross-platform persistence schema for the watch history (EPIC #134, B9).
//!
//! The companion to [`crate::settings`]: the shared, versioned document both shells
//! converge on for resume positions and per-file playback state. Schema and migration
//! only — path resolution and file IO stay behind a shell seam (XDG on Linux,
//! `%APPDATA%` on Windows). The presentation helpers (day buckets, "when" labels) live
//! separately in [`crate::history_format`]; this module is the on-disk model.
//!
//! The canonical form (`{ "version": 2, "files": { "<path>": { … } } }`, snake_case) is
//! a superset of both current dialects, so migration never drops a platform's state:
//!
//! - **Linux alpha dialect** — the GTK shell's `{ "version": 1, "files": {…} }`, a
//!   structural subset that upgrades in place.
//! - **Windows dialect** — `OkPlayer.Core`'s bare `Dictionary<string, FileRecord>`
//!   (no wrapper, PascalCase fields, ISO-8601 `LastOpenedUtc`). Each record is remapped;
//!   the timestamp is parsed to Unix seconds and the `SubtitleId`/`AudioId` sentinels
//!   fold into the enabled/track-id preference pair.
//!
//! See `docs/core-compatibility.md` for the full migration story and the field map.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::video_geometry::VideoGeometry;

/// Version stamped into the canonical document. Bumped from the Linux alpha `1` to mark
/// the unified cross-platform schema; a loaded `1` document upgrades to this.
pub const HISTORY_VERSION: u32 = 2;

/// The canonical history document: a version stamp and a map from media path to record.
/// The map is a [`BTreeMap`] so paths serialize in a stable, sorted order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub version: u32,
    #[serde(default)]
    pub files: BTreeMap<String, FileEntry>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: HISTORY_VERSION,
            files: BTreeMap::new(),
        }
    }
}

impl History {
    /// Load a history document from raw JSON, migrating whichever on-disk dialect it is.
    /// Returns `None` for input that matches no known dialect so the shell can fall back
    /// to an empty history, exactly as both shells already treat a corrupt file.
    pub fn load(raw: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        let object = value.as_object()?;
        // The Linux alpha / canonical form wraps the map in `{ version, files }`; the
        // Windows form is a bare path->record dictionary with neither key.
        let is_native = object.contains_key("version")
            && object
                .get("files")
                .is_some_and(serde_json::Value::is_object);

        if is_native {
            let mut history: History = serde_json::from_value(value).ok()?;
            if history.version == 0 || history.version > HISTORY_VERSION {
                return None;
            }
            history.version = HISTORY_VERSION;
            history.normalize_preferences();
            Some(history)
        } else {
            let windows: BTreeMap<String, WindowsFileRecord> =
                serde_json::from_value(value).ok()?;
            let files = windows
                .into_iter()
                .map(|(path, record)| (path, record.into_entry()))
                .collect();
            Some(History {
                version: HISTORY_VERSION,
                files,
            })
        }
    }

    fn normalize_preferences(&mut self) {
        for entry in self.files.values_mut() {
            if let Some(geometry) = entry.preferences.video_geometry.as_mut() {
                *geometry = geometry.normalized();
            }
        }
    }
}

/// One media file's record: resume progress, a last-opened stamp (Unix seconds), and
/// the platform-specific extras carried for the shared schema.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub position: f64,
    pub duration: f64,
    pub finished: bool,
    pub updated_at_unix: i64,
    /// Windows `Title` — the display title cached for the continue-watching shelf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Windows `PosterPath` — cached poster frame for the continue-watching shelf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_path: Option<String>,
    /// Windows `Bookmarks` — user bookmark timestamps (seconds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<f64>,
    /// Windows `UserChapters` — user-authored chapter marks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<ChapterMark>,
    #[serde(default, skip_serializing_if = "Preferences::is_empty")]
    pub preferences: Preferences,
}

/// A user-authored chapter mark (Windows `ChapterMark`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChapterMark {
    pub time: f64,
    pub title: String,
}

/// Per-file playback preferences remembered across sessions (the Linux alpha set). The
/// audio/subtitle enable flags pair with their track ids: an explicit `false` records
/// "keep it off", a `true` with a track id records the chosen track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_track_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_track_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_subtitle_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_subtitle_track_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_delay: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_scale: Option<f64>,
    /// Per-file audio delay in seconds, remembered like [`Self::subtitle_delay`]
    /// so a sync correction survives across sessions. Held independently of the
    /// subtitle delay: nudging one never disturbs the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_delay: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Per-file aspect, zoom/pan, rotation, fill, and deinterlace choices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_geometry: Option<VideoGeometry>,
}

impl Preferences {
    /// True when no preference is recorded — the record then omits the whole section.
    pub fn is_empty(&self) -> bool {
        self.audio_enabled.is_none()
            && self.audio_track_id.is_none()
            && self.subtitle_enabled.is_none()
            && self.subtitle_track_id.is_none()
            && self.secondary_subtitle_enabled.is_none()
            && self.secondary_subtitle_track_id.is_none()
            && self.subtitle_delay.is_none()
            && self.subtitle_scale.is_none()
            && self.audio_delay.is_none()
            && self.speed.is_none()
            && self.video_geometry.is_none()
    }

    /// Overlay the set fields of `updated` onto `self`, leaving untouched fields intact.
    /// An enable flag and its track id move together so a track choice never desyncs
    /// from its on/off state.
    pub fn merge(&mut self, updated: Preferences) {
        if updated.audio_enabled.is_some() {
            self.audio_enabled = updated.audio_enabled;
            self.audio_track_id = updated.audio_track_id;
        }
        if updated.subtitle_enabled.is_some() {
            self.subtitle_enabled = updated.subtitle_enabled;
            self.subtitle_track_id = updated.subtitle_track_id;
        }
        if updated.secondary_subtitle_enabled.is_some() {
            self.secondary_subtitle_enabled = updated.secondary_subtitle_enabled;
            self.secondary_subtitle_track_id = updated.secondary_subtitle_track_id;
        }
        if updated.subtitle_delay.is_some() {
            self.subtitle_delay = updated.subtitle_delay;
        }
        if updated.subtitle_scale.is_some() {
            self.subtitle_scale = updated.subtitle_scale;
        }
        if updated.audio_delay.is_some() {
            self.audio_delay = updated.audio_delay;
        }
        if updated.speed.is_some() {
            self.speed = updated.speed;
        }
        if updated.video_geometry.is_some() {
            self.video_geometry = updated.video_geometry.map(VideoGeometry::normalized);
        }
    }
}

/// Deserialization shape for one Windows `FileRecord`: PascalCase, with the optional
/// fields absent (not null) in older records. Converted to a canonical [`FileEntry`] by
/// [`WindowsFileRecord::into_entry`].
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct WindowsFileRecord {
    position: f64,
    duration: f64,
    finished: bool,
    last_opened_utc: String,
    title: Option<String>,
    poster_path: Option<String>,
    subtitle_id: Option<i64>,
    audio_id: Option<i64>,
    bookmarks: Vec<f64>,
    user_chapters: Vec<WindowsChapter>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct WindowsChapter {
    time: f64,
    title: String,
}

impl WindowsFileRecord {
    fn into_entry(self) -> FileEntry {
        let (audio_enabled, audio_track_id) = track_selection(self.audio_id);
        let (subtitle_enabled, subtitle_track_id) = track_selection(self.subtitle_id);
        FileEntry {
            position: self.position,
            duration: self.duration,
            finished: self.finished,
            // An unparseable stamp falls back to the epoch; real Windows files always
            // carry `DateTime.UtcNow.ToString("o")`, which parses.
            updated_at_unix: parse_iso8601_utc_seconds(&self.last_opened_utc).unwrap_or(0),
            title: self.title,
            poster_path: self.poster_path,
            bookmarks: self.bookmarks,
            chapters: self
                .user_chapters
                .into_iter()
                .map(|chapter| ChapterMark {
                    time: chapter.time,
                    title: chapter.title,
                })
                .collect(),
            preferences: Preferences {
                audio_enabled,
                audio_track_id,
                subtitle_enabled,
                subtitle_track_id,
                ..Preferences::default()
            },
        }
    }
}

/// Fold a Windows track-id sentinel into the canonical enable/track-id pair: `None`
/// (unrecorded) stays unset, a negative id (`-1` = explicitly off) records "disabled",
/// and any other id records "enabled" on that track.
fn track_selection(id: Option<i64>) -> (Option<bool>, Option<i64>) {
    match id {
        None => (None, None),
        Some(id) if id < 0 => (Some(false), None),
        Some(id) => (Some(true), Some(id)),
    }
}

/// Parse the whole-second UTC value of an ISO-8601 round-trip stamp
/// (`YYYY-MM-DDTHH:MM:SS[.fffffff]Z`, what `DateTime.UtcNow.ToString("o")` emits) into
/// Unix seconds. Fractional seconds and any zone suffix are ignored; only the `Z` UTC
/// form Windows writes is interpreted.
fn parse_iso8601_utc_seconds(text: &str) -> Option<i64> {
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i32 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    // Trim fractional seconds and the trailing zone designator from the seconds field.
    let seconds: i64 = time_parts
        .next()?
        .split(['.', 'Z', '+', '-'])
        .next()?
        .parse()
        .ok()?;

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + seconds)
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_an_empty_current_version_document() {
        let history = History::default();
        assert_eq!(history.version, HISTORY_VERSION);
        assert!(history.files.is_empty());
    }

    #[test]
    fn load_rejects_unrecognized_input() {
        assert!(History::load("not json").is_none());
        assert!(History::load("[]").is_none());
        assert!(History::load("42").is_none());
    }

    #[test]
    fn load_rejects_a_future_version() {
        assert!(History::load(r#"{ "version": 99, "files": {} }"#).is_none());
    }

    // ---- Linux alpha (snake_case) dialect ----

    #[test]
    fn migrates_linux_alpha_document_in_place() {
        let raw = r#"{
            "version": 1,
            "files": {
                "/media/movie.mkv": {
                    "position": 120.0,
                    "duration": 600.0,
                    "finished": false,
                    "updated_at_unix": 1700000000,
                    "preferences": {
                        "subtitle_enabled": true,
                        "subtitle_track_id": 3,
                        "subtitle_delay": 0.25,
                        "speed": 1.25
                    }
                }
            }
        }"#;

        let history = History::load(raw).expect("linux alpha document should load");

        assert_eq!(history.version, HISTORY_VERSION);
        let entry = history.files.get("/media/movie.mkv").expect("entry");
        assert_eq!(entry.position, 120.0);
        assert_eq!(entry.duration, 600.0);
        assert!(!entry.finished);
        assert_eq!(entry.updated_at_unix, 1_700_000_000);
        assert_eq!(entry.preferences.subtitle_enabled, Some(true));
        assert_eq!(entry.preferences.subtitle_track_id, Some(3));
        assert_eq!(entry.preferences.subtitle_delay, Some(0.25));
        assert_eq!(entry.preferences.speed, Some(1.25));
        // Windows-only extras default to absent.
        assert_eq!(entry.title, None);
        assert!(entry.bookmarks.is_empty());
        assert!(entry.chapters.is_empty());
    }

    #[test]
    fn a_canonical_document_round_trips() {
        let mut history = History::default();
        history.files.insert(
            "/media/clip.mkv".to_owned(),
            FileEntry {
                position: 42.0,
                duration: 600.0,
                finished: false,
                updated_at_unix: 1_700_000_000,
                bookmarks: vec![7.0, 100.0],
                chapters: vec![ChapterMark {
                    time: 12.0,
                    title: "Intro".to_owned(),
                }],
                ..FileEntry::default()
            },
        );

        let json = serde_json::to_string(&history).expect("serialize");
        assert_eq!(History::load(&json).expect("reload"), history);
    }

    #[test]
    fn a_linux_shaped_entry_omits_empty_windows_extras() {
        let mut history = History::default();
        history.files.insert(
            "/media/clip.mkv".to_owned(),
            FileEntry {
                position: 42.0,
                duration: 600.0,
                finished: false,
                updated_at_unix: 1_700_000_000,
                ..FileEntry::default()
            },
        );
        let json = serde_json::to_string(&history).expect("serialize");
        assert!(!json.contains("title"));
        assert!(!json.contains("poster_path"));
        assert!(!json.contains("bookmarks"));
        assert!(!json.contains("chapters"));
        assert!(!json.contains("preferences"));
    }

    // ---- Windows (PascalCase, bare dictionary) dialect ----

    #[test]
    fn migrates_windows_bare_dictionary() {
        // A representative Windows history.json: no wrapper, PascalCase, ISO-8601 stamp.
        let raw = r#"{
            "C:\\media\\movie.mkv": {
                "Position": 42.0,
                "Duration": 600.0,
                "Finished": false,
                "LastOpenedUtc": "2021-11-14T22:13:20.0000000Z",
                "Title": "movie",
                "PosterPath": "C:\\posters\\abcd.png",
                "SubtitleId": 3,
                "AudioId": 2,
                "Bookmarks": [7.0, 100.0],
                "UserChapters": [ { "Time": 12.0, "Title": "Intro" } ]
            }
        }"#;

        let history = History::load(raw).expect("windows document should load");

        assert_eq!(history.version, HISTORY_VERSION);
        let entry = history.files.get(r"C:\media\movie.mkv").expect("entry");
        assert_eq!(entry.position, 42.0);
        assert_eq!(entry.duration, 600.0);
        assert!(!entry.finished);
        // 2021-11-14T22:13:20Z == 1_636_928_000 Unix seconds.
        assert_eq!(entry.updated_at_unix, 1_636_928_000);
        assert_eq!(entry.title.as_deref(), Some("movie"));
        assert_eq!(entry.poster_path.as_deref(), Some(r"C:\posters\abcd.png"));
        assert_eq!(entry.bookmarks, vec![7.0, 100.0]);
        assert_eq!(entry.chapters.len(), 1);
        assert_eq!(entry.chapters[0].time, 12.0);
        assert_eq!(entry.chapters[0].title, "Intro");
        // Track ids fold into the enabled/track-id preference pair.
        assert_eq!(entry.preferences.subtitle_enabled, Some(true));
        assert_eq!(entry.preferences.subtitle_track_id, Some(3));
        assert_eq!(entry.preferences.audio_enabled, Some(true));
        assert_eq!(entry.preferences.audio_track_id, Some(2));
    }

    #[test]
    fn windows_off_and_absent_track_ids_map_to_the_right_preference() {
        let raw = r#"{
            "C:\\a.mkv": {
                "Position": 1.0, "Duration": 2.0, "Finished": false,
                "LastOpenedUtc": "2021-11-14T22:13:20.0000000Z",
                "SubtitleId": -1
            }
        }"#;
        let history = History::load(raw).expect("load");
        let entry = history.files.get(r"C:\a.mkv").expect("entry");
        // -1 = explicitly off: disabled, no track.
        assert_eq!(entry.preferences.subtitle_enabled, Some(false));
        assert_eq!(entry.preferences.subtitle_track_id, None);
        // Absent AudioId stays unrecorded.
        assert_eq!(entry.preferences.audio_enabled, None);
        assert_eq!(entry.preferences.audio_track_id, None);
    }

    #[test]
    fn migrates_a_minimal_windows_record_without_track_fields() {
        // Mirrors the C# HistoryServiceTests back-compat fixture.
        let raw = r#"{
            "C:\\media\\movie.mkv": {
                "Position": 42.0,
                "Duration": 600.0,
                "Finished": false,
                "LastOpenedUtc": "2021-11-14T22:13:20.0000000Z"
            }
        }"#;
        let history = History::load(raw).expect("load");
        let entry = history.files.get(r"C:\media\movie.mkv").expect("entry");
        assert!(entry.preferences.is_empty());
        assert!(entry.title.is_none());
    }

    #[test]
    fn empty_object_is_an_empty_windows_history() {
        let history = History::load("{}").expect("empty dictionary loads");
        assert_eq!(history.version, HISTORY_VERSION);
        assert!(history.files.is_empty());
    }

    // ---- helpers ----

    #[test]
    fn iso8601_epoch_and_a_known_stamp_parse() {
        assert_eq!(
            parse_iso8601_utc_seconds("1970-01-01T00:00:00.0000000Z"),
            Some(0)
        );
        assert_eq!(
            parse_iso8601_utc_seconds("2021-11-14T22:13:20.0000000Z"),
            Some(1_636_928_000)
        );
        // Fractional seconds are dropped; the whole second is kept.
        assert_eq!(
            parse_iso8601_utc_seconds("2021-11-14T22:13:20.9999999Z"),
            Some(1_636_928_000)
        );
        assert_eq!(parse_iso8601_utc_seconds("not a date"), None);
    }

    #[test]
    fn preferences_merge_moves_a_track_id_with_its_flag() {
        let mut base = Preferences {
            audio_enabled: Some(true),
            audio_track_id: Some(1),
            speed: Some(0.75),
            ..Preferences::default()
        };
        base.merge(Preferences {
            subtitle_enabled: Some(false),
            subtitle_scale: Some(1.2),
            ..Preferences::default()
        });

        assert_eq!(base.audio_enabled, Some(true));
        assert_eq!(base.audio_track_id, Some(1));
        assert_eq!(base.subtitle_enabled, Some(false));
        assert_eq!(base.subtitle_scale, Some(1.2));
        assert_eq!(base.speed, Some(0.75));
    }

    #[test]
    fn video_geometry_round_trips_in_the_shared_preferences_schema() {
        let raw = r#"{
            "version": 2,
            "files": {
                "/media/movie.mkv": {
                    "position": 120.0,
                    "duration": 600.0,
                    "finished": false,
                    "updated_at_unix": 1700000000,
                    "preferences": {
                        "video_geometry": {
                            "aspect": "2.35:1",
                            "zoom": 1.5,
                            "pan_x": -0.2,
                            "pan_y": 0.1,
                            "rotation_degrees": 90,
                            "fill_screen": true,
                            "deinterlace": true
                        }
                    }
                }
            }
        }"#;

        let history = History::load(raw).expect("geometry document should load");
        let geometry = history.files["/media/movie.mkv"]
            .preferences
            .video_geometry
            .expect("geometry");
        assert_eq!(geometry.aspect, crate::video_geometry::VideoAspect::Cinema);
        assert_eq!(geometry.zoom, 1.5);
        assert_eq!(geometry.pan_x, -0.2);
        assert_eq!(geometry.pan_y, 0.1);
        assert_eq!(geometry.rotation_degrees, 90);
        assert!(geometry.fill_screen);
        assert!(geometry.deinterlace);

        let serialized = serde_json::to_string(&history).expect("serialize");
        assert!(serialized.contains("\"video_geometry\""));
        assert_eq!(History::load(&serialized), Some(history));
    }

    #[test]
    fn video_geometry_merge_replaces_only_geometry_and_normalizes_it() {
        let mut preferences = Preferences {
            subtitle_delay: Some(0.25),
            speed: Some(0.75),
            video_geometry: Some(VideoGeometry {
                zoom: 1.5,
                ..VideoGeometry::default()
            }),
            ..Preferences::default()
        };
        preferences.merge(Preferences {
            video_geometry: Some(VideoGeometry {
                zoom: 99.0,
                pan_x: 0.4,
                rotation_degrees: -90,
                ..VideoGeometry::default()
            }),
            ..Preferences::default()
        });

        assert_eq!(preferences.subtitle_delay, Some(0.25));
        assert_eq!(preferences.speed, Some(0.75));
        let geometry = preferences.video_geometry.expect("geometry");
        assert_eq!(geometry.zoom, crate::video_geometry::ZOOM_MAX);
        assert_eq!(geometry.pan_x, 0.4);
        assert_eq!(geometry.rotation_degrees, 270);
    }

    #[test]
    fn loaded_geometry_is_normalized_before_the_shell_can_apply_it() {
        let raw = r#"{
            "version": 2,
            "files": {
                "/media/movie.mkv": {
                    "position": 0.0,
                    "duration": 0.0,
                    "finished": false,
                    "updated_at_unix": 0,
                    "preferences": {
                        "video_geometry": {
                            "zoom": 0.25,
                            "pan_x": 8.0,
                            "pan_y": -8.0,
                            "rotation_degrees": 91
                        }
                    }
                }
            }
        }"#;

        let history = History::load(raw).expect("geometry document should load");
        let geometry = history.files["/media/movie.mkv"]
            .preferences
            .video_geometry
            .expect("geometry");
        assert_eq!(geometry.zoom, 1.0);
        assert_eq!(geometry.pan_x, 0.0);
        assert_eq!(geometry.pan_y, 0.0);
        assert_eq!(geometry.rotation_degrees, 90);
    }

    #[test]
    fn audio_delay_is_remembered_independently_of_subtitle_delay() {
        // A lone audio_delay counts as content, so the section is not dropped.
        let mut base = Preferences {
            audio_delay: Some(-0.12),
            ..Preferences::default()
        };
        assert!(!base.is_empty());

        // Merging a subtitle-delay change must leave the audio delay untouched,
        // and vice versa — the two sync corrections never bleed into each other.
        base.merge(Preferences {
            subtitle_delay: Some(0.25),
            ..Preferences::default()
        });
        assert_eq!(base.audio_delay, Some(-0.12));
        assert_eq!(base.subtitle_delay, Some(0.25));

        base.merge(Preferences {
            audio_delay: Some(0.4),
            ..Preferences::default()
        });
        assert_eq!(base.audio_delay, Some(0.4));
        assert_eq!(base.subtitle_delay, Some(0.25));
    }

    #[test]
    fn audio_delay_round_trips_through_json() {
        let raw = r#"{
            "version": 1,
            "files": {
                "/media/song.mka": {
                    "position": 0.0,
                    "duration": 200.0,
                    "finished": false,
                    "updated_at_unix": 1700000000,
                    "preferences": { "audio_delay": 0.05 }
                }
            }
        }"#;

        let history = History::load(raw).expect("document with audio_delay should load");
        let entry = history.files.get("/media/song.mka").expect("entry");
        assert_eq!(entry.preferences.audio_delay, Some(0.05));

        // The field survives a save/load round-trip and stays snake_case.
        let serialized = serde_json::to_string(&history).expect("history serializes");
        assert!(serialized.contains("\"audio_delay\":0.05"));
    }
}
