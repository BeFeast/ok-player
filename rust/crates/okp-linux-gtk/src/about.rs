use super::*;

pub(crate) fn settings_about_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let snapshot = AboutSnapshot::capture(&state);
    let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pane.add_css_class("okp-about-pane");

    pane.append(&about_identity_hero(&snapshot));

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-about-identity-divider");
    pane.append(&divider);

    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 11);
    sheet.add_css_class("okp-about-sheet");
    sheet.append(&about_app_card(&snapshot));
    sheet.append(&about_updates_card(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    sheet.append(&about_engine_card(&snapshot));
    sheet.append(&about_host_card(&snapshot));
    pane.append(&sheet);

    pane.append(&about_footer(snapshot, status_toast));
    pane
}

pub(crate) fn about_identity_hero(snapshot: &AboutSnapshot) -> gtk::Box {
    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    hero.add_css_class("okp-about-identity");

    let illustration = gtk::Box::new(gtk::Orientation::Vertical, 0);
    illustration.add_css_class("okp-about-illustration");
    illustration.set_halign(gtk::Align::Center);
    illustration.set_valign(gtk::Align::Center);
    illustration.append(&about_illustration());
    hero.append(&illustration);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);

    let wordmark = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    wordmark.add_css_class("okp-about-wordmark");
    let ok = gtk::Label::new(Some("OK"));
    ok.add_css_class("okp-about-wordmark-ok");
    let player = gtk::Label::new(Some(" Player"));
    player.add_css_class("okp-about-wordmark-player");
    wordmark.append(&ok);
    wordmark.append(&player);
    text.append(&wordmark);

    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    chips.add_css_class("okp-about-chip-row");
    let version = gtk::Label::new(Some(&snapshot.version));
    version.add_css_class("okp-about-version-chip");
    chips.append(&version);
    let channel = gtk::Label::new(Some(&about_hero_channel(&snapshot.channel)));
    channel.add_css_class("okp-about-channel-chip");
    chips.append(&channel);
    text.append(&chips);

    let tagline = gtk::Label::new(Some("The most elegant media player on Linux."));
    tagline.add_css_class("okp-about-tagline");
    tagline.set_xalign(0.0);
    text.append(&tagline);

    let byline = gtk::Label::new(Some("Open source · by Oleg Kossoy"));
    byline.add_css_class("okp-about-byline");
    byline.set_xalign(0.0);
    text.append(&byline);

    hero.append(&text);
    hero
}

pub(crate) fn about_illustration() -> gtk::Widget {
    app_identity_image(92, "okp-about-mark").upcast()
}

#[cfg(test)]
pub(crate) fn about_illustration_path() -> Option<PathBuf> {
    app_icon_path()
}

pub(crate) fn about_display_version(version: &str) -> String {
    version
        .split_once("-linux-")
        .map(|(base, _)| base)
        .unwrap_or(version)
        .to_owned()
}

pub(crate) fn about_display_channel(version: &str) -> String {
    if version.contains("-linux-alpha") {
        "Linux alpha"
    } else if version.contains("-linux-beta") {
        "Linux beta"
    } else {
        "Linux"
    }
    .to_owned()
}

pub(crate) fn about_hero_channel(channel: &str) -> String {
    channel
        .split_whitespace()
        .last()
        .unwrap_or(channel)
        .to_uppercase()
}

pub(crate) fn about_app_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 9);
    rows.append(&about_spec_row("Version", &snapshot.version, true, None));
    rows.append(&about_spec_row("Channel", &snapshot.channel, false, None));
    rows.append(&about_spec_row("Build", &snapshot.build, true, None));
    rows.append(&about_spec_row("License", &snapshot.license, true, None));
    about_card("APP", &rows)
}

pub(crate) fn about_engine_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 9);
    let hwdec_tag = if snapshot.hwdec == "off" {
        ("OFF", false)
    } else {
        ("ON", true)
    };
    rows.append(&about_spec_row("libmpv", &snapshot.libmpv, true, None));
    rows.append(&about_spec_row(
        "FFmpeg",
        &snapshot.ffmpeg,
        true,
        Some(("SYSTEM", false)),
    ));
    rows.append(&about_spec_row(
        "Render API",
        &snapshot.render_api,
        true,
        None,
    ));
    rows.append(&about_spec_row("Graphics", &snapshot.graphics, true, None));
    rows.append(&about_spec_row(
        "Hardware decode",
        &snapshot.hwdec,
        false,
        Some(hwdec_tag),
    ));
    about_card("ENGINE", &rows)
}

