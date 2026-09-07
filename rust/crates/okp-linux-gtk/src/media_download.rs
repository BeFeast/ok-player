use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use okp_core::media_download::{
    DownloadBusy, DownloadContainer, DownloadJobId, DownloadRejection, DownloadedMedia,
    MediaDownloadEvent, MediaDownloadOutcome, MediaDownloadProgress, MediaDownloadRequest,
};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(20);
const PROCESS_TERM_GRACE: Duration = Duration::from_millis(300);
const MAX_PROBE_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;
const PROGRESS_PREFIX: &str = "OKP-PROGRESS\t";
const OUTPUT_PREFIX: &str = "OKP-OUTPUT\t";

/// One non-blocking native yt-dlp adapter. A request starts immediately or is
/// rejected as busy; completed work is never retained as an implicit queue.
pub(crate) struct MediaDownloader {
    executable: PathBuf,
    event_sender: mpsc::Sender<MediaDownloadEvent>,
    event_receiver: mpsc::Receiver<MediaDownloadEvent>,
    active: Option<ActiveJob>,
    next_job_id: u64,
}

struct ActiveJob {
    id: DownloadJobId,
    control: Arc<JobControl>,
    worker: JoinHandle<()>,
}

#[derive(Default)]
struct JobControl {
    cancelled: AtomicBool,
    process_group: Mutex<Option<libc::pid_t>>,
}

impl JobControl {
    fn cancel(&self) -> bool {
        if self.cancelled.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.signal_process_group(libc::SIGTERM);
        true
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn register(&self, pid: u32) -> io::Result<()> {
        let pid = libc::pid_t::try_from(pid)
            .map_err(|_| io::Error::other("child process id is outside pid_t range"))?;
        *self
            .process_group
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(pid);
        if self.is_cancelled() {
            self.signal_process_group(libc::SIGTERM);
        }
        Ok(())
    }

    fn clear(&self, pid: u32) {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return;
        };
        let mut active = self
            .process_group
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active == Some(pid) {
            *active = None;
        }
    }

    fn signal_process_group(&self, signal: libc::c_int) {
        let active = self
            .process_group
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pid) = *active {
            // The child calls setpgid(0, 0) before exec, so its PID is also the
            // process-group id. A negative target reaches yt-dlp and the ffmpeg
            // descendants it starts. The worker owns and reaps the group leader;
            // holding this registration lock keeps a reaped PID from being reused
            // between lookup and signal delivery.
            unsafe {
                libc::kill(-pid, signal);
            }
        }
    }
}

impl Default for MediaDownloader {
    fn default() -> Self {
        Self::with_executable(PathBuf::from(okp_core::youtube_open::YOUTUBE_RESOLVER))
    }
}

impl MediaDownloader {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Injecting the executable keeps process and cancellation tests offline;
    /// production uses the same PATH-resolved yt-dlp name as URL playback.
    pub(crate) fn with_executable(executable: PathBuf) -> Self {
        let (event_sender, event_receiver) = mpsc::channel();
        Self {
            executable,
            event_sender,
            event_receiver,
            active: None,
            next_job_id: 1,
        }
    }

    pub(crate) fn request(
        &mut self,
        request: MediaDownloadRequest,
    ) -> Result<DownloadJobId, DownloadBusy> {
        self.reap_finished_worker();
        if let Some(active) = &self.active {
            return Err(DownloadBusy {
                active_job_id: active.id,
            });
        }

        let job_id = DownloadJobId(self.next_job_id);
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        let control = Arc::new(JobControl::default());
        let worker_control = Arc::clone(&control);
        let executable = self.executable.clone();
        let events = self.event_sender.clone();

        // Started is queued before the worker can publish progress or its one
        // terminal outcome, giving consumers a stable per-job event order.
        let _ = self
            .event_sender
            .send(MediaDownloadEvent::Started { job_id });
        let worker_events = events.clone();
        match thread::Builder::new()
            .name(format!("okp-media-download-{}", job_id.0))
            .spawn(move || run_job(job_id, request, executable, worker_control, worker_events))
        {
            Ok(worker) => {
                self.active = Some(ActiveJob {
                    id: job_id,
                    control,
                    worker,
                });
            }
            Err(_) => {
                let _ = events.send(terminal_failed(
                    job_id,
                    "The media download worker could not be started.",
                ));
            }
        }
        Ok(job_id)
    }

    pub(crate) fn cancel(&mut self, job_id: DownloadJobId) -> bool {
        self.reap_finished_worker();
        self.active
            .as_ref()
            .filter(|active| active.id == job_id)
            .is_some_and(|active| active.control.cancel())
    }

