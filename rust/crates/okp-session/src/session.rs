//! The session: engine, core state machine, event thread and coalescing queue.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use okp_core::player::{
    ChapterInfo, CommandOutcome, CommandReply, CommandResult, EndReason as CoreEndReason,
    PlaybackStatus, PlayerCommand, PlayerError, PlayerErrorKind, PlayerEvent, PropertyChange,
    TrackInfo, TrackKind as CoreTrackKind,
};
use okp_core::playlist::PlaylistItem;
use okp_mpv::{EndFileReason, EntryEvent, MpvEvent, error_description};

use crate::coalesce::Coalescer;
use crate::engine::{Engine, MpvEngine, Observation};
use crate::machine::SessionMachine;
use crate::playlist::{Background, PlannedOpen, SessionPlaylist, Step};
use crate::protocol::{
    Command, CommandEnvelope, DispatchReply, DispatchResult, ErrorKind, Event, EventBatch,
    MediaSource, PlaylistAction, Query, QueryEnvelope, QueryReply, QueryResult, RejectReason,
    SCHEMA_VERSION, SessionConfig, SessionSnapshot, Track, TrackKind, VideoGeometry,
    parse_envelope, to_json,
};
use crate::reconcile::{core_end_reason, endfile_error, event_matches_source};

/// Called from the session's event thread when a new batch of events is ready.
/// It should only schedule a drain on the shell's own thread.
pub type Wakeup = Box<dyn Fn() + Send + Sync>;

/// Called from an engine thread when the render context has work for the
/// shell's render thread; see [`Session::render_create`].
pub type RenderUpdate = Box<dyn Fn() + Send + Sync>;

/// One playback session. Create it with [`Session::new`] or
/// [`Session::from_config_json`]; dropping it stops the event thread, discards
/// undrained events and shuts the engine down.
///
/// Threading: [`Session::dispatch`], [`Session::query`] and
/// [`Session::drain_events`] may be called from any thread. The wakeup runs on
/// the session's event thread, never on the caller's, and never after
/// [`Session::set_wakeup`] has replaced it or the session has been dropped. A
/// wakeup may call `drain_events`, `dispatch` and `query`, but must not call
/// `set_wakeup`.
pub struct Session {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    engine: Box<dyn Engine>,
    state: Mutex<State>,
    signal: Arc<Signal>,
    wakeup: Mutex<Option<Wakeup>>,
    running: AtomicBool,
}

struct State {
    machine: SessionMachine,
    queue: Coalescer,
    /// The shell has drained the previous batch, so the next one gets a wakeup.
    armed: bool,
    /// The engine's pause flag as last observed. Pause is applied optimistically
    /// on dispatch, so only a change in the engine's report may override it.
    observed_paused: Option<bool>,
    /// The engine's id for the current load, when it reports one. Events of an
    /// older load are about media the shell has already replaced.
    load: Option<i64>,
    /// Tracks the current open asked for, selected once its file has loaded.
    pending_tracks: Vec<(TrackKind, i64)>,
    playlist: SessionPlaylist,
}

/// Wakes the event thread: set by the engine listener, by commands and by
/// shutdown.
#[derive(Default)]
struct Signal {
    pending: Mutex<bool>,
    condvar: Condvar,
}

impl Signal {
    fn notify(&self) {
        *lock(&self.pending) = true;
        self.condvar.notify_one();
    }

    fn wait(&self, deadline: Option<Instant>) {
        let mut pending = lock(&self.pending);
        while !*pending {
            match deadline {
                None => {
                    pending = self
                        .condvar
                        .wait(pending)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    pending = self
                        .condvar
                        .wait_timeout(pending, deadline - now)
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .0;
                }
            }
        }
        *pending = false;
    }
}

impl Session {
    /// Start libmpv with `config` and the session's event thread.
    pub fn new(config: &SessionConfig) -> Result<Self, String> {
        validate_config(config)?;
        let engine = MpvEngine::new(config)?;
        Ok(Self::with_engine(Box::new(engine)))
    }

    /// [`Session::new`] from a config document.
    pub fn from_config_json(text: &str) -> Result<Self, String> {
        let config: SessionConfig = parse_envelope(text)
            .map_err(|(_, message)| format!("invalid session config: {message}"))?;
        Self::new(&config)
    }