pub(crate) fn about_updates_card(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let initial_update_status = state.borrow().linux_update_status.clone();

    let status_row = about_spec_row(
        "Status",
        &initial_update_status.about_status_text(),
        false,
        None,
    );
    let status_label = status_row
        .last_child()
        .and_then(|wrap| wrap.first_child())
        .and_then(|widget| widget.downcast::<gtk::Label>().ok())
        .unwrap_or_else(|| gtk::Label::new(Some("Not checked")));
    content.append(&status_row);

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    auto_row.add_css_class("okp-about-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Check automatically"));
    auto_label.add_css_class("okp-about-row-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some("On launch"));
    auto_detail.add_css_class("okp-about-row-detail");
    auto_detail.set_xalign(0.0);
    auto_text.append(&auto_detail);
    auto_row.append(&auto_text);

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let auto_switch = about_toggle_button(auto_check_enabled);
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    auto_switch.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        if enabled {
            button.add_css_class("is-active");
        } else {
            button.remove_css_class("is-active");
        }
        if let Some(knob) = button.first_child() {
            knob.set_halign(if enabled {
                gtk::Align::End
            } else {
                gtk::Align::Start
            });
        }
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_toast.show(if enabled {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        });
    });
    auto_row.append(&auto_switch);
    content.append(&auto_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    actions.set_halign(gtk::Align::Start);
    let pending_update = Rc::new(RefCell::new(initial_update_status.pending_update()));
    let check_button = gtk::Button::with_label(&initial_update_status.action_label());
    check_button.add_css_class("okp-about-check-button");
    check_button.set_has_frame(false);
    check_button.set_size_request(132, 34);
    check_button.set_sensitive(!matches!(
        initial_update_status,
        LinuxUpdateStatus::Checking
    ));
    let check_status = status_label.clone();
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
            "Checking...",
            true,
        );
    });
    actions.append(&check_button);
    if auto_check_enabled && matches!(initial_update_status, LinuxUpdateStatus::NotChecked) {
        let auto_button = check_button.clone();
        let auto_status = status_label.clone();
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
                "Checking...",
                false,
            );
        });
    }
    content.append(&actions);

    about_card("UPDATES", &content)
}

pub(crate) fn about_host_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let grid = gtk::Grid::new();
    grid.add_css_class("okp-about-host-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(26);
    grid.set_row_spacing(8);
    grid.attach(
        &about_spec_row("Linux", &snapshot.os, true, None),
        0,
        0,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("GTK", &snapshot.gtk, true, None),
        1,
        0,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("CPU", &snapshot.cpu, true, None),
        0,
        1,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("Install", &snapshot.install, false, None),
        1,
        1,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("Updates", &snapshot.updates, false, Some(("ON", true))),
        0,
        2,
        1,
        1,
    );
    about_card("HOST", &grid)
}

pub(crate) fn about_toggle_button(active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-toggle");
    button.set_has_frame(false);
    button.set_size_request(39, 22);
    button.set_halign(gtk::Align::End);
    button.set_valign(gtk::Align::Center);

    let knob = gtk::Box::new(gtk::Orientation::Vertical, 0);
    knob.add_css_class("okp-about-toggle-knob");
    knob.set_valign(gtk::Align::Center);
    button.set_child(Some(&knob));
    set_about_toggle_active(&button, active);
    button
}

pub(crate) fn set_about_toggle_active(button: &gtk::Button, active: bool) {
    if active {
        button.add_css_class("is-active");
    } else {
        button.remove_css_class("is-active");
    }
    if let Some(knob) = button.first_child() {
        knob.set_halign(if active {
            gtk::Align::End
        } else {
            gtk::Align::Start
        });
    }
}

pub(crate) fn about_card<T: IsA<gtk::Widget>>(title: &str, content: &T) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("okp-about-card");
    match title {
        "APP" => {
            card.add_css_class("okp-about-card-app");
            card.set_size_request(-1, 151);
        }
        "UPDATES" => {
            card.add_css_class("okp-about-card-updates");
            card.set_size_request(-1, 164);
        }
        "ENGINE" => {
            card.add_css_class("okp-about-card-engine");
            card.set_size_request(-1, 176);
        }
        "HOST" => {
            card.add_css_class("okp-about-card-host");
            card.set_size_request(-1, 125);
        }
        _ => {}
    }

    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-about-card-title");
    label.set_xalign(0.0);
    card.append(&label);
    card.append(content);
    card
}