    pub(crate) fn drain_events(&mut self) -> Vec<MediaDownloadEvent> {
        self.reap_finished_worker();
        self.event_receiver.try_iter().collect()
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(active) = self.active.take() {
            active.control.cancel();
            let _ = active.worker.join();
        }
    }

    fn reap_finished_worker(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.worker.is_finished())
            && let Some(active) = self.active.take()
        {
            let _ = active.worker.join();
        }
    }
}

impl Drop for MediaDownloader {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_job(
    job_id: DownloadJobId,
    request: MediaDownloadRequest,
    executable: PathBuf,
    control: Arc<JobControl>,
    events: mpsc::Sender<MediaDownloadEvent>,
) {
    let outcome = run_job_inner(job_id, &request, &executable, &control, &events);
    let _ = events.send(MediaDownloadEvent::Terminal { job_id, outcome });
}

fn run_job_inner(
    job_id: DownloadJobId,
    request: &MediaDownloadRequest,
    executable: &Path,
    control: &Arc<JobControl>,
    events: &mpsc::Sender<MediaDownloadEvent>,
) -> MediaDownloadOutcome {
    if control.is_cancelled() {
        return MediaDownloadOutcome::Cancelled;
    }
    if let Err(rejection) = MediaDownloadRequest::public_vod(
        request.source_url.clone(),
        request.target.clone(),
        request.purpose,
        request.container,
        request.max_bytes,
    ) {
        return MediaDownloadOutcome::Rejected(rejection);
    }

    let staging = match OwnedStagingDirectory::prepare(&request.target.staging_directory) {
        Ok(staging) => staging,
        Err(_) => return MediaDownloadOutcome::Rejected(DownloadRejection::UnsafeTarget),
    };

    let probe = match probe_source(executable, request, control) {
        Ok(probe) => probe,
        Err(ProcessFailure::Cancelled) => return MediaDownloadOutcome::Cancelled,
        Err(ProcessFailure::ToolUnavailable) => {
            return failed("yt-dlp could not be started; install it and ensure it is executable.");
        }
        Err(ProcessFailure::OutputLimit) => {
            return failed("yt-dlp returned more metadata than the downloader can safely inspect.");
        }
        Err(ProcessFailure::Io) => return failed("The media metadata probe failed."),
        Err(ProcessFailure::Exit(status)) => {
            return failed(format!("yt-dlp metadata probe failed with {status}."));
        }
    };
    if probe.playlist {
        return MediaDownloadOutcome::Rejected(DownloadRejection::Playlist);
    }
    if probe.live {
        return MediaDownloadOutcome::Rejected(DownloadRejection::LiveStream);
    }
    if control.is_cancelled() {
        return MediaDownloadOutcome::Cancelled;
    }

    let download =
        match download_source(executable, request, job_id, control, events, staging.path()) {
            Ok(download) => download,
            Err(ProcessFailure::Cancelled) => return MediaDownloadOutcome::Cancelled,
            Err(ProcessFailure::ToolUnavailable) => {
                return failed(
                    "yt-dlp could not be started; install it and ensure it is executable.",
                );
            }
            Err(ProcessFailure::OutputLimit) => {
                return failed("The download exceeded its staged byte limit.");
            }
            Err(ProcessFailure::Io) => return failed("The media download failed."),
            Err(ProcessFailure::Exit(status)) => {
                return failed(format!("yt-dlp download failed with {status}."));
            }
        };

    let media = match validate_completed_output(
        staging.path(),
        &request.target.file_stem,
        download.output_path.as_deref(),
        request.max_bytes,
    ) {
        Ok(media) => media,
        Err(CompletedOutputError::TooLarge) => {
            return failed("The download exceeded its staged byte limit.");
        }
        Err(CompletedOutputError::Invalid) => {
            return failed("yt-dlp did not produce one safe completed media file.");
        }
    };
    if control.is_cancelled() {
        return MediaDownloadOutcome::Cancelled;
    }

    if let Err(error) = staging.retain_only(&media.path) {
        eprintln!("Failed to clean media download intermediates: {error}");
        return failed("The completed download could not be finalized safely.");
    }
    if control.is_cancelled() {
        return MediaDownloadOutcome::Cancelled;
    }
    staging.keep();
    MediaDownloadOutcome::Completed(media)
}

fn failed(message: impl Into<String>) -> MediaDownloadOutcome {
    MediaDownloadOutcome::Failed {
        message: message.into(),
    }
}

fn terminal_failed(job_id: DownloadJobId, message: impl Into<String>) -> MediaDownloadEvent {
    MediaDownloadEvent::Terminal {
        job_id,
        outcome: failed(message),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ProbeResult {
    playlist: bool,
    live: bool,
}

fn probe_source(
    executable: &Path,
    request: &MediaDownloadRequest,
    control: &Arc<JobControl>,
) -> Result<ProbeResult, ProcessFailure> {
    let mut command = base_command(executable);
    command
        .arg("--flat-playlist")
        .arg("--playlist-items")
        .arg("1")
        .arg("--dump-single-json")
        .arg("--skip-download")
        .arg("--")
        .arg(&request.source_url);

    let result = run_captured_command(&mut command, control, MAX_PROBE_OUTPUT_BYTES)?;
    if !result.status.success() {
        return Err(ProcessFailure::Exit(result.status));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|_| ProcessFailure::Io)?;
    let kind = value
        .get("_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let playlist = kind.eq_ignore_ascii_case("playlist")
        || value
            .get("entries")
            .is_some_and(serde_json::Value::is_array);
    let live_status = value
        .get("live_status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let live = value
        .get("is_live")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        || matches!(live_status, "is_live" | "is_upcoming" | "post_live");
    Ok(ProbeResult { playlist, live })
}

fn base_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--ignore-config")
        .arg("--no-config-locations")
        .arg("--no-cookies")
        .arg("--no-cookies-from-browser")
        .arg("--no-cache-dir")
        .arg("--no-playlist")
        .arg("--no-mark-watched")
        .arg("--no-color");
    command
}

struct CapturedProcess {
    status: ExitStatus,
    stdout: Vec<u8>,
}

fn run_captured_command(
    command: &mut Command,
    control: &Arc<JobControl>,
    output_limit: usize,
) -> Result<CapturedProcess, ProcessFailure> {
    let mut child = spawn_grouped(command, control)?;
    let stdout = child.stdout.take().ok_or(ProcessFailure::Io)?;
    let stderr = child.stderr.take().ok_or(ProcessFailure::Io)?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, output_limit));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_DIAGNOSTIC_BYTES));
    let status = monitor_child(&mut child, control, None);
    let stdout = stdout_reader
        .join()
        .map_err(|_| ProcessFailure::Io)?
        .map_err(|error| match error.kind() {
            io::ErrorKind::FileTooLarge => ProcessFailure::OutputLimit,
            _ => ProcessFailure::Io,
        })?;
    let _ = stderr_reader.join();
    let status = status?;
    Ok(CapturedProcess { status, stdout })
}

