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
    section.append(&settings_value_row(
        "Install",
        linux_update_install_status(),
    ));

    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let preview_status = settings_update_preview_status();
    let initial_update_status = preview_status
        .clone()
        .unwrap_or_else(|| state.borrow().linux_update_status.clone());
    let status = gtk::Label::new(Some(
        &initial_update_status.settings_status_text(auto_check_enabled),
    ));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_width_chars(1);
    status.set_max_width_chars(58);
    status.set_wrap(true);
    row.append(&status);

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    auto_row.add_css_class("okp-settings-switch-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Automatic checks"));
    auto_label.add_css_class("okp-info-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some(
        "Check the linux pre-release feed on startup and show a toast when an update is ready.",
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

    let auto_switch = about_toggle_button(auto_check_enabled);
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    let auto_status = status.clone();
    let auto_state_text = auto_state_label.clone();
    auto_switch.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        set_about_toggle_active(button, enabled);
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_status.set_text(update_status_intro(enabled));
        auto_state_text.set_text(if enabled { "On" } else { "Off" });
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

    let pending_update = Rc::new(RefCell::new(initial_update_status.pending_update()));

    let check_button = gtk::Button::with_label(&initial_update_status.action_label());
    check_button.add_css_class("okp-settings-button");
    check_button.set_sensitive(!matches!(
        initial_update_status,
        LinuxUpdateStatus::Checking
    ));
    let check_status = status.clone();
    let check_pending = Rc::clone(&pending_update);
    let check_state = Rc::clone(&state);
    let check_toast = Rc::clone(&status_toast);
    check_button.connect_clicked(move |button| {
        if let Some(update) = check_pending.borrow().clone() {
            start_update_download(
                button,
                &check_status,
                update,
                Rc::clone(&check_state),
                Rc::clone(&check_toast),
            );
            return;
        }

        start_update_check_for_ui(
            button,
            &check_status,
            &check_pending,
            Rc::clone(&check_state),
            Rc::clone(&check_toast),
            "Checking the update feed...",
            true,
        );
    });
    actions.append(&check_button);
    if preview_status.is_none()
        && auto_check_enabled
        && matches!(initial_update_status, LinuxUpdateStatus::NotChecked)
    {
        let auto_button = check_button.clone();
        let auto_status = status.clone();
        let auto_pending = Rc::clone(&pending_update);
        let auto_state = Rc::clone(&state);
        let auto_toast = Rc::clone(&status_toast);
        glib::idle_add_local_once(move || {
            start_update_check_for_ui(
                &auto_button,
                &auto_status,
                &auto_pending,
                auto_state,
                auto_toast,
                "Checking the update feed...",
                false,
            );
        });
    }

    let releases_button = gtk::Button::with_label("Open Releases");
    releases_button.add_css_class("okp-settings-button");
    releases_button.connect_clicked(move |_| {
        open_external_url("https://github.com/BeFeast/ok-player/releases")
    });
    actions.append(&releases_button);

    row.append(&actions);
    section.append(&row);

    section
}

pub(crate) fn settings_update_preview_status() -> Option<LinuxUpdateStatus> {
    match env::var("OKP_SETTINGS_UPDATE_PREVIEW")
        .ok()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "up-to-date" => Some(LinuxUpdateStatus::UpToDate),
        "checking" => Some(LinuxUpdateStatus::Checking),
        "available" => Some(LinuxUpdateStatus::Available(PendingLinuxUpdate {
            manager: None,
            target: LinuxUpdateTarget::Deb(DebUpdate {
                version: "0.11.0-beta.2".to_owned(),
                name: "ok-player_0.11.0-beta.2_amd64.deb".to_owned(),
                url: "https://example.invalid/ok-player.deb".to_owned(),
                size: Some(42),
                sums_url: None,
                expected_sha256: None,
            }),
        })),
        "error" => Some(LinuxUpdateStatus::Failed(
            "the update feed is temporarily unavailable".to_owned(),
        )),
        _ => None,
    }
}

pub(crate) fn update_status_intro(auto_check_enabled: bool) -> &'static str {
    if auto_check_enabled {
        "Automatic update checks are on. AppImage installs restart in place; .deb installs request admin approval and fall back to opening the installer."
    } else {
        "Automatic update checks are off. Use Check for updates any time."
    }
}

