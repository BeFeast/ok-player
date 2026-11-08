//! Executes `ok-player:` links (#849; contract in #845, parser and policy in
//! `okp_core::ok_player_uri`).
//!
//! A link from the command line, MPRIS `OpenUri` or the Open URL dialog becomes a
//! [`LinkLaunch`] in the launch queue every other launch goes through, so it keeps its turn.
//! When the queue releases it:
//!
//! 1. the window comes forward, or the desktop's startup notification is ended without it,
//!    by the action and what plays, never by whether the link is accepted;
//! 2. a link the core already rejected shows the reason's fixed text;
//! 3. the target policy, filesystem check included, runs off the UI thread while the request
//!    stays in flight and holds every later one;
//! 4. back on the UI thread, the plan is computed against what plays at that moment and
//!    executed, and only then is the request reported applied.
//!
//! Every request writes one log line: where it came from, its action, and either the plan it
//! ran or the reason it was rejected. No path or URL from a link is logged or shown.

use std::sync::mpsc::TryRecvError;

use okp_core::ok_player_uri::{
    self, Action, ActionRequest, FsProbe, PathFlavor, Plan, PlaybackSummary, QueueMode, Reason,
    Request, StdFsProbe, Target, Validated,
};

use super::*;

/// The toast a link shows in the Flatpak build, which does not run links in v1.
pub(crate) const FLATPAK_LINK_MESSAGE: &str = "Links are not supported in the Flatpak build";
pub(crate) const ADDED_TO_UP_NEXT_MESSAGE: &str = "Added to Up Next";
pub(crate) const PLAYS_NEXT_MESSAGE: &str = "Plays next";
pub(crate) const ALREADY_IN_UP_NEXT_MESSAGE: &str = "Already in Up Next";
/// Appended to a queue toast when the link's start time was ignored (queue items carry no
/// start offset in v1) or dropped as malformed.
pub(crate) const START_IGNORED_SUFFIX: &str = " (start time ignored)";
/// Shown when a link opens its target but its start time was malformed and dropped.
pub(crate) const START_DROPPED_MESSAGE: &str = "Link start time ignored: it is not a valid time";

/// How often the UI thread looks for the answer of a running link check.
const LINK_CHECK_POLL: Duration = Duration::from_millis(10);

/// Where a link came from, for the log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkOrigin {
    CommandLine,
    Mpris,
    OpenUrl,
}

impl LinkOrigin {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CommandLine => "command line",
            Self::Mpris => "MPRIS",
            Self::OpenUrl => "Open URL",
        }
    }
}

/// A link waiting in the launch queue, already parsed (parsing is pure and cheap). Whether the
/// window comes forward, whether the target passes, and what the plan is are all decided when
/// the queue releases it.
#[derive(Clone, Debug)]
pub(crate) struct LinkLaunch {
    /// The parsed request, or the action it named (if any) and why it was rejected.
    pub(crate) parsed: Result<ActionRequest, (Option<Action>, Reason)>,
    pub(crate) origin: LinkOrigin,
    /// The desktop's startup-notification id the launching process handed over.
    pub(crate) startup_id: Option<String>,
}

impl LinkLaunch {
    /// Parse `text`; `None` when it does not use the `ok-player` scheme at all.
    pub(crate) fn from_text(
        text: &str,
        origin: LinkOrigin,
        startup_id: Option<String>,
    ) -> Option<Self> {
        let parsed = match ok_player_uri::interpret(text)? {
            Request::Action(request) => Ok(request),
            other => Err((
                other.action(),
                other.rejection().unwrap_or(Reason::InvalidAction),
            )),
        };
        Some(Self {
            parsed,
            origin,
            startup_id,
        })
    }

    /// A command line rejected as a whole (a link together with other arguments).
    pub(crate) fn rejected_launch(reason: Reason, startup_id: Option<String>) -> Self {
        Self {
            parsed: Err((None, reason)),
            origin: LinkOrigin::CommandLine,
            startup_id,
        }
    }

    pub(crate) fn without_startup_id(self) -> Self {
        Self {
            startup_id: None,
            ..self
        }
    }

    fn action(&self) -> Option<Action> {
        match &self.parsed {
            Ok(request) => Some(request.action),
            Err((action, _)) => *action,
        }
    }

    fn ignored_keys(&self) -> &[String] {
        self.parsed
            .as_ref()
            .map_or(&[], |request| request.ignored_keys.as_slice())
    }
}

/// What the dispatcher asks of the window it serves. The application hands it the real window
/// and toast; a test records the calls.
pub(crate) trait LinkSurface {
    /// Bring the window forward, handing the desktop the launch's startup id.
    fn present(&self, startup_id: Option<&str>);
    /// End the desktop's startup notification without bringing the window forward.
    fn finish_startup(&self, startup_id: Option<&str>);
    fn toast(&self, message: &str);
}

/// Everything a released link needs besides the player state: the queue it came from, the
/// window, the filesystem probe and whether this is the Flatpak build.
#[derive(Clone)]
pub(crate) struct LinkDispatch {
    delivery: LaunchDeliveryGate,
    surface: Rc<dyn LinkSurface>,
    probe: &'static (dyn FsProbe + Sync),
    flatpak: bool,
}

