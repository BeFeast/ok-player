use super::*;

pub(crate) fn settings_advanced_page(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");
    page.append(&settings_raw_mpv_section(state, status_toast));
    page
}

pub(crate) fn settings_updates_page(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("okp-settings-page");
    page.append(&settings_updates_section(state, status_toast));
    page
}

pub(crate) fn settings_raw_mpv_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("mpv.conf");

    let detail = gtk::Label::new(Some(
        "Raw mpv key=value options. Startup-only options apply when playback starts.",
    ));
    detail.add_css_class("okp-update-status");
    detail.set_xalign(0.0);
    detail.set_width_chars(1);
    detail.set_max_width_chars(58);
    detail.set_wrap(true);
    section.append(&detail);

    let editor = gtk::TextView::new();
    editor.add_css_class("okp-mpv-conf-editor");
    editor.set_monospace(true);
    editor.set_wrap_mode(gtk::WrapMode::None);
    editor.set_accepts_tab(true);
    editor
        .buffer()
        .set_text(state.borrow().settings.raw_mpv_config());

    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-mpv-conf-scroller");
    scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroller.set_min_content_height(132);
    scroller.set_child(Some(&editor));
    section.append(&scroller);

    let status = gtk::Label::new(Some(
        "Managed by OK Player: config, terminal, idle, force-window, vo.",
    ));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);
    section.append(&status);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("okp-settings-action-row");
    actions.set_halign(gtk::Align::End);

    let reset = gtk::Button::with_label("Reset");
    reset.add_css_class("okp-settings-button");
    let reset_buffer = editor.buffer();
    let reset_state = Rc::clone(&state);
    let reset_toast = Rc::clone(&status_toast);
    let reset_status = status.clone();
    reset.connect_clicked(move |_| {
        reset_buffer.set_text("");
        apply_raw_mpv_config_setting("", &reset_status, &reset_state, &reset_toast);
    });
    actions.append(&reset);

    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("okp-settings-button");
    let apply_buffer = editor.buffer();
    let apply_state = Rc::clone(&state);
    let apply_toast = Rc::clone(&status_toast);
    let apply_status = status.clone();
    apply.connect_clicked(move |_| {
        let text = text_buffer_string(&apply_buffer);
        apply_raw_mpv_config_setting(&text, &apply_status, &apply_state, &apply_toast);
    });
    actions.append(&apply);

    section.append(&actions);
    section
}

pub(crate) fn apply_raw_mpv_config_setting(
    text: &str,
    status: &gtk::Label,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
) {
    let options = match parse_raw_mpv_config(text) {
        Ok(options) => options,
        Err(error) => {
            status.set_text(&format!("Line {}: {}", error.line, error.message));
            status_toast.show("mpv.conf has an error");
            return;
        }
    };

    let live_result = {
        let state = state.borrow();
        (!options.is_empty())
            .then(|| state.mpv.as_ref().map(|mpv| mpv.apply_options(&options)))
            .flatten()
    };

    let save_result = {
        let mut state = state.borrow_mut();
        state.settings.set_raw_mpv_config(text);
        state.settings.save()
    };
    if let Err(error) = save_result {
        eprintln!("Failed to save custom mpv.conf setting: {error}");
        status.set_text("Could not save mpv.conf.");
        status_toast.show("Could not save mpv.conf");
        return;
    }

    match live_result {
        Some(Ok(())) => {
            status.set_text("Saved and applied to the current mpv session.");
            status_toast.show("mpv.conf applied");
        }
        Some(Err(error)) => {
            eprintln!("Failed to hot-apply custom mpv.conf options: {error}");
            status.set_text("Saved. Live apply failed; restart playback to retry.");
            status_toast.show("Saved. Restart playback to retry");
        }
        None if options.is_empty() => {
            status.set_text("Reset saved. Restart playback to clear hot-applied options.");
            status_toast.show("mpv.conf reset");
        }
        None => {
            status.set_text("Saved. It applies when playback starts.");
            status_toast.show("mpv.conf saved");
        }
    }
}

pub(crate) fn text_buffer_string(buffer: &gtk::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), true)
        .to_string()
}

pub(crate) fn persistent_update_surface(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let surface =
        build_update_action_surface(LinuxUpdateViewKind::Persistent, state, status_toast, None);
    surface.add_css_class("okp-persistent-update");
    surface.set_halign(gtk::Align::Center);
    surface.set_valign(gtk::Align::Start);
    surface.set_margin_top(58);
    surface.set_margin_start(24);
    surface.set_margin_end(24);
    surface
}

/// The executable this process was started from, and what the file behind that
/// path looked like at the time, resolved once before anything can replace it.
///
/// Both halves have to be taken early. After a package manager replaces the
/// file, `/proc/self/exe` — which [`env::current_exe`] reads — resolves to
/// `<path> (deleted)` (measured on a Debian host: a rename over a running
/// binary turns the link into `/tmp/…/t1 (deleted)`), so asking later yields a
/// path that stats nothing; and an upgrade that landed between startup and the
/// first watch tick would otherwise be adopted as the baseline and never
/// mentioned.
static STARTUP_INSTALL: OnceLock<Option<(PathBuf, InstalledFileIdentity)>> = OnceLock::new();

/// Resolves the executable path and its identity. Called first thing in
/// `main`, so "before anything can replace it" is true by construction.
pub(crate) fn record_startup_install() {
    let _ = STARTUP_INSTALL.get_or_init(|| {
        let path = env::current_exe().ok()?;
        let identity = InstalledFileIdentity::of(&path)?;
        Some((path, identity))
    });
}

fn startup_install() -> Option<&'static (PathBuf, InstalledFileIdentity)> {
    STARTUP_INSTALL.get_or_init(|| None).as_ref()
}

pub(crate) fn startup_executable_path() -> Option<&'static Path> {
    startup_install().map(|(path, _)| path.as_path())
}

/// What identifies the file the process was started from, for the one question
/// the watch asks: is this still the same file?
///
/// dpkg unpacks a new file and renames it over the old one, so the path keeps
/// resolving while the inode behind it changes. Measured on the Debian QA host,
/// reinstalling the very same package left the size and the timestamp
/// untouched — dpkg restores the archive's mtime — and moved the inode from
/// 622920 to 623163, so the inode is the signal that actually fires. Size and
/// timestamp are compared too, for the in-place rewrite that keeps the inode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InstalledFileIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) size: u64,
    pub(crate) modified_seconds: i64,
    pub(crate) modified_nanoseconds: i64,
}

impl InstalledFileIdentity {
    /// Reads the identity of `path`, or nothing when the file cannot be
    /// stat'ed — a package being removed is not an upgrade to announce.
    pub(crate) fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).ok()?;
        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        })
    }
}

/// How often the shell asks whether the file it was started from is still the
/// same file.
///
/// The question is one `stat`, measured at 2 µs on the Debian QA host, so the
/// interval is set by how long a stale session may go unmentioned rather than
/// by what asking costs. The package database — 25 ms for `dpkg-query`, 190 ms
/// for `dpkg -S` on the same host — is never touched here: those run on a
/// worker thread, and only after the `stat` says the install actually changed.
const INSTALL_WATCH_INTERVAL: Duration = Duration::from_secs(30);

/// Whether a replacement of this install *under a running process* is
/// observable from the executable's own path.
///
/// Only the `.deb` lane is watched today, and each exclusion is a fact rather
/// than a shortcut:
///
/// * `rpm` reports an encoded version — `0.11.0~beta.1` where the build calls
///   itself `0.11.0-beta.1`, because rpm forbids `-` in a version. Since #709
///   the `.deb` lane encodes its version too, and
///   [`okp_core::package_version`] owns that rule; but the rpm form carries no
///   epoch, so it is a different string to read back, and this lane would also
///   need its own `rpm -qf`/`rpm -q` resolver. Watching it is a change of its
///   own rather than a line here — #712.
/// * A Flatpak update does not touch the deployment the running sandbox is on;
///   the old one stays mounted until the app restarts, so this file never
///   changes and the watch would never fire. That lane needs the deploy
///   directory, not the executable.
/// * The self-applying lanes reach the same truth through their own apply,
///   which already records a pending restart.
pub(crate) fn install_replacement_is_observable(kind: InstallKind) -> bool {
    matches!(kind, InstallKind::Deb)
}

/// What the watch is holding between ticks.
struct InstallWatch {
    kind: InstallKind,
    path: PathBuf,
    /// The file the session is running out of. Advanced whenever an
    /// observation has been dealt with, so one upgrade is reported once.
    baseline: InstalledFileIdentity,
    /// The version resolved for a replacement, kept with the identity it was
    /// resolved for so a version is never attributed to a different file.
    resolved: Option<(InstalledFileIdentity, String)>,
    /// A worker resolving the version for the identity it was started for.
    resolving: Option<(InstalledFileIdentity, mpsc::Receiver<Option<String>>)>,
    /// Whether a poller is already waiting on that worker, so a replacement
    /// landing mid-query does not leave two of them running.
    following: bool,
    /// How many times the replaced file has been answered with a version that
    /// is not ahead of the running one. dpkg writes the new file before it
    /// records the new version, so the first answer about a file it is still
    /// unpacking can be the *old* version; adopting that as settled would
    /// swallow the upgrade for the rest of the session.
    unconfirmed: UnconfirmedAnswers,
}

/// How many times the file at the watched path has been answered with a
/// version that is not ahead — together with *which* file those answers were
/// about.
///
/// The two belong together. A query outstanding when the file is replaced again
/// answers about the file it asked about, not the one on disk now; counting it
/// against the newer file — or settling the newer file on the strength of it —
/// would spend a budget that was never about it and lose that upgrade for good.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct UnconfirmedAnswers(Option<(InstalledFileIdentity, u8)>);

impl UnconfirmedAnswers {
    /// Answers so far about `identity`. A different file starts from zero.
    pub(crate) fn about(self, identity: InstalledFileIdentity) -> u8 {
        match self.0 {
            Some((counted, answers)) if counted == identity => answers,
            _ => 0,
        }
    }

    /// Records one more answer about `identity`, dropping any count that was
    /// about a different file.
    pub(crate) fn count(&mut self, identity: InstalledFileIdentity) {
        let answers = self.about(identity).saturating_add(1);
        self.0 = Some((identity, answers));
    }

    pub(crate) fn clear(&mut self) {
        self.0 = None;
    }
}

/// How many times a replaced file may be answered with a version that is not
/// ahead before the watch takes that for the truth.
///
/// A package transaction records its new version seconds after it writes the
/// file, so a couple of re-asks — one per [`INSTALL_WATCH_INTERVAL`] — cover
/// the lag comfortably; past that, a version that is still not newer is not
/// newer. Re-asking is bounded because it costs a subprocess, and a genuine
/// reinstall of the same version would otherwise re-ask for as long as the
/// player runs.
pub(crate) const INSTALLED_VERSION_CONFIRMATIONS: u8 = 4;

