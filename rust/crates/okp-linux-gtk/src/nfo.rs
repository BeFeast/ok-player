use super::*;

#[derive(Debug)]
pub(crate) struct NfoTitleJobs {
    sender: mpsc::Sender<NfoTitleResult>,
    receiver: mpsc::Receiver<NfoTitleResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NfoTitleResult {
    pub(crate) source_generation: u64,
    pub(crate) path: PathBuf,
    pub(crate) title: Option<String>,
}

impl Default for NfoTitleJobs {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }
}

impl NfoTitleJobs {
    pub(crate) fn start(&self, source_generation: u64, path: PathBuf) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let title = resolve_local_nfo_title(&path);
            let _ = sender.send(NfoTitleResult {
                source_generation,
                path,
                title,
            });
        });
    }

    fn drain(&self) -> Vec<NfoTitleResult> {
        self.receiver.try_iter().collect()
    }
}

/// Apply completed local sidecar reads on the GTK thread. The generation and path
/// checks prevent a slow disk result from relabeling a newer playlist item.
pub(crate) fn drain_nfo_title_jobs(state: &Rc<RefCell<PlayerState>>) {
    let results = state.borrow().nfo_title_jobs.drain();
    if results.is_empty() {
        return;
    }

    let mut state = state.borrow_mut();
    for result in results {
        apply_nfo_title_result(&mut state, result);
    }
}

pub(crate) fn apply_nfo_title_result(state: &mut PlayerState, result: NfoTitleResult) -> bool {
    if result.source_generation != state.source_generation
        || state.current_file.as_ref() != Some(&result.path)
    {
        return false;
    }
    state.nfo_title = result.title;
    true
}

pub(crate) fn resolve_local_nfo_title(media_path: &Path) -> Option<String> {
    okp_core::nfo_metadata::resolve_with(media_path, read_bounded_nfo)
        .map(|metadata| metadata.title)
}

/// Effective title for live now-playing surfaces. A local NFO title outranks the
/// engine payload; URLs and files without a usable sidecar keep their existing
/// engine/source fallback behavior.
pub(crate) fn current_media_title(state: &PlayerState) -> String {
    state
        .nfo_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            state
                .mpv
                .as_ref()
                .and_then(Mpv::observed_media_info)
                .map(|info| info.title)
                .filter(|title| !title.trim().is_empty())
        })
        .or_else(|| {
            state
                .current_file
                .as_ref()
                .map(|path| PlaylistItem::Local(path.clone()).display_name())
        })
        .or_else(|| {
            state
                .current_url
                .as_ref()
                .map(|url| PlaylistItem::Url(url.clone()).display_name())
        })
        .unwrap_or_default()
}

/// Title persisted for the local recent/history record. The sidecar title is cached
/// when present; otherwise the existing filename-without-extension behavior is written
/// so removing a sidecar cannot leave an obsolete curated title behind.
pub(crate) fn current_history_title(state: &PlayerState) -> Option<String> {
    let path = state.current_file.as_ref()?;
    state
        .nfo_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .filter(|title| !title.is_empty())
        })
}

fn read_bounded_nfo(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 || len > okp_core::nfo_metadata::MAX_NFO_BYTES as u64 {
        return None;
    }

    let mut bytes = Vec::with_capacity(len as usize);
    file.by_ref()
        .take(okp_core::nfo_metadata::MAX_NFO_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= okp_core::nfo_metadata::MAX_NFO_BYTES).then_some(bytes)
}