    pub(crate) fn with_engine(engine: Box<dyn Engine>) -> Self {
        let signal = Arc::new(Signal::default());
        let listener_signal = Arc::clone(&signal);
        engine.set_listener(Some(Arc::new(move || listener_signal.notify())));
        let playlist_signal = Arc::clone(&signal);
        let shared = Arc::new(Shared {
            engine,
            state: Mutex::new(State {
                machine: SessionMachine::new(),
                queue: Coalescer::new(),
                armed: true,
                observed_paused: None,
                load: None,
                pending_tracks: Vec::new(),
                playlist: SessionPlaylist::new(Arc::new(move || playlist_signal.notify())),
            }),
            signal,
            wakeup: Mutex::new(None),
            running: AtomicBool::new(true),
        });
        // Publish the engine's initial state (volume, speed) without waiting for
        // the first engine change.
        shared.signal.notify();
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("okp-session-events".to_owned())
            .spawn(move || event_loop(&thread_shared))
            .expect("spawning the session event thread must succeed");
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// Install, replace or (with `None`) remove the wakeup. When this returns,
    /// the previous wakeup is not running and will not be called again. If
    /// events are already waiting, the new wakeup fires promptly.
    pub fn set_wakeup(&self, wakeup: Option<Wakeup>) {
        let mut slot = lock(&self.shared.wakeup);
        let installed = wakeup.is_some();
        *slot = wakeup;
        if installed {
            // The new wakeup has not been told about a batch that is still
            // waiting, so it gets its own announcement.
            lock(&self.shared.state).armed = true;
        }
        drop(slot);
        self.shared.signal.notify();
    }

    /// Validate `command` with the core state machine and, only if it accepts,
    /// hand it to the engine. A playlist command is decided by the session's
    /// playlist, and an item it plays is opened the same way.
    pub fn dispatch(&self, command: &Command) -> DispatchResult {
        let engine = &*self.shared.engine;
        let mut state = lock(&self.shared.state);
        let result = match command {
            Command::Playlist { action } => state.dispatch_playlist(engine, action),
            _ => {
                let result = state.forward(engine, command);
                state.playlist.follow(command, &result);
                result
            }
        };
        drop(state);
        self.shared.signal.notify();
        result
    }

    pub fn query(&self, query: &Query) -> QueryResult {
        match query {
            Query::Snapshot {} => QueryResult::Snapshot {
                snapshot: self.snapshot(),
            },
            Query::Playlist {} => QueryResult::Playlist {
                playlist: lock(&self.shared.state).playlist.state(),
            },
        }
    }

    /// The full state, including every change not yet drained. Answering folds
    /// in the engine's latest report first, so it reflects a command dispatched
    /// just before.
    pub fn snapshot(&self) -> SessionSnapshot {
        let mut state = lock(&self.shared.state);
        state.refresh(&*self.shared.engine, Instant::now());
        let snapshot = state.queue.published().clone();
        let pending = state.queue.has_pending();
        drop(state);
        if pending {
            self.shared.signal.notify();
        }
        snapshot
    }

    /// Take every queued event as one batch and re-arm the wakeup.
    pub fn drain_events(&self) -> Vec<Event> {
        let mut state = lock(&self.shared.state);
        state.armed = true;
        state.queue.drain()
    }

    /// Create the engine's OpenGL render context, with libmpv's advanced control
    /// and `on_update` installed before this returns. Call it on the shell's
    /// render thread with the shell's OpenGL context current; a second call while
    /// the context exists does nothing and drops its `on_update`. Free it with
    /// [`Session::render_destroy`] under the same OpenGL context before dropping
    /// the session.
    ///
    /// `on_update` runs on an engine thread and must only wake the render thread,
    /// which then calls [`Session::render_update`]. The render thread must never
    /// wait for a thread that is dispatching, querying or dropping the session.
    pub fn render_create(&self, on_update: RenderUpdate) -> Result<(), String> {
        self.shared.engine.render_create(on_update)
    }

    /// Serve the engine's pending render-thread work after `on_update` fired, and
    /// say whether a new frame is due. Call it every time `on_update` fires, even
    /// when nothing will be drawn. False without a render context.
    pub fn render_update(&self) -> bool {
        self.shared.engine.render_update()
    }

    /// Draw the due video frame, or redraw the current one, into the bound
    /// framebuffer of `width` by `height` pixels, on the thread and OpenGL context
    /// of [`Session::render_create`]. Fails if there is no render context.
    pub fn render(&self, width: i32, height: i32) -> Result<(), String> {
        self.shared.engine.render(width, height)
    }

    /// Tell the engine that the frame drawn last has been presented (after the
    /// shell's flush), so it can time the next one. Returns the swaps reported
    /// to the current render context so far, this one included; 0 without a
    /// render context.
    pub fn render_report_swap(&self) -> u64 {
        self.shared.engine.render_report_swap()
    }

    /// Free the render context, if there is one, with its OpenGL context
    /// current.
    pub fn render_destroy(&self) {
        self.shared.engine.render_destroy();
    }

    /// [`Session::dispatch`] for a command document; returns a reply document.
    pub fn dispatch_json(&self, text: &str) -> String {
        let result = match parse_envelope::<CommandEnvelope>(text) {
            Ok(envelope) => self.dispatch(&envelope.command),
            Err((reason, message)) => DispatchResult::Rejected { reason, message },
        };
        to_json(&DispatchReply {
            schema_version: SCHEMA_VERSION,
            result,
        })
    }

    /// [`Session::query`] for a query document; returns a reply document.
    pub fn query_json(&self, text: &str) -> String {
        let result = match parse_envelope::<QueryEnvelope>(text) {
            Ok(envelope) => self.query(&envelope.query),
            Err((reason, message)) => QueryResult::Rejected { reason, message },
        };
        to_json(&QueryReply {
            schema_version: SCHEMA_VERSION,
            result,
        })
    }

    /// [`Session::drain_events`] as an event batch document.
    pub fn drain_events_json(&self) -> String {
        to_json(&EventBatch {
            schema_version: SCHEMA_VERSION,
            events: self.drain_events(),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.engine.set_listener(None);
        self.shared.signal.notify();
        if let Some(thread) = self.thread.take() {
            // A wakeup that drops its own session runs on the event thread, which
            // cannot join itself; it exits after the wakeup returns instead.
            if thread.thread().id() != thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

fn event_loop(shared: &Arc<Shared>) {
    let mut deadline = None;
    loop {
        shared.signal.wait(deadline);
        if !shared.running.load(Ordering::Acquire) {
            break;
        }
        // The wakeup slot is held across the call so that `set_wakeup` returning
        // guarantees the replaced wakeup is no longer running.
        let wakeup = lock(&shared.wakeup);
        let fire = {
            let mut state = lock(&shared.state);
            deadline = state.refresh(&*shared.engine, Instant::now());
            let fire = wakeup.is_some() && state.armed && state.queue.has_pending();
            if fire {
                state.armed = false;
            }
            fire
        };
        if fire && let Some(wakeup) = wakeup.as_ref() {
            wakeup();
        }
    }
}

impl State {
    /// Fold the engine's latest report into the machine and the queue. Returns
    /// when a held playhead update falls due.
    fn refresh(&mut self, engine: &dyn Engine, now: Instant) -> Option<Instant> {
        // Events first: the engine publishes a pass's observations before its
        // events, so an observation read after the events is never older than
        // them (a loaded file's tracks are already there).
        let events = engine.take_events();
        let observation = engine.observe();
        let mut loaded = false;
        for event in events {
            loaded |= self.reconcile_event(event, &observation);
        }
        self.reconcile_observation(&observation);
        if loaded {
            self.select_pending_tracks(engine);
        }
        self.advance_playlist(engine);
        let current = self.project(&observation);
        self.queue.observe(&current, now)
    }

    /// Validate `command` with the core state machine and, only if it accepts,
    /// hand it to the engine.
    fn forward(&mut self, engine: &dyn Engine, command: &Command) -> DispatchResult {
        let core = command.to_core();
        let mut load = None;
        let (outcome, engine_result) = self.machine.dispatch(&core, |_, request_id| {
            engine
                .forward(command, request_id)
                .map(|entry| load = entry)
        });
        if engine_result.is_ok() {
            self.track_load(command, outcome, load);
        }
        match (outcome, engine_result) {
            (CommandOutcome::Accepted { request_id }, Ok(())) => {
                DispatchResult::Accepted { request_id }
            }
            (CommandOutcome::Accepted { request_id }, Err(message)) => {
                self.queue.push(Event::Error {
                    kind: if matches!(core, PlayerCommand::Open(_)) {
                        ErrorKind::LoadFailed
                    } else {
                        ErrorKind::CommandFailed
                    },
                    message: message.clone(),
                    request_id: Some(request_id),
                });
                DispatchResult::Failed {
                    request_id,
                    message,
                }
            }
            (CommandOutcome::NoOp, _) => DispatchResult::NoOp {},
            (CommandOutcome::Rejected(reason), _) => rejected(reason.into()),
        }
    }

    fn dispatch_playlist(
        &mut self,
        engine: &dyn Engine,
        action: &PlaylistAction,
    ) -> DispatchResult {
        let current = self.machine.machine().snapshot().source.clone();
        match self.playlist.dispatch(action, current.as_ref()) {
            Step::Reply(result) => result,
            Step::Open(open) => self.open_planned(engine, open),
        }
    }

    /// Open an item the playlist chose, as an `open` command would.
    fn open_planned(&mut self, engine: &dyn Engine, open: PlannedOpen) -> DispatchResult {
        let result = self.forward(engine, &open.command());
        self.playlist.opened(open, &result);
        result
    }

    /// Play what finished folder reads and an end of file call for, and queue
    /// the playlist if it changed.
    fn advance_playlist(&mut self, engine: &dyn Engine) {
        for step in self.playlist.poll() {
            match step {
                Background::Open(open) => {
                    // An engine refusal is already queued as an error event.
                    let _ = self.open_planned(engine, open);
                }
                Background::NothingPlayable => self.queue.push(Event::Error {
                    kind: ErrorKind::LoadFailed,
                    message: "nothing playable was found".to_owned(),
                    request_id: None,
                }),
            }
        }
        if let Some(playlist) = self.playlist.take_changed() {
            self.queue.set_playlist(playlist);
        }
    }

    /// Remember what a forwarded command means for the current load.
    fn track_load(&mut self, command: &Command, outcome: CommandOutcome, load: Option<i64>) {
        if !matches!(outcome, CommandOutcome::Accepted { .. }) {
            return;
        }
        match command {
            Command::Open {
                audio_track,
                subtitle_track,
                ..
            } => {
                self.load = load;
                self.pending_tracks = [
                    (TrackKind::Audio, *audio_track),
                    (TrackKind::Subtitle, *subtitle_track),
                ]
                .into_iter()
                .filter_map(|(kind, id)| id.map(|id| (kind, id)))
                .collect();
            }
            Command::Close {} => {
                self.load = None;
                self.pending_tracks.clear();
            }
            _ => {}
        }
    }

    /// Whether an event about engine load `entry` is about the current load.
    /// Load ids only grow, and the entries a playlist file expands into come
    /// after it, so an older id is media the shell has already replaced. Without
    /// ids either way, the caller decides from the event itself.
    fn is_current_load(&self, entry: Option<i64>) -> bool {
        match (entry, self.load) {
            (Some(entry), Some(load)) => entry >= load,
            _ => true,
        }
    }

    /// Select the tracks the open asked for, now that the file has loaded. A
    /// track the file does not have leaves the engine's own choice: forcing a
    /// missing id would turn that kind off, and for audio-only media leave
    /// nothing to play.
    fn select_pending_tracks(&mut self, engine: &dyn Engine) {
        let available = &self.machine.machine().snapshot().tracks;
        for (kind, id) in std::mem::take(&mut self.pending_tracks) {
            let core_kind = match kind {
                TrackKind::Audio => CoreTrackKind::Audio,
                TrackKind::Subtitle => CoreTrackKind::Subtitle,
            };
            if !available
                .iter()
                .any(|track| track.kind == core_kind && track.id == id)
            {
                continue;
            }
            let select = Command::SelectTrack { kind, id: Some(id) };
            if let Err(message) = engine.forward(&select, 0) {
                self.queue.push(Event::Error {
                    kind: ErrorKind::CommandFailed,
                    message,
                    request_id: None,
                });
            }
        }
    }

    /// Fold one engine event in. Returns whether it was the current file
    /// finishing its load.
    fn reconcile_event(&mut self, event: EntryEvent, observation: &Observation) -> bool {
        let EntryEvent {
            event,
            playlist_entry_id: entry,
        } = event;
        match event {
            MpvEvent::FileLoaded { .. } => {
                if !self.is_current_load(entry) {
                    return false;
                }
                self.machine.apply_event(PlayerEvent::Loaded {
                    duration: observation.playback.duration,
                });
                // Pause persists across loads in the engine; take its current
                // value for the new media whatever it was before.
                self.observed_paused = None;
                return true;
            }
            MpvEvent::EndFile {
                reason,
                diagnostic_messages,
                path,
                ..
            } => {
                let error = endfile_error(reason, &diagnostic_messages);
                if !self.ends_current_media(entry, reason, error.is_some(), path.as_deref()) {
                    return false;
                }
                if reason == EndFileReason::Eof && error.is_none() {
                    self.playlist.end_of_file();
                }
                if let Some(message) = error {
                    self.machine.apply_event(PlayerEvent::Error(PlayerError {
                        kind: PlayerErrorKind::LoadFailed,
                        message: message.clone(),
                    }));
                    self.queue.push(Event::Error {
                        kind: ErrorKind::LoadFailed,
                        message,
                        request_id: None,
                    });
                }
                self.machine
                    .apply_event(PlayerEvent::Ended(core_end_reason(reason)));
            }
            MpvEvent::CommandReply { request_id, error } => {
                let result = if error < 0 {
                    let message = error_description(error);
                    self.queue.push(Event::Error {
                        kind: ErrorKind::CommandFailed,
                        message: message.clone(),
                        request_id: Some(request_id),
                    });
                    CommandResult::Failed(message)
                } else {
                    CommandResult::Ok
                };
                self.machine
                    .apply_event(PlayerEvent::Reply(CommandReply { request_id, result }));
            }
            MpvEvent::Shutdown => {
                self.machine
                    .apply_event(PlayerEvent::Ended(CoreEndReason::Quit));
                self.queue.push(Event::Shutdown {});
            }
            MpvEvent::PlaybackRestart { .. }
            | MpvEvent::DecoderWarning { .. }
            | MpvEvent::VideoReconfig { .. } => {}
        }
        false
    }

    /// Whether an end-of-file ends the current media, rather than media it
    /// replaced (whose end arrives once the new open is already under way).
    fn ends_current_media(
        &self,
        entry: Option<i64>,
        reason: EndFileReason,
        failed: bool,
        path: Option<&str>,
    ) -> bool {
        let core = self.machine.machine();
        let Some(source) = core.snapshot().source.as_ref() else {
            return false;
        };
        // The load goes on with the entries a playlist file expanded into.
        if reason == EndFileReason::Redirect || !self.is_current_load(entry) {
            return false;
        }
        if entry.is_some() && self.load.is_some() {
            return true;
        }
        // Without load ids, judge by the event. Until the new media has
        // loaded, an ordinary end can only be the replaced media stopping; a
        // failure is the new media's unless the path it names is another one.
        // The path is read after the fact, so it is no proof of a match.
        if core.status() == PlaybackStatus::Opening && !failed {
            return false;
        }
        event_matches_source(Some(source), path)
    }

    fn reconcile_observation(&mut self, observation: &Observation) {
        let playback = observation.playback;
        // Volume and speed are engine-global and hold without media.
        self.machine
            .apply_event(PlayerEvent::Property(PropertyChange::Volume(
                playback.volume,
            )));
        self.machine
            .apply_event(PlayerEvent::Property(PropertyChange::Speed(playback.speed)));
        // Until the engine has loaded the current media, its per-file report
        // still describes the previous one.
        if !is_loaded(self.machine.machine().status()) {
            return;
        }
        self.machine
            .apply_event(PlayerEvent::Property(PropertyChange::TimePos(
                playback.time_pos,
            )));
        self.machine
            .apply_event(PlayerEvent::Property(PropertyChange::Duration(
                playback.duration,
            )));
        if self.observed_paused != Some(playback.paused) {
            self.observed_paused = Some(playback.paused);
            self.machine
                .apply_event(PlayerEvent::Property(PropertyChange::Paused(
                    playback.paused,
                )));
        }
        self.machine
            .apply_event(PlayerEvent::Property(PropertyChange::SubtitleDelay(
                observation.subtitle_delay,
            )));
        self.machine.apply_event(PlayerEvent::Tracks(
            observation.tracks.iter().map(core_track).collect(),
        ));
        self.machine.apply_event(PlayerEvent::Chapters(
            observation
                .chapters
                .iter()
                .map(|chapter| ChapterInfo {
                    index: chapter.index,
                    time: chapter.time,
                    title: chapter.title.clone(),
                })
                .collect(),
        ));
    }

    fn project(&self, observation: &Observation) -> SessionSnapshot {
        let core = self.machine.machine().snapshot();
        let idle = core.status == PlaybackStatus::Idle;
        let loaded = is_loaded(core.status);
        SessionSnapshot {
            status: core.status.into(),
            source: core.source.as_ref().map(MediaSource::from_item),
            time_pos: core.time_pos,
            duration: core.duration,
            volume: core.volume,
            mute: core.mute,
            speed: core.speed,
            subtitle_delay: core.subtitle_delay,
            tracks: core.tracks.iter().map(Track::from).collect(),
            chapters: core.chapters.iter().map(Into::into).collect(),
            end_reason: core.end_reason.map(Into::into),
            title: if idle {
                None
            } else {
                observation
                    .title
                    .clone()
                    .or_else(|| core.source.as_ref().map(source_title))
            },
            video: observation
                .video
                .filter(|_| loaded)
                .map(|video| VideoGeometry {
                    width: video.width,
                    height: video.height,
                    display_aspect: f64::from(video.width) / f64::from(video.height),
                }),
            buffering: !idle && observation.buffering,
            cache_ahead: observation.playback.cache_duration.filter(|_| loaded),
        }
    }
}

fn is_loaded(status: PlaybackStatus) -> bool {
    matches!(
        status,
        PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Ended
    )
}

fn core_track(track: &okp_mpv::Track) -> TrackInfo {
    TrackInfo {
        id: track.id,
        kind: match track.kind {
            okp_mpv::TrackKind::Audio => CoreTrackKind::Audio,
            okp_mpv::TrackKind::Subtitle => CoreTrackKind::Subtitle,
        },
        selected: track.selected,
        external: track.external,
        default: track.default,
        title: track.title.clone(),
        lang: track.lang.clone(),
    }
}

/// A title for media the engine has not named yet: the file name, or the URL.
fn source_title(source: &PlaylistItem) -> String {
    match source {
        PlaylistItem::Local(path) => path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
            .into_owned(),
        PlaylistItem::Url(url) => url.clone(),
    }
}

fn rejected(reason: RejectReason) -> DispatchResult {
    DispatchResult::Rejected {
        reason,
        message: reason.describe().to_owned(),
    }
}

fn validate_config(config: &SessionConfig) -> Result<(), String> {
    if config.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "schema_version {} is not supported; this session speaks {SCHEMA_VERSION}",
            config.schema_version
        ));
    }
    for (name, dir) in [
        ("data_dir", &config.data_dir),
        ("config_dir", &config.config_dir),
        ("cache_dir", &config.cache_dir),
    ] {
        if !Path::is_absolute(dir) {
            return Err(format!("{name} must be an absolute path"));
        }
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ChangeListener;
    use crate::protocol::{EndReason, SnapshotChange, SourceKind, Status};
    use okp_mpv::{Chapter, VideoDimensions};
    use std::sync::atomic::{AtomicI64, AtomicUsize};
    use std::time::Duration;

    /// A scripted engine. The test keeps the `Arc<FakeState>` and plays the part
    /// of the libmpv event pump through [`FakeState::publish`].
    #[derive(Default)]
    struct FakeState {
        forwarded: Mutex<Vec<(Command, u64)>>,
        observation: Mutex<Observation>,
        events: Mutex<Vec<EntryEvent>>,
        /// Give each open a load id, as libmpv 0.33 and later do.
        report_loads: AtomicBool,
        loads: AtomicI64,
        listener: Mutex<Option<ChangeListener>>,
        refuse_with: Mutex<Option<String>>,
        observations: AtomicUsize,
        dropped: AtomicBool,
        /// The render side: the installed update callback, whether a frame is
        /// due, and the swaps reported.
        render_update: Mutex<Option<RenderUpdate>>,
        frame_due: AtomicBool,
        swaps: AtomicUsize,
    }

    impl FakeState {
        /// Change what the engine reports, queue lifecycle events without a load
        /// id, and notify the session the way the pump does after each pass.
        fn publish(&self, change: impl FnOnce(&mut Observation), events: Vec<MpvEvent>) {
            self.publish_for_loads(
                change,
                events.into_iter().map(|event| (event, None)).collect(),
            );
        }

        /// [`FakeState::publish`] with each event's load id.
        fn publish_for_loads(
            &self,
            change: impl FnOnce(&mut Observation),
            events: Vec<(MpvEvent, Option<i64>)>,
        ) {
            change(&mut lock(&self.observation));
            lock(&self.events).extend(events.into_iter().map(|(event, playlist_entry_id)| {
                EntryEvent {
                    event,
                    playlist_entry_id,
                }
            }));
            let listener = lock(&self.listener).clone();
            if let Some(listener) = listener {
                listener();
            }
        }

        fn forwarded(&self) -> Vec<Command> {
            lock(&self.forwarded)
                .iter()
                .map(|(command, _)| command.clone())
                .collect()
        }
    }

    struct FakeEngine(Arc<FakeState>);

    impl Engine for FakeEngine {
        fn forward(&self, command: &Command, request_id: u64) -> Result<Option<i64>, String> {
            lock(&self.0.forwarded).push((command.clone(), request_id));
            if let Some(message) = lock(&self.0.refuse_with).take() {
                return Err(message);
            }
            let reports = self.0.report_loads.load(Ordering::SeqCst);
            Ok((reports && matches!(command, Command::Open { .. }))
                .then(|| self.0.loads.fetch_add(1, Ordering::SeqCst) + 1))
        }

        fn observe(&self) -> Observation {
            self.0.observations.fetch_add(1, Ordering::SeqCst);
            lock(&self.0.observation).clone()
        }

        fn take_events(&self) -> Vec<EntryEvent> {
            std::mem::take(&mut *lock(&self.0.events))
        }

        fn set_listener(&self, listener: Option<ChangeListener>) {
            *lock(&self.0.listener) = listener;
        }

        fn render_create(&self, on_update: RenderUpdate) -> Result<(), String> {
            lock(&self.0.render_update).get_or_insert(on_update);
            Ok(())
        }

        fn render_update(&self) -> bool {
            self.0.frame_due.swap(false, Ordering::SeqCst)
        }

        fn render_report_swap(&self) -> u64 {
            self.0.swaps.fetch_add(1, Ordering::SeqCst) as u64 + 1
        }

        fn render_destroy(&self) {
            lock(&self.0.render_update).take();
        }
    }

    impl Drop for FakeEngine {
        fn drop(&mut self) {
            self.0.dropped.store(true, Ordering::SeqCst);
        }
    }

    fn fake_session() -> (Session, Arc<FakeState>) {
        let state = Arc::new(FakeState::default());
        lock(&state.observation).playback.volume = Some(100.0);
        let session = Session::with_engine(Box::new(FakeEngine(Arc::clone(&state))));
        (session, state)
    }

    fn counting_wakeup(session: &Session) -> Arc<AtomicUsize> {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        session.set_wakeup(Some(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })));
        count
    }

    fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            thread::sleep(Duration::from_millis(2));
        }
        condition()
    }