/// What a watch tick does with the answer it got about a replaced file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplacementAnswer {
    /// The session is behind the installed build; the state now says so.
    Behind,
    /// Not ahead, and the packaging tool has had enough chances to say
    /// otherwise: this file is what the session is running.
    Settled,
    /// Not ahead *yet*. dpkg writes the new file before it records the new
    /// version, so an answer about a file it is still unpacking can be the old
    /// version; taking that as settled would swallow the upgrade for the rest
    /// of the session.
    AskAgain,
    /// A check, a download or an apply is in flight. What is on disk is a
    /// standing fact rather than an event, so it is offered again.
    Deferred,
}

/// Decides what a tick makes of one answer, given how many times this file has
/// already been answered with a version that is not ahead.
pub(crate) fn classify_replacement_answer(
    outcome: &Result<(), UpdateTransitionError>,
    answers_so_far: u8,
) -> ReplacementAnswer {
    match outcome {
        Ok(()) => ReplacementAnswer::Behind,
        Err(UpdateTransitionError::NotBehindInstalled { .. }) => {
            if answers_so_far.saturating_add(1) >= INSTALLED_VERSION_CONFIRMATIONS {
                ReplacementAnswer::Settled
            } else {
                ReplacementAnswer::AskAgain
            }
        }
        Err(_) => ReplacementAnswer::Deferred,
    }
}

/// Watches for the install being replaced underneath this process (#707).
///
/// A package manager can upgrade a `.deb` install while the player runs — the
/// APT channel makes it routine — and nothing in the app puts the new build
/// there. The session keeps executing the old binary, and until this watch
/// noticed it, it also kept saying it was up to date.
pub(crate) fn start_install_replacement_watch(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    kind: InstallKind,
) {
    if !install_replacement_is_observable(kind) {
        return;
    }
    let Some((path, baseline)) = startup_install() else {
        return;
    };
    let watch = Rc::new(RefCell::new(InstallWatch {
        kind,
        path: path.clone(),
        baseline: *baseline,
        resolved: None,
        resolving: None,
        following: false,
        unconfirmed: UnconfirmedAnswers::default(),
    }));
    glib::timeout_add_local(INSTALL_WATCH_INTERVAL, move || {
        poll_install_replacement(&watch, &state, &status_toast);
        glib::ControlFlow::Continue
    });
}

