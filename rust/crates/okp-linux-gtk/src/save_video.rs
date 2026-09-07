use super::*;

use std::io;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use okp_core::media_download::{
    DownloadContainer, DownloadJobId, DownloadPurpose, MediaDownloadEvent, MediaDownloadOutcome,
    MediaDownloadRequest,
};
use okp_core::replay_cache::ReplayCachePin;
use okp_core::save_export::{
    SaveExportCancellation, SaveExportError, SaveExportProgress, SaveExportRequest, SaveMediaPlan,
    SavePickerDecision, SavePickerOutcome, SaveVideoSnapshot, SavedVideo, SavedVideoIndex,
    decide_after_picker, export_saved_video, should_persist_saved_mapping,
};

const SAVE_JOB_POLL_INTERVAL: Duration = Duration::from_millis(50);
// An explicit destination, rather than the replay cache, owns capacity. Keep a
// finite adapter bound without applying the automatic cache's 5 GiB budget.
const EXPLICIT_SAVE_MAX_BYTES: u64 = u64::MAX;

#[derive(Default)]
pub(crate) struct SaveVideoSession {
    picker_open: bool,
    closing: bool,
    active: Option<Rc<RefCell<SaveVideoJob>>>,
    store: SavedVideoStore,
}

impl SaveVideoSession {
    fn busy(&self) -> bool {
        self.closing || self.picker_open || self.active.is_some()
    }
}

enum SaveSourceGuard {
    ReplayCache { _pin: ReplayCachePin },
    DownloadStaging { _directory: tempfile::TempDir },
}

struct PreparedSave {
    snapshot: SaveVideoSnapshot,
    source_guard: Option<SaveSourceGuard>,
}

pub(crate) fn start_save_video(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    original_url: String,
    requested_title: Option<String>,
) {
    if state.borrow().save_video.busy() {
        status_toast.show("A video save is already open");
        return;
    }

    let prepared = {
        let mut player = state.borrow_mut();
        let format_selector = configured_url_load_options(&player.settings, &original_url)
            .ytdl_format()
            .map(str::to_owned);
        let private_at_start = player.private_session;
        let title = requested_title
            .filter(|title| !title.trim().is_empty())
            .or_else(|| {
                player
                    .history
                    .search("")
                    .into_iter()
                    .find(|item| item.path == original_url)
                    .map(|item| item.title)
            })
            .or_else(|| {
                (current_history_source(&player) == Some(PlaylistItem::Url(original_url.clone())))
                    .then(|| current_media_title(&player))
                    .filter(|title| !title.trim().is_empty())
            })
            .unwrap_or_else(|| "Online video".to_owned());

        let acquired = if private_at_start {
            None
        } else {
            player
                .replay_cache
                .acquire(&original_url, format_selector.as_deref())
        };
        match acquired {
            Some(acquired) => PreparedSave {
                snapshot: SaveVideoSnapshot {
                    original_url,
                    title,
                    format_selector,
                    private_at_start,
                    media: SaveMediaPlan::ReadyCache {
                        source: acquired.path,
                        extension: acquired.extension,
                    },
                },
                source_guard: Some(SaveSourceGuard::ReplayCache { _pin: acquired.pin }),
            },
            None => PreparedSave {
                snapshot: SaveVideoSnapshot {
                    original_url,
                    title,
                    format_selector,
                    private_at_start,
                    media: SaveMediaPlan::DownloadMatroska,
                },
                source_guard: None,
            },
        }
    };

    open_prepared_save_picker(parent, state, status_toast, prepared);
}