struct DownloadProcess {
    output_path: Option<PathBuf>,
}

fn download_source(
    executable: &Path,
    request: &MediaDownloadRequest,
    job_id: DownloadJobId,
    control: &Arc<JobControl>,
    events: &mpsc::Sender<MediaDownloadEvent>,
    staging: &Path,
) -> Result<DownloadProcess, ProcessFailure> {
    let output_template = format!("{}.%(ext)s", request.target.file_stem);
    let mut command = base_command(executable);
    command
        .arg("--match-filters")
        .arg("!is_live")
        .arg("--abort-on-error")
        .arg("--max-filesize")
        .arg(request.max_bytes.to_string())
        .arg("--format")
        .arg(request.format_selector.as_deref().unwrap_or("bestvideo+bestaudio/best"))
        .arg("--paths")
        .arg(staging)
        .arg("--output")
        .arg(output_template)
        .arg("--newline")
        .arg("--progress")
        .arg("--progress-delta")
        .arg("0.1")
        .arg("--progress-template")
        .arg(format!(
            "download:{PROGRESS_PREFIX}%(progress.downloaded_bytes)s\t%(progress.total_bytes)s\t%(progress.total_bytes_estimate)s"
        ))
        .arg("--print")
        .arg(format!("after_move:{OUTPUT_PREFIX}%(filepath)j"));
    if request.container == DownloadContainer::Matroska {
        command
            .arg("--merge-output-format")
            .arg("mkv")
            .arg("--remux-video")
            .arg("mkv");
    }
    command.arg("--").arg(&request.source_url);

    let mut child = spawn_grouped(&mut command, control)?;
    let stdout = child.stdout.take().ok_or(ProcessFailure::Io)?;
    let stderr = child.stderr.take().ok_or(ProcessFailure::Io)?;
    let event_sender = events.clone();
    let output_path = Arc::new(Mutex::new(None));
    let reader_output = Arc::clone(&output_path);
    let stdout_reader =
        thread::spawn(move || read_download_output(stdout, job_id, &event_sender, &reader_output));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_DIAGNOSTIC_BYTES));
    let status = monitor_child(&mut child, control, Some((staging, request.max_bytes)));
    let reader = stdout_reader
        .join()
        .map_err(|_| ProcessFailure::Io)?
        .map_err(|_| ProcessFailure::Io);
    let _ = stderr_reader.join();
    let status = status?;
    reader?;
    if !status.success() {
        return Err(ProcessFailure::Exit(status));
    }
    let output_path = output_path
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    Ok(DownloadProcess { output_path })
}

