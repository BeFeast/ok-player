use super::*;

pub(crate) const ABOUT_FRAME_TICKS: [(f64, f64, f64, f64); 5] = [
    (14.0, 43.5, 5.5, 9.0),
    (22.0, 41.0, 5.5, 14.0),
    (31.0, 38.0, 5.5, 20.0),
    (40.5, 34.0, 5.5, 28.0),
    (51.0, 30.0, 5.5, 36.0),
];
pub(crate) const ABOUT_FRAME_TICK_OPACITY: [f64; 5] = [0.10, 0.18, 0.30, 0.44, 0.62];

/// Expensive host/engine metadata that must not be collected on the GTK main
/// thread before the Settings window is mapped. The about pane captures these
/// in a background thread and fills them in once they resolve.
pub(crate) struct AboutDeferredFields {
    pub libmpv: String,
    pub ffmpeg: String,
    pub os: String,
    pub install: String,
}

impl AboutDeferredFields {
    pub fn capture() -> Self {
        Self {
            libmpv: pkg_config_version("mpv").unwrap_or_else(|| "system".to_owned()),
            ffmpeg: ffmpeg_version().unwrap_or_else(|| "system".to_owned()),
            os: linux_os_label(),
            install: linux_update_install_status().to_owned(),
        }
    }

    fn apply(self, snapshot: &mut AboutSnapshot, labels: &AboutDeferredLabels) {
        snapshot.libmpv.clone_from(&self.libmpv);
        snapshot.ffmpeg.clone_from(&self.ffmpeg);
        snapshot.os.clone_from(&self.os);
        snapshot.install.clone_from(&self.install);

        if let Some(label) = labels.libmpv.as_ref() {
            label.set_text(&self.libmpv);
        }
        if let Some(label) = labels.ffmpeg.as_ref() {
            label.set_text(&self.ffmpeg);
        }
        if let Some(label) = labels.os.as_ref() {
            label.set_text(&self.os);
        }
        if let Some(label) = labels.install.as_ref() {
            label.set_text(&self.install);
        }
    }
}

/// Labels that must be updated after the deferred metadata resolves.
#[derive(Default)]
pub(crate) struct AboutDeferredLabels {
    pub libmpv: Option<gtk::Label>,
    pub ffmpeg: Option<gtk::Label>,
    pub os: Option<gtk::Label>,
    pub install: Option<gtk::Label>,
}

