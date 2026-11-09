//! Watch history for a session: resume on open, progress saving, per-file
//! memory and the private session, kept in the [`HistoryStore`] file under the
//! shell's data directory.
//!
//! Lifted from the GTK shell's orchestration (`playlist_ops.rs`: resume on open,
//! per-file preferences, progress and finishing; `playback.rs`: the private
//! session), so a shell over the session does no history file IO and holds no
//! history rules of its own:
//!
//! - An open that names no start position starts where the core resume rule
//!   allows (past the first 5%, before the last 30 s or 5%, unfinished), and a
//!   `resumed` event follows once the media has loaded.
//! - Progress is saved every [`PROGRESS_SAVE_INTERVAL`] while playing, and on
//!   pause, close, opening the next media, the session closing and the engine
//!   quitting. Playing to the end marks the entry finished.
//! - The audio and subtitle track, subtitle delay, volume and speed a shell sets
//!   are remembered for that media and applied when it is opened again. They
//!   are written with the next save, or [`PROGRESS_SAVE_INTERVAL`] after the
//!   first unsaved one, so a slider sending many values writes once.
//! - While the private session is on, nothing new is recorded and nothing
//!   remembered is applied; the history stays readable.
//!
//! The file is written on a thread of its own (`writer.rs`); the session lock
//! is held only to serialise the document.

mod store;
#[cfg(test)]
mod tests;
mod wire;
mod writer;

pub use store::{HISTORY_FILE_NAME, HistoryStore};
pub use wire::{
    HistoryAction, HistoryActionResult, HistoryBucket, HistoryEntry, HistoryEvent, HistoryQuery,
    HistoryQueryResult, HistorySection, HistoryState,
};

use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use okp_core::history::{HistoryOpenIntent, Preferences};
use okp_core::history_format::{self, LocalDateTime};
use okp_core::nfo_metadata::HistoryTitleUpdate;
use okp_core::player::{
    EndReason, PlaybackStatus, PlayerMachine, TrackInfo, TrackKind as CoreTrackKind,
};
use okp_core::playlist::PlaylistItem;
use okp_core::recents_shelf::{HistoryItem, WelcomeShelf};
use okp_core::settings::HistoryRetention;

use crate::coalesce::Coalescer;
use crate::engine::Engine;
use crate::protocol::{Command, ErrorKind, Event, MediaSource, TrackKind};
pub(crate) use writer::Waker;
use writer::Writer;

/// How often progress is saved while media plays: the GTK shell's cadence.
pub const PROGRESS_SAVE_INTERVAL: Duration = Duration::from_secs(10);

/// The history side of one session.
pub(crate) struct SessionHistory {
    store: HistoryStore,
    /// Writes the history file; `None` keeps the history in memory only.
    writer: Option<Writer>,
    private_session: bool,
    current: Option<Current>,
    /// The remembered position [`SessionHistory::prepare`] gave the open being
    /// dispatched.
    prepared_resume: Option<f64>,
    /// The last save failed and has been reported.
    save_failing: bool,
    /// When the oldest change still waiting for a save was recorded.
    unsaved_since: Option<Instant>,
}

/// What the engine's latest report means for the media the history follows.
pub(crate) struct Report<'a> {
    /// The current media has just finished loading.
    pub(crate) loaded: bool,
    /// The end of file that ended the current media carried an error.
    pub(crate) end_failed: bool,
    /// The engine's title for the current media.
    pub(crate) title: Option<&'a str>,
}

/// The media the history follows: the one the last accepted open named.
struct Current {
    source: PlaylistItem,
    /// The engine has loaded it.
    loaded: bool,
    /// The remembered position it was opened at, announced once it has loaded.
    resumed_at: Option<f64>,
    /// Remembered tracks and subtitle delay, applied once it has loaded.
    file_choices: Option<Preferences>,
    /// The last position and duration seen while it played.
    sample: Option<(f64, f64)>,
    /// The engine's title for it.
    title: Option<String>,
    last_saved: Instant,
    /// Its end has been recorded; nothing more is saved for it.
    ended: bool,
}