fn open_prepared_save_picker(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    prepared: PreparedSave,
) {
    {
        let mut player = state.borrow_mut();
        if player.save_video.busy() {
            status_toast.show("A video save is already open");
            return;
        }
        player.save_video.picker_open = true;
    }

    let parent_for_result = parent.clone();
    open_save_destination_dialog(parent, prepared.snapshot.clone(), move |decision| {
        {
            let mut player = state.borrow_mut();
            player.save_video.picker_open = false;
            if player.save_video.closing {
                return;
            }
        }
        match decision {
            SavePickerDecision::NoWork => {}
            SavePickerDecision::PickerFailed(error) => {
                eprintln!("Save video destination chooser failed: {error}");
                status_toast.show("Could not open the Save video chooser");
            }
            SavePickerDecision::ExtensionMismatch {
                required_extension, ..
            } => {
                status_toast.show(&format!(
                    "Choose a .{required_extension} name for this video"
                ));
                open_prepared_save_picker(&parent_for_result, state, status_toast, prepared);
            }
            SavePickerDecision::ExportReady {
                request,
                private_at_start: _,
            } => match SaveVideoJob::copy(&parent_for_result, prepared, request) {
                Ok(job) => activate_save_job(&parent_for_result, state, status_toast, job),
                Err((message, prepared)) => {
                    status_toast.show(&message);
                    open_prepared_save_picker(&parent_for_result, state, status_toast, prepared);
                }
            },
            SavePickerDecision::DownloadReady {
                target,
                original_url: _,
                format_selector: _,
                private_at_start: _,
            } => match SaveVideoJob::download(&parent_for_result, prepared, target) {
                Ok(job) => activate_save_job(&parent_for_result, state, status_toast, job),
                Err(BeginSaveError {
                    message,
                    prepared,
                    choose_again,
                }) => {
                    eprintln!("Could not begin Save video: {message}");
                    status_toast.show(&message);
                    if choose_again {
                        open_prepared_save_picker(
                            &parent_for_result,
                            state,
                            status_toast,
                            prepared,
                        );
                    }
                }
            },
        }
    });
}

struct BeginSaveError {
    message: String,
    prepared: PreparedSave,
    choose_again: bool,
}

enum SaveJobWork {
    Downloading {
        downloader: media_download::MediaDownloader,
        job_id: DownloadJobId,
    },
    Copying {
        cancellation: SaveExportCancellation,
        receiver: mpsc::Receiver<CopyEvent>,
        worker: Option<JoinHandle<()>>,
    },
    Idle,
}

enum CopyEvent {
    Progress(SaveExportProgress),
    Terminal(Result<SavedVideo, SaveExportError>),
}

struct SaveVideoJob {
    snapshot: SaveVideoSnapshot,
    source_guard: Option<SaveSourceGuard>,
    target: PathBuf,
    work: SaveJobWork,
    progress: SaveProgressDialog,
    finalizing: bool,
    cancel_requested: bool,
}

enum SaveJobPoll {
    Pending,
    Saved {
        video: SavedVideo,
        private_at_start: bool,
    },
    Cancelled,
    Failed(String),
    ChooseAgain {
        message: String,
        prepared: PreparedSave,
    },
}

impl SaveVideoJob {
    fn download(
        parent: &gtk::ApplicationWindow,
        mut prepared: PreparedSave,
        target: PathBuf,
    ) -> Result<Self, BeginSaveError> {
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(BeginSaveError {
                    message: "A file already exists there; choose another name".to_owned(),
                    prepared,
                    choose_again: true,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(BeginSaveError {
                    message: format!("Could not inspect that destination: {error}"),
                    prepared,
                    choose_again: false,
                });
            }
        }

        let Some(parent_directory) = target.parent() else {
            return Err(BeginSaveError {
                message: "Choose a destination inside a local folder".to_owned(),
                prepared,
                choose_again: true,
            });
        };
        let staging = match tempfile::Builder::new()
            .prefix(".ok-player-save-download-")
            .tempdir_in(parent_directory)
        {
            Ok(staging) => staging,
            Err(error) => {
                return Err(BeginSaveError {
                    message: format!("Could not prepare the destination folder: {error}"),
                    prepared,
                    choose_again: false,
                });
            }
        };
        let download_target =
            match okp_core::media_download::DownloadTarget::new(staging.path(), "media") {
                Ok(target) => target,
                Err(error) => {
                    return Err(BeginSaveError {
                        message: format!("Could not prepare the video download: {error}"),
                        prepared,
                        choose_again: false,
                    });
                }
            };
        let mut request = match MediaDownloadRequest::public_vod(
            prepared.snapshot.original_url.clone(),
            download_target,
            DownloadPurpose::UserSave,
            DownloadContainer::Matroska,
            EXPLICIT_SAVE_MAX_BYTES,
        ) {
            Ok(request) => request,
            Err(error) => {
                return Err(BeginSaveError {
                    message: format!("This video cannot be saved: {error}"),
                    prepared,
                    choose_again: false,
                });
            }
        };
        request.format_selector = prepared.snapshot.format_selector.clone();
        let mut downloader = media_download::MediaDownloader::new();
        let job_id = match downloader.request(request) {
            Ok(job_id) => job_id,
            Err(error) => {
                return Err(BeginSaveError {
                    message: format!("Another Save download is active: {error}"),
                    prepared,
                    choose_again: false,
                });
            }
        };
        prepared.source_guard = Some(SaveSourceGuard::DownloadStaging {
            _directory: staging,
        });
        let progress = SaveProgressDialog::new(parent, &prepared.snapshot.title);
        Ok(Self {
            snapshot: prepared.snapshot,
            source_guard: prepared.source_guard,
            target,
            work: SaveJobWork::Downloading { downloader, job_id },
            progress,
            finalizing: false,
            cancel_requested: false,
        })
    }