pub(crate) fn start_update_check_for_ui(
    button: &gtk::Button,
    status: &gtk::Label,
    pending: &Rc<RefCell<Option<PendingLinuxUpdate>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    checking_status: &str,
    show_toast: bool,
) {
    button.set_sensitive(false);
    button.set_label("Checking...");
    status.set_text(checking_status);
    pending.borrow_mut().take();
    state.borrow_mut().linux_update_status = LinuxUpdateStatus::Checking;

    let channel = state.borrow().settings.update_channel();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check_for_linux_update(channel));
    });

    let button = button.clone();
    let status = status.clone();
    let pending = Rc::clone(pending);
    glib::timeout_add_local(Duration::from_millis(120), move || {
        match receiver.try_recv() {
            Ok(result) => {
                apply_update_check_result(
                    &button,
                    &status,
                    &pending,
                    Rc::clone(&state),
                    &status_toast,
                    show_toast,
                    result,
                );
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Update check failed");
                state.borrow_mut().linux_update_status =
                    LinuxUpdateStatus::Failed("update check channel closed".to_owned());
                glib::ControlFlow::Break
            }
        }
    });
}

pub(crate) fn start_update_download(
    button: &gtk::Button,
    status: &gtk::Label,
    update: PendingLinuxUpdate,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    save_current_progress(&state, false);
    button.set_sensitive(false);
    button.set_label("Downloading...");
    status.set_text(&format!(
        "Downloading {}...",
        update
            .target_version()
            .unwrap_or_else(|| "update".to_owned())
    ));
    status_toast.show("Downloading update");

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(download_and_apply_linux_update(update));
    });

    let button = button.clone();
    let status = status.clone();
    let toast = Rc::clone(&status_toast);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        match receiver.try_recv() {
            Ok(Ok(LinuxUpdateApplyResult::Restarting)) => {
                button.set_label("Restarting...");
                status.set_text("Restarting to apply update...");
                glib::ControlFlow::Break
            }
            Ok(Ok(LinuxUpdateApplyResult::DebInstalled(_path))) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Installed. Restart OK Player to finish.");
                toast.show("Update installed");
                glib::ControlFlow::Break
            }
            Ok(Ok(LinuxUpdateApplyResult::InstallerOpened(_path))) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Installer opened. Complete it to update.");
                toast.show("Installer opened");
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text(&format!("Update failed: {error}"));
                toast.show("Update failed");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Update failed.");
                glib::ControlFlow::Break
            }
        }
    });
}

pub(crate) fn apply_update_check_result(
    button: &gtk::Button,
    status: &gtk::Label,
    pending: &Rc<RefCell<Option<PendingLinuxUpdate>>>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: &StatusToast,
    show_toast: bool,
    result: LinuxUpdateCheckResult,
) {
    state.borrow_mut().linux_update_status = LinuxUpdateStatus::from_check_result(&result);
    button.set_sensitive(true);
    match result {
        LinuxUpdateCheckResult::UpToDate => {
            pending.borrow_mut().take();
            button.set_label("Check for updates");
            status.set_text("Up to date");
            if show_toast {
                status_toast.show("OK Player is up to date");
            }
        }
        LinuxUpdateCheckResult::Available(update) => {
            let status_text = update.available_status();
            let action_label = update.action_label();
            pending.borrow_mut().replace(update);
            button.set_label(action_label);
            status.set_text(&status_text);
            if show_toast {
                status_toast.show("Update available");
            }
        }
        LinuxUpdateCheckResult::Failed(error) => {
            pending.borrow_mut().take();
            button.set_label("Check for updates");
            status.set_text(&format!("Update check failed: {error}"));
            if show_toast {
                status_toast.show("Update check failed");
            }
        }
    }
}