impl LinkDispatch {
    /// The dispatcher of the application window, with the process-wide filesystem probe.
    pub(crate) fn for_window(
        window: &gtk::ApplicationWindow,
        status_toast: Rc<StatusToast>,
        delivery: LaunchDeliveryGate,
    ) -> Self {
        let flatpak = flatpak_install_detected(
            env::var_os("FLATPAK_ID").as_deref(),
            Path::new("/.flatpak-info").is_file(),
        );
        Self::new(
            delivery,
            Rc::new(WindowLinkSurface {
                window: window.downgrade(),
                status_toast,
            }),
            StdFsProbe::shared(),
            flatpak,
        )
    }

    pub(crate) fn new(
        delivery: LaunchDeliveryGate,
        surface: Rc<dyn LinkSurface>,
        probe: &'static (dyn FsProbe + Sync),
        flatpak: bool,
    ) -> Self {
        Self {
            delivery,
            surface,
            probe,
            flatpak,
        }
    }
}

struct WindowLinkSurface {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    status_toast: Rc<StatusToast>,
}

impl LinkSurface for WindowLinkSurface {
    fn present(&self, startup_id: Option<&str>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if let Some(startup_id) = startup_id {
            window.set_startup_id(startup_id);
        }
        window.present();
    }

    fn finish_startup(&self, startup_id: Option<&str>) {
        let (Some(startup_id), Some(window)) = (startup_id, self.window.upgrade()) else {
            return;
        };
        // Deprecated since GTK 4.10 in favour of presenting a toplevel with the id, which is
        // exactly what a link that must not raise the window cannot do.
        #[allow(deprecated)]
        gtk::prelude::WidgetExt::display(&window).notify_startup_complete(startup_id);
    }

    fn toast(&self, message: &str) {
        self.status_toast.show(message);
    }
}

/// Queue a link that arrived at runtime (MPRIS `OpenUri`, the Open URL dialog) behind every
/// launch already waiting.
pub(crate) fn dispatch_link_text(state: &Rc<RefCell<PlayerState>>, text: &str, origin: LinkOrigin) {
    let Some(link) = LinkLaunch::from_text(text, origin, None) else {
        return;
    };
    let delivery = state
        .borrow()
        .link_dispatch
        .as_ref()
        .map(|dispatch| Rc::clone(&dispatch.delivery));
    match delivery {
        Some(delivery) => deliver_launch_request(&delivery, state, LaunchRequest::Link(link)),
        None => log_link(
            origin,
            link.action(),
            "rejected: no window to serve it",
            &[],
        ),
    }
}

/// Open a local file or media URL in turn behind every launch already waiting (MPRIS
/// `OpenUri`). Without a launch queue (no window yet) it opens at once, as it always did.
pub(crate) fn deliver_open_request(state: &Rc<RefCell<PlayerState>>, item: PlaylistItem) {
    let delivery = state
        .borrow()
        .link_dispatch
        .as_ref()
        .map(|dispatch| Rc::clone(&dispatch.delivery));
    match (delivery, item) {
        (Some(delivery), item) => {
            deliver_launch_request(&delivery, state, LaunchRequest::Open(item))
        }
        (None, PlaylistItem::Local(path)) => load_media_path(state, path),
        (None, PlaylistItem::Url(url)) => load_media_url(state, url),
    }
}

/// What plays now, as the core's planner sees it.
pub(crate) fn link_playback_summary(state: &PlayerState) -> PlaybackSummary {
    if let Some(path) = &state.current_file {
        PlaybackSummary::playing(Target::Path(path.clone().into_os_string()))
    } else if let Some(url) = &state.current_url {
        PlaybackSummary::playing(Target::Url(url.clone()))
    } else {
        PlaybackSummary::idle()
    }
}

/// The launch queue released `link` with `ticket`: present (or not), then reject it at once or
/// start its target check off the UI thread.
pub(crate) fn start_link_request(
    delivery: &LaunchDeliveryGate,
    state: &Rc<RefCell<PlayerState>>,
    ticket: launch_args::LaunchTicket,
    link: LinkLaunch,
) -> LaunchApply {
    let dispatch = state.borrow().link_dispatch.clone();
    let Some(dispatch) = dispatch else {
        log_link(
            link.origin,
            link.action(),
            "rejected: no window to serve it",
            &[],
        );
        return LaunchApply::Applied;
    };

    // Before anything is judged: a page must not learn from focus changes whether its link
    // was accepted, so this depends on the action and on what plays only.
    let playback = link_playback_summary(&state.borrow());
    if ok_player_uri::presents_window(link.action(), &playback) {
        dispatch.surface.present(link.startup_id.as_deref());
    } else {
        dispatch.surface.finish_startup(link.startup_id.as_deref());
    }

    if dispatch.flatpak {
        log_link(
            link.origin,
            link.action(),
            "rejected: links are not supported in the Flatpak build",
            &[],
        );
        dispatch.surface.toast(FLATPAK_LINK_MESSAGE);
        return LaunchApply::Applied;
    }
    let request = match &link.parsed {
        Ok(request) => request.clone(),
        Err((action, reason)) => {
            reject_link(&dispatch, link.origin, *action, *reason, &[]);
            return LaunchApply::Applied;
        }
    };

    // The policy asks the filesystem about a local target, which can take as long as a
    // spun-down disk needs to wake: never on the UI thread.
    let (sender, receiver) = mpsc::channel();
    let probe = dispatch.probe;
    let started = thread::Builder::new()
        .name("okp-link-check".to_owned())
        .spawn(move || {
            let _ = sender.send(ok_player_uri::evaluate(
                &request,
                probe,
                PathFlavor::native(),
            ));
        });
    if started.is_err() {
        reject_link(
            &dispatch,
            link.origin,
            link.action(),
            Reason::PathUnavailable,
            link.ignored_keys(),
        );
        return LaunchApply::Applied;
    }

    let delivery = Rc::clone(delivery);
    let state = Rc::clone(state);
    let mut link = Some(link);
    glib::timeout_add_local(LINK_CHECK_POLL, move || {
        let verdict = match receiver.try_recv() {
            Ok(verdict) => verdict,
            Err(TryRecvError::Empty) => return glib::ControlFlow::Continue,
            // The check ended without answering (it panicked): the target was not checked.
            Err(TryRecvError::Disconnected) => Err(Reason::PathUnavailable),
        };
        if let Some(link) = link.take() {
            complete_link_request(&delivery, &state, ticket, link, verdict);
        }
        glib::ControlFlow::Break
    });
    LaunchApply::Pending
}