fn spawn_grouped(
    command: &mut Command,
    control: &Arc<JobControl>,
) -> Result<Child, ProcessFailure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Keep the whole yt-dlp/ffmpeg tree addressable as one unit and kill the
    // group leader if OK Player exits unexpectedly. Normal shutdown also sends
    // an explicit group signal and waits for the child to be reaped.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() == 1 {
                libc::raise(libc::SIGKILL);
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|error| {
        if matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
        ) {
            ProcessFailure::ToolUnavailable
        } else {
            ProcessFailure::Io
        }
    })?;
    if let Err(_error) = control.register(child.id()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ProcessFailure::Io);
    }
    Ok(child)
}

fn monitor_child(
    child: &mut Child,
    control: &Arc<JobControl>,
    staged_limit: Option<(&Path, u64)>,
) -> Result<ExitStatus, ProcessFailure> {
    let pid = child.id();
    let mut stopping_since = None;
    let mut limit_exceeded = false;
    loop {
        if let Some((directory, limit)) = staged_limit {
            match directory_bytes(directory, limit) {
                Ok(bytes) if bytes > limit => limit_exceeded = true,
                Ok(_) => {}
                Err(_) => limit_exceeded = true,
            }
        }
        if control.is_cancelled() || limit_exceeded {
            let started = stopping_since.get_or_insert_with(|| {
                control.signal_process_group(libc::SIGTERM);
                Instant::now()
            });
            if started.elapsed() >= PROCESS_TERM_GRACE {
                control.signal_process_group(libc::SIGKILL);
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                if control.is_cancelled() || limit_exceeded || !status.success() {
                    // A failing/cancelled group leader must not leave an ffmpeg
                    // child holding pipes or writing into the owned directory.
                    control.signal_process_group(libc::SIGKILL);
                }
                control.clear(pid);
                if control.is_cancelled() {
                    return Err(ProcessFailure::Cancelled);
                }
                if limit_exceeded {
                    return Err(ProcessFailure::OutputLimit);
                }
                return Ok(status);
            }
            Ok(None) => thread::sleep(PROCESS_POLL_INTERVAL),
            Err(_) => {
                control.signal_process_group(libc::SIGKILL);
                let _ = child.wait();
                control.clear(pid);
                return if control.is_cancelled() {
                    Err(ProcessFailure::Cancelled)
                } else {
                    Err(ProcessFailure::Io)
                };
            }
        }
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut overflow = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) <= limit {
            output.extend_from_slice(&buffer[..read]);
        } else {
            overflow = true;
        }
    }
    if overflow {
        Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "process output exceeded limit",
        ))
    } else {
        Ok(output)
    }
}

fn read_download_output(
    stdout: impl Read,
    job_id: DownloadJobId,
    events: &mpsc::Sender<MediaDownloadEvent>,
    output_path: &Mutex<Option<PathBuf>>,
) -> io::Result<()> {
    let mut last_downloaded_bytes = 0;
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        if let Some(mut progress) = parse_progress_line(job_id, &line) {
            last_downloaded_bytes = last_downloaded_bytes.max(progress.downloaded_bytes);
            progress.downloaded_bytes = last_downloaded_bytes;
            let _ = events.send(MediaDownloadEvent::Progress(progress));
        } else if let Some(path) = parse_output_line(&line) {
            *output_path
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path);
        }
    }
    Ok(())
}

fn parse_progress_line(job_id: DownloadJobId, line: &str) -> Option<MediaDownloadProgress> {
    let fields: Vec<&str> = line.strip_prefix(PROGRESS_PREFIX)?.split('\t').collect();
    if fields.len() != 3 {
        return None;
    }
    let downloaded_bytes = parse_optional_u64(fields[0])?;
    let total_bytes = parse_optional_u64(fields[1]).or_else(|| parse_optional_u64(fields[2]));
    Some(MediaDownloadProgress {
        job_id,
        downloaded_bytes,
        total_bytes,
    })
}

fn parse_optional_u64(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("na") || value.eq_ignore_ascii_case("none") {
        None
    } else {
        value.parse().ok()
    }
}

