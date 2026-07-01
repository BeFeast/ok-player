use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::UNIX_EPOCH;

use okp_mpv::Chapter;

const THUMB_WIDTH: u32 = 144;
const THUMB_HEIGHT: u32 = 81;

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

pub fn warm_chapter_thumbnails(
    media_path: PathBuf,
    chapters: Vec<Chapter>,
    request_key: String,
    sender: Sender<String>,
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
                let _ = sender.send(request_key.clone());
            }
        }

        if wrote_any {
            let _ = sender.send(request_key);
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
