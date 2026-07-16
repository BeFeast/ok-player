use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc::Sender};
use std::thread;
use std::time::UNIX_EPOCH;

use okp_mpv::Chapter;

const THUMB_WIDTH: u32 = 144;
const THUMB_HEIGHT: u32 = 81;
const HOVER_BUCKET_SECONDS: f64 = 10.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThumbnailEvent {
    ChapterReady,
    HoverReady { request_key: String, path: PathBuf },
    HoverFailed { request_key: String },
}

/// Serializes expensive ffmpeg hover-frame decodes and lets queued workers
/// discard requests that the pointer has already superseded. A single 4K HEVC
/// decode can consume hundreds of megabytes, so unbounded parallel extraction is
/// not safe while the user scrubs across several timeline buckets.
#[derive(Clone, Default)]
pub struct HoverThumbnailGate {
    latest_request: Arc<Mutex<Option<String>>>,
    generation: Arc<Mutex<()>>,
}

impl HoverThumbnailGate {
    pub fn select(&self, request_key: &str) {
        *self
            .latest_request
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(request_key.to_owned());
    }

    fn is_latest(&self, request_key: &str) -> bool {
        self.latest_request
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_deref()
            == Some(request_key)
    }
}

pub fn request_key(media_path: &Path, chapters: &[Chapter]) -> String {
    let mut hasher = DefaultHasher::new();
    media_fingerprint(media_path).hash(&mut hasher);
    for chapter in chapters {
        chapter.index.hash(&mut hasher);
        chapter_time_key(chapter.time).hash(&mut hasher);
        chapter.title.hash(&mut hasher);
    }

    format!("{:016x}", hasher.finish())
}

pub fn thumbnail_path(media_path: &Path, chapter: &Chapter) -> PathBuf {
    cache_root()
        .join(media_fingerprint(media_path))
        .join(format!(
            "chapter-{:04}-{}.jpg",
            chapter.index.max(0),
            chapter_time_key(chapter.time)
        ))
}

pub fn existing_thumbnail_path(media_path: &Path, chapter: &Chapter) -> Option<PathBuf> {
    let path = thumbnail_path(media_path, chapter);
    path.exists().then_some(path)
}

pub fn hover_thumbnail_time(seconds: f64, duration: f64) -> f64 {
    if !seconds.is_finite() || seconds < 0.0 {
        return 0.0;
    }

    let duration = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        seconds
    };
    let clamped = seconds.min(duration);
    ((clamped / HOVER_BUCKET_SECONDS).round() * HOVER_BUCKET_SECONDS).min(duration)
}

pub fn hover_request_key(media_path: &Path, seconds: f64) -> String {
    format!(
        "{}:hover:{}",
        media_fingerprint(media_path),
        chapter_time_key(seconds)
    )
}

pub fn hover_thumbnail_path(media_path: &Path, seconds: f64) -> PathBuf {
    cache_root()
        .join(media_fingerprint(media_path))
        .join("hover")
        .join(format!("hover-{}.jpg", chapter_time_key(seconds)))
}

pub fn existing_hover_thumbnail_path(media_path: &Path, seconds: f64) -> Option<PathBuf> {
    let path = hover_thumbnail_path(media_path, seconds);
    path.exists().then_some(path)
}

pub fn warm_chapter_thumbnails(
    media_path: PathBuf,
    chapters: Vec<Chapter>,
    sender: Sender<ThumbnailEvent>,
) {
    thread::spawn(move || {
        let mut wrote_any = false;
        for chapter in chapters {
            let output = thumbnail_path(&media_path, &chapter);
            if output.exists() {
                continue;
            }

            if let Some(parent) = output.parent()
                && let Err(error) = fs::create_dir_all(parent)
            {
                eprintln!("Failed to create thumbnail cache: {error}");
                break;
            }

            if generate_thumbnail(&media_path, chapter.time, &output) {
                wrote_any = true;
                let _ = sender.send(ThumbnailEvent::ChapterReady);
            }
        }

        if wrote_any {
            let _ = sender.send(ThumbnailEvent::ChapterReady);
        }
    });
}