impl Current {
    /// The title a progress write carries: the engine's title for a URL; a
    /// local file keeps whatever the history already holds.
    fn title_update(&self) -> HistoryTitleUpdate {
        match (&self.source, &self.title) {
            (PlaylistItem::Url(url), Some(title)) if !title.trim().is_empty() && title != url => {
                HistoryTitleUpdate::Set(title.clone())
            }
            _ => HistoryTitleUpdate::Preserve,
        }
    }
}

impl SessionHistory {
    /// The history kept in `data_dir`.
    pub(crate) fn open(data_dir: &Path) -> Self {
        Self::with_store(HistoryStore::open(data_dir))
    }

    pub(crate) fn with_store(mut store: HistoryStore) -> Self {
        let writer = store.take_file().map(Writer::start);
        Self {
            store,
            writer,
            private_session: false,
            current: None,
            prepared_resume: None,
            save_failing: false,
            unsaved_since: None,
        }
    }

    /// Have `waker` called after each write of the history file, so the
    /// session collects the outcome without waiting for other news.
    pub(crate) fn set_waker(&self, waker: Waker) {
        if let Some(writer) = &self.writer {
            writer.set_waker(waker);
        }
    }

    /// When the next timed save falls due, if a change is waiting for one.
    pub(crate) fn next_save(&self) -> Option<Instant> {
        self.unsaved_since
            .map(|since| since + PROGRESS_SAVE_INTERVAL)
    }