fn parse_output_line(line: &str) -> Option<PathBuf> {
    let encoded = line.strip_prefix(OUTPUT_PREFIX)?;
    serde_json::from_str::<String>(encoded)
        .ok()
        .map(PathBuf::from)
}

struct OwnedStagingDirectory {
    path: PathBuf,
    keep: AtomicBool,
}

impl OwnedStagingDirectory {
    fn prepare(path: &Path) -> io::Result<Self> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() || fs::read_dir(path)?.next().is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "staging target is not an empty directory",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path)?,
            Err(error) => return Err(error),
        }
        Ok(Self {
            path: fs::canonicalize(path)?,
            keep: AtomicBool::new(false),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn retain_only(&self, retained: &Path) -> io::Result<()> {
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            if entry.path() == retained {
                continue;
            }
            remove_owned_entry(&entry.path())?;
        }
        Ok(())
    }

    fn keep(&self) {
        self.keep.store(true, Ordering::Release);
    }
}

impl Drop for OwnedStagingDirectory {
    fn drop(&mut self) {
        if !self.keep.load(Ordering::Acquire)
            && let Err(error) = fs::remove_dir_all(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("Failed to remove media download staging directory: {error}");
        }
    }
}

fn remove_owned_entry(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn directory_bytes(root: &Path, stop_after: u64) -> io::Result<u64> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "download staging contains a symlink",
                ));
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
                if total > stop_after {
                    return Ok(total);
                }
            }
        }
    }
    Ok(total)
}

enum CompletedOutputError {
    Invalid,
    TooLarge,
}

fn validate_completed_output(
    staging: &Path,
    file_stem: &str,
    reported: Option<&Path>,
    max_bytes: u64,
) -> Result<DownloadedMedia, CompletedOutputError> {
    let reported = reported.ok_or(CompletedOutputError::Invalid)?;
    let path = if reported.is_absolute() {
        reported.to_path_buf()
    } else {
        staging.join(reported)
    };
    let metadata = fs::symlink_metadata(&path).map_err(|_| CompletedOutputError::Invalid)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(CompletedOutputError::Invalid);
    }
    if metadata.len() > max_bytes
        || directory_bytes(staging, max_bytes).map_err(|_| CompletedOutputError::Invalid)?
            > max_bytes
    {
        return Err(CompletedOutputError::TooLarge);
    }
    let canonical_staging = fs::canonicalize(staging).map_err(|_| CompletedOutputError::Invalid)?;
    let canonical_path = fs::canonicalize(&path).map_err(|_| CompletedOutputError::Invalid)?;
    if canonical_path.parent() != Some(canonical_staging.as_path()) {
        return Err(CompletedOutputError::Invalid);
    }
    let extension = canonical_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 16
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .ok_or(CompletedOutputError::Invalid)?;
    let expected_name = format!("{file_stem}.{extension}");
    if canonical_path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err(CompletedOutputError::Invalid);
    }
    Ok(DownloadedMedia {
        path: canonical_path,
        byte_len: metadata.len(),
        extension,
    })
}

#[derive(Debug)]
enum ProcessFailure {
    Cancelled,
    ToolUnavailable,
    OutputLimit,
    Io,
    Exit(ExitStatus),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    use okp_core::media_download::{DownloadPurpose, DownloadTarget};
    use tempfile::TempDir;

