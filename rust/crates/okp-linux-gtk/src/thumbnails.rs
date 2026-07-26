use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc::Sender};
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use okp_core::image_luma;
use okp_core::poster_frame::{
    PosterFrameScorer, PosterSource, PosterVerdict, classify_source, poster_cache_key,
    poster_sample_offsets,
};
use okp_core::recents_shelf::HistoryItem;
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
    cache_base().join("chapter-thumbnails")
}

/// The `ok-player` root under the XDG cache home (with `$HOME/.cache` and the system temp dir
/// as fallbacks). Every thumbnail family — chapter, hover, and Continue Watching posters —
/// hangs off this so they share one prunable location.
fn cache_base() -> PathBuf {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(cache_home).join("ok-player");
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".cache/ok-player");
    }

    env::temp_dir().join("ok-player")
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

// ---- Continue Watching / History posters -------------------------------------------------
//
// The idle welcome shelf and the full History list render a representative frame for every
// resumable local video. Generation is bounded and asynchronous: a single background worker
// decodes candidate frames with ffmpeg (the same tool the hover/chapter thumbnails already
// use), scores them with the shared [`okp_core::poster_frame`] policy, and writes one small
// JPEG per file into the XDG cache. The GTK side is a thin projection: on each idle poll it
// resolves each row's poster from the cache and, for any still missing, enqueues one bounded
// generation. The pure identity/scoring/sampling rules live in the core so both surfaces (and
// a future non-GTK shell) share one cached result and one selection policy.

/// The scoring decode is deliberately tiny — mean luma does not need a full-resolution frame,
/// and a small scale keeps each probe cheap so sampling several positions stays bounded.
const POSTER_SCORE_WIDTH: u32 = 128;
const POSTER_SCORE_HEIGHT: u32 = 72;

/// A queued poster generation: the media to sample, its duration hint, and the cache targets
/// derived from the file's identity key.
#[derive(Clone, Debug)]
struct PosterJob {
    media_path: PathBuf,
    duration: f64,
    /// Where a usable frame is written.
    poster: PathBuf,
    /// The durable "no usable frame" sentinel written when even the brightest sample is black.
    sentinel: PathBuf,
}

#[derive(Default)]
struct PosterController {
    generation: AtomicU64,
    shutdown: AtomicBool,
    active_child: Mutex<Option<u32>>,
}

/// A cheap, cloneable lifecycle token handed to the worker's processor so a long sampling run
/// can bail promptly when playback supersedes the idle surface or the shelf shuts down.
#[derive(Clone)]
struct PosterCancel {
    controller: Arc<PosterController>,
    generation: u64,
}

impl PosterCancel {
    fn is_cancelled(&self) -> bool {
        self.controller.shutdown.load(Ordering::Acquire)
            || self.controller.generation.load(Ordering::Acquire) != self.generation
    }

    fn register_child(&self, pid: u32) {
        *self
            .controller
            .active_child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(pid);
    }

    fn try_wait_child(&self, child: &mut Child) -> io::Result<Option<ExitStatus>> {
        let pid = child.id();
        let mut active = self
            .controller
            .active_child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match child.try_wait() {
            Ok(Some(status)) => {
                if *active == Some(pid) {
                    *active = None;
                }
                Ok(Some(status))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                if *active == Some(pid) {
                    *active = None;
                }
                Err(error)
            }
        }
    }
}

impl PosterController {
    fn token(self: &Arc<Self>) -> PosterCancel {
        PosterCancel {
            controller: Arc::clone(self),
            generation: self.generation.load(Ordering::Acquire),
        }
    }