    /// The command to validate and forward in place of `command`: an open that
    /// names no start position starts at the remembered one, if the resume rule
    /// allows it and the private session is off.
    ///
    /// An open first saves the progress of the media it replaces, so reopening
    /// the media that is playing resumes where it is now.
    pub(crate) fn prepare<'c>(
        &mut self,
        command: &'c Command,
        queue: &mut Coalescer,
    ) -> Cow<'c, Command> {
        self.prepared_resume = None;
        if matches!(command, Command::Open { .. }) {
            self.save_progress(queue, Instant::now());
        }
        let Command::Open {
            source,
            resume_from: None,
            audio_track,
            subtitle_track,
        } = command
        else {
            return Cow::Borrowed(command);
        };
        if self.private_session {
            return Cow::Borrowed(command);
        }
        let Some(position) = self.store.resume_position_for_source(&source.to_item()) else {
            return Cow::Borrowed(command);
        };
        self.prepared_resume = Some(position);
        Cow::Owned(Command::Open {
            source: source.clone(),
            resume_from: Some(position),
            audio_track: *audio_track,
            subtitle_track: *subtitle_track,
        })
    }

    /// Follow up a command the state machine accepted. `forwarded` says whether
    /// the engine took it.
    pub(crate) fn dispatched(
        &mut self,
        command: &Command,
        forwarded: bool,
        machine: &PlayerMachine,
        engine: &dyn Engine,
        queue: &mut Coalescer,
    ) {
        let resumed_at = self.prepared_resume.take();
        let now = Instant::now();
        let choice = match command {
            Command::Open { source, .. } => {
                // `prepare` saved the progress of the media this replaces.
                self.current = None;
                if forwarded {
                    self.opened(source.to_item(), resumed_at, engine, queue, now);
                }
                return;
            }
            Command::Close {} => {
                self.save_progress(queue, now);
                self.current = None;
                return;
            }
            Command::SetPaused { .. } | Command::TogglePause {} => {
                if machine.status() == PlaybackStatus::Paused {
                    self.save_progress(queue, now);
                }
                return;
            }
            Command::SetVolume { volume } => Preferences {
                volume: Some(*volume),
                ..Preferences::default()
            },
            Command::SetSpeed { speed } => Preferences {
                speed: Some(*speed),
                ..Preferences::default()
            },
            Command::SelectTrack {
                kind: TrackKind::Audio,
                id,
            } => Preferences {
                audio_enabled: Some(id.is_some()),
                audio_track_id: *id,
                ..Preferences::default()
            },
            Command::SelectTrack {
                kind: TrackKind::Subtitle,
                id,
            } => Preferences {
                subtitle_enabled: Some(id.is_some()),
                subtitle_track_id: *id,
                ..Preferences::default()
            },
            Command::SetSubtitleDelay { seconds } => Preferences {
                subtitle_delay: Some(*seconds),
                ..Preferences::default()
            },
            // An invoked registry command arrives here as the command it stands
            // for, and a playlist item as the `open` that plays it.
            Command::Seek { .. }
            | Command::SetMute { .. }
            | Command::History { .. }
            | Command::InvokeCommand { .. }
            | Command::Playlist { .. } => return,
        };
        // The value the shell just set, not the engine's next report of it,
        // which may still hold the previous value. It is written with the next
        // save, not now: a slider or a held key sends many.
        if forwarded && let Some(current) = &self.current {
            self.store
                .record_source_preferences(&current.source, choice, self.private_session);
            if self.store.is_dirty() {
                self.unsaved_since.get_or_insert(now);
            }
        }
    }

    fn opened(
        &mut self,
        source: PlaylistItem,
        resumed_at: Option<f64>,
        engine: &dyn Engine,
        queue: &mut Coalescer,
        now: Instant,
    ) {
        self.store
            .begin_source_open(&source, HistoryOpenIntent::Explicit);
        let remembered = if self.private_session {
            None
        } else {
            self.store.playback_preferences_for_source(&source)
        };
        if let Some(remembered) = &remembered {
            // Volume and speed are engine-global, so they can be set before the
            // media loads and it starts with them.
            if let Some(volume) = remembered.volume.filter(|volume| volume.is_finite()) {
                forward(engine, &Command::SetVolume { volume }, queue);
            }
            if let Some(speed) = remembered.speed.filter(|speed| speed.is_finite()) {
                forward(engine, &Command::SetSpeed { speed }, queue);
            }
        }
        self.current = Some(Current {
            source,
            loaded: false,
            resumed_at,
            file_choices: remembered,
            sample: None,
            title: None,
            last_saved: now,
            ended: false,
        });
    }

    /// Fold in the session state after the engine's latest report, collect
    /// what the writer did, and save if a change has waited long enough.
    pub(crate) fn refresh(
        &mut self,
        report: Report<'_>,
        machine: &PlayerMachine,
        engine: &dyn Engine,
        queue: &mut Coalescer,
        now: Instant,
    ) {
        self.collect(queue);
        self.follow_current(report, machine, engine, queue, now);
        if self.next_save().is_some_and(|due| now >= due) {
            self.flush(queue);
        }
    }

    fn follow_current(
        &mut self,
        report: Report<'_>,
        machine: &PlayerMachine,
        engine: &dyn Engine,
        queue: &mut Coalescer,
        now: Instant,
    ) {
        let Report {
            loaded,
            end_failed,
            title,
        } = report;
        let private_session = self.private_session;
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let snapshot = machine.snapshot();
        if loaded && !current.loaded {
            current.loaded = true;
            current.last_saved = now;
            if let Some(choices) = current.file_choices.take() {
                apply_file_choices(&choices, &snapshot.tracks, engine, queue);
            }
            if let Some(position) = current.resumed_at.take() {
                queue.push(Event::History {
                    event: HistoryEvent::Resumed { position },
                });
            }
            current.title = title.map(str::to_owned);
            if matches!(current.source, PlaylistItem::Url(_)) {
                // A live stream may never report a duration, so a URL is recorded
                // as soon as it opens rather than at its first progress save.
                let update = current.title_update();
                self.store.record_source_opened(
                    &current.source,
                    snapshot.duration,
                    private_session,
                    update,
                );
                self.flush(queue);
            }
            return;
        }
        if !current.loaded || current.ended {
            return;
        }
        if title.is_some() {
            current.title = title.map(str::to_owned);
        }
        match snapshot.status {
            PlaybackStatus::Playing | PlaybackStatus::Paused => {
                if let (Some(position), Some(duration)) = (snapshot.time_pos, snapshot.duration)
                    && position.is_finite()
                    && duration.is_finite()
                    && duration > 0.0
                {
                    current.sample = Some((position, duration));
                }
                if snapshot.status == PlaybackStatus::Playing
                    && now >= current.last_saved + PROGRESS_SAVE_INTERVAL
                {
                    self.save_progress(queue, now);
                }
            }
            PlaybackStatus::Ended => {
                // Judged by the end of file itself: the machine's last error may
                // be any earlier failure, such as a refused seek.
                let watched_to_the_end = snapshot.end_reason == Some(EndReason::Eof) && !end_failed;
                if watched_to_the_end {
                    current.ended = true;
                    let source = current.source.clone();
                    self.store.mark_source_finished(&source, private_session);
                    self.flush(queue);
                } else {
                    self.save_progress(queue, now);
                    if let Some(current) = self.current.as_mut() {
                        current.ended = true;
                    }
                }
            }
            PlaybackStatus::Idle | PlaybackStatus::Opening => {}
        }
    }

    /// Save the current media's progress and everything else unsaved because
    /// the session is closing, and wait until the file is written.
    pub(crate) fn quit(&mut self, queue: &mut Coalescer) {
        self.save_progress(queue, Instant::now());
        if let Some(writer) = &self.writer {
            writer.wait_idle();
        }
    }

    /// Record where the current media is, if it has loaded and has not ended,
    /// and save everything unsaved.
    fn save_progress(&mut self, queue: &mut Coalescer, now: Instant) {
        if let Some(current) = self.current.as_mut() {
            current.last_saved = now;
            if current.loaded && !current.ended {
                let title = current.title_update();
                match current.sample {
                    Some((position, duration)) => self.store.record_source_with_title(
                        &current.source,
                        position.clamp(0.0, duration),
                        duration,
                        false,
                        self.private_session,
                        title,
                    ),
                    None if matches!(current.source, PlaylistItem::Url(_)) => self
                        .store
                        .record_source_opened(&current.source, None, self.private_session, title),
                    None => {}
                }
            }
        }
        self.flush(queue);
    }

    /// Hand what changed to the writer and tell the shell. Only the document is
    /// made here; the writer thread writes it.
    fn flush(&mut self, queue: &mut Coalescer) {
        self.unsaved_since = None;
        if !self.store.is_dirty() {
            return;
        }
        queue.push_once(Event::History {
            event: HistoryEvent::Changed {},
        });
        match self.store.take_unsaved() {
            Ok(Some(document)) => {
                if let Some(writer) = &self.writer {
                    writer.submit(document);
                }
            }
            Ok(None) => {}
            Err(error) => self.save_result(Err(error), queue),
        }
    }

    /// Fold in what the writer has done since the last look.
    fn collect(&mut self, queue: &mut Coalescer) {
        let Some(outcome) = self.writer.as_ref().and_then(Writer::take_outcome) else {
            return;
        };
        if outcome.moved_aside {
            // The page reports where the unreadable file went.
            queue.push_once(Event::History {
                event: HistoryEvent::Changed {},
            });
        }
        self.save_result(outcome.result, queue);
    }

    /// Report a failed save once until one succeeds. The history is kept, and
    /// written again with the next save.
    fn save_result(&mut self, result: io::Result<()>, queue: &mut Coalescer) {
        match result {
            Ok(()) => self.save_failing = false,
            Err(error) => {
                self.store.mark_unsaved();
                self.unsaved_since.get_or_insert_with(Instant::now);
                if !std::mem::replace(&mut self.save_failing, true) {
                    queue.push(Event::History {
                        event: HistoryEvent::SaveFailed {
                            message: format!("the history could not be saved: {error}"),
                        },
                    });
                }
            }
        }
    }

    /// Where the file that could not be read was moved, once it has been.
    fn unreadable_copy(&self) -> Option<PathBuf> {
        match &self.writer {
            Some(writer) => writer.kept(),
            None => self.store.unreadable_copy().map(Path::to_path_buf),
        }
    }

    #[cfg(test)]
    pub(crate) fn probe(&self) -> Option<writer::Probe> {
        self.writer.as_ref().map(Writer::probe)
    }

    /// Carry out a history action.
    pub(crate) fn act(
        &mut self,
        action: &HistoryAction,
        queue: &mut Coalescer,
    ) -> HistoryActionResult {
        match action {
            HistoryAction::SetPrivate { enabled } => {
                if self.private_session == *enabled {
                    return HistoryActionResult::Unchanged {};
                }
                self.private_session = *enabled;
                if *enabled && let Some(current) = self.current.as_mut() {
                    // Nothing remembered is applied to media opened just before.
                    current.file_choices = None;
                    current.resumed_at = None;
                }
                queue.push(Event::History {
                    event: HistoryEvent::PrivateSession { enabled: *enabled },
                });
                HistoryActionResult::Done {}
            }
            HistoryAction::Remove { source } => {
                let source = source.to_item();
                let active = self
                    .current
                    .as_ref()
                    .is_some_and(|current| current.source == source);
                self.collect(queue);
                // The shell is told whether the removal was saved, so this one
                // write is waited for. It carries every unsaved change with it.
                let writer = self.writer.as_ref();
                let removed = self.store.remove_source_with(&source, active, |document| {
                    writer.map_or(Ok(()), |writer| writer.write_now(document))
                });
                match removed {
                    Ok(true) => {
                        self.save_failing = false;
                        self.unsaved_since = None;
                        queue.push_once(Event::History {
                            event: HistoryEvent::Changed {},
                        });
                        HistoryActionResult::Done {}
                    }
                    Ok(false) => HistoryActionResult::Unchanged {},
                    Err(error) => {
                        // Changes made before the removal may not be on disk.
                        self.store.mark_unsaved();
                        self.unsaved_since.get_or_insert_with(Instant::now);
                        HistoryActionResult::Failed {
                            message: format!("the history could not be saved: {error}"),
                        }
                    }
                }
            }
        }
    }

    /// Answer a history query.
    pub(crate) fn query(&self, query: &HistoryQuery) -> HistoryQueryResult {
        let now = self.store.now_unix();
        match query {
            HistoryQuery::WelcomeShelf {
                limit,
                utc_offset_seconds,
            } => {
                let items = match self.store.welcome_shelf(*limit as usize) {
                    WelcomeShelf::Empty => Vec::new(),
                    WelcomeShelf::Items(items) => items,
                };
                let now_local = local_datetime(now, *utc_offset_seconds);
                HistoryQueryResult::WelcomeShelf {
                    private_session: self.private_session,
                    entries: items
                        .iter()
                        .map(|item| self.entry(item, now_local, *utc_offset_seconds))
                        .collect(),
                }
            }
            HistoryQuery::Page {
                search,
                utc_offset_seconds,
            } => {
                let now_local = local_datetime(now, *utc_offset_seconds);
                let items = self.store.search(search);
                let sections = [
                    history_format::HistoryBucket::Today,
                    history_format::HistoryBucket::Yesterday,
                    history_format::HistoryBucket::EarlierThisWeek,
                    history_format::HistoryBucket::Earlier,
                ]
                .into_iter()
                .filter_map(|bucket| {
                    let entries: Vec<HistoryEntry> = items
                        .iter()
                        .filter(|item| {
                            history_format::bucket_for(
                                local_datetime(item.updated_at_unix, *utc_offset_seconds),
                                now_local,
                            ) == bucket
                        })
                        .map(|item| self.entry(item, now_local, *utc_offset_seconds))
                        .collect();
                    (!entries.is_empty()).then(|| HistorySection {
                        bucket: bucket.into(),
                        header: history_format::bucket_header(bucket).to_owned(),
                        entries,
                    })
                })
                .collect();
                // Retention becomes a setting with the settings window; until
                // then it is the default.
                let retention = HistoryRetention::default();
                HistoryQueryResult::Page {
                    private_session: self.private_session,
                    read_failed: self.store.read_failed(),
                    unreadable_copy: self
                        .unreadable_copy()
                        .map(|path| path.to_string_lossy().into_owned()),
                    summary: history_format::retention_summary(retention),
                    end_cap: history_format::retention_end_cap(retention),
                    sections,
                }
            }
        }
    }

    fn entry(
        &self,
        item: &HistoryItem,
        now_local: LocalDateTime,
        utc_offset_seconds: i32,
    ) -> HistoryEntry {
        let source = item.source();
        HistoryEntry {
            source: MediaSource::from_item(&source),
            title: item.title.clone(),
            location: item.location.clone(),
            position: item.position,
            duration: item.duration,
            progress: item.progress,
            state: item.state_kind.into(),
            state_label: item.state_label.clone(),
            when_label: history_format::when_label(
                local_datetime(item.updated_at_unix, utc_offset_seconds),
                now_local,
            ),
            updated_at_unix: item.updated_at_unix,
            resume_from: if self.private_session {
                None
            } else {
                self.store.resume_position_for_source(&source)
            },
        }
    }
}

