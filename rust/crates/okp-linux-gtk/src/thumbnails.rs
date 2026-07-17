use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, mpsc::Sender};
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

struct HoverThumbnailRequest {
    media_path: PathBuf,
    seconds: f64,
    request_key: String,
}

#[derive(Default)]
struct HoverThumbnailWorkerState {
    pending: Option<HoverThumbnailRequest>,
    shutdown: bool,
}

#[derive(Default)]
struct HoverThumbnailWorkerShared {
    state: Mutex<HoverThumbnailWorkerState>,
    wake: Condvar,
}

/// Runs hover-frame extraction on one background thread and retains only the
/// latest request while that thread is busy. A single 4K HEVC decode can consume
/// hundreds of megabytes, so rapid timeline scrubbing must not create an
/// unbounded native-thread or request backlog.
pub struct HoverThumbnailWorker {
    shared: Arc<HoverThumbnailWorkerShared>,
}

impl HoverThumbnailWorker {
    pub fn new(sender: Sender<ThumbnailEvent>) -> Self {
        Self::with_processor(move |request| process_hover_thumbnail(request, &sender))
    }

    fn with_processor<F>(mut process: F) -> Self
    where
        F: FnMut(HoverThumbnailRequest) + Send + 'static,
    {
        let shared = Arc::new(HoverThumbnailWorkerShared::default());
        let worker_shared = Arc::clone(&shared);
        thread::spawn(move || {
            loop {
                let request = {
                    let mut state = worker_shared
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    while state.pending.is_none() && !state.shutdown {
                        state = worker_shared
                            .wake
                            .wait(state)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                    if state.shutdown {
                        return;
                    }
                    state.pending.take().expect("pending request checked above")
                };
                process(request);
            }
        });

        Self { shared }
    }

    pub fn enqueue(&self, media_path: PathBuf, seconds: f64, request_key: String) {
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: seek-thumbnail=queued");
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pending = Some(HoverThumbnailRequest {
            media_path,
            seconds,
            request_key,
        });
        self.shared.wake.notify_one();
    }
}

impl Drop for HoverThumbnailWorker {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.shutdown = true;
        state.pending = None;
        self.shared.wake.notify_one();
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

fn process_hover_thumbnail(request: HoverThumbnailRequest, sender: &Sender<ThumbnailEvent>) {
    let HoverThumbnailRequest {
        media_path,
        seconds,
        request_key,
    } = request;
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
    use std::sync::mpsc;
    use std::time::Duration;

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
    fn hover_thumbnail_worker_retains_only_the_latest_pending_request() {
        let (processed_sender, processed_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let worker = HoverThumbnailWorker::with_processor(move |request| {
            processed_sender.send(request.request_key.clone()).unwrap();
            if request.request_key == "active" {
                release_receiver.recv().unwrap();
            }
        });

        worker.enqueue(PathBuf::from("video.mkv"), 0.0, "active".to_owned());
        assert_eq!(
            processed_receiver.recv_timeout(Duration::from_secs(1)),
            Ok("active".to_owned())
        );

        for bucket in 1..=1_000 {
            worker.enqueue(
                PathBuf::from("video.mkv"),
                f64::from(bucket * 10),
                format!("queued-{bucket}"),
            );
        }
        release_sender.send(()).unwrap();

        assert_eq!(
            processed_receiver.recv_timeout(Duration::from_secs(1)),
            Ok("queued-1000".to_owned())
        );
        assert!(
            processed_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
        );
    }
}