    /// Publish like the pump, then wait until the event thread has read it.
    fn publish_and_settle(engine: &FakeState, change: impl FnOnce(&mut Observation)) {
        let seen = engine.observations.load(Ordering::SeqCst);
        engine.publish(change, vec![]);
        assert!(
            wait_for(|| engine.observations.load(Ordering::SeqCst) > seen),
            "the event thread never processed the change"
        );
        // Let the pass that read it finish deciding whether to wake the shell.
        thread::sleep(Duration::from_millis(30));
    }

    fn open(path: &str) -> Command {
        Command::Open {
            source: MediaSource {
                kind: SourceKind::Local,
                location: path.to_owned(),
            },
            resume_from: None,
            audio_track: None,
            subtitle_track: None,
        }
    }

    fn loaded(engine: &FakeState, duration: f64) {
        engine.publish(
            |observation| observation.playback.duration = Some(duration),
            vec![MpvEvent::FileLoaded {
                video_dimensions: None,
            }],
        );
    }

    fn changes(events: &[Event]) -> Vec<SnapshotChange> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::SnapshotChanged { changes } => Some(changes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn the_engines_render_updates_reach_the_shell_until_the_context_is_destroyed() {
        let (session, engine) = fake_session();
        let fired = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&fired);
        session
            .render_create(Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("the fake engine renders");
        let fire = || {
            if let Some(on_update) = lock(&engine.render_update).as_ref() {
                on_update();
            }
        };

        fire();
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        assert!(!session.render_update(), "no frame is due yet");
        engine.frame_due.store(true, Ordering::SeqCst);
        assert!(
            session.render_update(),
            "the engine's due frame is reported"
        );
        assert_eq!(
            session.render_report_swap(),
            1,
            "the engine's swap count comes back to the shell"
        );
        assert_eq!(session.render_report_swap(), 2);
        assert_eq!(engine.swaps.load(Ordering::SeqCst), 2);

        session.render_destroy();
        fire();
        assert_eq!(
            fired.load(Ordering::SeqCst),
            1,
            "a destroyed render context no longer wakes the shell"
        );
    }

    #[test]
    fn a_rejected_command_never_reaches_the_engine() {
        let (session, engine) = fake_session();
        for command in [
            Command::TogglePause {},
            Command::SetPaused { paused: true },
            Command::Seek {
                seconds: 5.0,
                mode: crate::protocol::SeekMode::Relative,
                precision: crate::protocol::SeekPrecision::Keyframe,
            },
            Command::SelectTrack {
                kind: TrackKind::Audio,
                id: Some(1),
            },
            Command::SetSubtitleDelay { seconds: 0.5 },
        ] {
            assert_eq!(
                session.dispatch(&command),
                DispatchResult::Rejected {
                    reason: RejectReason::NoActiveMedia,
                    message: RejectReason::NoActiveMedia.describe().to_owned(),
                },
                "{command:?}"
            );
        }
        assert_eq!(
            session.dispatch(&Command::Close {}),
            DispatchResult::NoOp {}
        );
        assert!(engine.forwarded().is_empty(), "{:?}", engine.forwarded());

        // The same commands go through once media is loaded.
        assert!(matches!(
            session.dispatch(&open("/media/a.mkv")),
            DispatchResult::Accepted { request_id: 1 }
        ));
        loaded(&engine, 10.0);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        assert!(matches!(
            session.dispatch(&Command::TogglePause {}),
            DispatchResult::Accepted { request_id: 2 }
        ));
        assert_eq!(
            engine.forwarded(),
            vec![open("/media/a.mkv"), Command::TogglePause {}]
        );
    }

    #[test]
    fn the_wakeup_fires_once_per_coalesced_batch() {
        let (session, engine) = fake_session();
        let wakeups = counting_wakeup(&session);
        assert!(wait_for(|| wakeups.load(Ordering::SeqCst) == 1));

        // More changes before the shell drains join the announced batch.
        for volume in [10.0, 20.0, 30.0] {
            publish_and_settle(&engine, |observation| {
                observation.playback.volume = Some(volume);
            });
        }
        assert_eq!(wakeups.load(Ordering::SeqCst), 1);

        // The initial 100 % and the three updates collapse into the latest value.
        assert_eq!(
            session.drain_events(),
            vec![Event::SnapshotChanged {
                changes: vec![SnapshotChange::Volume { value: Some(30.0) }],
            }]
        );

        // Draining re-arms it: the next change is a new batch with a new wakeup.
        engine.publish(
            |observation| observation.playback.volume = Some(40.0),
            vec![],
        );
        assert!(wait_for(|| wakeups.load(Ordering::SeqCst) == 2));
        thread::sleep(Duration::from_millis(30));
        assert_eq!(wakeups.load(Ordering::SeqCst), 2);
        assert_eq!(
            changes(&session.drain_events()),
            vec![SnapshotChange::Volume { value: Some(40.0) }]
        );
    }

    #[test]
    fn dropping_a_session_with_pending_events_stops_the_wakeup_and_the_engine() {
        let (session, engine) = fake_session();
        let wakeups = counting_wakeup(&session);
        assert!(matches!(
            session.dispatch(&open("/media/a.mkv")),
            DispatchResult::Accepted { .. }
        ));
        assert!(wait_for(|| wakeups.load(Ordering::SeqCst) == 1));
        engine.publish(
            |observation| observation.playback.volume = Some(5.0),
            vec![],
        );

        drop(session);
        assert!(
            engine.dropped.load(Ordering::SeqCst),
            "the engine shut down"
        );
        assert!(
            lock(&engine.listener).is_none(),
            "the engine listener is gone"
        );
        let after_drop = wakeups.load(Ordering::SeqCst);
        engine.publish(
            |observation| observation.playback.volume = Some(6.0),
            vec![],
        );
        thread::sleep(Duration::from_millis(30));
        assert_eq!(wakeups.load(Ordering::SeqCst), after_drop);
    }

    #[test]
    fn a_new_wakeup_learns_of_the_waiting_batch_and_the_old_one_goes_quiet() {
        let (session, engine) = fake_session();
        let first = counting_wakeup(&session);
        assert!(wait_for(|| first.load(Ordering::SeqCst) == 1));

        // The first batch is still undrained when the wakeup is replaced.
        let second = counting_wakeup(&session);
        assert!(
            wait_for(|| second.load(Ordering::SeqCst) == 1),
            "the new wakeup was not told about the waiting batch"
        );
        session.drain_events();
        engine.publish(
            |observation| observation.playback.volume = Some(7.0),
            vec![],
        );
        assert!(wait_for(|| second.load(Ordering::SeqCst) == 2));
        assert_eq!(first.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn tracks_chapters_and_media_facts_arrive_once_the_file_has_loaded() {
        let (session, engine) = fake_session();
        // The engine still describes the previous file while the new one opens.
        engine.publish(
            |observation| {
                observation.tracks = vec![okp_mpv::Track {
                    id: 9,
                    kind: okp_mpv::TrackKind::Audio,
                    selected: true,
                    external: false,
                    external_filename: None,
                    default: false,
                    title: Some("stale".to_owned()),
                    lang: None,
                    codec: None,
                    audio_channels: None,
                }];
                observation.video = Some(VideoDimensions {
                    width: 4,
                    height: 3,
                });
            },
            vec![],
        );
        let _ = session.dispatch(&open("/media/film.mkv"));
        let opening = session.snapshot();
        assert_eq!(opening.status, Status::Opening);
        assert!(opening.tracks.is_empty(), "no stale tracks while opening");
        assert_eq!(opening.video, None);
        assert_eq!(opening.title.as_deref(), Some("film.mkv"));

        engine.publish(
            |observation| {
                observation.tracks = vec![
                    okp_mpv::Track {
                        id: 1,
                        kind: okp_mpv::TrackKind::Audio,
                        selected: true,
                        external: false,
                        external_filename: None,
                        default: true,
                        title: None,
                        lang: Some("eng".to_owned()),
                        codec: Some("aac".to_owned()),
                        audio_channels: None,
                    },
                    okp_mpv::Track {
                        id: 1,
                        kind: okp_mpv::TrackKind::Subtitle,
                        selected: false,
                        external: true,
                        external_filename: Some("/media/film.srt".to_owned()),
                        default: false,
                        title: None,
                        lang: None,
                        codec: Some("subrip".to_owned()),
                        audio_channels: None,
                    },
                ];
                observation.chapters = vec![Chapter {
                    index: 0,
                    time: 0.0,
                    title: Some("Intro".to_owned()),
                }];
                observation.video = Some(VideoDimensions {
                    width: 1920,
                    height: 800,
                });
                observation.title = Some("Film".to_owned());
                observation.playback.duration = Some(60.0);
                observation.playback.cache_duration = Some(12.5);
                observation.buffering = true;
            },
            vec![MpvEvent::FileLoaded {
                video_dimensions: None,
            }],
        );
        let wakeups = counting_wakeup(&session);
        assert!(wait_for(|| wakeups.load(Ordering::SeqCst) == 1));
        let mut state = opening;
        for change in changes(&session.drain_events()) {
            state.apply(&change);
        }
        assert_eq!(state.status, Status::Playing);
        assert_eq!(state.tracks.len(), 2);
        assert_eq!(state.tracks[1].kind, TrackKind::Subtitle);
        assert!(state.tracks[1].external);
        assert_eq!(state.chapters[0].title.as_deref(), Some("Intro"));
        assert_eq!(state.title.as_deref(), Some("Film"));
        assert_eq!(state.duration, Some(60.0));
        assert_eq!(state.cache_ahead, Some(12.5));
        assert!(state.buffering);
        let video = state.video.clone().expect("video geometry is reported");
        assert_eq!((video.width, video.height), (1920, 800));
        assert_eq!(video.display_aspect, 2.4);
        assert_eq!(state, session.snapshot(), "events and query agree");
    }

    #[test]
    fn a_stale_engine_report_does_not_undo_a_pause() {
        let (session, engine) = fake_session();
        let _ = session.dispatch(&open("/media/a.mkv"));
        loaded(&engine, 10.0);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));

        let _ = session.dispatch(&Command::SetPaused { paused: true });
        // A pass from before the engine processed the pause still says playing.
        publish_and_settle(&engine, |observation| {
            observation.playback.time_pos = Some(1.0);
        });
        assert_eq!(session.snapshot().status, Status::Paused);

        // A change in the engine's own report does win.
        publish_and_settle(&engine, |observation| observation.playback.paused = true);
        publish_and_settle(&engine, |observation| observation.playback.paused = false);
        assert_eq!(session.snapshot().status, Status::Playing);
    }

    #[test]
    fn an_engine_refusal_answers_failed_and_ends_the_load_with_an_error_event() {
        let (session, engine) = fake_session();
        *lock(&engine.refuse_with) = Some("loadfile refused".to_owned());
        let reply = session.dispatch(&open("/media/a.mkv"));
        assert_eq!(
            reply,
            DispatchResult::Failed {
                request_id: 1,
                message: "loadfile refused".to_owned(),
            }
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.status, Status::Ended);
        assert_eq!(snapshot.end_reason, Some(EndReason::Error));
        let events = session.drain_events();
        assert!(events.contains(&Event::Error {
            kind: ErrorKind::LoadFailed,
            message: "loadfile refused".to_owned(),
            request_id: Some(1),
        }));
    }

    #[test]
    fn asynchronous_failures_and_shutdown_become_events() {
        let (session, engine) = fake_session();
        let _ = session.dispatch(&open("/media/a.mkv"));
        loaded(&engine, 10.0);
        engine.publish(
            |_| {},
            vec![
                MpvEvent::CommandReply {
                    request_id: 1,
                    error: -12,
                },
                // A failure of a source the user already replaced is dropped.
                MpvEvent::EndFile {
                    reason: EndFileReason::Error(-13),
                    path: Some("/media/old.mkv".to_owned()),
                    diagnostic_messages: vec![],
                    last_duration: None,
                },
                MpvEvent::Shutdown,
            ],
        );
        // A query folds in the engine's report on the calling thread.
        assert_eq!(session.snapshot().end_reason, Some(EndReason::Quit));
        let events = session.drain_events();
        let errors: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Error {
                    kind, request_id, ..
                } => Some((*kind, *request_id)),
                _ => None,
            })
            .collect();
        assert_eq!(errors, vec![(ErrorKind::CommandFailed, Some(1))]);
        assert_eq!(events.last(), Some(&Event::Shutdown {}));
    }

    fn ended(reason: EndFileReason) -> MpvEvent {
        MpvEvent::EndFile {
            reason,
            path: None,
            diagnostic_messages: vec![],
            last_duration: None,
        }
    }

    fn file_loaded() -> MpvEvent {
        MpvEvent::FileLoaded {
            video_dimensions: None,
        }
    }

    /// Every status the drained events step through, in order.
    fn statuses_of(events: &[Event]) -> Vec<Status> {
        changes(events)
            .into_iter()
            .filter_map(|change| match change {
                SnapshotChange::Status { value } => Some(value),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn late_events_of_a_replaced_load_leave_the_new_one_opening() {
        let (session, engine) = fake_session();
        engine.report_loads.store(true, Ordering::SeqCst);
        let _ = session.dispatch(&open("/media/a.mkv"));
        engine.publish_for_loads(|_| {}, vec![(file_loaded(), Some(1))]);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        session.drain_events();

        let _ = session.dispatch(&open("/media/b.mkv"));
        // The first file stops, and even fails, after the second open; its path
        // is read late, so it names nothing or the new file.
        engine.publish_for_loads(
            |_| {},
            vec![
                (ended(EndFileReason::Stop), Some(1)),
                (file_loaded(), Some(1)),
                (ended(EndFileReason::Error(-13)), Some(1)),
            ],
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.status, Status::Opening);
        assert_eq!(snapshot.end_reason, None);
        let events = session.drain_events();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::Error { .. })),
            "{events:?}"
        );
        assert_eq!(statuses_of(&events), vec![Status::Opening]);

        engine.publish_for_loads(|_| {}, vec![(file_loaded(), Some(2))]);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
    }

    #[test]
    fn without_load_ids_only_a_failure_ends_an_opening_load() {
        let (session, engine) = fake_session();
        let _ = session.dispatch(&open("/media/a.mkv"));
        loaded(&engine, 10.0);
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        session.drain_events();

        let _ = session.dispatch(&open("/media/b.mkv"));
        engine.publish(|_| {}, vec![ended(EndFileReason::Stop)]);
        assert_eq!(session.snapshot().status, Status::Opening);
        assert_eq!(statuses_of(&session.drain_events()), vec![Status::Opening]);

        // The new file failing still ends the load.
        engine.publish(|_| {}, vec![ended(EndFileReason::Error(-13))]);
        let snapshot = session.snapshot();
        assert_eq!(snapshot.status, Status::Ended);
        assert_eq!(snapshot.end_reason, Some(EndReason::Error));
    }

    #[test]
    fn an_opens_tracks_are_selected_after_load_only_if_the_file_has_them() {
        let (session, engine) = fake_session();
        let _ = session.dispatch(&Command::Open {
            source: MediaSource {
                kind: SourceKind::Local,
                location: "/media/film.mkv".to_owned(),
            },
            resume_from: None,
            audio_track: Some(2),
            subtitle_track: Some(5),
        });
        let audio = |id| okp_mpv::Track {
            id,
            kind: okp_mpv::TrackKind::Audio,
            selected: id == 1,
            external: false,
            external_filename: None,
            default: id == 1,
            title: None,
            lang: None,
            codec: None,
            audio_channels: None,
        };
        engine.publish(
            |observation| observation.tracks = vec![audio(1), audio(2)],
            vec![file_loaded()],
        );
        assert!(wait_for(|| session.snapshot().status == Status::Playing));
        assert_eq!(
            engine.forwarded()[1..],
            [Command::SelectTrack {
                kind: TrackKind::Audio,
                id: Some(2),
            }],
            "only the audio track the file has is selected"
        );
        // The choice belongs to that load: a later report does not repeat it.
        publish_and_settle(&engine, |observation| {
            observation.playback.time_pos = Some(1.0);
        });
        assert_eq!(engine.forwarded().len(), 2);
    }

    #[test]
    fn mute_is_owned_by_the_session_and_forwarded() {
        let (session, engine) = fake_session();
        assert!(matches!(
            session.dispatch(&Command::SetMute { muted: true }),
            DispatchResult::Accepted { .. }
        ));
        assert!(session.snapshot().mute);
        assert_eq!(engine.forwarded(), vec![Command::SetMute { muted: true }]);
    }

    #[test]
    fn json_documents_are_checked_before_dispatch() {
        let (session, engine) = fake_session();
        let reply = session.dispatch_json(r#"{"schema_version":1,"command":{"set_volume":{}}}"#);
        assert!(reply.contains(r#""reason":"malformed""#), "{reply}");
        let reply = session.dispatch_json(r#"{"schema_version":9,"command":{"close":{}}}"#);
        assert!(
            reply.contains(r#""reason":"unsupported_schema_version""#),
            "{reply}"
        );
        assert!(engine.forwarded().is_empty());

        let reply = session
            .dispatch_json(r#"{"schema_version":1,"command":{"set_volume":{"volume":55.0}}}"#);
        assert_eq!(
            reply,
            r#"{"schema_version":1,"result":{"accepted":{"request_id":1}}}"#
        );
    }

    #[test]
    fn a_config_needs_absolute_directories_and_the_current_schema() {
        let mut config = SessionConfig {
            schema_version: SCHEMA_VERSION,
            app: okp_core::app::AppIdentity {
                name: "OK Player".to_owned(),
                platform: okp_core::app::Platform::MacOs,
            },
            data_dir: "/data".into(),
            config_dir: "relative/config".into(),
            cache_dir: "/cache".into(),
            hwdec: None,
            mpv_options: Vec::new(),
        };
        assert_eq!(
            validate_config(&config),
            Err("config_dir must be an absolute path".to_owned())
        );
        config.config_dir = "/config".into();
        assert_eq!(validate_config(&config), Ok(()));
        config.schema_version = 2;
        assert!(validate_config(&config).is_err());
    }

    /// The playlist through the session: folder scans, auto-advance, commands.
    mod playlist;
}