/// The target check of the link released with `ticket` answered. Apply it only while the queue
/// still holds it in flight, and only to a live player; then report it.
fn complete_link_request(
    delivery: &LaunchDeliveryGate,
    state: &Rc<RefCell<PlayerState>>,
    ticket: launch_args::LaunchTicket,
    link: LinkLaunch,
    verdict: Result<Validated, Reason>,
) {
    if !delivery.borrow().is_in_flight(ticket) {
        // The queue gave up on it at the deadline and later requests may already have run.
        log_link(
            link.origin,
            link.action(),
            "dropped: its check finished after the queue moved on",
            &[],
        );
        return;
    }
    if state.borrow().mpv.is_none() {
        // The engine went away while the check ran (window closing, engine being rebuilt).
        // The link goes back to the head of the queue and runs once a player is ready.
        log_link(
            link.origin,
            link.action(),
            "waiting: the player went away during its check",
            &[],
        );
        let next = {
            let mut delivery = delivery.borrow_mut();
            delivery.mark_player_unavailable();
            delivery.requeue(ticket, LaunchRequest::Link(link))
        };
        apply_released_launch_requests(delivery, state, next);
        return;
    }

    let dispatch = state.borrow().link_dispatch.clone();
    if let Some(dispatch) = dispatch {
        match verdict {
            Ok(validated) => execute_link(&dispatch, state, link.origin, &validated),
            Err(reason) => reject_link(
                &dispatch,
                link.origin,
                link.action(),
                reason,
                link.ignored_keys(),
            ),
        }
    }
    finish_launch_request(delivery, state, ticket);
}

/// Plan a validated link against what plays now and carry the plan out.
fn execute_link(
    dispatch: &LinkDispatch,
    state: &Rc<RefCell<PlayerState>>,
    origin: LinkOrigin,
    validated: &Validated,
) {
    let playback = link_playback_summary(&state.borrow());
    let plan = ok_player_uri::plan(validated, &playback);
    let (ran, toast) = match plan {
        Plan::Open { target, start } => {
            open_link_target(state, target, start);
            (
                "open",
                validated
                    .start_dropped()
                    .then(|| START_DROPPED_MESSAGE.to_owned()),
            )
        }
        Plan::Seek { start } => {
            seek_current_media_to(state, start);
            ("seek", None)
        }
        Plan::Queue {
            item,
            mode,
            start_ignored,
        } => match queue_link_item(state, item.clone(), mode) {
            QueueRequestOutcome::Queued(_) => {
                let message = match mode {
                    QueueMode::Append => ADDED_TO_UP_NEXT_MESSAGE,
                    QueueMode::AfterCurrent => PLAYS_NEXT_MESSAGE,
                };
                let suffix = if start_ignored || validated.start_dropped() {
                    START_IGNORED_SUFFIX
                } else {
                    ""
                };
                let ran = match mode {
                    QueueMode::Append => "queue (append)",
                    QueueMode::AfterCurrent => "queue (next)",
                };
                (ran, Some(format!("{message}{suffix}")))
            }
            QueueRequestOutcome::AlreadyQueued => (
                "queue (already queued)",
                Some(ALREADY_IN_UP_NEXT_MESSAGE.to_owned()),
            ),
            // The plan saw something loaded on this same turn, so these two only guard the
            // contract: nothing loaded falls back to opening, as an idle `add` does.
            QueueRequestOutcome::NothingLoaded => {
                open_link_target(state, item, validated.start());
                ("open", None)
            }
            QueueRequestOutcome::NothingQueueable => {
                let reason = match item {
                    Target::Path(_) => Reason::NotMediaFile,
                    Target::Url(_) => Reason::InvalidUrl,
                };
                reject_link(
                    dispatch,
                    origin,
                    Some(validated.action()),
                    reason,
                    validated.ignored_keys(),
                );
                return;
            }
        },
    };
    log_link(
        origin,
        Some(validated.action()),
        &format!("accepted: {ran}"),
        validated.ignored_keys(),
    );
    if let Some(toast) = toast {
        dispatch.surface.toast(&toast);
    }
}

/// Open a link's target through the ordinary open path, with the link's start time as the
/// explicit launch position. A local file builds the folder-sibling Up Next like a file-manager
/// open; the path is the link's own, never joined to any working directory.
fn open_link_target(state: &Rc<RefCell<PlayerState>>, target: Target, start: Option<f64>) {
    state.borrow_mut().next_launch_directives = Some(LaunchDirectives {
        resume_seconds: start,
        ..LaunchDirectives::default()
    });
    match target {
        Target::Path(path) => load_media_path(state, PathBuf::from(path)),
        Target::Url(url) => load_link_url(state, url),
    }
    // A load that went through consumed the directives; a refused one must not hand them to
    // whatever opens next.
    state.borrow_mut().next_launch_directives = None;
}