pub(crate) fn check_updates_on_startup(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let channel = state.borrow().settings.update_channel();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check_for_linux_update(channel));
    });

    glib::timeout_add_local(Duration::from_millis(500), move || {
        match receiver.try_recv() {
            Ok(result) => {
                state.borrow_mut().linux_update_status =
                    LinuxUpdateStatus::from_check_result(&result);
                match result {
                    LinuxUpdateCheckResult::Available(update) => {
                        let version = update
                            .target_version()
                            .unwrap_or_else(|| "new version".to_owned());
                        status_toast.show(&format!("Update available: {version}"));
                    }
                    LinuxUpdateCheckResult::Failed(error) => {
                        eprintln!("Startup update check failed: {error}");
                    }
                    LinuxUpdateCheckResult::UpToDate => {}
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

pub(crate) fn check_for_linux_update(channel: UpdateChannel) -> LinuxUpdateCheckResult {
    let channel = effective_update_channel(channel);
    if channel == UpdateChannel::Candidate {
        return check_for_linux_candidate_channel_update();
    }

    if linux_install_lane() == CandidateInstallLane::Debian {
        return match check_for_linux_deb_update() {
            Ok(Some(update)) => LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                manager: None,
                target: LinuxUpdateTarget::Deb(update),
            }),
            Ok(None) => LinuxUpdateCheckResult::UpToDate,
            Err(error) => LinuxUpdateCheckResult::Failed(error),
        };
    }

    let manager = match linux_update_manager(UpdateChannel::Public) {
        Ok(manager) => manager,
        Err(error) => return LinuxUpdateCheckResult::Failed(error),
    };

    if let Some(asset) = manager.get_update_pending_restart() {
        return LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
            manager: Some(manager),
            target: LinuxUpdateTarget::Asset(Box::new(asset)),
        });
    }

    match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => {
            LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                manager: Some(manager),
                target: LinuxUpdateTarget::Info(update),
            })
        }
        Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
            LinuxUpdateCheckResult::UpToDate
        }
        Err(error) => LinuxUpdateCheckResult::Failed(error.to_string()),
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

fn check_for_linux_candidate_channel_update() -> LinuxUpdateCheckResult {
    let candidate = match fetch_linux_candidate_update() {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            eprintln!("Candidate update stage=selection result=up-to-date");
            return LinuxUpdateCheckResult::UpToDate;
        }
        Err(error) => {
            eprintln!("Candidate update stage=selection result=failed error={error}");
            return LinuxUpdateCheckResult::Failed(error);
        }
    };
    eprintln!(
        "Candidate update stage=selection result=available version={} build={} commit_sha={}",
        candidate.version, candidate.build, candidate.commit_sha
    );
    let deb_update = candidate_deb_update(&candidate);
    let lane = linux_install_lane();
    eprintln!("Candidate update stage=install-lane lane={lane:?}");
    match candidate_channel::route_candidate_update(&candidate, lane, None) {
        Ok(CandidateUpdateRoute::Debian) => {
            eprintln!(
                "Candidate update stage=final result=available lane=deb version={}",
                candidate.version
            );
            return LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                manager: None,
                target: LinuxUpdateTarget::Deb(deb_update),
            });
        }
        Err(candidate_channel::CandidateUpdateRouteError::MissingAppImageCheck) => {}
        Ok(_) => unreachable!("a route without an AppImage result can only select Debian"),
        Err(error) => {
            eprintln!("Candidate update stage=final result=failed error={error}");
            return LinuxUpdateCheckResult::Failed(error.to_string());
        }
    }

    let manager = match linux_candidate_update_manager(&candidate) {
        Ok(manager) => manager,
        Err(error) => {
            let error = format!("candidate AppImage updater failed: {error}");
            eprintln!("Candidate update stage=velopack result=failed error={error}");
            return LinuxUpdateCheckResult::Failed(error);
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
                LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                    manager: Some(manager),
                    target: LinuxUpdateTarget::Asset(Box::new(asset)),
                })
            }
            Ok(_) => unreachable!("AppImage pending restart must route to its pending asset"),
            Err(error) => {
                eprintln!("Candidate update stage=final result=failed error={error}");
                LinuxUpdateCheckResult::Failed(error.to_string())
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
                    LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                        manager: Some(manager),
                        target: LinuxUpdateTarget::Info(update),
                    })
                }
                Ok(_) => unreachable!("AppImage update must route to its update information"),
                Err(error) => {
                    eprintln!("Candidate update stage=final result=failed error={error}");
                    LinuxUpdateCheckResult::Failed(error.to_string())
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
            LinuxUpdateCheckResult::Failed(error.to_string())
        }
    }
}

fn candidate_appimage_empty_result(
    candidate: &CandidateUpdate,
    lane: CandidateInstallLane,
    check: CandidateAppImageCheck,
) -> LinuxUpdateCheckResult {
    let result = candidate_channel::route_candidate_update(candidate, lane, Some(&check));
    let error = result.expect_err("a newer accepted AppImage candidate cannot be empty");
    eprintln!("Candidate update stage=velopack result=failed error={error}");
    LinuxUpdateCheckResult::Failed(error.to_string())
}