    fn copy(
        parent: &gtk::ApplicationWindow,
        prepared: PreparedSave,
        request: SaveExportRequest,
    ) -> Result<Self, (String, PreparedSave)> {
        let progress = SaveProgressDialog::new(parent, &prepared.snapshot.title);
        let mut job = Self {
            snapshot: prepared.snapshot.clone(),
            source_guard: prepared.source_guard,
            target: request.target.clone(),
            work: SaveJobWork::Idle,
            progress,
            finalizing: false,
            cancel_requested: false,
        };
        if let Err(error) = job.begin_copy(request) {
            let prepared = PreparedSave {
                snapshot: job.snapshot.clone(),
                source_guard: job.source_guard.take(),
            };
            return Err((error, prepared));
        }
        Ok(job)
    }

    fn begin_copy(&mut self, request: SaveExportRequest) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel();
        let cancellation = SaveExportCancellation::default();
        let worker_cancellation = cancellation.clone();
        let worker = thread::Builder::new()
            .name("okp-save-video-copy".to_owned())
            .spawn(move || {
                let progress_sender = sender.clone();
                let result = export_saved_video(request, &worker_cancellation, move |progress| {
                    let _ = progress_sender.send(CopyEvent::Progress(progress));
                });
                let _ = sender.send(CopyEvent::Terminal(result));
            })
            .map_err(|error| format!("Could not start the video copy: {error}"))?;
        self.progress
            .update(SaveProgressStage::Copying { fraction: 0.0 });
        self.work = SaveJobWork::Copying {
            cancellation,
            receiver,
            worker: Some(worker),
        };
        Ok(())
    }

    fn cancel(&mut self) {
        self.cancel_requested = true;
        let requested = match &mut self.work {
            SaveJobWork::Downloading { downloader, job_id } => downloader.cancel(*job_id),
            SaveJobWork::Copying { cancellation, .. } => {
                cancellation.cancel();
                true
            }
            SaveJobWork::Idle => false,
        };
        if requested {
            self.progress.update(SaveProgressStage::Canceling);
            self.progress.set_cancel_sensitive(false);
        }
    }

    fn poll(&mut self) -> SaveJobPoll {
        let work = std::mem::replace(&mut self.work, SaveJobWork::Idle);
        match work {
            SaveJobWork::Downloading {
                mut downloader,
                job_id,
            } => {
                let mut terminal = None;
                for event in downloader.drain_events() {
                    match event {
                        MediaDownloadEvent::Started { .. } => {}
                        MediaDownloadEvent::Progress(progress) if progress.job_id == job_id => {
                            let percent = progress.percent();
                            self.finalizing = percent == Some(100);
                            self.progress.update(if self.finalizing {
                                SaveProgressStage::Finalizing
                            } else {
                                SaveProgressStage::Downloading { percent }
                            });
                        }
                        MediaDownloadEvent::Progress(_) => {}
                        MediaDownloadEvent::Terminal {
                            job_id: completed,
                            outcome,
                        } if completed == job_id => terminal = Some(outcome),
                        MediaDownloadEvent::Terminal { .. } => {}
                    }
                }
                let Some(outcome) = terminal else {
                    if self.finalizing {
                        self.progress.update(SaveProgressStage::Finalizing);
                    }
                    self.work = SaveJobWork::Downloading { downloader, job_id };
                    return SaveJobPoll::Pending;
                };
                downloader.shutdown();
                match outcome {
                    MediaDownloadOutcome::Completed(media) => {
                        if self.cancel_requested {
                            return SaveJobPoll::Cancelled;
                        }
                        self.snapshot.media = SaveMediaPlan::ReadyCache {
                            source: media.path.clone(),
                            extension: media.extension.clone(),
                        };
                        let chosen_extension = self
                            .target
                            .extension()
                            .and_then(|extension| extension.to_str());
                        if !chosen_extension
                            .is_some_and(|chosen| chosen.eq_ignore_ascii_case(&media.extension))
                        {
                            return SaveJobPoll::ChooseAgain {
                                message: format!(
                                    "The completed video is .{}; choose a matching name",
                                    media.extension
                                ),
                                prepared: self.take_prepared(),
                            };
                        }
                        let request = SaveExportRequest::new(
                            self.snapshot.original_url.clone(),
                            media.path,
                            self.target.clone(),
                        );
                        match self.begin_copy(request) {
                            Ok(()) => SaveJobPoll::Pending,
                            Err(error) => SaveJobPoll::Failed(error),
                        }
                    }
                    MediaDownloadOutcome::Cancelled => SaveJobPoll::Cancelled,
                    MediaDownloadOutcome::Rejected(reason) => {
                        SaveJobPoll::Failed(format!("This video cannot be saved: {reason}"))
                    }
                    MediaDownloadOutcome::Failed { message } => SaveJobPoll::Failed(message),
                }
            }
            SaveJobWork::Copying {
                cancellation,
                receiver,
                mut worker,
            } => {
                // Join before draining when the writer has finished. Checking completion
                // after a drain can race its final send and incorrectly report failure.
                let finished = worker.as_ref().is_some_and(|worker| worker.is_finished());
                if finished && let Some(worker) = worker.take() {
                    let _ = worker.join();
                }
                let mut terminal = None;
                for event in receiver.try_iter() {
                    match event {
                        CopyEvent::Progress(progress) => {
                            self.progress.update(SaveProgressStage::Copying {
                                fraction: progress.fraction(),
                            });
                        }
                        CopyEvent::Terminal(result) => terminal = Some(result),
                    }
                }
                if let Some(result) = terminal {
                    if let Some(worker) = worker.take() {
                        let _ = worker.join();
                    }
                    return match result {
                        Ok(video) => SaveJobPoll::Saved {
                            video,
                            private_at_start: self.snapshot.private_at_start,
                        },
                        Err(SaveExportError::Cancelled) => SaveJobPoll::Cancelled,
                        Err(SaveExportError::DestinationExists) => SaveJobPoll::ChooseAgain {
                            message: "A file appeared there; choose another name".to_owned(),
                            prepared: self.take_prepared(),
                        },
                        Err(error) => {
                            eprintln!("Save video copy failed: {error}");
                            SaveJobPoll::Failed(error.user_message().to_owned())
                        }
                    };
                }
                if finished {
                    return SaveJobPoll::Failed("The video copy stopped unexpectedly".to_owned());
                }
                self.work = SaveJobWork::Copying {
                    cancellation,
                    receiver,
                    worker,
                };
                SaveJobPoll::Pending
            }
            SaveJobWork::Idle => {
                SaveJobPoll::Failed("The video save stopped unexpectedly".to_owned())
            }
        }
    }

    fn take_prepared(&mut self) -> PreparedSave {
        PreparedSave {
            snapshot: self.snapshot.clone(),
            source_guard: self.source_guard.take(),
        }
    }
}