    fn cancel_active(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let active = self
            .active_child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pid) = *active {
            // The worker still owns and reaps this one registered child; the signal only
            // interrupts its blocking decoder promptly. Holding the registration lock through
            // kill prevents a reaped PID from being reused between lookup and signal delivery.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}

#[derive(Default)]
struct PosterQueue {
    pending: VecDeque<PosterJob>,
    /// Keys queued or in flight — dedupes concurrent requests for the same file.
    active: HashSet<String>,
    /// Keys resolved this session (any outcome). Prevents a transient failure — which leaves
    /// neither a poster nor a sentinel on disk — from re-enqueuing on every 200 ms poll (a hot
    /// loop). It is in-memory only, so a genuinely transient failure still retries next launch,
    /// while a durable black-film verdict is remembered by the on-disk sentinel across launches.
    done: HashSet<String>,
    in_flight: Option<String>,
    suspended: bool,
    shutdown: bool,
}

struct PosterShared {
    state: Mutex<PosterQueue>,
    wake: Condvar,
}

/// Bounded, asynchronous poster generation for the idle surfaces. Owns a single worker thread
/// (decoder concurrency of one, so a burst of resumable videos never spawns a pile of native
/// decodes) and the cache directory the projection reads from.
pub(crate) struct PosterShelf {
    dir: PathBuf,
    shared: Arc<PosterShared>,
    controller: Arc<PosterController>,
}

impl PosterShelf {
    /// The production shelf: decode with ffmpeg and score with the shared luma policy.
    fn production(dir: PathBuf) -> Self {
        Self::with_processor(dir, process_poster)
    }

    /// Construct a shelf with an injected processor so the queue/dedup/cancellation behaviour
    /// can be unit-tested without a decoder.
    fn with_processor<F>(dir: PathBuf, mut process: F) -> Self
    where
        F: FnMut(&PosterJob, &PosterCancel) + Send + 'static,
    {
        let shared = Arc::new(PosterShared {
            state: Mutex::new(PosterQueue::default()),
            wake: Condvar::new(),
        });
        let controller = Arc::new(PosterController::default());
        let worker_shared = Arc::clone(&shared);
        let worker_controller = Arc::clone(&controller);
        thread::spawn(move || {
            loop {
                let (job, worker_cancel) = {
                    let mut state = worker_shared
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    while state.pending.is_empty() && !state.shutdown {
                        state = worker_shared
                            .wake
                            .wait(state)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                    if state.shutdown {
                        return;
                    }
                    let job = state
                        .pending
                        .pop_front()
                        .expect("pending job checked above");
                    state.in_flight = Some(poster_key_of(&job));
                    let cancel = worker_controller.token();
                    (job, cancel)
                };

                process(&job, &worker_cancel);
                let cancelled = worker_cancel.is_cancelled();

                let mut state = worker_shared
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.active.remove(&poster_key_of(&job));
                state.in_flight = None;
                if !cancelled {
                    state.done.insert(poster_key_of(&job));
                }
            }
        });

        Self {
            dir,
            shared,
            controller,
        }
    }

    /// Retire all Welcome/History work while another player surface owns the window. Returning
    /// to idle resumes the queue, and unfinished rows are eligible for projection again.
    fn suspend(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.suspended || state.shutdown {
            return;
        }
        state.suspended = true;
        state.pending.clear();
        let in_flight = state.in_flight.clone();
        state
            .active
            .retain(|key| in_flight.as_ref().is_some_and(|active| active == key));
        // Invalidate the worker token before releasing the queue lock so a fast idle resume
        // cannot admit a new job into the generation being retired.
        self.controller.cancel_active();
    }

    fn resume(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.shutdown {
            state.suspended = false;
        }
    }

    /// Fill each row's `poster_path` from the cache and, unless the session is private, kick
    /// off bounded generation for any still missing. Private sessions keep existing history
    /// fully readable but never generate a new poster trace.
    fn project(&self, items: &mut [HistoryItem], private_session: bool) {
        for item in items.iter_mut() {
            item.poster_path = self.resolve(item, !private_session);
        }
    }

    /// Resolve one row to a poster path if the cache already holds one, enqueuing bounded
    /// generation when it does not. Returns `None` (an honest placeholder) for anything that
    /// is not a present local video, and for a file whose durable sentinel says it has no
    /// usable frame — without ever re-deriving it.
    fn resolve(&self, item: &HistoryItem, allow_generation: bool) -> Option<String> {
        // Deterministic render hook for the visual smokes: a poster placed by file stem in
        // OKP_POSTER_FIXTURE_DIR is used verbatim, so the render/projection path can be proven
        // without invoking a decoder. Never enqueues generation.
        if let Some(fixture) = poster_fixture_dir()
            && let Some(stem) = Path::new(&item.path)
                .file_stem()
                .and_then(|stem| stem.to_str())
        {
            let candidate = fixture.join(format!("{stem}.jpg"));
            if is_nonempty_file(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }

        if classify_source(&item.path) != PosterSource::LocalVideo {
            return None; // audio-only / URL / network: honest non-video fallback, no decode
        }
        let metadata = fs::metadata(&item.path).ok()?;
        if !metadata.is_file() {
            return None; // missing/deleted since it was listed — placeholder, no retry loop
        }
        let (modified_secs, modified_nanos) = modified_parts(&metadata);
        let key = poster_cache_key(&item.path, metadata.len(), modified_secs, modified_nanos);
        let poster = self.dir.join(format!("{key}.jpg"));
        if is_nonempty_file(&poster) {
            return Some(poster.to_string_lossy().into_owned());
        }
        // A zero-byte leftover (an interrupted write that somehow reached the final name) is
        // treated as absent and cleared so a healthy frame can replace it.
        if poster.exists() {
            let _ = fs::remove_file(&poster);
        }
        let sentinel = self.dir.join(format!("{key}.none"));
        if sentinel.is_file() {
            return None; // durably no usable frame — keep the placeholder, never re-derive
        }

        if allow_generation {
            self.enqueue(PosterJob {
                media_path: PathBuf::from(&item.path),
                duration: item.duration,
                poster,
                sentinel,
            });
        }
        None
    }

    fn enqueue(&self, job: PosterJob) {
        let key = poster_key_of(&job);
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.shutdown
            || state.suspended
            || state.active.contains(&key)
            || state.done.contains(&key)
        {
            return;
        }
        state.active.insert(key);
        state.pending.push_back(job);
        self.shared.wake.notify_one();
    }
}

impl Drop for PosterShelf {
    fn drop(&mut self) {
        self.controller.shutdown.store(true, Ordering::Release);
        self.controller.cancel_active();
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.shutdown = true;
        state.pending.clear();
        self.shared.wake.notify_all();
    }
}

/// The process-wide shelf. Lazily started on first idle projection, so the worker thread and
/// its cache directory come from the environment in effect at first use (the smokes set
/// `XDG_CACHE_HOME` before launch).
static POSTER_SHELF: OnceLock<PosterShelf> = OnceLock::new();

fn poster_shelf() -> &'static PosterShelf {
    POSTER_SHELF.get_or_init(|| PosterShelf::production(poster_cache_dir()))
}

/// Fill the rows' posters from the cache and enqueue any missing generations. The single entry
/// point both idle surfaces call so they share one cached result and one crop/selection policy.
pub(crate) fn project_posters(items: &mut [HistoryItem], private_session: bool) {
    let shelf = poster_shelf();
    shelf.resume();
    shelf.project(items, private_session);
}

/// Stop idle-only poster work as soon as loading/playback/error content replaces Welcome.
pub(crate) fn suspend_poster_generation() {
    if let Some(shelf) = POSTER_SHELF.get() {
        shelf.suspend();
    }
}

fn poster_cache_dir() -> PathBuf {
    cache_base().join("continue-watching-posters")
}

fn poster_fixture_dir() -> Option<PathBuf> {
    env::var_os("OKP_POSTER_FIXTURE_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn poster_key_of(job: &PosterJob) -> String {
    job.poster
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_default()
}

fn is_nonempty_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn modified_parts(metadata: &fs::Metadata) -> (u64, u32) {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
        .unwrap_or((0, 0))
}

/// Sample the media at the bounded plan, keep the brightest lit frame, and write the poster —
/// or the durable "no usable frame" sentinel when the whole file reads as black. Nothing is
/// written on a purely transient decode failure, so a momentarily-unreadable file is retried
/// on the next launch instead of being marked posterless forever.
fn process_poster(job: &PosterJob, cancel: &PosterCancel) {
    if let Some(parent) = job.poster.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!("Failed to create poster cache: {error}");
        return;
    }

    let mut scorer = PosterFrameScorer::new();
    for offset in poster_sample_offsets(job.duration) {
        if cancel.is_cancelled() {
            return; // shutting down — do not cache a verdict from a half-finished sampling run
        }
        if let Some(luma) = decode_frame_luma(&job.media_path, offset, cancel) {
            scorer.observe(offset, luma);
            if scorer.is_satisfied() {
                break;
            }
        }
    }

    match scorer.verdict() {
        PosterVerdict::Usable { offset, .. } => {
            let _ = generate_poster_thumbnail(&job.media_path, offset, &job.poster, cancel);
        }
        PosterVerdict::Unusable => write_poster_sentinel(&job.sentinel),
        PosterVerdict::NoFrame => { /* transient — leave the cache clean and retry next launch */
        }
    }
}

/// Mean luma (0–255) of a single frame at `seconds`, decoded to a small raw BGRA buffer by
/// ffmpeg and scored by the shared [`image_luma`] policy. Input-side `-ss` gives a fast
/// keyframe seek, which is plenty accurate for a poster. Returns `None` when nothing decodes.
fn decode_frame_luma(media_path: &Path, seconds: f64, cancel: &PosterCancel) -> Option<f64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let timestamp = format!("{:.3}", seconds.max(0.0));
    let filter = format!("scale={POSTER_SCORE_WIDTH}:{POSTER_SCORE_HEIGHT}");
    let mut command = Command::new("ffmpeg");
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(&timestamp)
        .arg("-i")
        .arg(media_path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(filter)
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("bgra")
        .arg("-");
    let output = run_poster_command_with_output(&mut command, cancel)?;
    if output.is_empty() {
        return None;
    }
    Some(image_luma::mean_bgra(&output, image_luma::DEFAULT_STRIDE))
}

fn generate_poster_thumbnail(
    media_path: &Path,
    seconds: f64,
    output: &Path,
    cancel: &PosterCancel,
) -> bool {
    if !seconds.is_finite() || seconds < 0.0 || cancel.is_cancelled() {
        return false;
    }

    let tmp = output.with_extension("tmp.jpg");
    let timestamp = format!("{:.3}", seconds.max(0.0));
    let filter = format!(
        "scale={THUMB_WIDTH}:{THUMB_HEIGHT}:force_original_aspect_ratio=increase,crop={THUMB_WIDTH}:{THUMB_HEIGHT}"
    );
    let mut command = Command::new("ffmpeg");
    command
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
        .arg(&tmp);

    match run_poster_command(&mut command, cancel) {
        Some(status) if status.success() && !cancel.is_cancelled() => {
            fs::rename(&tmp, output).is_ok()
        }
        Some(status) => {
            if !cancel.is_cancelled() {
                eprintln!("ffmpeg poster generation failed with status {status}");
            }
            let _ = fs::remove_file(&tmp);
            false
        }
        None => {
            let _ = fs::remove_file(&tmp);
            false
        }
    }
}

fn run_poster_command_with_output(command: &mut Command, cancel: &PosterCancel) -> Option<Vec<u8>> {
    let mut child = spawn_poster_command(command, Stdio::piped(), cancel).ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).map(|_| output)
    });
    let status = wait_for_poster_command(&mut child, cancel);
    let output = reader.join().ok()?.ok()?;
    status.filter(ExitStatus::success).map(|_| output)
}