pub(crate) fn linux_install_lane() -> CandidateInstallLane {
    match APP_PACKAGE_KIND {
        "appimage" => CandidateInstallLane::AppImage,
        "deb" | "development" => CandidateInstallLane::Debian,
        _ => unreachable!("build.rs validates OKP_PACKAGE_KIND"),
    }
}

pub(crate) fn linux_update_feed_base_url() -> String {
    env::var("OKP_LINUX_UPDATE_FEED_URL").unwrap_or_else(|_| LINUX_UPDATE_FEED_BASE_URL.to_owned())
}

pub(crate) fn download_and_apply_linux_update(
    update: PendingLinuxUpdate,
) -> Result<LinuxUpdateApplyResult, String> {
    match update.target {
        LinuxUpdateTarget::Info(info) => {
            let info = info.as_ref();
            let manager = update
                .manager
                .as_ref()
                .ok_or_else(|| "Self-update manager unavailable.".to_owned())?;
            manager
                .download_updates(info, None)
                .map_err(|error| error.to_string())?;
            manager
                .apply_updates_and_restart(info)
                .map_err(|error| error.to_string())?;
            Ok(LinuxUpdateApplyResult::Restarting)
        }
        LinuxUpdateTarget::Asset(asset) => {
            let asset = asset.as_ref();
            let manager = update
                .manager
                .as_ref()
                .ok_or_else(|| "Self-update manager unavailable.".to_owned())?;
            manager
                .apply_updates_and_restart(asset)
                .map_err(|error| error.to_string())?;
            Ok(LinuxUpdateApplyResult::Restarting)
        }
        LinuxUpdateTarget::Deb(update) => {
            let path = download_deb_update(update)?;
            if try_install_deb_update(&path)? {
                Ok(LinuxUpdateApplyResult::DebInstalled(path))
            } else {
                open_deb_installer(&path)?;
                Ok(LinuxUpdateApplyResult::InstallerOpened(path))
            }
        }
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
    let mut response = ureq::get(&url)
        .header("Accept", "application/json")
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
        APP_BUILD_VERSION,
    ))
}

fn candidate_deb_update(candidate: &CandidateUpdate) -> DebUpdate {
    DebUpdate {
        version: candidate.version.clone(),
        name: candidate.package.name.clone(),
        url: candidate.package.url.clone(),
        size: candidate.package.size,
        sums_url: candidate.sums_url.clone(),
        expected_sha256: Some(candidate.package.sha256.clone()),
    }
}

pub(crate) fn download_deb_update(update: DebUpdate) -> Result<PathBuf, String> {
    let cache_dir = linux_update_cache_dir();
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Could not create update cache: {error}"))?;

    let manifest = download_deb_checksums(&update)?;

    let mut response = ureq::get(&update.url)
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("Download failed: {error}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| format!("Download failed: {error}"))?;
    if let Some(expected) = update.size
        && expected > 0
        && bytes.len() as u64 != expected
    {
        return Err(format!(
            "Download size mismatch: expected {expected} bytes, got {}.",
            bytes.len()
        ));
    }

    stage_verified_deb(&bytes, &manifest, &update.name, &cache_dir)
}

/// Fetches the release's `SHA256SUMS` manifest. Fails closed when the
/// release publishes none: a stripped manifest must block the install, not
/// downgrade it to unverified. Errors here are checksum-download errors,
/// deliberately distinct from package download and verification errors.
pub(crate) fn download_deb_checksums(update: &DebUpdate) -> Result<String, String> {
    let sums_url = update.sums_url.as_deref().ok_or_else(|| {
        format!("Release {} does not publish {SHA256SUMS_ASSET}; refusing to install an unverifiable update.", update.version)
    })?;
    let mut response = ureq::get(sums_url)
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("Checksum download failed: {error}"))?;
    let manifest = response
        .body_mut()
        .with_config()
        .limit(LINUX_SHA256SUMS_MAX_BYTES)
        .read_to_string()
        .map_err(|error| format!("Checksum download failed: {error}"))?;
    verify_deb_feed_identity(update, &manifest)?;
    Ok(manifest)
}