pub(crate) fn settings_about_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let snapshot = Rc::new(RefCell::new(AboutSnapshot::capture_cheap(&state)));
    let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pane.add_css_class("okp-about-pane");

    pane.append(&about_identity_hero(&snapshot.borrow()));

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-about-identity-divider");
    pane.append(&divider);

    let mut deferred_labels = AboutDeferredLabels::default();
    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 12);
    sheet.add_css_class("okp-about-sheet");
    sheet.append(&about_app_card(&snapshot.borrow()));
    sheet.append(&about_engine_card(&snapshot.borrow(), &mut deferred_labels));
    sheet.append(&about_host_card(&snapshot.borrow(), &mut deferred_labels));
    pane.append(&sheet);

    pane.append(&about_footer(Rc::clone(&snapshot), status_toast));

    let (sender, receiver) = mpsc::channel::<AboutDeferredFields>();
    std::thread::spawn(move || {
        let _ = sender.send(AboutDeferredFields::capture());
    });

    glib::timeout_add_local(Duration::from_millis(10), move || {
        match receiver.try_recv() {
            Ok(fields) => {
                fields.apply(&mut snapshot.borrow_mut(), &deferred_labels);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

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
    let area = gtk::DrawingArea::new();
    area.add_css_class("okp-about-illustration-art");
    area.set_size_request(116, 90);
    area.set_draw_func(draw_about_illustration);
    area.upcast()
}

pub(crate) fn draw_about_illustration(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
) {
    let scale = f64::min(width as f64 / 124.0, height as f64 / 96.0);
    let tick = area.color();
    let _ = cr.save();
    cr.translate(
        (width as f64 - 124.0 * scale) / 2.0,
        (height as f64 - 96.0 * scale) / 2.0,
    );
    cr.scale(scale, scale);
    cr.set_line_cap(cairo::LineCap::Round);
    for ((x, y, tick_width, tick_height), opacity) in
        ABOUT_FRAME_TICKS.into_iter().zip(ABOUT_FRAME_TICK_OPACITY)
    {
        cr.set_source_rgba(
            tick.red().into(),
            tick.green().into(),
            tick.blue().into(),
            opacity,
        );
        cr.set_line_width(tick_width);
        let center_x = x + tick_width / 2.0;
        cr.move_to(center_x, y + tick_width / 2.0);
        cr.line_to(center_x, y + tick_height - tick_width / 2.0);
        let _ = cr.stroke();
    }

    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(64.0, 28.0);
    cr.line_to(108.0, 54.0);
    cr.line_to(64.0, 80.0);
    cr.close_path();
    cr.set_source_rgba(0.039, 0.396, 0.373, 0.09);
    cr.set_line_width(12.0);
    let _ = cr.stroke();

    cr.move_to(64.0, 22.0);
    cr.line_to(108.0, 48.0);
    cr.line_to(64.0, 74.0);
    cr.close_path();
    let gradient = cairo::LinearGradient::new(64.0, 22.0, 108.0, 74.0);
    gradient.add_color_stop_rgb(0.0, 0.110, 0.686, 0.635);
    gradient.add_color_stop_rgb(0.55, 0.067, 0.541, 0.502);
    gradient.add_color_stop_rgb(1.0, 0.039, 0.396, 0.373);
    let _ = cr.set_source(&gradient);
    cr.set_line_width(6.0);
    let _ = cr.fill_preserve();
    let _ = cr.stroke();
    let _ = cr.restore();
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
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 10);
    rows.append(&about_spec_row("Version", &snapshot.version, true, None));
    rows.append(&about_spec_row("Channel", &snapshot.channel, false, None));
    rows.append(&about_spec_row("Build", &snapshot.build, true, None));
    rows.append(&about_spec_row("License", &snapshot.license, true, None));
    about_card("APP", &rows)
}

pub(crate) fn about_engine_card(
    snapshot: &AboutSnapshot,
    deferred: &mut AboutDeferredLabels,
) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let hwdec_tag = if snapshot.hwdec == "off" {
        ("OFF", false)
    } else {
        ("ON", true)
    };
    let (libmpv_row, libmpv_label) =
        about_spec_row_with_label("libmpv", &snapshot.libmpv, true, None);
    deferred.libmpv = Some(libmpv_label);
    rows.append(&libmpv_row);
    let (ffmpeg_row, ffmpeg_label) =
        about_spec_row_with_label("FFmpeg", &snapshot.ffmpeg, true, Some(("SYSTEM", false)));
    deferred.ffmpeg = Some(ffmpeg_label);
    rows.append(&ffmpeg_row);
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

pub(crate) fn about_host_card(
    snapshot: &AboutSnapshot,
    deferred: &mut AboutDeferredLabels,
) -> gtk::Box {
    let grid = gtk::Grid::new();
    grid.add_css_class("okp-about-host-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(26);
    grid.set_row_spacing(10);

    let (os_row, os_label) = about_spec_row_with_label("Linux", &snapshot.os, true, None);
    deferred.os = Some(os_label);
    grid.attach(&os_row, 0, 0, 1, 1);

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
    let (install_row, install_label) =
        about_spec_row_with_label("Install", &snapshot.install, false, None);
    deferred.install = Some(install_label);
    grid.attach(&install_row, 1, 1, 1, 1);
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
    about_spec_row_with_label(label, value, mono, tag).0
}

pub(crate) fn about_spec_row_with_label(
    label: &str,
    value: &str,
    mono: bool,
    tag: Option<(&str, bool)>,
) -> (gtk::Box, gtk::Label) {
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
    (row, val)
}

pub(crate) fn about_footer(
    snapshot: Rc<RefCell<AboutSnapshot>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("okp-about-footer");

    let copy = about_action_button("Copy diagnostics", "edit-copy-symbolic");
    let copy_snapshot = snapshot;
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display
                .clipboard()
                .set_text(&about_diagnostics_text(&copy_snapshot.borrow()));
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
        "OK Player {} ({})
Build {} - current
License {}

Engine
  libmpv           {}
  FFmpeg           {}
  Render API       {}
  Graphics         {}
  Hardware decode  {}

Host
  Linux            {}
  GTK              {}
  CPU              {}
  Install          {}",
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
        snapshot.install
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
    match linux_install_lane() {
        CandidateInstallLane::AppImage => "Self-update enabled",
        CandidateInstallLane::Debian if deb_self_install_available() => "Deb self-install",
        CandidateInstallLane::Debian => "Deb installer",
    }
}