/// One tick of the watch: a `stat`, and nothing else unless the file moved.
fn poll_install_replacement(
    watch: &Rc<RefCell<InstallWatch>>,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
) {
    let shared = Rc::clone(watch);
    let mut watch = watch.borrow_mut();
    let Some(current) = InstalledFileIdentity::of(&watch.path) else {
        // The path stopped resolving: the package is mid-transaction or gone.
        // Neither is an upgrade this session can ask the user to restart into.
        return;
    };
    if current == watch.baseline {
        watch.resolved = None;
        watch.resolving = None;
        watch.unconfirmed.clear();
        return;
    }

    if let Some((identity, receiver)) = watch.resolving.take() {
        match receiver.try_recv() {
            Ok(Some(version)) => watch.resolved = Some((identity, version)),
            // The package manager could not name a version — it may be
            // mid-transaction, holding its own database. Saying nothing is the
            // honest answer for now, but adopting this file as the one the
            // session runs would lose the upgrade for good, so the question is
            // re-asked on the same budget as an answer that is not ahead. An
            // answer about a file that has since been replaced again is about
            // neither: it is dropped, and the file on disk now is asked about
            // from scratch.
            Ok(None) if identity == current => {
                watch.unconfirmed.count(current);
                if watch.unconfirmed.about(current) >= INSTALLED_VERSION_CONFIRMATIONS {
                    watch.baseline = current;
                    watch.unconfirmed.clear();
                }
                return;
            }
            Ok(None) => {}
            Err(mpsc::TryRecvError::Empty) => {
                watch.resolving = Some((identity, receiver));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    match watch.resolved.clone() {
        Some((identity, version)) if identity == current => {
            eprintln!(
                "Installed build observed: installed={version} running={}",
                state.borrow().linux_update.lifecycle.running_version()
            );
            let outcome = state
                .borrow_mut()
                .linux_update
                .lifecycle
                .installed_version_observed(version)
                .map(|_| ());
            watch.resolved = None;
            match classify_replacement_answer(&outcome, watch.unconfirmed.about(current)) {
                ReplacementAnswer::Behind => {
                    watch.baseline = current;
                    watch.unconfirmed.clear();
                    eprintln!(
                        "Update session: {}",
                        state.borrow().linux_update.describe().updates_message
                    );
                    refresh_linux_update_views(state);
                    status_toast.show("Update installed — restart to use it");
                }
                ReplacementAnswer::Settled => {
                    watch.baseline = current;
                    watch.unconfirmed.clear();
                    eprintln!("Installed build is not ahead of this session");
                }
                ReplacementAnswer::AskAgain => {
                    watch.unconfirmed.count(current);
                    eprintln!("Installed build is not ahead of this session; asking again");
                }
                ReplacementAnswer::Deferred => {
                    eprintln!("Update lifecycle deferred the installed version: {outcome:?}");
                }
            }
        }
        _ => {
            // A different file than anything resolved so far: ask the package
            // manager which version it is, off the main thread.
            let (sender, receiver) = mpsc::channel();
            let kind = watch.kind;
            let path = watch.path.clone();
            std::thread::spawn(move || {
                let _ = sender.send(installed_package_version(kind, &path));
            });
            watch.resolved = None;
            watch.resolving = Some((current, receiver));
            drop(watch);
            follow_the_running_resolver(shared, state, status_toast);
        }
    }
}

/// Picks the resolver's answer up as soon as it lands.
///
/// The query takes a couple of hundred milliseconds; waiting for the next slow
/// tick to collect it would mean a stale session goes unmentioned for two
/// intervals rather than one, which is what the first live run on the Debian
/// QA host showed. This polls only while a query is outstanding, so it costs
/// nothing when nothing is happening.
fn follow_the_running_resolver(
    watch: Rc<RefCell<InstallWatch>>,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
) {
    if watch.borrow().following {
        return;
    }
    watch.borrow_mut().following = true;
    let state = Rc::clone(state);
    let status_toast = Rc::clone(status_toast);
    glib::timeout_add_local(Duration::from_millis(250), move || {
        if watch.borrow().resolving.is_none() {
            watch.borrow_mut().following = false;
            return glib::ControlFlow::Break;
        }
        poll_install_replacement(&watch, &state, &status_toast);
        glib::ControlFlow::Continue
    });
}

/// The **build** version of the package that owns `path`.
///
/// Only the `.deb` lane is answered — see
/// [`install_replacement_is_observable`]. dpkg's `Version:` is not the build
/// version: it is the Debian encoding of it (`1:0.11.0~beta.0.209` for the
/// build that calls itself `0.11.0-beta.0.209`), because the raw build version
/// sorts above the release that follows it and the APT lane could not ship one
/// (#709). So the answer is read back through
/// [`package_version::build_version_from_debian_version`], the inverse of the
/// rule the packaging applies, and what reaches the lifecycle is the same
/// string family the running version belongs to — one comparator, not two.
///
/// A version that is not one this packaging emits — no epoch, or a Debian
/// revision somebody's rebuild added — establishes nothing and is answered as
/// "unknown" rather than compared. The watch treats that like a package
/// database that has not caught up: it re-asks, bounded, and then settles
/// quietly. Announcing an ordering the two comparators disagree about is the
/// bug this closes, not a state to reach by another route.
///
/// The owning package is looked up rather than assumed, so a renamed or derived
/// package is still answered correctly.
fn installed_package_version(kind: InstallKind, path: &Path) -> Option<String> {
    use std::ffi::OsStr;
    if kind != InstallKind::Deb {
        return None;
    }
    let dpkg = find_executable("dpkg")?;
    let dpkg_query = find_executable("dpkg-query")?;
    let owner = command_output(&dpkg, &[OsStr::new("-S"), path.as_os_str()])?;
    let package = owning_package_name(&owner, path)?;
    let version = command_output(
        &dpkg_query,
        &[
            OsStr::new("-W"),
            OsStr::new("-f=${Version}"),
            OsStr::new(&package),
        ],
    )?;
    build_version_of_installed_deb(version.trim())
}

/// The build version behind a `dpkg-query` answer, or nothing when the answer
/// does not establish one. Split out so the translation is testable without a
/// package database.
pub(crate) fn build_version_of_installed_deb(reported: &str) -> Option<String> {
    if reported.is_empty() {
        return None;
    }
    let build_version = package_version::build_version_from_debian_version(reported);
    if build_version.is_none() {
        eprintln!(
            "Installed build not established: dpkg reports {reported}, which this packaging does not emit"
        );
    }
    build_version
}

/// The package name out of a `dpkg -S` answer (`ok-player: /usr/bin/…`).
///
/// The line is matched against the path that was asked about, because `dpkg`
/// answers a diverted file with the diversion first — prose on the left of the
/// colon and the same path on the right — and the package that owns the file
/// somewhere further down. A path several packages claim is reported as a
/// comma-separated list, whose first entry is the one whose version describes
/// the file.
pub(crate) fn owning_package_name(dpkg_search_output: &str, path: &Path) -> Option<String> {
    dpkg_search_output.lines().find_map(|line| {
        let (packages, owned) = line.rsplit_once(':')?;
        if Path::new(owned.trim()) != path {
            return None;
        }
        let name = packages.split(',').next()?.trim();
        // A diversion line has a sentence where a package name would be.
        (!name.is_empty() && !name.contains(char::is_whitespace)).then(|| name.to_owned())
    })
}

/// The package name the archive publishes, and the one `apt-cache policy` is asked about.
pub(crate) const APT_PACKAGE_NAME: &str = "ok-player";

/// What apt can actually deliver for OK Player on this machine (#725).
///
/// `UpdateCapability::SystemManaged` used to be derived from the install kind alone, so the
/// app told every `.deb` install to update through apt without ever asking whether apt had
/// heard of the package. On the machine that reported #725 it had not: the package came from
/// a downloaded file, `apt-cache policy ok-player` listed only `/var/lib/dpkg/status`, and the
/// desktop updater the app offered correctly answered that everything was up to date.
///
/// So the question is put to apt, and the answer is apt's own. A missing `apt-cache`, or one
/// that fails, is [`PackageSourceEvidence::Unestablished`] — not evidence that no source
/// exists, only that the app could not find out, which is a state of its own that still must
/// not claim a delivery path.
pub(crate) fn observe_package_source(install_kind: InstallKind) -> PackageSourceEvidence {
    if !install_kind.delivery_must_be_established() {
        return PackageSourceEvidence::Unestablished;
    }
    // In the C locale, because `Installed:` and `Candidate:` are prose and apt translates
    // them. On a machine whose session is not English the version table would still parse —
    // the origins are URLs — but the candidate line would not, and a subscribed machine with
    // an update waiting would be reported as current on its suite. That is #725 again, in the
    // half of the world that does not run in English.
    let output = Command::new("apt-cache")
        .args(["policy", APT_PACKAGE_NAME])
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let Some(output) = output else {
        return PackageSourceEvidence::Unestablished;
    };
    package_source_from_policy(&output)
}

fn command_output(program: &Path, args: &[&std::ffi::OsStr]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Set on the process a restart starts, telling it to wait for this one to let
/// go of the application's bus name before it registers its own.
const RESTART_AWAIT_PRIMARY: &str = "OKP_RESTART_AWAIT_PRIMARY";

/// Restarts into the build a package manager installed underneath this process
/// (#707).
///
/// Nothing else is going to relaunch the player here: the self-applying lane
/// hands off to an updater that waits for this process to exit, and a package
/// manager finished long before the user pressed anything. So the replacement
/// is started from the path this process was launched from — which now holds
/// the new build — and only then is the window closed. A window that declines
/// to close leaves the replacement waiting on a bus name that is never given
/// up; it stops waiting after ten seconds, finds this process still primary,
/// and hands over to it rather than opening a second player.
pub(crate) fn restart_into_installed_build(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    let Some(path) = startup_executable_path() else {
        status_toast.show("Could not restart OK Player");
        return;
    };
    save_current_progress(state, false);
    let spawned = Command::new(path)
        .env(RESTART_AWAIT_PRIMARY, "1")
        .stdin(Stdio::null())
        .spawn();
    if let Err(error) = spawned {
        eprintln!("Could not start the installed build at {path:?}: {error}");
        status_toast.show("Could not restart OK Player");
        return;
    }
    if !close_player_window() {
        eprintln!("Could not close the player window to restart into the installed build");
        status_toast.show("Could not restart OK Player");
    }
}

/// Held at startup by the process a restart started, until the process it
/// replaces has let go of the application's bus name.
///
/// GTK routes a second instance to the primary one, so a replacement that
/// registered while the outgoing process still owned the name would hand its
/// command line to the process it exists to replace and exit — the restart
/// would silently do nothing. The bus is asked directly rather than timed,
/// and the wait is bounded: after ten seconds the replacement starts anyway
/// rather than never appearing.
pub(crate) fn await_replaced_instance_shutdown(application_id: &str) {
    if env::var_os(RESTART_AWAIT_PRIMARY).is_none() {
        return;
    }
    let Ok(connection) =
        gtk::gio::bus_get_sync(gtk::gio::BusType::Session, None::<&gtk::gio::Cancellable>)
    else {
        return;
    };
    let Ok(reply_type) = glib::VariantTy::new("(b)") else {
        return;
    };
    for _ in 0..200 {
        let owned = connection.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(application_id,).to_variant()),
            Some(reply_type),
            gtk::gio::DBusCallFlags::NONE,
            1_000,
            None::<&gtk::gio::Cancellable>,
        );
        match owned {
            Ok(reply) => {
                if !reply.child_value(0).get::<bool>().unwrap_or(false) {
                    return;
                }
            }
            // Without an answer there is nothing to wait for; starting is
            // better than hanging on the way to a window.
            Err(_) => return,
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("The outgoing instance still owns {application_id}; starting anyway");
}

/// The install kind of this process, decided by `okp_core` from evidence this
/// shell gathers. Resolved once: the package-ownership question costs a
/// subprocess, and the answer cannot change while the process runs.
static INSTALL_KIND: OnceLock<InstallKind> = OnceLock::new();

/// Blocks until the install kind is known, running the probe if nothing else
/// has. Never call this from the GTK main thread before
/// [`start_install_kind_probe`] has had a chance to resolve it — the point of
/// the probe is that the ownership query does not stall a frame.
pub(crate) fn install_kind() -> InstallKind {
    *INSTALL_KIND.get_or_init(detect_process_install_kind)
}

/// Facts about the running process, gathered for [`detect_install_kind`].
/// `ask_package_manager` decides whether the ownership question is asked at
/// all: it costs a subprocess, and the cheap signals settle Flatpak and a
/// mounted AppImage without it.
pub(crate) fn process_install_evidence(ask_package_manager: bool) -> InstallEvidence {
    let executable_path = startup_executable_path()
        .map(Path::to_path_buf)
        .or_else(|| env::current_exe().ok())
        .map(|path| path.to_string_lossy().into_owned());
    let package_ownership = if ask_package_manager {
        query_package_ownership(executable_path.as_deref())
    } else {
        PackageOwnership::Unknown
    };
    InstallEvidence {
        flatpak_id: env::var("FLATPAK_ID").ok(),
        flatpak_info_present: Path::new("/.flatpak-info").is_file(),
        appimage_path: env::var("APPIMAGE").ok(),
        appdir_path: env::var("APPDIR").ok(),
        executable_path,
        package_ownership,
        // The Velopack `current/` layout is a Windows install; on Linux
        // Velopack updates the AppImage file itself.
        velopack_layout_present: false,
    }
}

/// Resolves the install kind for this process.
///
/// The cheap evidence is collected first, and a package manager is asked only
/// when that evidence leaves the answer open — which is exactly the deb, rpm
/// and extract-and-run cases. A Flatpak or a mounted AppImage never spawns
/// anything.
pub(crate) fn detect_process_install_kind() -> InstallKind {
    if let Some(forced) = forced_install_kind() {
        eprintln!("Update install kind: forced={forced} package-kind={APP_PACKAGE_KIND}");
        return forced;
    }
    let cheap = process_install_evidence(false);
    let kind = match detect_install_kind(&cheap) {
        InstallKind::DevBuild => detect_install_kind(&process_install_evidence(true)),
        settled => settled,
    };
    eprintln!(
        "Update install kind: detected={kind} capability={:?} package-kind={APP_PACKAGE_KIND}",
        kind.capability()
    );
    kind
}

/// QA override for the install kind. GUI verification of the system-managed
/// lane must be possible on a host that is not running that package, and the
/// alternative — believing a build-time stamp — is the parallel decision this
/// change exists to delete.
fn forced_install_kind() -> Option<InstallKind> {
    let value = env::var("OKP_UPDATE_INSTALL_KIND").ok()?;
    parse_install_kind(value.trim())
}

pub(crate) fn parse_install_kind(value: &str) -> Option<InstallKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "deb" => Some(InstallKind::Deb),
        "rpm" => Some(InstallKind::Rpm),
        "appimage" => Some(InstallKind::AppImage),
        "flatpak" => Some(InstallKind::Flatpak),
        "dev-build" | "development" => Some(InstallKind::DevBuild),
        _ => None,
    }
}

/// Asks the package managers this system has whether they own `path`. An
/// absent tool is not an answer, so the result stays `Unknown` unless a
/// manager was actually asked — core treats a failed query as "not packaged"
/// rather than guessing.
fn query_package_ownership(path: Option<&str>) -> PackageOwnership {
    let Some(path) = path else {
        return PackageOwnership::Unknown;
    };
    let mut asked = false;
    for (tool, args) in [("dpkg", ["-S"]), ("rpm", ["-qf"])] {
        let Some(executable) = find_executable(tool) else {
            continue;
        };
        let Ok(output) = Command::new(executable)
            .args(args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            continue;
        };
        asked = true;
        if output.status.success() {
            return if tool == "dpkg" {
                PackageOwnership::Dpkg
            } else {
                PackageOwnership::Rpm
            };
        }
    }
    if asked {
        PackageOwnership::Unowned
    } else {
        PackageOwnership::Unknown
    }
}

/// Resolves the install kind off the main thread and adopts it, so the first
/// frame is never held up by a package-manager query. Until it lands the
/// session says nothing updates this install, which is the honest answer to
/// "how was I installed?" before anything has looked.
pub(crate) fn start_install_kind_probe(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    then_check: bool,
) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send((install_kind(), take_pending_restart()));
    });
    glib::timeout_add_local(Duration::from_millis(20), move || {
        match receiver.try_recv() {
            Ok((kind, pending)) => {
                adopt_detected_install_kind(&state, kind, pending);
                refresh_linux_update_views(&state);
                start_install_replacement_watch(Rc::clone(&state), Rc::clone(&status_toast), kind);
                if then_check && update_checks_are_possible(kind) {
                    check_updates_on_startup(Rc::clone(&state), Rc::clone(&status_toast));
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

/// Whether this install kind has anything to check. A system tool that owns
/// the payload outright is not polled, and a dev build has no feed at all.
pub(crate) fn update_checks_are_possible(kind: InstallKind) -> bool {
    kind.capability() != UpdateCapability::Unmanaged && kind.discovers_versions()
}

/// Replaces the placeholder session with one for the detected install kind,
/// settling any restart that was pending from the previous process.
pub(crate) fn adopt_detected_install_kind(
    state: &Rc<RefCell<PlayerState>>,
    kind: InstallKind,
    pending_restart: Option<String>,
) {
    let mut state = state.borrow_mut();
    state.linux_update = LinuxUpdateSession::resumed(kind, pending_restart);
    if let Some(preview) = linux_update_preview_session(kind) {
        state.linux_update = preview;
    }
    eprintln!(
        "Update session: {}",
        state.linux_update.describe().updates_message
    );
}

/// The file that carries an in-flight update across the restart that finishes
/// it. Written before the hand-off, read once by the process that comes back:
/// without it, a restart that silently came up on the old binary is invisible,
/// which is #660.
pub(crate) fn pending_restart_marker_path() -> PathBuf {
    linux_update_cache_dir().join("pending-restart")
}

pub(crate) fn record_pending_restart(version: &str) {
    let path = pending_restart_marker_path();
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        eprintln!("Failed to prepare the pending-restart marker: {error}");
        return;
    }
    if let Err(error) = fs::write(&path, version) {
        eprintln!("Failed to record the pending update restart: {error}");
    }
}

/// Reads the pending target and removes the marker: it settles exactly one
/// restart, so a stale record cannot keep reporting an update that already
/// landed — or already failed.
pub(crate) fn take_pending_restart() -> Option<String> {
    let path = pending_restart_marker_path();
    let recorded = fs::read_to_string(&path).ok();
    if recorded.is_some() {
        let _ = fs::remove_file(&path);
    }
    recorded
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
}

fn build_update_action_surface(
    kind: LinuxUpdateViewKind,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    check: Option<&gtk::Button>,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("okp-update-action-surface");
    root.set_accessible_role(gtk::AccessibleRole::Group);
    root.update_property(&[gtk::accessible::Property::Label("Available update actions")]);

    let copy = gtk::Box::new(gtk::Orientation::Vertical, 3);
    copy.set_hexpand(true);
    let title = gtk::Label::new(Some("Update status"));
    title.add_css_class("okp-update-action-title");
    title.set_xalign(0.0);
    let status = gtk::Label::new(None);
    status.add_css_class("okp-update-action-detail");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(if kind == LinuxUpdateViewKind::Persistent {
        52
    } else {
        44
    });
    status.set_wrap(true);
    // The actionable path out of "apt cannot deliver this" (#700, #725). It is on the
    // Settings surface only: the offer banner sits over video, where a block of shell
    // commands is not something a user can act on, and its button copies them anyway.
    let setup = gtk::Label::new(None);
    setup.add_css_class("okp-update-setup-commands");
    setup.set_xalign(0.0);
    setup.set_selectable(true);
    setup.set_wrap(true);
    setup.set_width_chars(1);
    setup.set_max_width_chars(58);
    setup.set_visible(false);
    copy.append(&title);
    copy.append(&status);
    copy.append(&setup);
    root.append(&copy);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::Center);

    let skip = gtk::Button::with_label("Skip this version");
    skip.add_css_class("okp-settings-button");
    skip.update_property(&[gtk::accessible::Property::Label("Skip this update version")]);
    let skip_state = Rc::clone(&state);
    let skip_toast = Rc::clone(&status_toast);
    skip.connect_clicked(move |_| skip_current_update(&skip_state, &skip_toast));
    actions.append(&skip);

    let system = gtk::Button::with_label(SYSTEM_UPDATER_ACTION_LABEL);
    system.add_css_class("okp-settings-button");
    system.update_property(&[gtk::accessible::Property::Label(
        "Open the system software updater",
    )]);
    let system_toast = Rc::clone(&status_toast);
    system.connect_clicked(move |_| open_system_software_surface(&system_toast));
    actions.append(&system);

    let primary = gtk::Button::with_label("Update");
    primary.add_css_class("okp-update-primary-button");
    primary.update_property(&[gtk::accessible::Property::Label("Update OK Player")]);
    let primary_state = Rc::clone(&state);
    let primary_toast = Rc::clone(&status_toast);
    primary.connect_clicked(move |_| {
        take_primary_update_action(Rc::clone(&primary_state), Rc::clone(&primary_toast));
    });
    actions.append(&primary);
    root.append(&actions);

    let root_widget = root.clone().upcast::<gtk::Widget>();
    state.borrow_mut().linux_update_views.push(LinuxUpdateView {
        kind,
        root: root_widget.downgrade(),
        title: title.downgrade(),
        status: status.downgrade(),
        primary: primary.downgrade(),
        skip: skip.downgrade(),
        system: system.downgrade(),
        setup: setup.downgrade(),
        check: check.map(|button| button.downgrade()),
    });
    refresh_linux_update_views(&state);
    root
}

/// The heading over the update copy. Derived from the shared projection — the
/// version it is about, or nothing when no version is in play — so it cannot
/// name a state the message below it is not in.
pub(crate) fn update_surface_title(presentation: &UpdatePresentation) -> String {
    match &presentation.target_version {
        Some(version) => format!("Update · {version}"),
        None => "Update status".to_owned(),
    }
}

/// The action the Settings page's primary button offers, if any. `CheckNow` is
/// that page's own dedicated button, so it never doubles as the offer's action
/// there.
pub(crate) fn primary_update_action(presentation: &UpdatePresentation) -> Option<UpdateAction> {
    presentation
        .action
        .filter(|action| *action != UpdateAction::CheckNow)
}

/// What a screen reader announces for the primary button. The visible label
/// comes from the action, so the announced one has to follow it: a button that
/// only starts a check must not be announced as an update (#701), and one that
/// installs a version the user skipped says which (#682).
pub(crate) fn primary_action_accessible_label(action: UpdateAction) -> &'static str {
    match action {
        UpdateAction::CheckNow => "Check for OK Player updates",
        UpdateAction::InstallAnyway => "Install the skipped update anyway",
        // Not a step of an update: it copies the commands that would give this
        // install an update path at all (#725).
        UpdateAction::CopyRepositorySetup => {
            "Copy the commands that add the OK Player package repository"
        }
        // Everything else is a step of the update itself — the download, the
        // apply, the restart that finishes it, or a retry of one of those.
        UpdateAction::DownloadUpdate
        | UpdateAction::ApplyAndRestart
        | UpdateAction::RestartToFinish
        | UpdateAction::RestartToUseInstalled
        | UpdateAction::Retry
        | UpdateAction::SkipVersion => "Update OK Player",
    }
}

/// The action the offer banner's button offers. The banner carries no check
/// control of its own, so unlike the Settings page it shows the check when the
/// check is what the state offers — a restart that came back on the old build
/// recovers by re-checking, and its button says so (#701).
pub(crate) fn offer_primary_update_action(
    presentation: &UpdatePresentation,
) -> Option<UpdateAction> {
    presentation.action
}

/// The repository instructions as one copyable block: what they achieve, the commands, the
/// fingerprint the archive is signed with, and where to download a package instead. The
/// fingerprint is here so the user can check the key against something the app did not fetch
/// for them, and the download link so "add a repository" is not the only way out.
pub(crate) fn repository_setup_text(setup: &RepositorySetup) -> String {
    format!(
        "{}\n\n{}\n\nThe archive is signed with {}.\nOr download a package from {}.",
        setup.summary, setup.commands, setup.key_fingerprint, setup.downloads_url
    )
}

pub(crate) fn refresh_linux_update_views(state: &Rc<RefCell<PlayerState>>) {
    let (presentation, offer_visible, busy) = {
        let state = state.borrow();
        (
            state.linux_update.describe(),
            state.linux_update.offer_surface_visible(),
            state.linux_update.is_busy(),
        )
    };
    let settings_primary_action = primary_update_action(&presentation);
    let offer_primary_action = offer_primary_update_action(&presentation);
    let checks_possible = update_checks_are_possible(presentation.install_kind);
    let system_updater = system_software_surface();
    // Derived from the projection rather than from the install kind: the desktop updater
    // reads the same package data the app just read, so offering it anywhere the app's claim
    // did not come from that data is offering a tool guaranteed to contradict it (#725).
    let system_visible = presentation.system_updater_offered;
    let title_text = update_surface_title(&presentation);
    let setup_text = presentation
        .repository_setup
        .as_ref()
        .map(repository_setup_text);

    state.borrow_mut().linux_update_views.retain(|view| {
        let Some(root) = view.root.upgrade() else {
            return false;
        };
        let Some(title) = view.title.upgrade() else {
            return false;
        };
        let Some(status) = view.status.upgrade() else {
            return false;
        };
        let Some(primary) = view.primary.upgrade() else {
            return false;
        };
        let Some(skip) = view.skip.upgrade() else {
            return false;
        };
        let Some(system) = view.system.upgrade() else {
            return false;
        };

        root.set_visible(view.kind == LinuxUpdateViewKind::Settings || offer_visible);
        title.set_text(&title_text);
        status.set_text(&presentation.updates_message);

        let primary_action = if view.kind == LinuxUpdateViewKind::Settings {
            settings_primary_action
        } else {
            offer_primary_action
        };
        primary.set_visible(primary_action.is_some());
        if let Some(action) = primary_action {
            primary.set_label(action.label());
            primary.set_sensitive(presentation.actions_enabled);
            primary.set_tooltip_text(
                presentation
                    .action_closes_the_app
                    .then_some("OK Player closes to finish this update."),
            );
            primary.update_property(&[gtk::accessible::Property::Label(
                primary_action_accessible_label(action),
            )]);
        }

        let can_skip = presentation.secondary_action == Some(UpdateAction::SkipVersion);
        skip.set_visible(can_skip);
        skip.set_sensitive(can_skip && presentation.actions_enabled);

        if let Some(setup) = view.setup.upgrade() {
            let show = view.kind == LinuxUpdateViewKind::Settings && setup_text.is_some();
            setup.set_visible(show);
            if let Some(text) = setup_text.as_deref() {
                setup.set_text(text);
            }
        }

        system.set_visible(system_visible);
        system.set_sensitive(system_updater.is_some());
        system.set_tooltip_text(match &system_updater {
            Some(_) => None,
            None => Some(SYSTEM_UPDATER_ABSENT_HINT),
        });

        if let Some(check) = view.check.as_ref().and_then(glib::WeakRef::upgrade) {
            check.set_visible(checks_possible);
            check.set_sensitive(checks_possible && presentation.check_available && !busy);
        }
        true
    });

    // About reads the same projection, so a cached About page cannot keep
    // reporting a state the Updates page has moved on from.
    state.borrow_mut().about_update_labels.retain(|label| {
        let Some(label) = label.upgrade() else {
            return false;
        };
        label.set_text(&presentation.about_message);
        true
    });
}

pub(crate) fn settings_updates_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Updates");
    section.append(&settings_value_row("Current version", APP_BUILD_VERSION));
    let channel = effective_update_channel(state.borrow().settings.update_channel());
    let (channel_label, feed_label) = match channel {
        UpdateChannel::Public => ("linux", "Static (GitHub Pages)"),
        UpdateChannel::Candidate => ("candidate (QA)", "Rolling candidate"),
    };
    section.append(&settings_value_row("Channel", channel_label));
    section.append(&settings_value_row("Feed", feed_label));

    let (install_row, install_label) = settings_value_row_with_label("Install", "—");
    section.append(&install_row);

    let (install_sender, install_receiver) = mpsc::channel::<InstallKind>();
    std::thread::spawn(move || {
        let _ = install_sender.send(install_kind());
    });
    glib::timeout_add_local(Duration::from_millis(10), move || {
        match install_receiver.try_recv() {
            Ok(kind) => {
                install_label.set_text(kind.as_str());
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    // The packages this lane installs come from a real apt repository now, so
    // a `.deb` install is told where its updates live rather than being handed
    // a downloaded file.
    let (repository_row, repository_label) = settings_value_row_with_label("Repository", "—");
    repository_row.set_visible(false);
    section.append(&repository_row);
    let (repository_sender, repository_receiver) = mpsc::channel::<Option<&'static str>>();
    std::thread::spawn(move || {
        let _ = repository_sender.send(system_repository_hint(install_kind()));
    });
    glib::timeout_add_local(
        Duration::from_millis(10),
        move || match repository_receiver.try_recv() {
            Ok(hint) => {
                repository_row.set_visible(hint.is_some());
                repository_label.set_text(hint.unwrap_or("—"));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        },
    );

    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let already_checked = !matches!(
        state.borrow().linux_update.lifecycle.state(),
        UpdateState::Idle
    );

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    auto_row.add_css_class("okp-settings-switch-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Automatic checks"));
    auto_label.add_css_class("okp-info-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some(
        "Check the selected Linux feed on startup and keep available updates actionable until you choose.",
    ));
    auto_detail.add_css_class("okp-update-status");
    auto_detail.set_xalign(0.0);
    auto_detail.set_width_chars(1);
    auto_detail.set_max_width_chars(50);
    auto_detail.set_wrap(true);
    auto_text.append(&auto_detail);
    auto_row.append(&auto_text);

    let auto_state_label = gtk::Label::new(Some(if auto_check_enabled { "On" } else { "Off" }));
    auto_state_label.add_css_class("okp-settings-state-pill");
    auto_state_label.set_valign(gtk::Align::Center);
    auto_row.append(&auto_state_label);

    let auto_switch = settings_switch_button(auto_check_enabled, "Automatic checks");
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    let auto_state_text = auto_state_label.clone();
    auto_switch.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_settings_switch_active(button, enabled);
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_state_text.set_text(if enabled { "On" } else { "Off" });
        refresh_linux_update_views(&auto_state);
        auto_toast.show(if enabled {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        });
    });
    auto_row.append(&auto_switch);
    row.append(&auto_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);

    let check_button = gtk::Button::with_label("Check for updates");
    check_button.add_css_class("okp-settings-button");
    let check_state = Rc::clone(&state);
    let check_toast = Rc::clone(&status_toast);
    check_button.connect_clicked(move |_| {
        start_update_check_for_ui(Rc::clone(&check_state), Rc::clone(&check_toast), true);
    });
    actions.append(&check_button);
    if auto_check_enabled && !already_checked {
        let auto_state = Rc::clone(&state);
        let auto_toast = Rc::clone(&status_toast);
        glib::idle_add_local_once(move || {
            start_update_check_for_ui(auto_state, auto_toast, false);
        });
    }

    let releases_button = gtk::Button::with_label("Open Releases");
    releases_button.add_css_class("okp-settings-button");
    releases_button.connect_clicked(move |_| {
        open_external_url("https://github.com/BeFeast/ok-player/releases")
    });
    actions.append(&releases_button);

    row.append(&build_update_action_surface(
        LinuxUpdateViewKind::Settings,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        Some(&check_button),
    ));
    row.append(&actions);
    section.append(&row);

    section
}

pub(crate) const PREVIEW_UPDATE_VERSION: &str = "0.11.0-beta.2";

/// The visual-smoke hook: drives the shared lifecycle into one state so a
/// screenshot can be taken of it. Every preview walks real transitions, so a
/// state the model cannot reach cannot be previewed either.
pub(crate) fn linux_update_preview_session(kind: InstallKind) -> Option<LinuxUpdateSession> {
    let preview = env::var("OKP_SETTINGS_UPDATE_PREVIEW").ok()?;
    // A dev build has no update lane at all, so there is nothing to draw for
    // it; the self-applying lane stands in, which is what the visual smoke run
    // is there to photograph. `OKP_UPDATE_INSTALL_KIND` picks a specific lane.
    let kind = match kind.capability() {
        UpdateCapability::Unmanaged => InstallKind::AppImage,
        UpdateCapability::SelfApply
        | UpdateCapability::SystemManaged
        | UpdateCapability::SystemUnreachable => kind,
    };
    build_linux_update_preview(kind, preview.trim())
}

pub(crate) fn build_linux_update_preview(
    kind: InstallKind,
    preview: &str,
) -> Option<LinuxUpdateSession> {
    let mut session = LinuxUpdateSession::for_install_kind(kind);
    let lifecycle = &mut session.lifecycle;
    let preview = preview.to_ascii_lowercase();
    // What a `.deb` surface says depends on what that machine's apt can deliver (#725), so a
    // preview has to say which machine it is a picture of. Every preview but one is of a
    // subscribed machine; `no-apt-source` is the one that is not, and it is the state the
    // operator was in.
    if kind.delivery_must_be_established() {
        lifecycle.package_source_observed(if preview == "no-apt-source" {
            PackageSourceEvidence::NoSource
        } else {
            PackageSourceEvidence::Source {
                suite: "stable".to_owned(),
                deliverable: Some(PREVIEW_UPDATE_VERSION.to_owned()),
            }
        });
    }
    match preview.as_str() {
        "up-to-date" => {
            lifecycle.start_check().ok()?;
            lifecycle.check_found_none().ok()?;
        }
        "checking" => {
            lifecycle.start_check().ok()?;
        }
        "available" => {
            lifecycle.start_check().ok()?;
            lifecycle.check_found(PREVIEW_UPDATE_VERSION).ok()?;
        }
        // A published build this machine cannot fetch, with the way out on screen. The same
        // transitions as `available`; the difference is entirely the machine (#725).
        "no-apt-source" => {
            lifecycle.start_check().ok()?;
            lifecycle.check_found(PREVIEW_UPDATE_VERSION).ok()?;
        }
        "skipped" => {
            lifecycle.start_check().ok()?;
            lifecycle.check_found(PREVIEW_UPDATE_VERSION).ok()?;
            lifecycle.skip_offer().ok()?;
        }
        "install-error" => {
            lifecycle.start_check().ok()?;
            lifecycle.check_found(PREVIEW_UPDATE_VERSION).ok()?;
            lifecycle.start_download().ok()?;
            lifecycle
                .download_failed("the package download was interrupted")
                .ok()?;
        }
        "replaced-on-disk" => {
            lifecycle
                .installed_version_observed(PREVIEW_UPDATE_VERSION)
                .ok()?;
        }
        "restart-pending" => {
            lifecycle.start_check().ok()?;
            lifecycle.check_found(PREVIEW_UPDATE_VERSION).ok()?;
            lifecycle.start_download().ok()?;
            lifecycle.download_and_apply_needs_restart().ok()?;
        }
        "error" => {
            lifecycle.start_check().ok()?;
            lifecycle
                .check_failed("the update feed is temporarily unavailable")
                .ok()?;
        }
        _ => return None,
    }
    Some(session)
}

pub(crate) fn start_update_check_for_ui(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    show_toast: bool,
) {
    if state.borrow().linux_update.is_busy() {
        return;
    }
    if !transition_update(&state, "start check", UpdateLifecycle::start_check) {
        return;
    }
    refresh_linux_update_views(&state);

    let channel = effective_update_channel(state.borrow().settings.update_channel());
    let install_kind = state.borrow().linux_update.install_kind();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let source = observe_package_source(install_kind);
        let _ = sender.send((check_for_linux_update(channel, install_kind), source));
    });

    glib::timeout_add_local(Duration::from_millis(120), move || {
        match receiver.try_recv() {
            Ok((outcome, source)) => {
                state
                    .borrow_mut()
                    .linux_update
                    .lifecycle
                    .package_source_observed(source);
                apply_update_check_outcome(
                    Rc::clone(&state),
                    &status_toast,
                    show_toast,
                    channel,
                    outcome,
                );
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                transition_update(&state, "check failed", |lifecycle| {
                    lifecycle.check_failed("update check channel closed")
                });
                refresh_linux_update_views(&state);
                glib::ControlFlow::Break
            }
        }
    });
}