pub(crate) fn verify_deb_feed_identity(update: &DebUpdate, manifest: &str) -> Result<(), String> {
    let Some(expected_sha256) = update.expected_sha256.as_deref() else {
        return Ok(());
    };
    let sums = sha256sums::Sha256Sums::parse(manifest)
        .map_err(|error| format!("Candidate identity check failed: {error}"))?;
    let package = candidate_channel::CandidatePackage {
        name: update.name.clone(),
        url: update.url.clone(),
        size: update.size,
        sha256: expected_sha256.to_owned(),
    };
    package
        .matches_sums(&sums)
        .map_err(|error| format!("Candidate identity check failed: {error}"))
}

/// Stages the downloaded package and verifies it against the manifest
/// before it is renamed into place. A payload that fails verification is
/// deleted and never becomes the path handed to the privileged installer.
pub(crate) fn stage_verified_deb(
    bytes: &[u8],
    manifest: &str,
    file_name: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let target = cache_dir.join(file_name);
    let temp = cache_dir.join(format!("{file_name}.part"));

    fs::write(&temp, bytes).map_err(|error| format!("Could not save update: {error}"))?;
    if let Err(error) = verify_staged_deb(&temp, file_name, manifest) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    fs::rename(&temp, &target).map_err(|error| format!("Could not finalize update: {error}"))?;
    Ok(target)
}

/// Re-reads the staged file from disk so the digest covers the exact bytes
/// the installer will consume, not the in-memory copy they came from.
pub(crate) fn verify_staged_deb(
    path: &Path,
    file_name: &str,
    manifest: &str,
) -> Result<(), String> {
    let staged = fs::read(path)
        .map_err(|error| format!("Could not read staged update for verification: {error}"))?;
    sha256sums::verify_payload(manifest, file_name, &staged)
        .map_err(|error| format!("Update integrity check failed: {error}"))
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

pub(crate) fn deb_self_install_available() -> bool {
    find_executable("pkexec").is_some()
        && (find_executable("apt-get").is_some() || find_executable("apt").is_some())
}

pub(crate) fn try_install_deb_update(path: &Path) -> Result<bool, String> {
    if env::var_os("OKP_SKIP_DEB_SELF_INSTALL").is_some() {
        return Ok(false);
    }

    let Some(pkexec) = find_executable("pkexec") else {
        return Ok(false);
    };
    let Some(apt) = find_executable("apt-get").or_else(|| find_executable("apt")) else {
        return Ok(false);
    };

    let mut child = Command::new(pkexec)
        .arg(apt)
        .arg("install")
        .arg("-y")
        .arg(path)
        .spawn()
        .map_err(|error| {
            format!(
                "Downloaded to {}, but could not request administrator approval: {error}",
                path.display()
            )
        })?;

    let timeout = deb_self_install_timeout();
    match wait_for_child_with_timeout(&mut child, timeout).map_err(|error| {
        format!(
            "Downloaded to {}, but could not wait for administrator approval: {error}",
            path.display()
        )
    })? {
        Some(status) if status.success() => Ok(true),
        Some(status) => {
            eprintln!(
                "Privileged .deb install exited with {status}; falling back to installer open."
            );
            Ok(false)
        }
        None => {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!(
                "Privileged .deb install timed out after {}s; falling back to installer open.",
                timeout.as_secs()
            );
            Ok(false)
        }
    }
}

pub(crate) fn deb_self_install_timeout() -> Duration {
    parse_deb_self_install_timeout(
        env::var("OKP_DEB_SELF_INSTALL_TIMEOUT_SECS")
            .ok()
            .as_deref(),
    )
}

pub(crate) fn parse_deb_self_install_timeout(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEB_SELF_INSTALL_TIMEOUT)
}

pub(crate) fn wait_for_child_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<ExitStatus>, std::io::Error> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Ok(None);
        }
        let remaining = timeout.saturating_sub(elapsed);
        std::thread::sleep(remaining.min(Duration::from_millis(100)));
    }
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

pub(crate) fn open_deb_installer(path: &Path) -> Result<(), String> {
    if env::var_os("OKP_SKIP_OPEN_INSTALLER").is_some() {
        return Ok(());
    }

    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|error| {
            format!(
                "Downloaded to {}, but could not open installer: {error}",
                path.display()
            )
        })?;
    Ok(())
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