    fn executable(root: &Path, body: &str) -> PathBuf {
        let path = root.join("fake-yt-dlp");
        fs::write(&path, format!("#!/usr/bin/env bash\nset -eu\n{body}\n"))
            .expect("fake yt-dlp should be written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("fake yt-dlp should be executable");
        path
    }

    fn request(
        staging: &Path,
        container: DownloadContainer,
        max_bytes: u64,
    ) -> MediaDownloadRequest {
        MediaDownloadRequest::public_vod(
            "https://video.example.test/watch?v=public",
            DownloadTarget::new(staging, "media").expect("safe staging target"),
            DownloadPurpose::ReplayCache,
            container,
            max_bytes,
        )
        .expect("public VOD request")
    }

    fn wait_for_terminal(
        downloader: &mut MediaDownloader,
        job_id: DownloadJobId,
    ) -> Vec<MediaDownloadEvent> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received = Vec::new();
        loop {
            received.extend(downloader.drain_events());
            if received.iter().any(|event| {
                matches!(
                    event,
                    MediaDownloadEvent::Terminal {
                        job_id: terminal_id,
                        ..
                    } if *terminal_id == job_id
                )
            }) {
                return received;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for job {job_id:?}: {received:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn terminal_outcome(events: &[MediaDownloadEvent]) -> &MediaDownloadOutcome {
        events
            .iter()
            .find_map(|event| match event {
                MediaDownloadEvent::Terminal { outcome, .. } => Some(outcome),
                _ => None,
            })
            .expect("terminal outcome")
    }

    #[test]
    fn completed_job_reports_progress_and_real_suffix_under_isolated_policy() {
        let root = TempDir::new().expect("temp root");
        let staging = root.path().join("staging");
        let arguments = root.path().join("arguments.txt");
        let program = executable(
            root.path(),
            &format!(
                r#"
printf '%s\n' "$@" >> '{}'
limit=0
for argument in "$@"; do
  if [ "$argument" = "--no-netrc" ]; then exit 2; fi
  if [ "$argument" = "--max-downloads" ]; then limit=1; fi

  if [ "$argument" = "--dump-single-json" ]; then
    printf '%s\n' '{{"_type":"video","is_live":false,"live_status":"not_live"}}'
    exit 0
  fi
done
stage=''
previous=''
for argument in "$@"; do
  if [ "$previous" = "--paths" ]; then stage="$argument"; fi
  previous="$argument"
done
printf 'complete-media' > "$stage/media.mp4"
printf 'temporary' > "$stage/media.f137.mp4"
printf 'OKP-PROGRESS\t7\t14\tNA\n'
printf 'OKP-OUTPUT\t"%s"\n' "$stage/media.mp4"
if [ "$limit" = "1" ]; then exit 101; fi
"#,
                arguments.display()
            ),
        );
        let mut downloader = MediaDownloader::with_executable(program);

        let mut selected = request(&staging, DownloadContainer::Preserve, 1024);
        selected.format_selector = Some("best[height<=1080]".into());
        let job_id = downloader.request(selected).expect("request should start");
        let events = wait_for_terminal(&mut downloader, job_id);

        assert!(matches!(
            events.first(),
            Some(MediaDownloadEvent::Started { job_id: started }) if *started == job_id
        ));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                MediaDownloadEvent::Progress(MediaDownloadProgress {
                    job_id: progress_id,
                    downloaded_bytes: 7,
                    total_bytes: Some(14),
                }) if *progress_id == job_id
            )
        }));
        let MediaDownloadOutcome::Completed(media) = terminal_outcome(&events) else {
            panic!("job did not complete: {events:?}");
        };
        assert_eq!(media.extension, "mp4");
        assert_eq!(media.byte_len, 14);
        assert_eq!(
            fs::read(&media.path).expect("completed bytes"),
            b"complete-media"
        );
        assert_eq!(
            fs::read_dir(&staging).expect("retained staging").count(),
            1,
            "successful finalization should remove only job intermediates"
        );

        let arguments = fs::read_to_string(arguments).expect("recorded arguments");
        let lines: Vec<&str> = arguments.lines().collect();
        for required in [
            "--ignore-config",
            "--no-config-locations",
            "--no-playlist",
            "--no-cookies",
            "--no-cookies-from-browser",
            "--no-cache-dir",
            "--match-filters",
            "!is_live",
            "--max-filesize",
            "1024",
            "best[height<=1080]",
            "--progress-template",
            "--print",
        ] {
            assert!(lines.contains(&required), "missing argument {required:?}");
        }
        assert!(!lines.contains(&"--cookies"));
        assert!(!lines.contains(&"--cookies-from-browser"));
        assert_eq!(
            lines
                .iter()
                .filter(|argument| **argument == "https://video.example.test/watch?v=public")
                .count(),
            2,
            "the probe and download should each receive exactly one source"
        );
    }

    #[test]
    fn matroska_request_is_truthful_about_the_post_move_file() {
        let root = TempDir::new().expect("temp root");
        let staging = root.path().join("staging");
        let arguments = root.path().join("arguments.txt");
        let program = executable(
            root.path(),
            &format!(
                r#"
printf '%s\n' "$@" >> '{}'
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{{"_type":"video","is_live":false}}'; exit 0 ;;
esac
stage=''
previous=''
for argument in "$@"; do
  if [ "$previous" = "--paths" ]; then stage="$argument"; fi
  previous="$argument"
done
printf 'mkv' > "$stage/media.mkv"
printf 'OKP-OUTPUT\t"%s"\n' "$stage/media.mkv"
"#,
                arguments.display()
            ),
        );
        let mut downloader = MediaDownloader::with_executable(program);

        let job_id = downloader
            .request(request(&staging, DownloadContainer::Matroska, 1024))
            .expect("request should start");
        let events = wait_for_terminal(&mut downloader, job_id);
        let MediaDownloadOutcome::Completed(media) = terminal_outcome(&events) else {
            panic!("job did not complete: {events:?}");
        };
        assert_eq!(
            media.path.file_name().and_then(|name| name.to_str()),
            Some("media.mkv")
        );
        assert_eq!(media.extension, "mkv");
        let arguments = fs::read_to_string(arguments).expect("recorded arguments");
        let lines: Vec<&str> = arguments.lines().collect();
        assert!(lines.contains(&"--merge-output-format"));
        assert!(lines.contains(&"--remux-video"));
        assert!(lines.iter().filter(|value| **value == "mkv").count() >= 2);
    }

