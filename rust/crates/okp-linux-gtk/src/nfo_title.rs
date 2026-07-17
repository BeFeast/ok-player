use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NfoTitleResult {
    pub(crate) source_generation: u64,
    pub(crate) media_path: PathBuf,
    pub(crate) title: Option<String>,
}

/// Background NFO sidecar jobs. The shared core owns discovery, bounds, decoding,
/// parsing, and title precedence; this shell seam only schedules blocking local I/O and
/// projects completed results into the currently loaded source.
#[derive(Debug)]
pub(crate) struct NfoTitleJobs {
    sender: mpsc::Sender<NfoTitleResult>,
    receiver: mpsc::Receiver<NfoTitleResult>,
}

impl Default for NfoTitleJobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }
}

impl NfoTitleJobs {
    pub(crate) fn resolve(&self, source_generation: u64, media_path: PathBuf) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let title =
                okp_core::nfo_metadata::read_sidecar(&media_path).map(|metadata| metadata.title);
            let _ = sender.send(NfoTitleResult {
                source_generation,
                media_path,
                title,
            });
        });
    }

    pub(crate) fn drain(&self) -> Vec<NfoTitleResult> {
        self.receiver.try_iter().collect()
    }

    #[cfg(test)]
    pub(crate) fn recv_timeout(&self, timeout: std::time::Duration) -> Option<NfoTitleResult> {
        self.receiver.recv_timeout(timeout).ok()
    }
}

/// Apply finished jobs only when both the source generation and local path still match.
/// A result for a file that was superseded by another file or a URL is ignored, so a
/// slow mount can never overwrite the new item's title.
pub(crate) fn apply_pending_nfo_titles(state: &Rc<RefCell<PlayerState>>) {
    let results = state.borrow().nfo_title_jobs.drain();
    if results.is_empty() {
        return;
    }

    let mut state = state.borrow_mut();
    for result in results {
        if nfo_result_matches(
            &result,
            state.source_generation,
            state.current_file.as_deref(),
        ) {
            state.current_nfo_title = okp_core::nfo_metadata::NfoTitleState::Resolved(result.title);
        }
    }
}

/// The current source title with portable precedence: NFO title, observed engine title,
/// then the same file/URL display name Linux already used before sidecar support.
pub(crate) fn current_media_title(state: &PlayerState) -> String {
    let engine_title = state
        .mpv
        .as_ref()
        .and_then(Mpv::observed_media_info)
        .map(|info| info.title);
    let fallback = current_source_display_name(state);
    okp_core::nfo_metadata::display_title(
        state.current_nfo_title.title(),
        engine_title.as_deref(),
        &fallback,
    )
}

pub(crate) fn playlist_item_title(state: &PlayerState, item: &PlaylistItem) -> String {
    if item.is_current(state.current_file.as_deref(), state.current_url.as_deref()) {
        current_media_title(state)
    } else {
        item.display_name()
    }
}

fn current_source_display_name(state: &PlayerState) -> String {
    state
        .current_file
        .as_ref()
        .map(|path| PlaylistItem::Local(path.clone()).display_name())
        .or_else(|| {
            state
                .current_url
                .as_ref()
                .map(|url| PlaylistItem::Url(url.clone()).display_name())
        })
        .unwrap_or_default()
}

pub(crate) fn nfo_result_matches(
    result: &NfoTitleResult,
    source_generation: u64,
    current_file: Option<&Path>,
) -> bool {
    result.source_generation == source_generation
        && current_file == Some(result.media_path.as_path())
}