/// Runs one lifecycle transition, logging a refusal instead of forcing it.
/// Core refuses a transition by leaving the state untouched, so a refused step
/// leaves the surface exactly as it was rather than half-moved.
pub(crate) fn transition_update<F>(state: &Rc<RefCell<PlayerState>>, label: &str, step: F) -> bool
where
    F: FnOnce(&mut UpdateLifecycle) -> Result<&UpdateState, UpdateTransitionError>,
{
    let mut state = state.borrow_mut();
    match step(&mut state.linux_update.lifecycle) {
        Ok(_) => true,
        Err(error) => {
            eprintln!("Update lifecycle refused {label}: {error:?}");
            false
        }
    }
}

/// Takes whatever action the shared projection is offering. The surface never
/// decides what the button does — the state does.
pub(crate) fn take_primary_update_action(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let presentation = state.borrow().linux_update.describe();
    if !presentation.actions_enabled {
        return;
    }
    // The button may be either surface's, and the offer banner shows the check
    // when that is the action, so the dispatch reads the projection's own
    // action rather than the Settings page's filtered view of it.
    match presentation.action {
        Some(UpdateAction::DownloadUpdate) => start_update_download(state, status_toast),
        Some(UpdateAction::InstallAnyway) => {
            if transition_update(&state, "install anyway", UpdateLifecycle::install_anyway) {
                continue_after_offer_accepted(state, status_toast);
            }
        }
        Some(UpdateAction::ApplyAndRestart) => start_update_apply(state, status_toast),
        Some(UpdateAction::RestartToFinish) => restart_for_pending_update(&state, &status_toast),
        Some(UpdateAction::RestartToUseInstalled) => {
            restart_into_installed_build(&state, &status_toast)
        }
        Some(UpdateAction::Retry) => {
            // A failure that kept its target is retried where it failed; a
            // check that failed before finding anything is retried by checking
            // again, which is the only thing there is to repeat.
            if transition_update(&state, "retry", UpdateLifecycle::retry_failed_update) {
                continue_after_offer_accepted(state, status_toast);
            } else {
                start_update_check_for_ui(state, status_toast, true);
            }
        }
        // The check the projection offers is a check and nothing more — after
        // a restart that came back on the old build it is the recovery, and
        // what it finds is a fresh offer the user takes on purpose (#701).
        Some(UpdateAction::CheckNow) => start_update_check_for_ui(state, status_toast, true),
        // The app copies the commands and never runs them: adding an archive
        // and its signing key is a privileged, system-wide change the user
        // makes deliberately, and the privileged install path was removed in
        // #698.
        Some(UpdateAction::CopyRepositorySetup) => {
            copy_repository_setup(&state, &status_toast);
        }
        Some(UpdateAction::SkipVersion) | None => {}
    }
}