fn run_poster_command(command: &mut Command, cancel: &PosterCancel) -> Option<ExitStatus> {
    let mut child = spawn_poster_command(command, Stdio::null(), cancel).ok()?;
    wait_for_poster_command(&mut child, cancel)
}

fn spawn_poster_command(
    command: &mut Command,
    stdout: Stdio,
    cancel: &PosterCancel,
) -> io::Result<Child> {
    command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null());
    // The decoder must never outlive the GTK process or inherit its log/IPC descriptors. The
    // post-prctl parent check closes the small fork/exec race where the parent exits first.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() == 1 {
                libc::raise(libc::SIGKILL);
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    cancel.register_child(pid);
    if cancel.is_cancelled() {
        let _ = child.kill();
    }
    Ok(child)
}

fn wait_for_poster_command(child: &mut Child, cancel: &PosterCancel) -> Option<ExitStatus> {
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
        }
        match cancel.try_wait_child(child) {
            Ok(Some(status)) => {
                return (!cancel.is_cancelled()).then_some(status);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                if !cancel.is_cancelled() {
                    eprintln!("Failed to wait for poster decoder: {error}");
                }
                return None;
            }
        }
    }
}

fn write_poster_sentinel(sentinel: &Path) {
    if let Some(parent) = sentinel.parent()
        && fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let tmp = sentinel.with_extension("none.tmp");
    if fs::write(&tmp, []).is_ok() && fs::rename(&tmp, sentinel).is_err() {
        let _ = fs::remove_file(&tmp);
    }
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

    // ---- Continue Watching / History posters ----

    use std::sync::atomic::AtomicUsize;

    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!("okp-poster-test-{}-{tag}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn touch(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, bytes).expect("write file");
    }

    fn video_item(path: &str, duration: f64) -> HistoryItem {
        HistoryItem {
            path: path.to_owned(),
            title: "Title".to_owned(),
            location: "loc".to_owned(),
            position: 10.0,
            duration,
            progress: 0.2,
            state_kind: okp_core::history_format::HistoryStateKind::Progress,
            state_label: "20%".to_owned(),
            updated_at_unix: 0,
            poster_path: None,
        }
    }

    /// The cache key the shelf will derive for a media file that exists on disk.
    fn key_for(media: &Path) -> String {
        let metadata = fs::metadata(media).expect("media metadata");
        let (secs, nanos) = modified_parts(&metadata);
        poster_cache_key(&media.to_string_lossy(), metadata.len(), secs, nanos)
    }

    /// A shelf whose injected processor records each processed media path, so queue/dedup
    /// behaviour is observable without a decoder.
    fn recording_shelf(dir: PathBuf) -> (PosterShelf, mpsc::Receiver<PathBuf>) {
        let (sender, receiver) = mpsc::channel();
        let shelf = PosterShelf::with_processor(dir, move |job, _cancel| {
            sender.send(job.media_path.clone()).unwrap();
        });
        (shelf, receiver)
    }

    #[test]
    fn resolve_serves_a_cached_poster_without_enqueuing() {
        let dir = unique_dir("cached");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");
        let poster = dir.join(format!("{}.jpg", key_for(&media)));
        touch(&poster, b"jpeg");

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);

        assert_eq!(
            items[0].poster_path.as_deref(),
            Some(poster.to_str().unwrap())
        );
        assert!(
            processed.recv_timeout(Duration::from_millis(100)).is_err(),
            "a cache hit must not enqueue generation"
        );
    }

    #[test]
    fn resolve_enqueues_generation_for_a_missing_poster() {
        let dir = unique_dir("missing-poster");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);

        assert_eq!(items[0].poster_path, None, "placeholder while pending");
        assert_eq!(
            processed.recv_timeout(Duration::from_secs(1)),
            Ok(media.clone())
        );
    }

    #[test]
    fn concurrent_requests_for_the_same_file_are_deduplicated() {
        let dir = unique_dir("dedup");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel();
        let shelf = PosterShelf::with_processor(dir.clone(), move |job, _cancel| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap(); // hold the key "active" across the second request
            done_tx.send(job.media_path.clone()).unwrap();
        });

        let mut first = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut first, false);
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        // A second poll for the same still-missing file must not enqueue a duplicate.
        let mut second = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut second, false);
        release_tx.send(()).unwrap();

        assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)), Ok(media));
        assert!(
            done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "only one generation should have run"
        );
    }

    #[test]
    fn a_resolved_key_is_not_re_enqueued_on_the_next_poll() {
        // A transient failure leaves neither a poster nor a sentinel on disk; the in-memory
        // "done" set must still stop the 200 ms poll from re-enqueuing it in a hot loop.
        let dir = unique_dir("no-hot-loop");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        assert_eq!(
            processed.recv_timeout(Duration::from_secs(1)),
            Ok(media.clone())
        );

        // Second poll: the key is done, so nothing is enqueued.
        let mut again = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut again, false);
        assert!(processed.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn a_private_session_exposes_cached_posters_without_generating_new_ones() {
        let dir = unique_dir("private");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");
        let poster = dir.join(format!("{}.jpg", key_for(&media)));
        touch(&poster, b"jpeg");

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, true);

        assert_eq!(items[0].poster_path.as_deref(), poster.to_str());
        assert!(processed.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn a_private_session_does_not_generate_a_missing_poster() {
        let dir = unique_dir("private-missing");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");

        let (shelf, processed) = recording_shelf(dir);
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, true);

        assert_eq!(items[0].poster_path, None);
        assert!(processed.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn missing_url_and_audio_rows_keep_an_honest_fallback_without_enqueuing() {
        let dir = unique_dir("fallbacks");
        let audio = dir.join("song.flac");
        touch(&audio, b"fake audio bytes");

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![
            video_item(&dir.join("gone.mkv").to_string_lossy(), 600.0), // never existed
            video_item("https://example.com/live.mkv", 600.0),          // URL
            video_item(&audio.to_string_lossy(), 300.0),                // audio-only
        ];
        shelf.project(&mut items, false);

        assert!(items.iter().all(|item| item.poster_path.is_none()));
        assert!(
            processed.recv_timeout(Duration::from_millis(100)).is_err(),
            "no decode should be attempted for missing/url/audio rows"
        );
    }

    #[test]
    fn a_durable_sentinel_keeps_the_placeholder_without_re_deriving() {
        let dir = unique_dir("sentinel");
        let media = dir.join("black-film.mkv");
        touch(&media, b"fake video bytes");
        touch(&dir.join(format!("{}.none", key_for(&media))), b"");

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);

        assert_eq!(items[0].poster_path, None);
        assert!(processed.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn a_zero_byte_poster_is_treated_as_corrupt_and_regenerated() {
        let dir = unique_dir("corrupt");
        let media = dir.join("movie.mkv");
        touch(&media, b"fake video bytes");
        let poster = dir.join(format!("{}.jpg", key_for(&media)));
        touch(&poster, b""); // interrupted write left a 0-byte file at the final name

        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);

        assert_eq!(items[0].poster_path, None);
        assert!(!poster.exists(), "the corrupt leftover is cleared");
        assert_eq!(processed.recv_timeout(Duration::from_secs(1)), Ok(media));
    }

    #[test]
    fn a_changed_file_invalidates_its_old_poster() {
        let dir = unique_dir("invalidate");
        let media = dir.join("movie.mkv");
        touch(&media, b"original bytes");
        let old_poster = dir.join(format!("{}.jpg", key_for(&media)));
        touch(&old_poster, b"jpeg");

        // The unchanged file serves the cached poster.
        let (shelf, processed) = recording_shelf(dir.clone());
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        assert_eq!(
            items[0].poster_path.as_deref(),
            Some(old_poster.to_str().unwrap())
        );
        assert!(processed.recv_timeout(Duration::from_millis(100)).is_err());

        // Replacing the file changes its size, so the derived key — and thus the expected
        // poster path — changes: the stale frame is no longer served and a fresh one is queued.
        touch(&media, b"a longer set of replacement bytes");
        let mut items = vec![video_item(&media.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        assert_eq!(items[0].poster_path, None);
        assert_eq!(processed.recv_timeout(Duration::from_secs(1)), Ok(media));
    }

    #[test]
    fn playback_suspend_cancels_active_work_and_resumes_from_a_clean_queue() {
        let dir = unique_dir("suspend");
        let first = dir.join("first.mkv");
        let second = dir.join("second.mkv");
        touch(&first, b"fake video bytes");
        touch(&second, b"other video bytes");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel();
        let shelf = PosterShelf::with_processor(dir.clone(), move |job, cancel| {
            started_tx.send(job.media_path.clone()).unwrap();
            release_rx.recv().unwrap();
            done_tx
                .send((job.media_path.clone(), cancel.is_cancelled()))
                .unwrap();
        });

        let mut items = vec![video_item(&first.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(1)),
            Ok(first.clone())
        );

        let mut items = vec![video_item(&second.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        shelf.suspend();
        release_tx.send(()).unwrap();

        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(1)),
            Ok((first, true))
        );
        assert!(
            started_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "queued idle work must not start behind playback"
        );

        shelf.resume();
        let mut items = vec![video_item(&second.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(1)),
            Ok(second.clone())
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(1)),
            Ok((second, false))
        );
    }

    #[test]
    fn shutdown_cancels_pending_work_and_stops_the_worker() {
        let dir = unique_dir("cancel");
        let first = dir.join("first.mkv");
        let second = dir.join("second.mkv");
        touch(&first, b"fake video bytes");
        touch(&second, b"other video bytes");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (done_tx, done_rx) = mpsc::channel();
        let cancel_seen = Arc::new(AtomicBool::new(false));
        let worker_cancel_seen = Arc::clone(&cancel_seen);
        let shelf = PosterShelf::with_processor(dir.clone(), move |job, cancel| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            if cancel.is_cancelled() {
                worker_cancel_seen.store(true, Ordering::Relaxed);
            }
            done_tx.send(job.media_path.clone()).unwrap();
        });

        // Job A starts and blocks; job B queues behind it.
        let mut items = vec![video_item(&first.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let mut items = vec![video_item(&second.to_string_lossy(), 600.0)];
        shelf.project(&mut items, false);

        // Drop signals shutdown + cancellation and clears the pending queue (job B).
        drop(shelf);
        release_tx.send(()).unwrap();

        assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)), Ok(first));
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "the pending second job must be dropped on shutdown"
        );
        assert!(
            cancel_seen.load(Ordering::Relaxed),
            "the in-flight processor observes the cancellation signal"
        );
    }
}
