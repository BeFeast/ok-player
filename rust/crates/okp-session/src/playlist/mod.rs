//! The session playlist: what plays next, and the commands that edit it.
//!
//! The model is okp-core's [`Playlist`] (repeat, shuffle, queue insertion,
//! moves, removal, wrap rules); this module is the orchestration the GTK shell
//! keeps in `playlist_ops.rs`, lifted so that every shell gets the same
//! behaviour from the session:
//!
//! - opening one local file plays it at once, alone in Up Next, while its
//!   folder is listed off the caller's thread; the folder's media fill in, in
//!   natural order, when the listing finishes, unless a newer open has
//!   superseded it (#870);
//! - opening several sources plays them as a list in the given order, folders
//!   expanded; opening an M3U file plays its entries. Until they are read the
//!   old playlist stays: picking an item from it drops the read, and what is
//!   queued meanwhile follows the opened items;
//! - previous and next wrap at both ends, like the transport buttons;
//! - the end of a file advances per the repeat mode and restarts the item on
//!   repeat-one; while a folder or playlist file is being read, it waits for
//!   the read, so that the advance sees the playlist the read produces.
//!
//! The module decides and the session carries out: a step that plays another
//! item comes back as a [`PlannedOpen`], which the session opens through the
//! same validated path as an `open` command and then reports back with
//! [`SessionPlaylist::opened`].

pub(crate) mod scan;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use okp_core::media_formats;
use okp_core::playlist::{Playlist, PlaylistItem, QueueInsertMode, RepeatMode as CoreRepeatMode};
use serde::{Deserialize, Serialize};

use crate::protocol::{Command, DispatchResult, MediaSource, RejectReason};
use scan::{Classified, Filesystem, Finished, HostFilesystem, Inbox, Job, Notify, Outcome};

pub use scan::FOLDER_SCAN_LIMIT;

/// A playlist command, carried as `{"command":{"playlist":{"action":{...}}}}`.
/// Indexes count from 0 in the order of [`PlaylistState::items`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaylistAction {
    /// Open files, folders, M3U playlists or URLs, replacing the playlist. One
    /// local media file plays at once with its folder as the playlist; several
    /// sources play in the given order, folders expanded in natural order. An
    /// M3U file counts only when nothing else is media.
    Open {
        sources: Vec<MediaSource>,
    },
    /// The next item in play order, wrapping at the end.
    Next {},
    /// The previous item in play order, wrapping at the start.
    Previous {},
    /// Play the item at `index`.
    Jump {
        index: usize,
    },
    /// Add sources around the current item. Only media files and plain
    /// `http`/`https` URLs are queued; the current item and, when appending,
    /// items already queued are skipped.
    Queue {
        sources: Vec<MediaSource>,
        mode: QueueMode,
    },
    /// Move the item at `index` to play right after the current one.
    PlayNext {
        index: usize,
    },
    /// Move the item at `from` to `to` (a position after its removal).
    Move {
        from: usize,
        to: usize,
    },
    /// Remove the item at `index`; never the current item or the last one.
    Remove {
        index: usize,
    },
    /// Remove every item except the current one.
    ClearQueue {},
    SetRepeat {
        mode: RepeatMode,
    },
    /// Shuffle the play order (the current item stays first). A `seed` makes
    /// the order reproducible; without one the session's own seed is used.
    SetShuffle {
        enabled: bool,
        #[serde(default)]
        seed: Option<u64>,
    },
}

/// What happens when an item ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    /// Stop after the last item.
    Off,
    /// Play the current item again.
    One,
    /// Go back to the first item after the last.
    All,
}

/// Where queued sources go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueMode {
    /// After the last item.
    Append,
    /// Right after the current item, moving sources already queued there.
    PlayNext,
}

/// The playlist a shell renders: the items in their identity order (natural
/// order for a folder, document order for an M3U, the given order for a
/// selection), the current one, and the play modes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistState {
    pub items: Vec<PlaylistEntry>,
    pub current_index: Option<usize>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    /// A folder is being listed, or an open's folders and playlist files are
    /// being read; the items change when it finishes.
    pub scanning: bool,
}