/// Puts the repository instructions on the clipboard (#725).
fn copy_repository_setup(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let Some(setup) = state.borrow().linux_update.describe().repository_setup else {
        return;
    };
    let Some(display) = gtk::gdk::Display::default() else {
        status_toast.show("Could not copy the setup commands");
        return;
    };
    display.clipboard().set_text(setup.commands);
    status_toast.show("Setup commands copied");
}

/// Whether the shell drives the state an accepted offer restored any further on
/// its own.
///
/// Only a step already *in flight* is continued: `install_anyway` over an
/// unstaged offer enters `Downloading`, which nothing but this shell will
/// carry, and on the one-call lane that download is the apply and the relaunch
/// — which is exactly what the projection warned about before the press.
///
/// A state that offers the user a control of its own is left to the user. A
/// payload the skip kept comes back as `ReadyToApply`, whose action is "Install
/// and restart" and whose projection says it closes the player; applying it
/// from here would end the session behind a press the projection said closes
/// nothing. The same holds for the `Available` a retry restores. So a press
/// does what the projection said it would and no more (#696).
pub(crate) fn continues_without_the_user(restored: &UpdateState) -> bool {
    matches!(restored, UpdateState::Downloading { .. })
}

/// Continues from whatever "install anyway" or "try again" restored, as far as
/// [`continues_without_the_user`] allows — which is the download an accepted
/// offer already entered, and nothing else.
fn continue_after_offer_accepted(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) {
    let restored = state.borrow().linux_update.lifecycle.state().clone();
    refresh_linux_update_views(&state);
    if continues_without_the_user(&restored) {
        run_update_download(state, status_toast);
    }
}

/// Whether the offer has to be discovered again before anything can be
/// fetched. A session that came back from a restart — or from an apply the
/// previous process staged — knows *which* version it is about, but the
/// payload that names where to get it died with that process. Discovering it
/// again is the recovery; reporting the payload missing would replace the
/// error the user is looking at with an implementation detail.
pub(crate) fn update_needs_rediscovery(state: &Rc<RefCell<PlayerState>>) -> bool {
    state.borrow().linux_update.payload.is_none()
}