pub fn warm_hover_thumbnail(
    media_path: PathBuf,
    seconds: f64,
    request_key: String,
    gate: HoverThumbnailGate,
    sender: Sender<ThumbnailEvent>,
) {
    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
        eprintln!("interaction: seek-thumbnail=queued");
    }
    thread::spawn(move || {
        let _generation = gate
            .generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !gate.is_latest(&request_key) {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: seek-thumbnail=superseded");
            }
            return;
        }

        let output = hover_thumbnail_path(&media_path, seconds);
        if output.exists() {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: seek-thumbnail=cached");
            }
            let _ = sender.send(ThumbnailEvent::HoverReady {
                request_key,
                path: output,
            });
            return;
        }

        if let Some(parent) = output.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("Failed to create hover thumbnail cache: {error}");
            let _ = sender.send(ThumbnailEvent::HoverFailed { request_key });
            return;
        }

        if generate_thumbnail(&media_path, seconds, &output) {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: seek-thumbnail=generated");
            }
            let _ = sender.send(ThumbnailEvent::HoverReady {
                request_key,
                path: output,
            });
        } else {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: seek-thumbnail=failed");
            }
            let _ = sender.send(ThumbnailEvent::HoverFailed { request_key });
        }
    });
}

fn generate_thumbnail(media_path: &Path, seconds: f64, output: &Path) -> bool {
    if !seconds.is_finite() || seconds < 0.0 {
        return false;
    }

    let tmp = output.with_extension("tmp.jpg");
    let timestamp = format!("{:.3}", seconds.max(0.0));
    let filter = format!(
        "scale={THUMB_WIDTH}:{THUMB_HEIGHT}:force_original_aspect_ratio=increase,crop={THUMB_WIDTH}:{THUMB_HEIGHT}"
    );
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(&timestamp)
        .arg("-i")
        .arg(media_path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(filter)
        .arg("-q:v")
        .arg("4")
        .arg(&tmp)
        .status();

    match status {
        Ok(status) if status.success() => fs::rename(&tmp, output).is_ok(),
        Ok(status) => {
            eprintln!("ffmpeg thumbnail generation failed with status {status}");
            let _ = fs::remove_file(&tmp);
            false
        }
        Err(error) => {
            eprintln!("ffmpeg thumbnail generation failed: {error}");
            let _ = fs::remove_file(&tmp);
            false
        }
    }
}

fn cache_root() -> PathBuf {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(cache_home).join("ok-player/chapter-thumbnails");
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".cache/ok-player/chapter-thumbnails");
    }

    env::temp_dir().join("ok-player/chapter-thumbnails")
}

fn media_fingerprint(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);

    if let Ok(metadata) = fs::metadata(path) {
        metadata.len().hash(&mut hasher);
        if let Ok(modified) = metadata.modified()
            && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
        {
            duration.as_secs().hash(&mut hasher);
            duration.subsec_nanos().hash(&mut hasher);
        }
    }

    format!("{:016x}", hasher.finish())
}

fn chapter_time_key(seconds: f64) -> i64 {
    (seconds.max(0.0) * 1000.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_thumbnail_time_quantizes_to_ten_second_buckets() {
        assert_eq!(hover_thumbnail_time(0.0, 120.0), 0.0);
        assert_eq!(hover_thumbnail_time(4.9, 120.0), 0.0);
        assert_eq!(hover_thumbnail_time(5.0, 120.0), 10.0);
        assert_eq!(hover_thumbnail_time(53.42, 120.0), 50.0);
    }

    #[test]
    fn hover_thumbnail_time_clamps_to_duration_and_rejects_invalid_values() {
        assert_eq!(hover_thumbnail_time(f64::NAN, 120.0), 0.0);
        assert_eq!(hover_thumbnail_time(-1.0, 120.0), 0.0);
        assert_eq!(hover_thumbnail_time(118.0, 116.0), 116.0);
    }

    #[test]
    fn hover_thumbnail_gate_tracks_only_the_latest_request() {
        let gate = HoverThumbnailGate::default();
        gate.select("first");
        assert!(gate.is_latest("first"));

        gate.select("second");
        assert!(!gate.is_latest("first"));
        assert!(gate.is_latest("second"));
    }
}