    #[test]
    fn playlist_and_live_probe_results_are_terminal_rejections_without_download() {
        for (name, metadata, expected) in [
            (
                "playlist",
                r#"{"_type":"playlist","entries":[{"id":"one"}]}"#,
                DownloadRejection::Playlist,
            ),
            (
                "live",
                r#"{"_type":"video","is_live":true,"live_status":"is_live"}"#,
                DownloadRejection::LiveStream,
            ),
        ] {
            let root = TempDir::new().expect("temp root");
            let staging = root.path().join("staging");
            let downloaded = root.path().join("download-ran");
            let program = executable(
                root.path(),
                &format!(
                    r#"
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{}'; exit 0 ;;
esac
touch '{}'
exit 9
"#,
                    metadata,
                    downloaded.display()
                ),
            );
            let mut downloader = MediaDownloader::with_executable(program);

            let job_id = downloader
                .request(request(&staging, DownloadContainer::Preserve, 1024))
                .expect("request should start");
            let events = wait_for_terminal(&mut downloader, job_id);

            assert_eq!(
                terminal_outcome(&events),
                &MediaDownloadOutcome::Rejected(expected),
                "wrong rejection for {name}"
            );
            assert!(!downloaded.exists(), "download ran for rejected {name}");
            assert!(!staging.exists(), "rejected {name} staging survived");
        }
    }

    #[test]
    fn one_active_job_is_rejected_instead_of_queued() {
        let root = TempDir::new().expect("temp root");
        let first_staging = root.path().join("first");
        let second_staging = root.path().join("second");
        let program = executable(
            root.path(),
            r#"
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{"_type":"video","is_live":false}'; exit 0 ;;
esac
trap '' TERM
while :; do sleep 1; done
"#,
        );
        let mut downloader = MediaDownloader::with_executable(program);

        let first = downloader
            .request(request(&first_staging, DownloadContainer::Preserve, 1024))
            .expect("first request should start");
        let busy = downloader
            .request(request(&second_staging, DownloadContainer::Preserve, 1024))
            .expect_err("second request must not queue");
        assert_eq!(busy.active_job_id, first);
        assert!(!second_staging.exists());

        assert!(downloader.cancel(first));
        assert!(!downloader.cancel(first), "cancel should be idempotent");
        let events = wait_for_terminal(&mut downloader, first);
        assert_eq!(terminal_outcome(&events), &MediaDownloadOutcome::Cancelled);
    }