pub(crate) fn start_update_download(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    if update_needs_rediscovery(&state) {
        start_update_check_for_ui(state, status_toast, true);
        return;
    }
    if !transition_update(&state, "start download", UpdateLifecycle::start_download) {
        return;
    }
    refresh_linux_update_views(&state);
    run_update_download(state, status_toast);
}

/// Fetches the payload. On the AppImage lane accepting the offer downloads,
/// applies and asks for the restart in one step, which is why the outcome is
/// reported through the one-call transitions.
fn run_update_download(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) {
    let Some(job) = update_apply_job(&state) else {
        transition_update(&state, "download failed", |lifecycle| {
            lifecycle.download_failed(MISSING_PAYLOAD_REASON)
        });
        refresh_linux_update_views(&state);
        return;
    };
    save_current_progress(&state, false);

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(download_and_apply_linux_update(&job));
    });

    glib::timeout_add_local(Duration::from_millis(150), move || {
        match receiver.try_recv() {
            Ok(Ok(LinuxUpdateApplyResult::RestartRequired)) => {
                enter_restart_pending(&state, &status_toast, true);
                glib::ControlFlow::Break
            }
            Ok(Err(failure)) => {
                report_download_lane_failure(&state, &failure);
                status_toast.show("Update failed");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                report_download_lane_failure(
                    &state,
                    &UpdateStepFailure {
                        reason: "update worker channel closed".to_owned(),
                        // Nothing is known to have landed.
                        staged: false,
                    },
                );
                glib::ControlFlow::Break
            }
        }
    });
}

/// Records which half of the one-call step failed, so the retry core offers
/// repeats that half: a download that never landed is fetched again, an apply
/// that failed re-applies the payload still on disk.
pub(crate) fn report_download_lane_failure(
    state: &Rc<RefCell<PlayerState>>,
    failure: &UpdateStepFailure,
) {
    let reason = failure.reason.clone();
    if failure.staged {
        transition_update(state, "download and apply failed", move |lifecycle| {
            lifecycle.download_and_apply_failed(reason)
        });
    } else {
        transition_update(state, "download failed", move |lifecycle| {
            lifecycle.download_failed(reason)
        });
    }
    refresh_linux_update_views(state);
}

/// Applies a payload that is already staged and verified.
pub(crate) fn start_update_apply(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) {
    if !transition_update(&state, "start apply", UpdateLifecycle::start_apply) {
        return;
    }
    refresh_linux_update_views(&state);
    let Some(job) = update_apply_job(&state) else {
        transition_update(&state, "apply failed", |lifecycle| {
            lifecycle.apply_failed(MISSING_PAYLOAD_REASON)
        });
        refresh_linux_update_views(&state);
        return;
    };
    save_current_progress(&state, false);

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(apply_staged_linux_update(&job));
    });

    glib::timeout_add_local(Duration::from_millis(150), move || {
        match receiver.try_recv() {
            Ok(Ok(LinuxUpdateApplyResult::RestartRequired)) => {
                enter_restart_pending(&state, &status_toast, false);
                glib::ControlFlow::Break
            }
            Ok(Err(failure)) => {
                let reason = failure.reason.clone();
                transition_update(&state, "apply failed", move |lifecycle| {
                    lifecycle.apply_failed(reason)
                });
                refresh_linux_update_views(&state);
                status_toast.show("Update failed");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                transition_update(&state, "apply failed", |lifecycle| {
                    lifecycle.apply_failed("update worker channel closed")
                });
                refresh_linux_update_views(&state);
                glib::ControlFlow::Break
            }
        }
    });
}

/// The payload is installed and this process is still the old build. The
/// target is recorded first: the process that comes back compares it against
/// the version it actually came up as, so a hand-off that quietly kept the old
/// binary is reported as a failure instead of a success (#660).
fn enter_restart_pending(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &Rc<StatusToast>,
    one_call_lane: bool,
) {
    let moved = if one_call_lane {
        transition_update(
            state,
            "download and apply",
            UpdateLifecycle::download_and_apply_needs_restart,
        )
    } else {
        transition_update(state, "apply", UpdateLifecycle::apply_needs_restart)
    };
    if !moved {
        refresh_linux_update_views(state);
        return;
    }
    if let Some(version) = state.borrow().linux_update.describe().target_version {
        record_pending_restart(&version);
    }
    refresh_linux_update_views(state);
    status_toast.show("Update installed — restart to switch");
    // The waiting updater applies the payload once this process exits, so the
    // hand-off is taken immediately; the restart action stays offered in case
    // the window declines to close.
    restart_for_pending_update(state, status_toast);
}

/// Finishes a pending restart by closing the player: the Velopack updater is
/// already waiting on this process and relaunches once it exits.
pub(crate) fn restart_for_pending_update(
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
) {
    save_current_progress(state, false);
    if !close_player_window() {
        eprintln!("Could not close the player window to finish the update restart");
        status_toast.show("Could not restart OK Player");
    }
}

/// Closes the main player window, which runs the app's ordinary shutdown path.
/// Returns whether a window was found to close.
pub(crate) fn close_player_window() -> bool {
    let Some(application) = gtk::gio::Application::default() else {
        return false;
    };
    let Ok(application) = application.downcast::<gtk::Application>() else {
        return false;
    };
    let Some(window) = application
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<gtk::ApplicationWindow>().ok())
    else {
        return false;
    };
    window.close();
    true
}

fn skip_current_update(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let (channel, version) = {
        let state = state.borrow();
        let presentation = state.linux_update.describe();
        if presentation.secondary_action != Some(UpdateAction::SkipVersion) {
            return;
        }
        let Some(version) = presentation.target_version else {
            return;
        };
        (
            effective_update_channel(state.settings.update_channel()),
            version,
        )
    };

    let save_result = {
        let mut state = state.borrow_mut();
        let previous = state
            .settings
            .skipped_update_version(channel)
            .map(str::to_owned);
        state
            .settings
            .set_skipped_update_version(channel, Some(version.clone()));
        match state.settings.save() {
            Ok(()) => Ok(()),
            Err(error) => {
                state.settings.set_skipped_update_version(channel, previous);
                Err(error)
            }
        }
    };
    if let Err(error) = save_result {
        eprintln!("Failed to save skipped update version: {error}");
        status_toast.show("Could not save skipped version");
        return;
    }

    transition_update(state, "skip offer", UpdateLifecycle::skip_offer);
    refresh_linux_update_views(state);
    status_toast.show(&format!("Skipped version {version}"));
}

pub(crate) fn apply_update_check_outcome(
    state: Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    show_toast: bool,
    channel: UpdateChannel,
    outcome: LinuxUpdateCheckOutcome,
) {
    let mut toast = None;
    match outcome {
        LinuxUpdateCheckOutcome::UpToDate => {
            if transition_update(&state, "up to date", UpdateLifecycle::check_found_none) {
                // "The feed has nothing newer" is not the same answer as "there is
                // nothing to do": apt may already be holding a build the feed has
                // moved past, and the lifecycle turns that into an offer. The toast
                // follows the state so it cannot contradict the surface behind it,
                // or hide a release apt can actually deliver (#725).
                toast = Some(match found_offer_target(&state) {
                    None => "OK Player is up to date",
                    Some(target) if skip_persisted_version(&state, channel, &target) => {
                        "This version was skipped"
                    }
                    Some(_) => "Update available",
                });
            }
        }
        LinuxUpdateCheckOutcome::Failed(error) => {
            transition_update(&state, "check failed", |lifecycle| {
                lifecycle.check_failed(error.clone())
            });
            toast = Some("Update check failed");
        }
        LinuxUpdateCheckOutcome::Available { version, payload } => {
            if transition_update(&state, "check found", |lifecycle| {
                lifecycle.check_found(version.clone())
            }) {
                state.borrow_mut().linux_update.payload = payload;
                // What the check found and what it means for this machine are
                // different questions (#725): a `stable` subscriber told about a
                // candidate build ends up current on its own channel, and a toast
                // saying "Update available" would contradict the surface behind
                // it. Both the toast and the skip are therefore about the state
                // the transition produced.
                toast = Some(match found_offer_target(&state) {
                    None => "OK Player is up to date",
                    Some(target) if skip_persisted_version(&state, channel, &target) => {
                        "This version was skipped"
                    }
                    Some(_) => "Update available",
                });
            }
        }
        LinuxUpdateCheckOutcome::Staged { version, payload } => {
            // The payload was downloaded and verified in an earlier session, so
            // the lifecycle walks to the state that describes it rather than
            // offering a download that has already happened.
            if transition_update(&state, "check found staged", |lifecycle| {
                lifecycle.check_found(version.clone())
            }) {
                state.borrow_mut().linux_update.payload = Some(payload);
                transition_update(&state, "staged download", UpdateLifecycle::start_download);
                transition_update(&state, "staged payload", UpdateLifecycle::download_finished);
                // Same rule as above: the skip is looked up under the version the
                // surface names, which is the one it was recorded against.
                toast = Some(match found_offer_target(&state) {
                    None => "OK Player is up to date",
                    Some(target) if skip_persisted_version(&state, channel, &target) => {
                        "This version was skipped"
                    }
                    Some(_) => "Update ready to install",
                });
            }
        }
    }
    refresh_linux_update_views(&state);
    if let Some(message) = toast.filter(|_| show_toast) {
        status_toast.show(message);
    }
}

/// The version a completed check is offering, or nothing when it produced a verdict with no
/// target — `WithheldBySuite`, where the machine is current on its own channel (#725).
///
/// The feed's version and the offer's version are not always the same. A system-managed
/// install announces what its own source can deliver, so a skip recorded against what the
/// surface named has to be looked up under that same version or it will not be found again.
fn found_offer_target(state: &Rc<RefCell<PlayerState>>) -> Option<String> {
    state.borrow().linux_update.describe().target_version
}

/// Re-applies a skip the user already made for this exact version, so a
/// re-check does not resurrect an offer they dismissed.
fn skip_persisted_version(
    state: &Rc<RefCell<PlayerState>>,
    channel: UpdateChannel,
    version: &str,
) -> bool {
    let skipped = state
        .borrow()
        .settings
        .skipped_update_versions()
        .is_skipped(channel, version);
    if skipped {
        transition_update(state, "persisted skip", UpdateLifecycle::skip_offer);
    }
    skipped
}