/// One item of the playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistEntry {
    pub source: MediaSource,
    /// A short label: the file name, or the URL's last segment.
    pub title: String,
}

impl From<CoreRepeatMode> for RepeatMode {
    fn from(mode: CoreRepeatMode) -> Self {
        match mode {
            CoreRepeatMode::Off => Self::Off,
            CoreRepeatMode::One => Self::One,
            CoreRepeatMode::All => Self::All,
        }
    }
}

impl From<RepeatMode> for CoreRepeatMode {
    fn from(mode: RepeatMode) -> Self {
        match mode {
            RepeatMode::Off => Self::Off,
            RepeatMode::One => Self::One,
            RepeatMode::All => Self::All,
        }
    }
}

/// An item the session should open, and what the playlist does once it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedOpen {
    item: PlaylistItem,
    effect: Effect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Effect {
    /// A step within the playlist: the cursor moves to this index.
    Cursor(usize),
    /// A new local file: it plays while its folder is listed, followed only by
    /// what was `queued` while the open that found it was being read.
    Folder { queued: Vec<PlaylistItem> },
    /// A new list, starting with its first item.
    List(Vec<PlaylistItem>),
}

impl PlannedOpen {
    /// The `open` command that plays the item.
    pub(crate) fn command(&self) -> Command {
        Command::Open {
            source: MediaSource::from_item(&self.item),
            resume_from: None,
            audio_track: None,
            subtitle_track: None,
        }
    }
}

/// How the session answers a playlist command.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Reply(DispatchResult),
    Open(PlannedOpen),
}

/// Work that falls due on the session's event thread.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Background {
    Open(PlannedOpen),
    /// An open's folders and playlist files held nothing playable.
    NothingPlayable,
}

/// The job in flight: a folder listing or an open's resolution.
struct ActiveJob {
    id: u64,
    cancel: Arc<AtomicBool>,
    kind: JobKind,
}

enum JobKind {
    /// The current local file's folder is being listed; the current playlist
    /// stays and fills in around it.
    Folder,
    /// An open's folders and playlist files are being read; the playlist they
    /// replace stays until then. `queued` is what was queued meanwhile, which
    /// follows the opened items.
    Resolve { queued: Vec<PlaylistItem> },
}

/// The session's playlist, its folder jobs and its change tracking.
pub(crate) struct SessionPlaylist {
    list: Playlist,
    active: Option<ActiveJob>,
    next_job: u64,
    /// The current item reached its end; what plays next is decided as soon
    /// as no folder or playlist file is being read.
    advance_due: bool,
    /// Bumped on every change a shell can see.
    revision: u64,
    published: u64,
    fs: Arc<dyn Filesystem>,
    inbox: Inbox,
    notify: Notify,
}