/// Put a link's target into Up Next. A page URL is remembered so it arms the replay cache when
/// it is reached; a direct media URL never does.
fn queue_link_item(
    state: &Rc<RefCell<PlayerState>>,
    item: Target,
    mode: QueueMode,
) -> QueueRequestOutcome {
    let mode = match mode {
        QueueMode::Append => QueueInsertMode::Append,
        QueueMode::AfterCurrent => QueueInsertMode::PlayNext,
    };
    let item = match item {
        Target::Path(path) => PlaylistItem::Local(PathBuf::from(path)),
        Target::Url(url) => PlaylistItem::Url(url),
    };
    let outcome = queue_playlist_items(state, vec![item.clone()], mode);
    if let (QueueRequestOutcome::Queued(_), PlaylistItem::Url(url)) = (outcome, item)
        && !ok_player_uri::is_direct_media_url(&url)
    {
        state.borrow_mut().replay_on_reach.insert(url);
    }
    outcome
}

fn reject_link(
    dispatch: &LinkDispatch,
    origin: LinkOrigin,
    action: Option<Action>,
    reason: Reason,
    ignored_keys: &[String],
) {
    log_link(
        origin,
        action,
        &format!("rejected: {}", reason.as_str()),
        ignored_keys,
    );
    dispatch.surface.toast(reason.message());
}