pub(crate) fn check_updates_on_startup(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    if !transition_update(&state, "start startup check", UpdateLifecycle::start_check) {
        return;
    }
    refresh_linux_update_views(&state);
    let channel = effective_update_channel(state.borrow().settings.update_channel());
    let install_kind = state.borrow().linux_update.install_kind();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let source = observe_package_source(install_kind);
        let _ = sender.send((check_for_linux_update(channel, install_kind), source));
    });

    glib::timeout_add_local(Duration::from_millis(500), move || {
        match receiver.try_recv() {
            Ok((outcome, source)) => {
                state
                    .borrow_mut()
                    .linux_update
                    .lifecycle
                    .package_source_observed(source);
                apply_update_check_outcome(
                    Rc::clone(&state),
                    &status_toast,
                    false,
                    channel,
                    outcome,
                );
                eprintln!(
                    "Startup update check: {}",
                    state.borrow().linux_update.describe().updates_message
                );
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

/// Discovers what is published for this install kind. The lane it reads is the
/// install's own — a `.deb` reads the deb feed even though the app will never
/// install what it finds, because knowing about a release is what lets the
/// surface point at the package manager that can.
pub(crate) fn check_for_linux_update(
    channel: UpdateChannel,
    install_kind: InstallKind,
) -> LinuxUpdateCheckOutcome {
    let channel = effective_update_channel(channel);
    let lane = candidate_install_lane(install_kind);
    if channel == UpdateChannel::Candidate {
        return check_for_linux_candidate_channel_update(lane);
    }

    if lane != CandidateInstallLane::AppImage {
        return match check_for_linux_deb_update() {
            Ok(Some(update)) => LinuxUpdateCheckOutcome::Available {
                version: update.version,
                payload: None,
            },
            Ok(None) => LinuxUpdateCheckOutcome::UpToDate,
            Err(error) => LinuxUpdateCheckOutcome::Failed(error),
        };
    }

    let manager = match linux_update_manager(UpdateChannel::Public) {
        Ok(manager) => manager,
        Err(error) => return LinuxUpdateCheckOutcome::Failed(error),
    };

    if let Some(asset) = manager.get_update_pending_restart() {
        return LinuxUpdateCheckOutcome::Staged {
            version: asset.Version.clone(),
            payload: SelfApplyPayload {
                manager,
                target: VelopackTarget::Asset(Box::new(asset)),
            },
        };
    }

    match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => LinuxUpdateCheckOutcome::Available {
            version: update.TargetFullRelease.Version.clone(),
            payload: Some(SelfApplyPayload {
                manager,
                target: VelopackTarget::Info(update),
            }),
        },
        Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
            LinuxUpdateCheckOutcome::UpToDate
        }
        Err(error) => LinuxUpdateCheckOutcome::Failed(error.to_string()),
    }
}

/// The candidate lane an install kind reads. The AppImage lane self-applies;
/// everything else that discovers versions is served the packaged artifact it
/// tells the user to install through their package manager.
pub(crate) fn candidate_install_lane(install_kind: InstallKind) -> CandidateInstallLane {
    match install_kind {
        InstallKind::AppImage => CandidateInstallLane::AppImage,
        InstallKind::Rpm | InstallKind::Flatpak => CandidateInstallLane::SystemPackage,
        InstallKind::Deb | InstallKind::DevBuild | InstallKind::WindowsVelopack => {
            CandidateInstallLane::Debian
        }
    }
}

pub(crate) fn linux_update_manager(channel: UpdateChannel) -> Result<UpdateManager, String> {
    debug_assert_eq!(channel, UpdateChannel::Public);
    let source = HttpSource::new(linux_update_feed_base_url());
    let options = UpdateOptions {
        ExplicitChannel: Some("linux".to_owned()),
        ..Default::default()
    };
    UpdateManager::new(source, Some(options), None).map_err(|error| match error {
        velopack::Error::NotInstalled(_) => "This install cannot self-update.".to_owned(),
        other => other.to_string(),
    })
}

#[derive(Clone)]
struct CandidateVelopackSource {
    asset: VelopackAsset,
    download_url: String,
}

impl UpdateSource for CandidateVelopackSource {
    fn get_release_feed(
        &self,
        _channel: &str,
        _app: &Manifest,
        _staged_user_id: &str,
    ) -> Result<VelopackAssetFeed, velopack::Error> {
        Ok(VelopackAssetFeed {
            Assets: vec![self.asset.clone()],
        })
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress_sender: Option<mpsc::Sender<i16>>,
    ) -> Result<(), velopack::Error> {
        debug_assert_eq!(asset.FileName, self.asset.FileName);
        velopack::download::download_url_to_file(&self.download_url, local_file, move |progress| {
            if let Some(sender) = &progress_sender {
                let _ = sender.send(progress);
            }
        })
    }
}

pub(crate) fn candidate_velopack_asset(
    candidate: &CandidateAppImage,
    version: &str,
) -> VelopackAsset {
    VelopackAsset {
        PackageId: candidate.package_id.clone(),
        Version: version.to_owned(),
        Type: "Full".to_owned(),
        FileName: candidate.name.clone(),
        SHA1: candidate.sha1.clone(),
        SHA256: candidate.sha256.clone(),
        Size: candidate.size,
        NotesMarkdown: String::new(),
        NotesHtml: String::new(),
    }
}

fn linux_candidate_update_manager(candidate: &CandidateUpdate) -> Result<UpdateManager, String> {
    let source = CandidateVelopackSource {
        asset: candidate_velopack_asset(&candidate.appimage, &candidate.version),
        download_url: candidate.appimage.url.clone(),
    };
    let options = UpdateOptions {
        ExplicitChannel: Some("linux-candidate".to_owned()),
        ..Default::default()
    };
    UpdateManager::new(source, Some(options), None).map_err(|error| match error {
        velopack::Error::NotInstalled(_) => "This install cannot self-update.".to_owned(),
        other => other.to_string(),
    })
}

fn check_for_linux_candidate_channel_update(lane: CandidateInstallLane) -> LinuxUpdateCheckOutcome {
    let candidate = match fetch_linux_candidate_update() {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            eprintln!("Candidate update stage=selection result=up-to-date");
            return LinuxUpdateCheckOutcome::UpToDate;
        }
        Err(error) => {
            eprintln!("Candidate update stage=selection result=failed error={error}");
            return LinuxUpdateCheckOutcome::Failed(error);
        }
    };
    eprintln!(
        "Candidate update stage=selection result=available version={} build={} commit_sha={}",
        candidate.version, candidate.build, candidate.commit_sha
    );
    eprintln!("Candidate update stage=install-lane lane={lane:?}");
    match candidate_channel::route_candidate_update(&candidate, lane, None) {
        Ok(CandidateUpdateRoute::Debian) => {
            eprintln!(
                "Candidate update stage=final result=available lane=deb version={}",
                candidate.version
            );
            return LinuxUpdateCheckOutcome::Available {
                version: candidate.version,
                payload: None,
            };
        }
        Err(candidate_channel::CandidateUpdateRouteError::MissingAppImageCheck) => {}
        Ok(_) => unreachable!("a route without an AppImage result can only select Debian"),
        Err(error) => {
            eprintln!("Candidate update stage=final result=failed error={error}");
            return LinuxUpdateCheckOutcome::Failed(error.to_string());
        }
    }

    let manager = match linux_candidate_update_manager(&candidate) {
        Ok(manager) => manager,
        Err(error) => {
            let error = format!("candidate AppImage updater failed: {error}");
            eprintln!("Candidate update stage=velopack result=failed error={error}");
            return LinuxUpdateCheckOutcome::Failed(error);
        }
    };

    if let Some(asset) = manager.get_update_pending_restart() {
        let check = CandidateAppImageCheck::PendingRestart {
            version: asset.Version.clone(),
            sha256: asset.SHA256.clone(),
        };
        return match candidate_channel::route_candidate_update(&candidate, lane, Some(&check)) {
            Ok(CandidateUpdateRoute::PendingAppImage) => {
                eprintln!(
                    "Candidate update stage=final result=pending-restart lane=appimage version={}",
                    candidate.version
                );
                LinuxUpdateCheckOutcome::Staged {
                    version: asset.Version.clone(),
                    payload: SelfApplyPayload {
                        manager,
                        target: VelopackTarget::Asset(Box::new(asset)),
                    },
                }
            }
            Ok(_) => unreachable!("AppImage pending restart must route to its pending asset"),
            Err(error) => {
                eprintln!("Candidate update stage=final result=failed error={error}");
                LinuxUpdateCheckOutcome::Failed(error.to_string())
            }
        };
    }

    match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => {
            let check = CandidateAppImageCheck::UpdateAvailable {
                version: update.TargetFullRelease.Version.clone(),
                sha256: update.TargetFullRelease.SHA256.clone(),
            };
            match candidate_channel::route_candidate_update(&candidate, lane, Some(&check)) {
                Ok(CandidateUpdateRoute::AppImage) => {
                    eprintln!(
                        "Candidate update stage=final result=available lane=appimage version={}",
                        candidate.version
                    );
                    LinuxUpdateCheckOutcome::Available {
                        version: update.TargetFullRelease.Version.clone(),
                        payload: Some(SelfApplyPayload {
                            manager,
                            target: VelopackTarget::Info(update),
                        }),
                    }
                }
                Ok(_) => unreachable!("AppImage update must route to its update information"),
                Err(error) => {
                    eprintln!("Candidate update stage=final result=failed error={error}");
                    LinuxUpdateCheckOutcome::Failed(error.to_string())
                }
            }
        }
        Ok(UpdateCheck::NoUpdateAvailable) => candidate_appimage_empty_result(
            &candidate,
            lane,
            CandidateAppImageCheck::NoUpdateAvailable,
        ),
        Ok(UpdateCheck::RemoteIsEmpty) => {
            candidate_appimage_empty_result(&candidate, lane, CandidateAppImageCheck::RemoteIsEmpty)
        }
        Err(error) => {
            eprintln!("Candidate update stage=velopack result=failed error={error}");
            LinuxUpdateCheckOutcome::Failed(error.to_string())
        }
    }
}

fn candidate_appimage_empty_result(
    candidate: &CandidateUpdate,
    lane: CandidateInstallLane,
    check: CandidateAppImageCheck,
) -> LinuxUpdateCheckOutcome {
    let result = candidate_channel::route_candidate_update(candidate, lane, Some(&check));
    let error = result.expect_err("a newer accepted AppImage candidate cannot be empty");
    eprintln!("Candidate update stage=velopack result=failed error={error}");
    LinuxUpdateCheckOutcome::Failed(error.to_string())
}

pub(crate) fn linux_update_feed_base_url() -> String {
    env::var("OKP_LINUX_UPDATE_FEED_URL").unwrap_or_else(|_| LINUX_UPDATE_FEED_BASE_URL.to_owned())
}

/// The reason an apply step gives up when the payload it was going to act on
/// is not there. Only reachable if a check landed the lifecycle on an offer
/// without the payload that belongs to it, which is a bug rather than a state
/// the user can produce — it fails loudly instead of reporting a success.
pub(crate) const MISSING_PAYLOAD_REASON: &str = "the downloaded update is no longer available";

/// The payload the current state acts on. Cloned rather than taken: a worker
/// that fails leaves the offer retryable, and a retry that had to re-discover
/// the payload would replace the error the user is looking at with a missing
/// one.
fn update_apply_job(state: &Rc<RefCell<PlayerState>>) -> Option<SelfApplyPayload> {
    state.borrow().linux_update.payload.clone()
}

