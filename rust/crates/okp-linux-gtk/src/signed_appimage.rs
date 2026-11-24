use super::*;
use okp_core::signed_updates::{CHECK_INTERVAL, Channel, FOCUS_THROTTLE, Platform, Release};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime};

pub(crate) struct AppImageUpdates {
    root: PathBuf,
    helper: PathBuf,
    build: String,
    view: RefCell<View>,
    rows: RefCell<Vec<Rows>>,
    about: RefCell<Vec<glib::WeakRef<gtk::Label>>>,
    last_automatic: Cell<Option<Instant>>,
    player: RefCell<std::rc::Weak<RefCell<PlayerState>>>,
}

struct View {
    channel: Channel,
    offer: Option<Release>,
    problem: Option<String>,
    last_check: Option<SystemTime>,
    work: Option<&'static str>,
    skipped: bool,
}

struct Rows {
    channel: glib::WeakRef<gtk::DropDown>,
    result: glib::WeakRef<gtk::Label>,
    check: glib::WeakRef<gtk::Button>,
    install: glib::WeakRef<gtk::Button>,
    skip: glib::WeakRef<gtk::Button>,
}

impl AppImageUpdates {
    pub(crate) fn detect() -> Option<Rc<Self>> {
        if env::var_os("OKP_MANAGED_APPIMAGE").is_none() || env::var_os("APPIMAGE").is_none() {
            return None;
        }
        let build = APP_BUILD_VERSION.strip_prefix("0.11.0-beta.0.")?;
        Platform::Linux.build(build).ok()?;
        let helper = env::current_exe().ok()?.with_file_name("okp-updater");
        if !helper.is_file() {
            return None;
        }
        let root = PathBuf::from(
            env::var_os("XDG_DATA_HOME")
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    PathBuf::from(env::var_os("HOME").unwrap_or_default())
                        .join(".local/share")
                        .into_os_string()
                }),
        )
        .join("ok-player");
        let channel = std::fs::read_to_string(root.join("update-channel"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(Channel::Beta);
        Some(Rc::new(Self {
            root,
            helper,
            build: build.to_owned(),
            view: RefCell::new(View {
                channel,
                offer: None,
                problem: None,
                last_check: None,
                work: None,
                skipped: false,
            }),
            rows: RefCell::new(Vec::new()),
            about: RefCell::new(Vec::new()),
            last_automatic: Cell::new(None),
            player: RefCell::new(std::rc::Weak::new()),
        }))
    }

    pub(crate) fn attach(
        self: &Rc<Self>,
        window: &gtk::ApplicationWindow,
        state: &Rc<RefCell<PlayerState>>,
        toast: &Rc<StatusToast>,
    ) {
        *self.player.borrow_mut() = Rc::downgrade(state);
        let weak = Rc::downgrade(self);
        let settings = Rc::downgrade(state);
        let focus_settings = settings.clone();
        let notices = Rc::clone(toast);
        let focus_notices = Rc::clone(toast);
        let focus = weak.clone();
        window.connect_is_active_notify(move |window| {
            if window.is_active()
                && let (Some(updates), Some(state)) = (focus.upgrade(), focus_settings.upgrade())
                && state.borrow().settings.auto_check_updates()
            {
                updates.check(true, Some(Rc::clone(&focus_notices)));
            }
        });
        glib::timeout_add_local(CHECK_INTERVAL, move || {
            let (Some(updates), Some(state)) = (weak.upgrade(), settings.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if state.borrow().settings.auto_check_updates() {
                updates.check(true, Some(Rc::clone(&notices)));
            }
            glib::ControlFlow::Continue
        });
        if let Ok(message) = env::var("OKP_UPDATE_RESTART_ERROR") {
            toast.show(&message);
        }
        if state.borrow().settings.auto_check_updates() {
            self.check(true, Some(Rc::clone(toast)));
        }
    }

    pub(crate) fn section(self: &Rc<Self>, state: &Rc<RefCell<PlayerState>>) -> gtk::Box {
        let section = settings_section("Updates");
        section.append(&settings_value_row("Current version", APP_BUILD_VERSION));
        section.append(&settings_value_row("Current build", &self.build));
        section.append(&settings_value_row("Install", "Managed AppImage"));
        let result = gtk::Label::new(None);
        result.set_xalign(0.0);
        result.set_wrap(true);
        result.set_width_chars(1);
        result.add_css_class("okp-update-status");

        let channel = gtk::DropDown::from_strings(&["Beta", "Stable"]);
        channel.update_property(&[gtk::accessible::Property::Label("Release channel")]);
        channel.set_selected(u32::from(self.view.borrow().channel == Channel::Stable));
        let channel_label = gtk::Label::new(Some("Release channel"));
        channel_label.set_xalign(0.0);
        section.append(&channel_label);
        section.append(&channel);

        let automatic = gtk::CheckButton::with_label("Automatic checks");
        automatic.set_active(state.borrow().settings.auto_check_updates());
        let settings = Rc::downgrade(state);
        automatic.connect_toggled(move |control| {
            if let Some(state) = settings.upgrade() {
                let mut state = state.borrow_mut();
                state.settings.set_auto_check_updates(control.is_active());
                if let Err(error) = state.settings.save() {
                    eprintln!("Could not save automatic update checks: {error}");
                }
            }
        });
        section.append(&automatic);
        section.append(&result);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let check = gtk::Button::with_label("Check for updates");
        let install = gtk::Button::with_label("Install & Relaunch");
        let skip = gtk::Button::with_label("Skip This Build");
        for button in [&check, &install, &skip] {
            button.add_css_class("okp-settings-button");
            actions.append(button);
        }
        section.append(&actions);
        self.rows.borrow_mut().push(Rows {
            channel: channel.downgrade(),
            result: result.downgrade(),
            check: check.downgrade(),
            install: install.downgrade(),
            skip: skip.downgrade(),
        });

        let updates = Rc::clone(self);
        channel.connect_selected_notify(move |button| {
            let selection = if button.selected() == 0 {
                Channel::Beta
            } else {
                Channel::Stable
            };
            if updates.view.borrow().channel != selection {
                match updates.save_preference("update-channel", selection.name()) {
                    Ok(()) => {
                        let mut view = updates.view.borrow_mut();
                        view.channel = selection;
                        view.offer = None;
                        view.last_check = None;
                        view.skipped = false;
                        drop(view);
                        updates.check(false, None);
                    }
                    Err(error) => {
                        updates.view.borrow_mut().problem = Some(error.to_string());
                        updates.refresh();
                    }
                }
            }
        });
        let updates = Rc::clone(self);
        check.connect_clicked(move |_| updates.check(false, None));
        let updates = Rc::clone(self);
        install.connect_clicked(move |_| updates.install());
        let updates = Rc::clone(self);
        skip.connect_clicked(move |_| {
            let mut view = updates.view.borrow_mut();
            if let Some(offer) = &view.offer {
                match updates.save_preference(
                    &format!("skipped-{}", view.channel.name()),
                    &offer.build.to_string(),
                ) {
                    Ok(()) => view.skipped = true,
                    Err(error) => view.problem = Some(error.to_string()),
                }
            }
            drop(view);
            updates.refresh();
        });
        self.refresh();
        section
    }

    fn save_preference(&self, name: &str, value: &str) -> std::io::Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        writeln!(temp, "{value}")?;
        temp.persist(self.root.join(name))?;
        Ok(())
    }

    fn command(&self, operation: &str, channel: Channel) -> Command {
        let mut command = Command::new(&self.helper);
        command
            .args([operation, channel.name(), &self.build, APP_BUILD_SHA])
            .arg(self.root.join("OK-Player.AppImage"));
        command
    }

    fn check(self: &Rc<Self>, automatic: bool, toast: Option<Rc<StatusToast>>) {
        if self.view.borrow().work.is_some() {
            return;
        }
        if automatic {
            if self
                .last_automatic
                .get()
                .is_some_and(|at| at.elapsed() < FOCUS_THROTTLE)
            {
                return;
            }
            self.last_automatic.set(Some(Instant::now()));
        }
        let channel = self.view.borrow().channel;
        let mut command = self.command("check", channel);
        self.work(
            "Checking for updates…",
            move || {
                let output = command.output().map_err(|error| error.to_string())?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
                }
                serde_json::from_slice::<Option<Release>>(&output.stdout)
                    .map_err(|error| error.to_string())
            },
            move |updates, result| {
                let mut view = updates.view.borrow_mut();
                view.last_check = Some(SystemTime::now());
                match result {
                    Ok(offer) => {
                        let skipped = std::fs::read_to_string(
                            updates.root.join(format!("skipped-{}", channel.name())),
                        )
                        .ok();
                        view.skipped = offer.as_ref().is_some_and(|release| {
                            skipped
                                .as_ref()
                                .is_some_and(|value| value.trim() == release.build.to_string())
                        });
                        if let (Some(toast), Some(release)) = (toast, &offer)
                            && !view.skipped
                            && view.offer.as_ref() != Some(release)
                        {
                            toast.show(&format!(
                                "OK Player build {} is available in Settings → Updates.",
                                release.build
                            ));
                        }
                        view.offer = offer;
                    }
                    Err(error) => view.problem = Some(error),
                }
            },
        );
    }

    fn install(self: &Rc<Self>) {
        let view = self.view.borrow();
        let Some(offer) = &view.offer else { return };
        if view.work.is_some() {
            return;
        }
        let mut command = self.command("install", view.channel);
        command
            .arg(&offer.sha256)
            .arg(std::process::id().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        drop(view);
        self.work(
            "Downloading and verifying update…",
            move || {
                let mut child = command.spawn().map_err(|error| error.to_string())?;
                let mut line = String::new();
                BufReader::new(child.stdout.take().expect("piped stdout"))
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                if line.trim_end() != "ready" {
                    let output = child
                        .wait_with_output()
                        .map_err(|error| error.to_string())?;
                    return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
                }
                Ok(child)
            },
            |updates, result| {
                let result = result.and_then(|mut child| {
                    let player = updates
                        .player
                        .borrow()
                        .upgrade()
                        .ok_or("The player window has closed.")?;
                    save_current_progress(&player, false);
                    child
                        .stdin
                        .take()
                        .expect("piped stdin")
                        .write_all(b"relaunch\n")
                        .map_err(|error| error.to_string())?;
                    if !close_player_window() {
                        return Err("Could not close OK Player for restart.".into());
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    updates.view.borrow_mut().problem = Some(error);
                }
            },
        );
    }

    fn work<T: Send + 'static>(
        self: &Rc<Self>,
        message: &'static str,
        run: impl FnOnce() -> Result<T, String> + Send + 'static,
        finish: impl FnOnce(&Self, Result<T, String>) + 'static,
    ) {
        self.view.borrow_mut().work = Some(message);
        self.view.borrow_mut().problem = None;
        self.refresh();
        let updates = Rc::downgrade(self);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(run());
        });
        let mut finish = Some(finish);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(updates) = updates.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The updater stopped unexpectedly.".into())
                }
            };
            updates.view.borrow_mut().work = None;
            finish.take().expect("one update result")(&updates, result);
            updates.refresh();
            glib::ControlFlow::Break
        });
    }

    pub(crate) fn register_about(&self, label: &gtk::Label) {
        self.about.borrow_mut().push(label.downgrade());
    }

    pub(crate) fn status(&self) -> String {
        let view = self.view.borrow();
        if let Some(work) = view.work {
            work.to_owned()
        } else if let Some(problem) = &view.problem {
            problem.clone()
        } else if let Some(offer) = &view.offer {
            format!(
                "Available version {} · Build {}{}",
                offer.version,
                offer.build,
                if view.skipped { " (skipped)" } else { "" }
            )
        } else if view.last_check.is_some() {
            "You’re up to date.".to_owned()
        } else {
            "No update check yet.".to_owned()
        }
    }

    fn refresh(&self) {
        let status = self.status();
        self.about.borrow_mut().retain(|label| {
            let Some(label) = label.upgrade() else {
                return false;
            };
            label.set_text(&status);
            true
        });
        let view = self.view.borrow();
        let last = view
            .last_check
            .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
            .and_then(|at| glib::DateTime::from_unix_local(at.as_secs() as i64).ok())
            .and_then(|at| at.format("%x %X").ok())
            .map_or_else(|| "Never".to_owned(), |at| at.to_string());
        self.rows.borrow_mut().retain(|row| {
            let (Some(channel), Some(result), Some(check), Some(install), Some(skip)) = (
                row.channel.upgrade(),
                row.result.upgrade(),
                row.check.upgrade(),
                row.install.upgrade(),
                row.skip.upgrade(),
            ) else {
                return false;
            };
            result.set_text(&format!("Last checked {last}\n{status}"));
            channel.set_sensitive(view.work.is_none());
            check.set_sensitive(view.work.is_none());
            install.set_visible(view.offer.is_some());
            install.set_sensitive(view.work.is_none());
            install.set_label(if view.skipped {
                "Install Anyway"
            } else {
                "Install & Relaunch"
            });
            skip.set_visible(view.offer.is_some());
            skip.set_sensitive(view.work.is_none() && !view.skipped);
            true
        });
    }

    pub(crate) fn confirm_startup(&self) {
        let Some(path) = env::var_os("OKP_UPDATE_RESTART_RECEIPT") else {
            return;
        };
        if let Ok(mut receipt) = std::fs::File::create_new(path) {
            let _ = writeln!(receipt, "{}\n{}", self.build, APP_BUILD_SHA);
        }
    }
}