/// The one log line of a link. `ignored_keys` are already made log-safe by the core.
fn log_link(origin: LinkOrigin, action: Option<Action>, outcome: &str, ignored_keys: &[String]) {
    let action = action.map_or("(no action)", Action::as_str);
    let origin = origin.as_str();
    if ignored_keys.is_empty() {
        eprintln!("ok-player link from {origin}: {action} {outcome}");
    } else {
        eprintln!(
            "ok-player link from {origin}: {action} {outcome}; ignored keys: {}",
            ignored_keys.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;
    use std::sync::atomic::{AtomicBool, Ordering};

    use okp_core::ok_player_uri::FsKind;

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Effect {
        Present(Option<String>),
        FinishStartup(Option<String>),
        Toast(String),
    }

    fn toast(message: &str) -> Effect {
        Effect::Toast(message.to_owned())
    }

    #[derive(Default)]
    struct RecordingSurface {
        effects: RefCell<Vec<Effect>>,
    }

    impl RecordingSurface {
        fn take(&self) -> Vec<Effect> {
            std::mem::take(&mut self.effects.borrow_mut())
        }
    }

    impl LinkSurface for RecordingSurface {
        fn present(&self, startup_id: Option<&str>) {
            self.effects
                .borrow_mut()
                .push(Effect::Present(startup_id.map(str::to_owned)));
        }

        fn finish_startup(&self, startup_id: Option<&str>) {
            self.effects
                .borrow_mut()
                .push(Effect::FinishStartup(startup_id.map(str::to_owned)));
        }

        fn toast(&self, message: &str) {
            self.effects.borrow_mut().push(toast(message));
        }
    }

    /// The real filesystem behind a gate the test opens, standing in for a slow disk.
    struct GatedProbe {
        open: AtomicBool,
    }

    impl GatedProbe {
        fn new(open: bool) -> &'static Self {
            Box::leak(Box::new(Self {
                open: AtomicBool::new(open),
            }))
        }

        fn open(&self) {
            self.open.store(true, Ordering::Release);
        }
    }

    impl FsProbe for GatedProbe {
        fn probe(&self, path: &Path) -> FsKind {
            let give_up = Instant::now() + Duration::from_secs(5);
            while !self.open.load(Ordering::Acquire) && Instant::now() < give_up {
                thread::sleep(Duration::from_millis(1));
            }
            StdFsProbe::shared().probe(path)
        }
    }

    const STARTUP_ID: &str = "launch-token";

    /// A player with a real engine, a launch queue that already ran an empty cold start, and a
    /// dispatcher that records what it asks of the window.
    struct Player {
        root: tempfile::TempDir,
        state: Rc<RefCell<PlayerState>>,
        delivery: LaunchDeliveryGate,
        surface: Rc<RecordingSurface>,
    }

    impl Player {
        fn new(probe: &'static (dyn FsProbe + Sync), flatpak: bool) -> Self {
            Self::with_startup(LaunchRequest::Args(LaunchArgs::default()), probe, flatpak)
        }

        /// Not running yet: `startup` is the cold-start launch, and neither the window nor the
        /// engine is there until [`Player::become_ready`].
        fn with_startup(
            startup: LaunchRequest,
            probe: &'static (dyn FsProbe + Sync),
            flatpak: bool,
        ) -> Self {
            let root = tempfile::tempdir().expect("test folder");
            let mut settings = settings::SettingsStore::default();
            settings.set_replay_cache_enabled(true);
            let state = Rc::new(RefCell::new(PlayerState {
                history: history::HistoryStore::open_test(root.path().join("history.json")),
                settings,
                replay_cache: replay_cache::ReplayCacheRuntime::with_tool_for_tests(
                    root.path().join("replay"),
                ),
                ..PlayerState::default()
            }));
            let delivery: LaunchDeliveryGate = Rc::new(RefCell::new(
                launch_args::LaunchDelivery::with_startup(startup),
            ));
            let surface = Rc::new(RecordingSurface::default());
            state.borrow_mut().link_dispatch = Some(LinkDispatch::new(
                Rc::clone(&delivery),
                Rc::clone(&surface) as Rc<dyn LinkSurface>,
                probe,
                flatpak,
            ));
            Self {
                root,
                state,
                delivery,
                surface,
            }
        }

        fn running(probe: &'static (dyn FsProbe + Sync)) -> Self {
            let player = Self::new(probe, false);
            player.become_ready();
            player.surface.take();
            player
        }

        fn become_ready(&self) {
            mark_startup_window_mapped(&self.delivery, &self.state);
            self.state.borrow_mut().mpv =
                Some(Mpv::new().expect("libmpv is required by the existing GTK suite"));
            mark_startup_player_ready(&self.delivery, &self.state);
        }

        /// A media file in its own folder (so its folder Up Next is just itself), or with the
        /// given siblings.
        fn clip(&self, folder: &str, name: &str) -> PathBuf {
            let folder = self.root.path().join(folder);
            fs::create_dir_all(&folder).expect("clip folder");
            let path = folder.join(name);
            fs::write(&path, []).expect("clip file");
            path
        }

        /// Hand a command line to the second-instance entry point, as a forwarded launch.
        fn forward(&self, args: &[&str]) {
            deliver_command_line_launch(
                &self.delivery,
                &self.state,
                command_line_launch(
                    args.iter().map(Into::into).collect(),
                    Some(Path::new("/cwd-is-never-used")),
                    Some(STARTUP_ID.to_owned()),
                ),
            );
        }

        fn forward_link(&self, link: &str) {
            self.forward(&[link]);
        }

        fn playlist(&self) -> Vec<PlaylistItem> {
            self.state.borrow().playlist.items().to_vec()
        }

        fn current_file(&self) -> Option<PathBuf> {
            self.state.borrow().current_file.clone()
        }

        fn generation(&self) -> u64 {
            self.state.borrow().source_generation
        }
    }

    /// Run the main context until every launch has been applied.
    fn settle(context: &glib::MainContext, delivery: &LaunchDeliveryGate) {
        run_until(context, || delivery.borrow().is_idle());
        assert!(
            delivery.borrow().is_idle(),
            "the launch queue never settled"
        );
    }

    fn run_until(context: &glib::MainContext, mut done: impl FnMut() -> bool) {
        let give_up = Instant::now() + Duration::from_secs(5);
        while !done() && Instant::now() < give_up {
            context.iteration(false);
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn run_for(context: &glib::MainContext, duration: Duration) {
        let until = Instant::now() + duration;
        run_until(context, || Instant::now() >= until);
    }

    /// `encodeURIComponent` of raw bytes.
    fn encode(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|&byte| {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect()
    }

    fn path_link(action: &str, path: &Path, extra: &str) -> String {
        use std::os::unix::ffi::OsStrExt;
        format!(
            "ok-player://{action}?path={}{extra}",
            encode(path.as_os_str().as_bytes())
        )
    }

    fn url_link(action: &str, url: &str) -> String {
        format!("ok-player://{action}?url={}", encode(url.as_bytes()))
    }

    fn local(path: &Path) -> PlaylistItem {
        PlaylistItem::Local(path.to_path_buf())
    }

    macro_rules! main_context {
        ($context:ident) => {
            let _serial = SAVE_SHUTDOWN_CONTEXT_TEST_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let $context = glib::MainContext::default();
            let _context_guard = $context.acquire().expect("own the main context");
        };
    }

    #[test]
    fn forwarded_links_wait_for_readiness_then_run_in_arrival_order() {
        main_context!(context);
        let player = Player::new(GatedProbe::new(true), false);
        let a = player.clip("one", "A.mkv");
        let b = player.clip("two", "B.mkv");
        // The window maps before the engine exists; two links arrive meanwhile.
        mark_startup_window_mapped(&player.delivery, &player.state);
        player.forward_link(&path_link("open", &a, ""));
        player.forward_link(&path_link("add", &b, ""));
        run_for(&context, Duration::from_millis(50));
        assert_eq!(player.current_file(), None, "no engine, nothing applied");
        assert_eq!(player.surface.take(), [], "nothing is released yet");

        player.state.borrow_mut().mpv =
            Some(Mpv::new().expect("libmpv is required by the existing GTK suite"));
        mark_startup_player_ready(&player.delivery, &player.state);
        settle(&context, &player.delivery);

        assert_eq!(player.current_file(), Some(a.clone()));
        assert_eq!(player.playlist(), [local(&a), local(&b)]);
        assert_eq!(
            player.surface.take(),
            [
                // `open` comes forward; `add` is taken off the queue while A plays, so it
                // stays behind and only ends the startup notification.
                Effect::Present(Some(STARTUP_ID.to_owned())),
                Effect::FinishStartup(Some(STARTUP_ID.to_owned())),
                toast(ADDED_TO_UP_NEXT_MESSAGE),
            ]
        );
    }

    #[test]
    fn a_cold_open_followed_by_add_yields_both_even_while_the_first_check_is_slow() {
        main_context!(context);
        let probe = GatedProbe::new(false);
        let root = tempfile::tempdir().expect("clips");
        let a = root.path().join("one/A.mkv");
        let b = root.path().join("two/B.mkv");
        for path in [&a, &b] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, []).unwrap();
        }
        let CommandLineLaunch::Link(open_a) =
            command_line_launch(vec![path_link("open", &a, "").into()], None, None)
        else {
            panic!("a link launch");
        };
        let player = Player::with_startup(LaunchRequest::Link(open_a), probe, false);
        player.forward_link(&path_link("add", &b, ""));
        player.become_ready();

        // A's check is still running: A holds B, and nothing reaches the engine.
        run_for(&context, Duration::from_millis(50));
        assert_eq!(player.current_file(), None);
        assert!(player.playlist().is_empty());
        assert!(!player.delivery.borrow().is_idle());

        probe.open();
        settle(&context, &player.delivery);
        assert_eq!(player.current_file(), Some(a.clone()));
        assert_eq!(
            player.playlist(),
            [local(&a), local(&b)],
            "cold open A, add B"
        );
    }

    #[test]
    fn each_action_does_what_the_contract_says_in_each_player_state() {
        main_context!(context);
        for action in ["open", "add", "next"] {
            // Not running: the link is the cold-start launch.
            let probe = GatedProbe::new(true);
            let root = tempfile::tempdir().expect("clips");
            let target = root.path().join("T.mkv");
            fs::write(&target, []).unwrap();
            let CommandLineLaunch::Link(link) =
                command_line_launch(vec![path_link(action, &target, "").into()], None, None)
            else {
                panic!("a link launch");
            };
            let cold = Player::with_startup(LaunchRequest::Link(link), probe, false);
            cold.become_ready();
            settle(&context, &cold.delivery);
            assert_eq!(
                cold.current_file(),
                Some(target.clone()),
                "{action}: not running"
            );
            assert_eq!(cold.surface.take(), [Effect::Present(None)], "{action}");

            // Running, idle: every action plays the target and comes forward.
            let idle = Player::running(probe);
            let target = idle.clip("idle", "T.mkv");
            idle.forward_link(&path_link(action, &target, ""));
            settle(&context, &idle.delivery);
            assert_eq!(idle.current_file(), Some(target.clone()), "{action}: idle");
            assert_eq!(
                idle.surface.take(),
                [Effect::Present(Some(STARTUP_ID.to_owned()))],
                "{action}: idle"
            );

            // Running, playing X out of the folder [X, Y].
            let playing = Player::running(probe);
            let x = playing.clip("show", "X.mkv");
            let y = playing.clip("show", "Y.mkv");
            let target = playing.clip("elsewhere", "T.mkv");
            load_media_path(&playing.state, x.clone());
            assert_eq!(playing.playlist(), [local(&x), local(&y)]);
            playing.forward_link(&path_link(action, &target, ""));
            settle(&context, &playing.delivery);
            let effects = playing.surface.take();
            match action {
                "open" => {
                    assert_eq!(playing.current_file(), Some(target.clone()));
                    assert_eq!(effects, [Effect::Present(Some(STARTUP_ID.to_owned()))]);
                }
                "add" => {
                    assert_eq!(playing.current_file(), Some(x.clone()));
                    assert_eq!(playing.playlist(), [local(&x), local(&y), local(&target)]);
                    assert_eq!(
                        effects,
                        [
                            Effect::FinishStartup(Some(STARTUP_ID.to_owned())),
                            toast(ADDED_TO_UP_NEXT_MESSAGE),
                        ]
                    );
                }
                _ => {
                    assert_eq!(playing.current_file(), Some(x.clone()));
                    assert_eq!(playing.playlist(), [local(&x), local(&target), local(&y)]);
                    assert_eq!(
                        effects,
                        [
                            Effect::FinishStartup(Some(STARTUP_ID.to_owned())),
                            toast(PLAYS_NEXT_MESSAGE),
                        ]
                    );
                }
            }
        }
    }

    #[test]
    fn open_plays_from_the_link_start_and_builds_the_folder_up_next() {
        main_context!(context);
        let player = Player::running(GatedProbe::new(true));
        let a = player.clip("show", "A.mkv");
        let b = player.clip("show", "B.mkv");
        player.forward_link(&path_link("open", &b, "&t=1:30"));
        settle(&context, &player.delivery);

        let state = player.state.borrow();
        assert_eq!(state.current_file.as_deref(), Some(b.as_path()));
        assert_eq!(state.playlist.items(), [local(&a), local(&b)]);
        assert_eq!(
            state.pending_resume.map(|pending| pending.target),
            Some(launch_args::ResumeTarget {
                seconds: 90.0,
                origin: launch_args::ResumeOrigin::ExplicitLaunch,
            })
        );
        assert_eq!(state.next_launch_directives, None);
    }

    #[test]
    fn opening_the_current_file_with_a_start_time_seeks_instead_of_reloading() {
        main_context!(context);
        let player = Player::running(GatedProbe::new(true));
        let a = player.clip("show", "A.mkv");
        load_media_path(&player.state, a.clone());
        player.state.borrow_mut().pending_resume = None;
        let generation = player.generation();

        player.forward_link(&path_link("open", &a, "&t=42"));
        settle(&context, &player.delivery);

        let state = player.state.borrow();
        assert_eq!(state.source_generation, generation, "no reload");
        assert_eq!(state.current_file.as_deref(), Some(a.as_path()));
        assert_eq!(
            state.pending_resume,
            Some(PendingResume {
                source_generation: generation,
                target: launch_args::ResumeTarget {
                    seconds: 42.0,
                    origin: launch_args::ResumeOrigin::ExplicitLaunch,
                },
            })
        );
        drop(state);
        assert_eq!(
            player.surface.take(),
            [Effect::Present(Some(STARTUP_ID.to_owned()))]
        );
    }

    #[test]
    fn rejected_links_leave_playback_alone_and_raise_by_their_action_only() {
        main_context!(context);
        let player = Player::running(GatedProbe::new(true));
        let a = player.clip("show", "A.mkv");
        load_media_path(&player.state, a.clone());
        let generation = player.generation();
        let playlist = player.playlist();
        let missing = player.root.path().join("show/gone.mkv");
        let present = Effect::Present(Some(STARTUP_ID.to_owned()));
        let stay_behind = Effect::FinishStartup(Some(STARTUP_ID.to_owned()));

        let cases: Vec<(Vec<String>, Effect, Reason)> = vec![
            (
                vec![path_link("open", &missing, "")],
                present.clone(),
                Reason::NotFound,
            ),
            (
                vec![path_link("add", &missing, "")],
                stay_behind.clone(),
                Reason::NotFound,
            ),
            (
                vec![path_link("next", &player.root.path().join("show"), "")],
                stay_behind.clone(),
                Reason::NotMediaFile,
            ),
            (
                vec!["ok-player://open?path=%2F%2Fnas%2Fshare%2Fa.mkv".to_owned()],
                present.clone(),
                Reason::NetworkPath,
            ),
            (
                vec!["ok-player://add?url=edl%3A%2F%2Fa.mkv".to_owned()],
                stay_behind.clone(),
                Reason::UnsupportedScheme,
            ),
            (
                vec!["ok-player://open/media/list.m3u".to_owned()],
                present.clone(),
                Reason::PlaylistTarget,
            ),
            (
                vec!["ok-player://add?url=%G1".to_owned()],
                stay_behind.clone(),
                Reason::BadPercentEncoding,
            ),
            // No v1 action: raises like `open`, with the core's fixed text, never the name.
            (
                vec!["ok-player://pause".to_owned()],
                present.clone(),
                Reason::UnsupportedAction,
            ),
            (
                vec![path_link("open", &a, ""), a.display().to_string()],
                present.clone(),
                Reason::LaunchShape,
            ),
        ];
        for (args, raise, reason) in cases {
            let args = args.iter().map(String::as_str).collect::<Vec<_>>();
            player.forward(&args);
            settle(&context, &player.delivery);
            assert_eq!(
                player.surface.take(),
                [raise, toast(reason.message())],
                "{args:?}"
            );
            assert_eq!(
                player.generation(),
                generation,
                "{args:?}: playback untouched"
            );
            assert_eq!(player.current_file(), Some(a.clone()));
            assert_eq!(player.playlist(), playlist);
        }
    }

    #[test]
    fn a_dropped_or_ignored_start_time_is_noted_in_the_toast() {
        main_context!(context);
        let player = Player::running(GatedProbe::new(true));
        let a = player.clip("one", "A.mkv");
        let b = player.clip("two", "B.mkv");
        let c = player.clip("three", "C.mkv");

        player.forward_link(&path_link("open", &a, "&t=%ZZ"));
        player.forward_link(&path_link("add", &b, "&t=30"));
        player.forward_link(&path_link("next", &c, "&t=soon"));
        settle(&context, &player.delivery);

        let toasts = player
            .surface
            .take()
            .into_iter()
            .filter(|effect| matches!(effect, Effect::Toast(_)))
            .collect::<Vec<_>>();
        assert_eq!(
            toasts,
            [
                toast(START_DROPPED_MESSAGE),
                toast(&format!("{ADDED_TO_UP_NEXT_MESSAGE}{START_IGNORED_SUFFIX}")),
                toast(&format!("{PLAYS_NEXT_MESSAGE}{START_IGNORED_SUFFIX}")),
            ]
        );
        assert_eq!(player.playlist(), [local(&a), local(&c), local(&b)]);
    }

    #[test]
    fn direct_media_urls_from_links_never_arm_the_replay_cache_but_page_urls_do() {
        main_context!(context);
        // A closed port on the loopback address: nothing is fetched while the test runs.
        let direct = "http://127.0.0.1:9/clips/A001.MP4";
        let page = "http://127.0.0.1:9/watch?v=one";
        let armed = |player: &Player| {
            player
                .state
                .borrow()
                .replay_cache
                .armed_url()
                .map(str::to_owned)
        };

        let player = Player::running(GatedProbe::new(true));
        player.forward_link(&url_link("open", direct));
        settle(&context, &player.delivery);
        assert_eq!(player.state.borrow().current_url.as_deref(), Some(direct));
        assert_eq!(
            armed(&player),
            None,
            "a direct media URL streams, never copies"
        );

        player.forward_link(&url_link("open", page));
        settle(&context, &player.delivery);
        assert_eq!(player.state.borrow().current_url.as_deref(), Some(page));
        assert_eq!(
            armed(&player).as_deref(),
            Some(page),
            "a page URL as before"
        );

        // Queued from links while the page plays: each is judged when Up Next reaches it.
        let queued_direct = "http://127.0.0.1:9/clips/B002.mov";
        let queued_page = "http://127.0.0.1:9/watch?v=two";
        player.forward_link(&url_link("add", queued_direct));
        player.forward_link(&url_link("add", queued_page));
        settle(&context, &player.delivery);
        assert!(navigate_playlist(&player.state, 1));
        assert_eq!(
            player.state.borrow().current_url.as_deref(),
            Some(queued_direct)
        );
        assert_eq!(armed(&player), None);
        assert!(navigate_playlist(&player.state, 1));
        assert_eq!(
            player.state.borrow().current_url.as_deref(),
            Some(queued_page)
        );
        assert_eq!(armed(&player).as_deref(), Some(queued_page));

        // Outside links nothing changed: an ordinary URL open still arms, direct or not.
        load_media_url(&player.state, direct.to_owned());
        assert_eq!(armed(&player).as_deref(), Some(direct));
    }

    #[test]
    fn the_flatpak_build_says_links_are_unsupported_and_runs_nothing() {
        main_context!(context);
        let player = Player::new(GatedProbe::new(true), true);
        player.become_ready();
        player.surface.take();
        let a = player.clip("one", "A.mkv");
        player.forward_link(&path_link("open", &a, ""));
        settle(&context, &player.delivery);
        assert_eq!(player.current_file(), None);
        assert_eq!(
            player.surface.take(),
            [
                Effect::Present(Some(STARTUP_ID.to_owned())),
                toast(FLATPAK_LINK_MESSAGE),
            ]
        );
    }

    #[test]
    fn mpris_open_uri_and_open_url_wait_their_turn_behind_a_running_link_check() {
        main_context!(context);
        let probe = GatedProbe::new(false);
        let player = Player::running(probe);
        let a = player.clip("one", "A.mkv");
        let b = player.clip("two", "B.mkv");
        let c = player.clip("three", "C.mkv");

        // A link typed into Open URL is checked; meanwhile MPRIS opens a file and sends a link.
        dispatch_link_text(
            &player.state,
            &path_link("open", &a, ""),
            LinkOrigin::OpenUrl,
        );
        deliver_open_request(&player.state, local(&b));
        dispatch_link_text(&player.state, &path_link("add", &c, ""), LinkOrigin::Mpris);
        run_for(&context, Duration::from_millis(50));
        assert_eq!(
            player.current_file(),
            None,
            "the OpenUri file waits behind A"
        );

        probe.open();
        settle(&context, &player.delivery);
        assert_eq!(
            player.current_file(),
            Some(b.clone()),
            "A, then B replaces it"
        );
        assert_eq!(player.playlist(), [local(&b), local(&c)], "then C is added");
    }

    #[test]
    fn a_link_whose_player_goes_away_during_its_check_runs_on_the_next_player() {
        main_context!(context);
        let probe = GatedProbe::new(false);
        let player = Player::running(probe);
        let a = player.clip("one", "A.mkv");
        player.forward_link(&path_link("open", &a, ""));
        assert_eq!(
            player.surface.take(),
            [Effect::Present(Some(STARTUP_ID.to_owned()))]
        );

        // The engine is detached while A's check runs, and the detach site says so.
        let engine = player.state.borrow_mut().mpv.take();
        player.delivery.borrow_mut().mark_player_unavailable();
        probe.open();
        run_for(&context, Duration::from_millis(100));
        assert_eq!(player.current_file(), None, "never applied to no engine");
        assert!(!player.delivery.borrow().is_idle(), "A waits at the head");

        player.state.borrow_mut().mpv = engine;
        mark_startup_player_ready(&player.delivery, &player.state);
        settle(&context, &player.delivery);
        assert_eq!(player.current_file(), Some(a));
    }

    #[test]
    fn a_link_check_that_outlives_the_deadline_is_dropped_and_the_queue_moves_on() {
        main_context!(context);
        let probe = GatedProbe::new(false);
        let player = Player::running(probe);
        let a = player.clip("one", "A.mkv");
        let page = "http://127.0.0.1:9/watch?v=later";

        let CommandLineLaunch::Link(open_a) =
            command_line_launch(vec![path_link("open", &a, "").into()], None, None)
        else {
            panic!("a link launch");
        };
        let released = player
            .delivery
            .borrow_mut()
            .push(LaunchRequest::Link(open_a));
        drive_released_launch_requests(
            &player.delivery,
            &player.state,
            released,
            Duration::from_millis(30),
            |delivery, state, released| {
                let launch_args::ReleasedLaunch { ticket, request } = released;
                let LaunchRequest::Link(link) = request else {
                    panic!("a link");
                };
                start_link_request(delivery, state, ticket, link)
            },
        );
        // B arrives behind A; the queue gives up on A at the deadline and runs B.
        player.forward_link(&url_link("open", page));
        settle(&context, &player.delivery);
        assert_eq!(player.state.borrow().current_url.as_deref(), Some(page));

        // A's check answers at last: its stale ticket changes nothing.
        let generation = player.generation();
        probe.open();
        run_for(&context, Duration::from_millis(100));
        assert_eq!(player.generation(), generation);
        assert_eq!(player.state.borrow().current_url.as_deref(), Some(page));
        assert_eq!(player.current_file(), None);
    }

    #[test]
    fn a_link_never_joins_its_path_to_the_working_directory() {
        main_context!(context);
        let player = Player::running(GatedProbe::new(true));
        player.forward_link("ok-player://open?path=clips%2FA.mkv");
        settle(&context, &player.delivery);
        assert_eq!(player.current_file(), None);
        assert!(
            player
                .surface
                .take()
                .contains(&toast(Reason::RelativePath.message()))
        );
    }
}