/// Downloads and applies in one step, the AppImage lane's shape. The updater
/// is launched to wait for this process, so a success here means the payload
/// is committed and the restart is what is left — never that the new version
/// is already running.
pub(crate) fn download_and_apply_linux_update(
    payload: &SelfApplyPayload,
) -> Result<LinuxUpdateApplyResult, UpdateStepFailure> {
    if let VelopackTarget::Info(info) = &payload.target {
        payload
            .manager
            .download_updates(info.as_ref(), None)
            .map_err(|error| UpdateStepFailure {
                reason: error.to_string(),
                // Nothing landed: recovering means fetching it again.
                staged: false,
            })?;
    }
    apply_staged_linux_update(payload)
}

/// Hands a staged payload to the Velopack updater. `restart` is on and the
/// updater waits for this process, so the swap happens the moment the player
/// exits and the new build comes up by itself.
pub(crate) fn apply_staged_linux_update(
    payload: &SelfApplyPayload,
) -> Result<LinuxUpdateApplyResult, UpdateStepFailure> {
    payload
        .manager
        .wait_exit_then_apply_updates(payload.target.asset(), false, true, Vec::<String>::new())
        .map_err(|error| UpdateStepFailure {
            reason: error.to_string(),
            // The payload is downloaded and verified; only the hand-off failed.
            staged: true,
        })?;
    Ok(LinuxUpdateApplyResult::RestartRequired)
}

pub(crate) const SYSTEM_UPDATER_ACTION_LABEL: &str = "Open software updater";
pub(crate) const SYSTEM_UPDATER_ABSENT_HINT: &str =
    "No graphical software updater was found on this desktop.";

/// The desktop's own update surface, in the order a GTK desktop is likely to
/// have one. Nothing here installs anything: the action hands the user to the
/// tool that owns the package, and when the desktop has none the surface says
/// so instead of pretending.
pub(crate) const SYSTEM_SOFTWARE_SURFACES: [(&str, &[&str]); 5] = [
    ("gnome-software", &["--mode=updates"]),
    ("mintupdate", &[]),
    ("update-manager", &[]),
    ("gpk-update-viewer", &[]),
    ("plasma-discover", &["--mode", "update"]),
];

pub(crate) fn system_software_surface() -> Option<(PathBuf, &'static [&'static str])> {
    SYSTEM_SOFTWARE_SURFACES
        .iter()
        .find_map(|(name, args)| find_executable(name).map(|path| (path, *args)))
}

fn open_system_software_surface(status_toast: &StatusToast) {
    let Some((executable, args)) = system_software_surface() else {
        status_toast.show(SYSTEM_UPDATER_ABSENT_HINT);
        return;
    };
    if let Err(error) = Command::new(&executable).args(args).spawn() {
        eprintln!("Failed to open {}: {error}", executable.display());
        status_toast.show("Could not open the software updater");
    }
}

/// Where a system-managed install gets its packages, for the surface to show.
/// Only the apt lane has a repository of OK Player's own; the others are
/// whatever the user configured, and the app does not guess.
pub(crate) fn system_repository_hint(install_kind: InstallKind) -> Option<&'static str> {
    match install_kind {
        InstallKind::Deb => Some(APT_REPOSITORY_URL),
        InstallKind::Rpm | InstallKind::Flatpak => None,
        InstallKind::AppImage | InstallKind::DevBuild | InstallKind::WindowsVelopack => None,
    }
}

pub(crate) fn check_for_linux_deb_update() -> Result<Option<DebUpdate>, String> {
    let url = linux_deb_feed_url();
    let mut response = ureq::get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!(".deb update check failed: {error}"))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!(".deb update check failed: {error}"))?;
    // A fetch or parse failure surfaces as Err ("couldn't check"); a feed that
    // is not newer than the running build returns Ok(None) ("up to date"). The
    // two must stay distinct, as on Windows (issue #162 acceptance).
    let feed: DebFeed = serde_json::from_str(&body)
        .map_err(|error| format!(".deb update feed was invalid: {error}"))?;

    Ok(update_selection::select_deb_update_from_feed(
        feed,
        APP_BUILD_VERSION,
    ))
}

pub(crate) fn linux_deb_feed_url() -> String {
    env::var("OKP_LINUX_DEB_FEED_URL").unwrap_or_else(|_| LINUX_DEB_FEED_URL.to_owned())
}

/// Resolves the effective discovery channel: the persisted enrollment, unless
/// `OKP_LINUX_UPDATE_CHANNEL=candidate` explicitly overrides it for a QA test
/// run. The override can only *enrol* — any other value leaves the install on
/// its persisted channel, so it can never silently move a candidate install back
/// to public or vice versa by accident.
pub(crate) fn effective_update_channel(persisted: UpdateChannel) -> UpdateChannel {
    match env::var("OKP_LINUX_UPDATE_CHANNEL") {
        Ok(value) if value.trim().eq_ignore_ascii_case("candidate") => UpdateChannel::Candidate,
        _ => persisted,
    }
}

pub(crate) fn linux_candidate_feed_url() -> String {
    env::var("OKP_LINUX_CANDIDATE_FEED_URL").unwrap_or_else(|_| LINUX_CANDIDATE_FEED_URL.to_owned())
}

/// Checks the rolling candidate feed for an enrolled install. A fetch or parse
/// failure surfaces as `Err` ("couldn't check"); an accepted-but-not-newer,
/// pending, rejected, or non-candidate feed returns `Ok(None)` ("up to date").
/// The two stay distinct exactly as on the public lane (issue #339). Selection
/// gates on acceptance status in okp-core, so a pending candidate on the rolling
/// surface is never offered to the fleet.
fn fetch_linux_candidate_update() -> Result<Option<CandidateUpdate>, String> {
    let url = linux_candidate_feed_url();
    let cache_bust = candidate_feed_cache_bust();
    fetch_linux_candidate_update_from_url(&url, &cache_bust, APP_BUILD_VERSION)
}

pub(crate) fn candidate_feed_cache_bust() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let unix_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{unix_nanos}-{sequence}")
}

pub(crate) fn fetch_linux_candidate_update_from_url(
    url: &str,
    cache_bust: &str,
    current_version: &str,
) -> Result<Option<CandidateUpdate>, String> {
    let mut response = ureq::get(url)
        .query("okp-cache-bust", cache_bust)
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("candidate update check failed: {error}"))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("candidate update check failed: {error}"))?;
    let feed: CandidateFeed = serde_json::from_str(&body)
        .map_err(|error| format!("candidate update feed was invalid: {error}"))?;
    eprintln!(
        "Candidate update stage=feed version={} build={} acceptance={:?} commit_sha={}",
        feed.version, feed.build, feed.acceptance, feed.commit_sha
    );

    Ok(candidate_channel::select_candidate_update_from_feed(
        feed,
        current_version,
    ))
}

pub(crate) fn linux_update_cache_dir() -> PathBuf {
    if let Some(cache_dir) =
        env::var_os("OKP_LINUX_UPDATE_CACHE_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(cache_dir);
    }
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache_home).join("ok-player/updates");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".cache/ok-player/updates");
    }
    env::temp_dir().join("ok-player/updates")
}

pub(crate) fn find_executable(name: &str) -> Option<PathBuf> {
    if name.contains(std::path::MAIN_SEPARATOR) {
        let path = PathBuf::from(name);
        return is_executable_file(&path).then_some(path);
    }

    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|path| is_executable_file(path))
}

/// True when `path` resolves to a regular file that carries an execute bit — the Unix
/// definition of a runnable program. A file on `PATH` with no execute permission cannot be
/// spawned, so treating it as "found" would advertise a tool that then fails with a generic
/// exec error; `find_executable` rejects it here, matching `command -v` / `which` semantics.
/// `fs::metadata` follows symlinks, so a symlinked resolver is judged by its target's mode.
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub(crate) fn open_external_url(url: &str) {
    if let Err(error) = Command::new("xdg-open").arg(url).spawn() {
        eprintln!("Failed to open {url}: {error}");
    }
}

pub(crate) fn settings_appearance_section(
    window: &gtk::Window,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Appearance");

    let theme_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    theme_row.add_css_class("okp-settings-row");
    let theme_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    theme_text.set_hexpand(true);
    let theme_label = gtk::Label::new(Some("App theme"));
    theme_label.add_css_class("okp-info-label");
    theme_label.set_xalign(0.0);
    theme_text.append(&theme_label);
    let theme_detail = gtk::Label::new(Some("Auto follows the desktop color scheme."));
    theme_detail.add_css_class("okp-update-status");
    theme_detail.set_xalign(0.0);
    theme_text.append(&theme_detail);
    theme_row.append(&theme_text);

    let current = state.borrow().settings.appearance_theme();
    let choices = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    choices.add_css_class("okp-settings-segmented");
    let light = appearance_theme_button("Light", current == AppearanceTheme::Light);
    let auto = appearance_theme_button("Auto", current != AppearanceTheme::Light);

    let light_window = window.clone();
    let light_state = Rc::clone(&state);
    let light_toast = Rc::clone(&status_toast);
    let light_button = light.clone();
    let light_auto = auto.clone();
    light.connect_clicked(move |_| {
        set_appearance_theme(
            AppearanceTheme::Light,
            &light_window,
            &light_state,
            &light_toast,
            &light_button,
            &light_auto,
        );
    });
    choices.append(&light);

    let auto_window = window.clone();
    let auto_state = state;
    let auto_toast = status_toast;
    let auto_button = auto.clone();
    let auto_light = light.clone();
    auto.connect_clicked(move |_| {
        set_appearance_theme(
            AppearanceTheme::Auto,
            &auto_window,
            &auto_state,
            &auto_toast,
            &auto_button,
            &auto_light,
        );
    });
    choices.append(&auto);
    theme_row.append(&choices);
    section.append(&theme_row);

    section.append(&settings_value_row("Player surface", "Dark video plane"));
    section.append(&settings_value_row(
        "Window chrome",
        "Custom captionless controls",
    ));
    section.append(&settings_value_row("Fullscreen caption", "Hidden"));
    section.append(&settings_value_row("Accent", "OK teal"));
    section
}

fn appearance_theme_button(label: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("okp-settings-segment-button");
    button.set_has_frame(false);
    button.set_size_request(72, 30);
    if selected {
        button.add_css_class("is-selected");
    }
    button
}

fn set_appearance_theme(
    theme: AppearanceTheme,
    window: &gtk::Window,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    selected: &gtk::Button,
    other: &gtk::Button,
) {
    {
        let mut state = state.borrow_mut();
        state.settings.set_appearance_theme(theme);
        if let Err(error) = state.settings.save() {
            eprintln!("Failed to save appearance theme: {error}");
            status_toast.show("Could not save appearance theme");
            return;
        }
    }
    selected.add_css_class("is-selected");
    other.remove_css_class("is-selected");
    apply_settings_window_theme(window, theme);
    status_toast.show(match theme {
        AppearanceTheme::Light => "Light theme selected",
        AppearanceTheme::Auto | AppearanceTheme::Dark => "Automatic theme selected",
    });
}