impl SessionPlaylist {
    /// An empty playlist whose jobs call `notify` when they finish.
    pub(crate) fn new(notify: Notify) -> Self {
        let mut list = Playlist::default();
        list.reseed(clock_seed());
        Self {
            list,
            active: None,
            next_job: 0,
            advance_due: false,
            revision: 0,
            published: 0,
            fs: Arc::new(HostFilesystem),
            inbox: Arc::new(Mutex::new(Vec::new())),
            notify,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_filesystem(&mut self, fs: Arc<dyn Filesystem>) {
        self.fs = fs;
    }

    pub(crate) fn state(&self) -> PlaylistState {
        PlaylistState {
            items: self
                .list
                .items()
                .iter()
                .map(|item| PlaylistEntry {
                    source: MediaSource::from_item(item),
                    title: item.display_name(),
                })
                .collect(),
            current_index: self.list.current_index(),
            repeat: self.list.repeat().into(),
            shuffle: self.list.shuffle(),
            scanning: self.active.is_some(),
        }
    }

    /// The state, if it changed since the last call.
    pub(crate) fn take_changed(&mut self) -> Option<PlaylistState> {
        (self.revision != self.published).then(|| {
            self.published = self.revision;
            self.state()
        })
    }

    /// Decide a playlist command. `current` is the media the session has open.
    pub(crate) fn dispatch(
        &mut self,
        action: &PlaylistAction,
        current: Option<&PlaylistItem>,
    ) -> Step {
        match action {
            PlaylistAction::Open { sources } => self.plan_open(sources),
            PlaylistAction::Next {} => self.plan_step(1),
            PlaylistAction::Previous {} => self.plan_step(-1),
            PlaylistAction::Jump { index } => match self.list.get(*index) {
                Some(item) => Step::Open(PlannedOpen {
                    item: item.clone(),
                    effect: Effect::Cursor(*index),
                }),
                None => Step::Reply(DispatchResult::NoOp {}),
            },
            PlaylistAction::Queue { sources, mode } => {
                Step::Reply(self.queue(sources, *mode, current))
            }
            PlaylistAction::PlayNext { index } => {
                let changed = self.list.play_next(*index);
                Step::Reply(self.edited(changed))
            }
            PlaylistAction::Move { from, to } => {
                let changed = self.list.reorder(*from, *to);
                Step::Reply(self.edited(changed))
            }
            PlaylistAction::Remove { index } => {
                let changed = self.list.remove(*index);
                Step::Reply(self.edited(changed))
            }
            PlaylistAction::ClearQueue {} => {
                let changed = self.list.clear_queue();
                Step::Reply(self.edited(changed))
            }
            PlaylistAction::SetRepeat { mode } => {
                let mode = CoreRepeatMode::from(*mode);
                let changed = self.list.repeat() != mode;
                self.list.set_repeat(mode);
                Step::Reply(self.edited(changed))
            }
            PlaylistAction::SetShuffle { enabled, seed } => {
                let Some(seed) = seed else {
                    let changed = self.list.shuffle() != *enabled;
                    self.list.set_shuffle(*enabled);
                    return Step::Reply(self.edited(changed));
                };
                self.list.reseed(*seed);
                // Rebuild the order from the new seed even if shuffle was on.
                self.list.set_shuffle(false);
                self.list.set_shuffle(*enabled);
                Step::Reply(self.edited(true))
            }
        }
    }

    /// Follow a command the session forwarded to the engine: an open replaces
    /// the playlist, a close empties it.
    pub(crate) fn follow(&mut self, command: &Command, result: &DispatchResult) {
        if matches!(result, DispatchResult::Rejected { .. }) {
            return;
        }
        match command {
            Command::Open { source, .. } => {
                let item = source.to_item();
                let effect = match &item {
                    PlaylistItem::Local(path) if media_formats::is_media(path) => {
                        Effect::Folder { queued: Vec::new() }
                    }
                    _ => Effect::List(vec![item.clone()]),
                };
                self.opened(PlannedOpen { item, effect }, result);
            }
            Command::Close {} => {
                self.supersede();
                self.list.clear();
                self.advance_due = false;
                self.touch();
            }
            _ => {}
        }
    }

    /// Record how the session's open of `open` went. An open the core state
    /// machine accepted owns the playlist whether or not the engine took it;
    /// only an accepted local file has its folder listed. Whatever ended
    /// before is followed by this open, not by the auto-advance.
    pub(crate) fn opened(&mut self, open: PlannedOpen, result: &DispatchResult) {
        let accepted = match result {
            DispatchResult::Accepted { .. } => true,
            DispatchResult::Failed { .. } => false,
            // An open is answered with none of the others.
            DispatchResult::NoOp {}
            | DispatchResult::Rejected { .. }
            | DispatchResult::Applied {}
            | DispatchResult::Perform {}
            | DispatchResult::History { .. } => return,
        };
        self.advance_due = false;
        match open.effect {
            Effect::Cursor(index) => {
                // An item picked from the playlist an open is still being read
                // to replace: the pick is newer, so the read is dropped.
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| matches!(active.kind, JobKind::Resolve { .. }))
                {
                    self.supersede();
                }
                self.list.set_current_index(index);
            }
            Effect::Folder { queued } => {
                self.supersede();
                let mut items = vec![open.item.clone()];
                for item in queued {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
                self.list.reset(items, &open.item);
                if accepted && let PlaylistItem::Local(path) = &open.item {
                    self.start(Job::Folder(path.clone()));
                }
            }
            Effect::List(items) => {
                self.supersede();
                self.list.reset(items, &open.item);
            }
        }
        self.touch();
    }

    /// The current item reached its end of file.
    pub(crate) fn end_of_file(&mut self) {
        self.advance_due = true;
    }

    /// Fold in finished jobs and decide what plays after an end of file.
    /// `ended` is the media the session is at the end of, if it is.
    pub(crate) fn poll(&mut self, ended: Option<&PlaylistItem>) -> Vec<Background> {
        let finished = std::mem::take(&mut *self.inbox.lock().unwrap_or_else(|p| p.into_inner()));
        let mut steps = Vec::new();
        for Finished { job, outcome } in finished {
            // A job superseded by a newer open is no longer the active one.
            let Some(active) = self.active.take_if(|active| active.id == job) else {
                continue;
            };
            self.touch();
            match outcome {
                Outcome::Folder {
                    file,
                    siblings: Some(siblings),
                } => self.merge_folder(&file, siblings),
                Outcome::Folder { siblings: None, .. } => {}
                Outcome::Resolved(resolution) => {
                    let queued = match active.kind {
                        JobKind::Resolve { queued } => queued,
                        JobKind::Folder => Vec::new(),
                    };
                    steps.push(self.resolved(resolution, &queued));
                }
            }
        }
        // An end of file waits for a read in flight: the rest of the folder,
        // or the open being read, decides what plays next.
        if !self.advance_due || self.active.is_some() {
            return steps;
        }
        self.advance_due = false;
        // A newer open of the user's wins over advancing the old playlist, and
        // nothing advances once the session has left the item that ended.
        if !steps.iter().any(|step| matches!(step, Background::Open(_)))
            && ended.is_some_and(|ended| self.list.current() == Some(ended))
            && let Some(index) = self.list.auto_advance_target_index()
            && let Some(item) = self.list.get(index)
        {
            steps.push(Background::Open(PlannedOpen {
                item: item.clone(),
                effect: Effect::Cursor(index),
            }));
        }
        steps
    }

    /// What a finished open's read plays: its items, then whatever of
    /// `queued` is still in the playlist, in playlist order.
    fn resolved(&self, resolution: scan::Resolution, queued: &[PlaylistItem]) -> Background {
        let queued = self
            .list
            .items()
            .iter()
            .filter(|item| queued.contains(item))
            .cloned();
        match resolution {
            scan::Resolution::Folder(path) => Background::Open(PlannedOpen {
                item: PlaylistItem::Local(path),
                effect: Effect::Folder {
                    queued: queued.collect(),
                },
            }),
            scan::Resolution::List(mut items) => {
                for item in queued {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
                Background::Open(PlannedOpen {
                    item: items[0].clone(),
                    effect: Effect::List(items),
                })
            }
            scan::Resolution::Nothing => Background::NothingPlayable,
        }
    }

    fn plan_open(&mut self, sources: &[MediaSource]) -> Step {
        let items: Vec<PlaylistItem> = sources.iter().map(MediaSource::to_item).collect();
        match scan::classify(&items) {
            Classified::Resolved(scan::Resolution::Nothing) => Step::Reply(DispatchResult::NoOp {}),
            Classified::Resolved(scan::Resolution::Folder(path)) => Step::Open(PlannedOpen {
                item: PlaylistItem::Local(path),
                effect: Effect::Folder { queued: Vec::new() },
            }),
            Classified::Resolved(scan::Resolution::List(items)) => Step::Open(PlannedOpen {
                item: items[0].clone(),
                effect: Effect::List(items),
            }),
            Classified::NeedsFilesystem => {
                self.supersede();
                self.start(Job::Resolve(items));
                self.touch();
                Step::Reply(DispatchResult::Applied {})
            }
        }
    }

    /// The transport buttons: one step in play order, wrapping at both ends
    /// whatever the repeat mode.
    fn plan_step(&self, direction: isize) -> Step {
        match self
            .list
            .peek_wrapping_index(direction)
            .and_then(|index| Some((index, self.list.get(index)?.clone())))
        {
            Some((index, item)) => Step::Open(PlannedOpen {
                item,
                effect: Effect::Cursor(index),
            }),
            None => Step::Reply(DispatchResult::NoOp {}),
        }
    }

    /// The GTK shell's `queue_playlist_items`: only queueable sources, each
    /// once, around the current media.
    fn queue(
        &mut self,
        sources: &[MediaSource],
        mode: QueueMode,
        current: Option<&PlaylistItem>,
    ) -> DispatchResult {
        let mut additions = Vec::new();
        for item in sources.iter().map(MediaSource::to_item) {
            if item.is_queueable() && !additions.contains(&item) {
                additions.push(item);
            }
        }
        if additions.is_empty() {
            return DispatchResult::NoOp {};
        }
        let Some(current) = current else {
            return DispatchResult::Rejected {
                reason: RejectReason::NoActiveMedia,
                message: RejectReason::NoActiveMedia.describe().to_owned(),
            };
        };
        let (current_file, current_url) = match current {
            PlaylistItem::Local(path) => (Some(path.as_path()), None),
            PlaylistItem::Url(url) => (None, Some(url.as_str())),
        };
        let mode = match mode {
            QueueMode::Append => QueueInsertMode::Append,
            QueueMode::PlayNext => QueueInsertMode::PlayNext,
        };
        let inserted = self
            .list
            .queue_insert(current_file, current_url, additions.clone(), mode);
        // While an open is read, what is queued is kept for the opened list.
        if inserted.is_some()
            && let Some(ActiveJob {
                kind: JobKind::Resolve { queued },
                ..
            }) = &mut self.active
        {
            for item in additions {
                if item != *current && !queued.contains(&item) {
                    queued.push(item);
                }
            }
        }
        self.edited(inserted.is_some())
    }

    /// Fill in a finished folder listing: the folder's media in natural order,
    /// then anything queued while it was being listed.
    fn merge_folder(&mut self, file: &Path, siblings: Vec<PathBuf>) {
        let mut items = scan::folder_playlist(file, siblings);
        for item in self.list.items() {
            if !items.contains(item) {
                items.push(item.clone());
            }
        }
        let current = self
            .list
            .current()
            .cloned()
            .unwrap_or_else(|| PlaylistItem::Local(file.to_path_buf()));
        self.list.reset(items, &current);
    }

    fn edited(&mut self, changed: bool) -> DispatchResult {
        if changed {
            self.touch();
            DispatchResult::Applied {}
        } else {
            DispatchResult::NoOp {}
        }
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Start `job` as the one in flight.
    fn start(&mut self, job: Job) {
        self.next_job += 1;
        let kind = match job {
            Job::Folder(_) => JobKind::Folder,
            Job::Resolve(_) => JobKind::Resolve { queued: Vec::new() },
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let started = scan::spawn(
            Arc::clone(&self.fs),
            job,
            self.next_job,
            Arc::clone(&cancel),
            Arc::clone(&self.inbox),
            Arc::clone(&self.notify),
        );
        if started {
            self.active = Some(ActiveJob {
                id: self.next_job,
                cancel,
                kind,
            });
        }
    }

    /// Stop the job in flight; whatever it reports is ignored.
    fn supersede(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.store(true, Ordering::Release);
        }
    }
}

/// Entropy for shuffle when a shell gives no seed (the GTK shell's
/// `shuffle_seed`).
fn clock_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

#[cfg(test)]
mod tests;