    #[test]
    fn cancellation_kills_the_process_group_and_removes_only_owned_staging() {
        let root = TempDir::new().expect("temp root");
        let staging = root.path().join("staging");
        let unrelated = root.path().join("keep.txt");
        let descendant_pid = root.path().join("descendant.pid");
        fs::write(&unrelated, b"keep").expect("unrelated fixture");
        let program = executable(
            root.path(),
            &format!(
                r#"
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{{"_type":"video","is_live":false}}'; exit 0 ;;
esac
stage=''
previous=''
for argument in "$@"; do
  if [ "$previous" = "--paths" ]; then stage="$argument"; fi
  previous="$argument"
done
printf 'partial' > "$stage/media.mp4.part"
(trap '' TERM; exec sleep 30) &
printf '%s\n' "$!" > '{}'
trap '' TERM
while :; do sleep 1; done
"#,
                descendant_pid.display()
            ),
        );
        let mut downloader = MediaDownloader::with_executable(program);
        let job_id = downloader
            .request(request(&staging, DownloadContainer::Preserve, 1024))
            .expect("request should start");

        let deadline = Instant::now() + Duration::from_secs(3);
        while !descendant_pid.exists() {
            assert!(Instant::now() < deadline, "fake descendant never started");
            thread::sleep(Duration::from_millis(10));
        }
        let pid: libc::pid_t = fs::read_to_string(&descendant_pid)
            .expect("descendant pid")
            .trim()
            .parse()
            .expect("numeric descendant pid");
        assert!(process_exists(pid));

        assert!(downloader.cancel(job_id));
        let events = wait_for_terminal(&mut downloader, job_id);
        assert_eq!(terminal_outcome(&events), &MediaDownloadOutcome::Cancelled);
        assert!(!staging.exists(), "cancelled staging survived");
        assert_eq!(fs::read(&unrelated).expect("unrelated file"), b"keep");
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(pid),
            "ffmpeg-like descendant survived cancel"
        );
    }

    #[test]
    fn size_failure_and_process_failure_remove_staging_but_not_siblings() {
        for (name, body, limit) in [
            (
                "size",
                r#"
stage=''
previous=''
for argument in "$@"; do
  if [ "$previous" = "--paths" ]; then stage="$argument"; fi
  previous="$argument"
done
dd if=/dev/zero of="$stage/media.mp4.part" bs=64 count=1 2>/dev/null
trap '' TERM
while :; do sleep 1; done
"#,
                16,
            ),
            (
                "exit",
                r#"
stage=''
previous=''
for argument in "$@"; do
  if [ "$previous" = "--paths" ]; then stage="$argument"; fi
  previous="$argument"
done
printf 'partial' > "$stage/media.mp4.part"
exit 7
"#,
                1024,
            ),
        ] {
            let root = TempDir::new().expect("temp root");
            let staging = root.path().join("staging");
            let unrelated = root.path().join("keep.txt");
            fs::write(&unrelated, b"keep").expect("unrelated fixture");
            let program = executable(
                root.path(),
                &format!(
                    r#"
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{{"_type":"video","is_live":false}}'; exit 0 ;;
esac
{body}
"#
                ),
            );
            let mut downloader = MediaDownloader::with_executable(program);
            let job_id = downloader
                .request(request(&staging, DownloadContainer::Preserve, limit))
                .expect("request should start");
            let events = wait_for_terminal(&mut downloader, job_id);

            assert!(
                matches!(
                    terminal_outcome(&events),
                    MediaDownloadOutcome::Failed { .. }
                ),
                "{name} should fail: {events:?}"
            );
            assert!(!staging.exists(), "{name} staging survived");
            assert_eq!(fs::read(&unrelated).expect("unrelated file"), b"keep");
        }
    }

    #[test]
    fn shutdown_cancels_and_reaps_the_active_job_before_returning() {
        let root = TempDir::new().expect("temp root");
        let staging = root.path().join("staging");
        let started = root.path().join("started");
        let program = executable(
            root.path(),
            &format!(
                r#"
case " $* " in
  *" --dump-single-json "*) printf '%s\n' '{{"_type":"video","is_live":false}}'; exit 0 ;;
esac
touch '{}'
trap '' TERM
while :; do sleep 1; done
"#,
                started.display()
            ),
        );
        let mut downloader = MediaDownloader::with_executable(program);
        let job_id = downloader
            .request(request(&staging, DownloadContainer::Preserve, 1024))
            .expect("request should start");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !started.exists() {
            assert!(Instant::now() < deadline, "fake download never started");
            thread::sleep(Duration::from_millis(10));
        }

        downloader.shutdown();

        assert!(!staging.exists(), "shutdown left owned staging behind");
        let events = downloader.drain_events();
        assert!(events.iter().any(|event| {
            matches!(
                event,
                MediaDownloadEvent::Terminal {
                    job_id: terminal_id,
                    outcome: MediaDownloadOutcome::Cancelled,
                } if *terminal_id == job_id
            )
        }));
    }

    #[test]
    fn unsafe_existing_target_is_not_claimed_or_cleaned() {
        let root = TempDir::new().expect("temp root");
        let staging = root.path().join("not-owned");
        fs::create_dir(&staging).expect("fixture directory");
        let sentinel = staging.join("sentinel.txt");
        fs::write(&sentinel, b"unrelated").expect("fixture sentinel");
        let program = executable(root.path(), "exit 99");
        let mut downloader = MediaDownloader::with_executable(program);

        let job_id = downloader
            .request(request(&staging, DownloadContainer::Preserve, 1024))
            .expect("request is accepted asynchronously");
        let events = wait_for_terminal(&mut downloader, job_id);

        assert_eq!(
            terminal_outcome(&events),
            &MediaDownloadOutcome::Rejected(DownloadRejection::UnsafeTarget)
        );
        assert_eq!(
            fs::read(sentinel).expect("sentinel preserved"),
            b"unrelated"
        );
    }

    fn process_exists(pid: libc::pid_t) -> bool {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"));
        match stat {
            Ok(stat) => stat
                .rsplit_once(") ")
                .and_then(|(_, suffix)| suffix.chars().next())
                .is_some_and(|state| state != 'Z'),
            Err(_) => false,
        }
    }
}