pub(crate) fn about_spec_row(
    label: &str,
    value: &str,
    mono: bool,
    tag: Option<(&str, bool)>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("okp-about-row");
    row.set_hexpand(true);

    let key = gtk::Label::new(Some(label));
    key.add_css_class("okp-about-row-label");
    key.set_xalign(0.0);
    key.set_hexpand(true);
    row.append(&key);

    let value_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    value_wrap.set_halign(gtk::Align::End);

    let val = gtk::Label::new(Some(value));
    val.add_css_class(if mono {
        "okp-about-row-value-mono"
    } else {
        "okp-about-row-value"
    });
    val.set_xalign(1.0);
    val.set_width_chars(1);
    val.set_max_width_chars(34);
    val.set_ellipsize(pango::EllipsizeMode::End);
    val.set_selectable(true);
    value_wrap.append(&val);

    if let Some((text, accent)) = tag {
        let tag = gtk::Label::new(Some(text));
        tag.add_css_class("okp-about-tag");
        if accent {
            tag.add_css_class("is-accent");
        }
        value_wrap.append(&tag);
    }

    row.append(&value_wrap);
    row
}

pub(crate) fn about_footer(snapshot: AboutSnapshot, status_toast: Rc<StatusToast>) -> gtk::Box {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("okp-about-footer");

    let copy = about_action_button("Copy diagnostics", "edit-copy-symbolic");
    let copy_snapshot = snapshot.clone();
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display
                .clipboard()
                .set_text(&about_diagnostics_text(&copy_snapshot));
        }
        status_toast.show("Diagnostics copied");
    });
    footer.append(&copy);

    let links = gtk::Box::new(gtk::Orientation::Horizontal, 13);
    links.set_halign(gtk::Align::End);
    links.set_hexpand(true);

    let github = about_link_button("GitHub");
    github.connect_clicked(|_| open_external_url("https://github.com/BeFeast/ok-player"));
    links.append(&github);

    let dot = gtk::Label::new(Some("•"));
    dot.add_css_class("okp-about-link-dot");
    dot.set_valign(gtk::Align::Center);
    links.append(&dot);

    let license = about_link_button("License");
    license.connect_clicked(|_| {
        open_external_url("https://github.com/BeFeast/ok-player/blob/main/LICENSE")
    });
    links.append(&license);
    footer.append(&links);

    footer
}

pub(crate) fn about_action_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-copy-button");
    button.set_has_frame(false);
    button.set_size_request(147, 34);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

pub(crate) fn about_link_button(label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-link-button");
    button.set_has_frame(false);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    content.append(&gtk::Label::new(Some(label)));
    let icon = gtk::Label::new(Some("↗"));
    icon.add_css_class("okp-about-link-arrow");
    content.append(&icon);
    button.set_child(Some(&content));
    button
}

pub(crate) fn about_diagnostics_text(snapshot: &AboutSnapshot) -> String {
    format!(
        "OK Player {} ({})\nBuild {} - current\nLicense {}\n\nEngine\n  libmpv           {}\n  FFmpeg           {}\n  Render API       {}\n  Graphics         {}\n  Hardware decode  {}\n\nHost\n  Linux            {}\n  GTK              {}\n  CPU              {}\n  Install          {}\n  Updates          {}",
        snapshot.package_version,
        snapshot.channel,
        snapshot.build,
        snapshot.license,
        snapshot.libmpv,
        snapshot.ffmpeg,
        snapshot.render_api,
        snapshot.graphics,
        snapshot.hwdec,
        snapshot.os,
        snapshot.gtk,
        snapshot.cpu,
        snapshot.install,
        snapshot.updates
    )
}

pub(crate) fn pkg_config_version(package: &str) -> Option<String> {
    Command::new("pkg-config")
        .args(["--modversion", package])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(crate) fn ffmpeg_version() -> Option<String> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| {
            output
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(2))
                .map(str::to_owned)
        })
}

pub(crate) fn linux_os_label() -> String {
    if let Ok(os_release) = fs::read_to_string("/etc/os-release")
        && let Some(pretty_name) = os_release.lines().find_map(|line| {
            line.strip_prefix("PRETTY_NAME=")
                .map(|value| value.trim_matches('"').to_owned())
        })
        && !pretty_name.is_empty()
    {
        return pretty_name;
    }

    Command::new("uname")
        .arg("-sr")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Linux".to_owned())
}

pub(crate) fn linux_update_install_status() -> &'static str {
    if linux_update_manager().is_ok() {
        "Self-update enabled"
    } else if deb_self_install_available() {
        "Deb self-install"
    } else {
        "Deb installer"
    }
}