impl Drop for SaveVideoJob {
    fn drop(&mut self) {
        match std::mem::replace(&mut self.work, SaveJobWork::Idle) {
            SaveJobWork::Downloading {
                mut downloader,
                job_id,
            } => {
                downloader.cancel(job_id);
                downloader.shutdown();
            }
            SaveJobWork::Copying {
                cancellation,
                mut worker,
                ..
            } => {
                cancellation.cancel();
                if let Some(worker) = worker.take() {
                    let _ = worker.join();
                }
            }
            SaveJobWork::Idle => {}
        }
    }
}

fn activate_save_job(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    job: SaveVideoJob,
) {
    let job = Rc::new(RefCell::new(job));
    let cancel_job = Rc::downgrade(&job);
    job.borrow()
        .progress
        .dialog
        .connect_response(move |_, response| {
            if response == gtk::ResponseType::Cancel
                && let Some(job) = cancel_job.upgrade()
            {
                job.borrow_mut().cancel();
            }
        });
    let close_job = Rc::downgrade(&job);
    job.borrow()
        .progress
        .dialog
        .connect_close_request(move |_| {
            if let Some(job) = close_job.upgrade()
                && let Ok(mut job) = job.try_borrow_mut()
                && !matches!(job.work, SaveJobWork::Idle)
            {
                job.cancel();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    job.borrow().progress.present();
    state.borrow_mut().save_video.active = Some(Rc::clone(&job));

    let parent = parent.clone();
    let weak_job = Rc::downgrade(&job);
    let weak_state = Rc::downgrade(&state);
    glib::timeout_add_local(SAVE_JOB_POLL_INTERVAL, move || {
        let (Some(job), Some(state)) = (weak_job.upgrade(), weak_state.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let outcome = job.borrow_mut().poll();
        if matches!(outcome, SaveJobPoll::Pending) {
            return glib::ControlFlow::Continue;
        }

        let dialog = job.borrow().progress.dialog.clone();
        dialog.close();
        {
            let mut player = state.borrow_mut();
            if player
                .save_video
                .active
                .as_ref()
                .is_some_and(|active| Rc::ptr_eq(active, &job))
            {
                player.save_video.active = None;
            }
        }

        match outcome {
            SaveJobPoll::Pending => unreachable!(),
            SaveJobPoll::Saved {
                video,
                private_at_start,
            } => {
                let result = {
                    let mut player = state.borrow_mut();
                    let private_at_completion = player.private_session;
                    player.save_video.store.record_completed(
                        &video,
                        unix_now_for_save(),
                        private_at_start,
                        private_at_completion,
                    )
                };
                match result {
                    Ok(_) => status_toast.show(&format!(
                        "Video saved to {}",
                        video
                            .path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("the chosen destination")
                    )),
                    Err(error) => {
                        eprintln!("Video saved but saved-path state could not be written: {error}");
                        status_toast.show("Video saved, but its location could not be remembered");
                    }
                }
            }
            SaveJobPoll::Cancelled => status_toast.show("Save canceled"),
            SaveJobPoll::Failed(message) => status_toast.show(&message),
            SaveJobPoll::ChooseAgain { message, prepared } => {
                status_toast.show(&message);
                open_prepared_save_picker(
                    &parent,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                    prepared,
                );
            }
        }
        glib::ControlFlow::Break
    });
}

pub(crate) fn shutdown_save_video(state: &Rc<RefCell<PlayerState>>) {
    let active = {
        let mut player = state.borrow_mut();
        player.save_video.picker_open = false;
        player.save_video.closing = true;
        player.save_video.active.take()
    };
    if let Some(active) = active
        && let Ok(mut job) = active.try_borrow_mut()
    {
        job.cancel();
    }
}

fn unix_now_for_save() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SaveProgressStage {
    Downloading { percent: Option<u8> },
    Finalizing,
    Copying { fraction: f64 },
    Canceling,
}

impl SaveProgressStage {
    fn label(self) -> String {
        match self {
            Self::Downloading {
                percent: Some(percent),
            } => format!("Downloading video… {percent}%"),
            Self::Downloading { percent: None } => "Downloading video…".to_owned(),
            Self::Finalizing => "Finalizing video and audio…".to_owned(),
            Self::Copying { fraction } => {
                format!("Copying to destination… {:.0}%", fraction * 100.0)
            }
            Self::Canceling => "Canceling save…".to_owned(),
        }
    }

    fn fraction(self) -> Option<f64> {
        match self {
            Self::Downloading {
                percent: Some(percent),
            } => Some(f64::from(percent) / 100.0),
            Self::Copying { fraction } => Some(fraction.clamp(0.0, 1.0)),
            Self::Downloading { percent: None } | Self::Finalizing | Self::Canceling => None,
        }
    }
}

struct SaveProgressDialog {
    dialog: gtk::Dialog,
    status: gtk::Label,
    progress: gtk::ProgressBar,
}

impl SaveProgressDialog {
    #[allow(deprecated)]
    fn new(parent: &gtk::ApplicationWindow, title: &str) -> Self {
        let dialog = gtk::Dialog::builder()
            .title("Save video")
            .transient_for(parent)
            .modal(false)
            .build();
        dialog.set_decorated(false);
        dialog.add_css_class("okp-command-dialog");
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);

        let content = dialog.content_area();
        content.set_spacing(10);
        content.set_margin_top(14);
        content.set_margin_end(14);
        content.set_margin_bottom(14);
        content.set_margin_start(14);
        content.append(&command_dialog_title("Save video"));

        let file = gtk::Label::new(Some(title));
        file.add_css_class("okp-info-label");
        file.set_xalign(0.0);
        file.set_wrap(true);
        file.set_max_width_chars(52);
        content.append(&file);

        let status = gtk::Label::new(None);
        status.add_css_class("okp-info-label");
        status.set_xalign(0.0);
        content.append(&status);

        let progress = gtk::ProgressBar::new();
        progress.set_show_text(false);
        content.append(&progress);

        let view = Self {
            dialog,
            status,
            progress,
        };
        view.update(SaveProgressStage::Downloading { percent: None });
        view
    }

    fn update(&self, stage: SaveProgressStage) {
        self.status.set_text(&stage.label());
        if let Some(fraction) = stage.fraction() {
            self.progress.set_fraction(fraction);
        } else {
            self.progress.pulse();
        }
    }

    fn set_cancel_sensitive(&self, sensitive: bool) {
        if let Some(button) = self.dialog.widget_for_response(gtk::ResponseType::Cancel) {
            button.set_sensitive(sensitive);
        }
    }

    fn present(&self) {
        self.dialog.present();
    }
}

pub(crate) fn open_save_destination_dialog(
    parent: &gtk::ApplicationWindow,
    snapshot: SaveVideoSnapshot,
    on_decision: impl FnOnce(SavePickerDecision) + 'static,
) {
    let extension = snapshot.media.required_extension().to_owned();
    let dialog = gtk::FileDialog::builder()
        .title("Save video")
        .accept_label("Save")
        .initial_name(snapshot.suggested_filename())
        .modal(true)
        .build();
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    let format_filter = gtk::FileFilter::new();
    format_filter.set_name(Some(&format!("{} video", extension.to_ascii_uppercase())));
    format_filter.add_pattern(&format!("*.{extension}"));
    filters.append(&format_filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&format_filter));

    dialog.save(
        Some(parent),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            on_decision(decide_after_picker(
                snapshot,
                native_save_dialog_outcome(result),
            ));
        },
    );
}

pub(crate) fn native_save_dialog_outcome(
    result: Result<gtk::gio::File, glib::Error>,
) -> SavePickerOutcome {
    match result {
        Ok(file) => match file.path() {
            Some(path) => SavePickerOutcome::Selected(path),
            None => SavePickerOutcome::Failed(
                "file dialog selection is not a local filesystem path".to_owned(),
            ),
        },
        Err(error) if native_file_dialog_was_cancelled(&error) => SavePickerOutcome::Cancelled,
        Err(error) => SavePickerOutcome::Failed(error.to_string()),
    }
}

#[derive(Debug)]
pub(crate) struct SavedVideoStore {
    path: PathBuf,
    index: SavedVideoIndex,
}

impl Default for SavedVideoStore {
    fn default() -> Self {
        Self::open_path(saved_video_index_path())
    }
}

impl SavedVideoStore {
    fn open_path(path: PathBuf) -> Self {
        let index = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| SavedVideoIndex::load(&raw))
            .unwrap_or_default();
        Self { path, index }
    }

    #[cfg(test)]
    fn open_test(path: PathBuf) -> Self {
        Self::open_path(path)
    }

    pub(crate) fn record_completed(
        &mut self,
        saved: &SavedVideo,
        saved_at_unix: i64,
        private_at_start: bool,
        private_at_completion: bool,
    ) -> io::Result<bool> {
        if !should_persist_saved_mapping(private_at_start, private_at_completion) {
            return Ok(false);
        }
        let previous = self.index.clone();
        self.index.record(saved, saved_at_unix);
        if let Err(error) = self.save() {
            self.index = previous;
            return Err(error);
        }
        Ok(true)
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(&self.index).map_err(io::Error::other)?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json)?;
        fs::rename(temporary, &self.path)
    }

    #[cfg(test)]
    fn saved_path(&self, original_url: &str) -> Option<&Path> {
        self.index.saved_path(original_url)
    }
}