/// Select the remembered tracks a loaded file has, and set its subtitle delay.
/// A remembered "off" turns the kind off; a remembered track the file lacks
/// leaves the engine's own choice.
fn apply_file_choices(
    choices: &Preferences,
    tracks: &[TrackInfo],
    engine: &dyn Engine,
    queue: &mut Coalescer,
) {
    for (kind, core_kind, enabled, id) in [
        (
            TrackKind::Audio,
            CoreTrackKind::Audio,
            choices.audio_enabled,
            choices.audio_track_id,
        ),
        (
            TrackKind::Subtitle,
            CoreTrackKind::Subtitle,
            choices.subtitle_enabled,
            choices.subtitle_track_id,
        ),
    ] {
        let select = match (enabled, id) {
            (Some(false), _) => None,
            (Some(true), Some(id))
                if tracks
                    .iter()
                    .any(|track| track.kind == core_kind && track.id == id) =>
            {
                Some(id)
            }
            _ => continue,
        };
        forward(engine, &Command::SelectTrack { kind, id: select }, queue);
    }
    if let Some(seconds) = choices.subtitle_delay.filter(|delay| delay.is_finite()) {
        forward(engine, &Command::SetSubtitleDelay { seconds }, queue);
    }
}

/// Hand the engine a command the session issues on its own behalf.
fn forward(engine: &dyn Engine, command: &Command, queue: &mut Coalescer) {
    if let Err(message) = engine.forward(command, 0) {
        queue.push(Event::Error {
            kind: ErrorKind::CommandFailed,
            message,
            request_id: None,
        });
    }
}

/// The civil date and time `utc_offset_seconds` from UTC at Unix time `unix`.
fn local_datetime(unix: i64, utc_offset_seconds: i32) -> LocalDateTime {
    let local = unix.saturating_add(i64::from(utc_offset_seconds));
    let seconds = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    LocalDateTime::new(
        year,
        month,
        day,
        (seconds / 3_600) as u32,
        (seconds % 3_600 / 60) as u32,
    )
}

/// Year, month and day of the date `days` after 1970-01-01 (Howard Hinnant's
/// `civil_from_days`, the inverse of the `days_from_civil` okp-core uses).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}