fn saved_video_index_path() -> PathBuf {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(state_home).join("ok-player/saved-videos.json");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".local/state/ok-player/saved-videos.json");
    }
    PathBuf::from("ok-player-saved-videos.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_picker_distinguishes_local_selection_cancel_and_failure() {
        assert_eq!(
            native_save_dialog_outcome(Ok(gtk::gio::File::for_path("/videos/chosen.mkv"))),
            SavePickerOutcome::Selected(PathBuf::from("/videos/chosen.mkv"))
        );
        for error in [
            glib::Error::new(gtk::DialogError::Cancelled, "cancelled"),
            glib::Error::new(gtk::DialogError::Dismissed, "dismissed"),
            glib::Error::new(gtk::gio::IOErrorEnum::Cancelled, "cancelled"),
        ] {
            assert_eq!(
                native_save_dialog_outcome(Err(error)),
                SavePickerOutcome::Cancelled
            );
        }
        assert!(matches!(
            native_save_dialog_outcome(Err(glib::Error::new(
                gtk::gio::IOErrorEnum::Failed,
                "portal unavailable"
            ))),
            SavePickerOutcome::Failed(error) if error.contains("portal unavailable")
        ));
    }

    #[test]
    fn progress_copy_and_finalization_stages_have_honest_labels() {
        assert_eq!(
            SaveProgressStage::Downloading { percent: Some(37) }.label(),
            "Downloading video… 37%"
        );
        assert_eq!(
            SaveProgressStage::Finalizing.label(),
            "Finalizing video and audio…"
        );
        assert_eq!(
            SaveProgressStage::Copying { fraction: 0.625 }.label(),
            "Copying to destination… 62%"
        );
        assert_eq!(
            SaveProgressStage::Copying { fraction: 2.0 }.fraction(),
            Some(1.0)
        );
        assert_eq!(SaveProgressStage::Canceling.fraction(), None);
    }

    #[test]
    fn saved_video_store_round_trips_url_to_completed_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state/saved-videos.json");
        let url = "https://example.test/watch/794";
        let saved = SavedVideo {
            original_url: url.to_owned(),
            path: root.path().join("user/movie.mkv"),
            bytes: 42,
        };
        let mut store = SavedVideoStore::open_test(path.clone());

        assert!(
            store
                .record_completed(&saved, 1_700_000_794, false, false)
                .unwrap()
        );
        let reopened = SavedVideoStore::open_test(path);

        assert_eq!(reopened.saved_path(url), Some(saved.path.as_path()));
        assert_eq!(reopened.saved_path(saved.path.to_str().unwrap()), None);
    }

    #[test]
    fn failed_store_write_rolls_back_in_memory_mapping() {
        let root = tempfile::tempdir().unwrap();
        let unwritable = root.path().join("not-a-directory");
        fs::write(&unwritable, b"file").unwrap();
        let mut store = SavedVideoStore::open_test(unwritable.join("saved-videos.json"));
        let saved = SavedVideo {
            original_url: "https://example.test/watch/794".to_owned(),
            path: root.path().join("movie.mkv"),
            bytes: 42,
        };

        assert!(store.record_completed(&saved, 1, false, false).is_err());
        assert_eq!(store.saved_path(&saved.original_url), None);
    }

    #[test]
    fn explicit_private_save_keeps_the_file_but_writes_no_url_mapping() {
        let root = tempfile::tempdir().unwrap();
        let state_path = root.path().join("state/saved-videos.json");
        let output = root.path().join("chosen/movie.mkv");
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, b"completed public video").unwrap();
        let saved = SavedVideo {
            original_url: "https://example.test/watch/794".to_owned(),
            path: output.clone(),
            bytes: fs::metadata(&output).unwrap().len(),
        };
        let mut store = SavedVideoStore::open_test(state_path.clone());

        assert!(
            !store
                .record_completed(&saved, 1_700_000_794, true, false)
                .unwrap()
        );

        assert_eq!(fs::read(&output).unwrap(), b"completed public video");
        assert!(!state_path.exists());
        assert_eq!(store.saved_path(&saved.original_url), None);
    }

    #[test]
    fn private_toggle_during_save_suppresses_the_pending_url_mapping() {
        let root = tempfile::tempdir().unwrap();
        let state_path = root.path().join("state/saved-videos.json");
        let saved = SavedVideo {
            original_url: "https://example.test/watch/794".to_owned(),
            path: root.path().join("movie.mkv"),
            bytes: 42,
        };
        let mut store = SavedVideoStore::open_test(state_path.clone());

        assert!(
            !store
                .record_completed(&saved, 1_700_000_794, false, true)
                .unwrap()
        );

        assert!(!state_path.exists());
        assert_eq!(store.saved_path(&saved.original_url), None);
    }
}
