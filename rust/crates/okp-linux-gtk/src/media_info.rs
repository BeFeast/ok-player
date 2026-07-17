use super::*;

pub(crate) const MEDIA_INFO_IDENTITY_BADGE_SIZE: i32 = 38;
pub(crate) const MEDIA_INFO_IDENTITY_VIEWBOX_SIZE: f64 = 20.0;
pub(crate) const MEDIA_INFO_IDENTITY_RING_RADIUS: f64 = 7.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaInfoTab {
    Streams,
    Stats,
}

pub(crate) fn open_media_info_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    if present_existing_companion_window(state, CompanionWindowKind::MediaInfo) {
        return;
    }

    let media_info = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };

        mpv.observed_media_info().map(|mut media_info| {
            let display_title = current_media_title(&state);
            if !display_title.is_empty() {
                media_info.title = display_title;
            }
            media_info
        })
    };

    match media_info {
        Some(media_info) => show_media_info_window(parent, state, &media_info, status_toast),
        None => {
            status_toast.show("Media information unavailable");
        }
    }
}

pub(crate) fn show_media_info_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    media_info: &MediaInfo,
    status_toast: Rc<StatusToast>,
) {
    if present_existing_companion_window(state, CompanionWindowKind::MediaInfo) {
        return;
    }

    let window = build_companion_window(
        parent,
        state,
        CompanionWindowKind::MediaInfo,
        "Media Information",
    );
    window.add_css_class("okp-media-info-window");
    apply_settings_window_theme(&window, state.borrow().settings.appearance_theme());
    watch_system_settings_theme(&window, Rc::clone(state));

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("okp-media-info-card");
    card.set_halign(gtk::Align::Fill);
    card.set_valign(gtk::Align::Fill);
    card.set_hexpand(true);
    card.set_vexpand(true);
    card.set_overflow(gtk::Overflow::Hidden);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 13);
    header.add_css_class("okp-media-info-header");

    header.append(&media_info_identity());

    let heading = gtk::Box::new(gtk::Orientation::Vertical, 1);
    heading.set_hexpand(true);
    heading.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some("Media information"));
    title.add_css_class("okp-media-info-title");
    title.set_xalign(0.0);
    heading.append(&title);
    let subtitle = gtk::Label::new(Some(&media_info.title));
    subtitle.add_css_class("okp-media-info-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_hexpand(true);
    subtitle.set_width_chars(1);
    subtitle.set_ellipsize(pango::EllipsizeMode::Middle);
    heading.append(&subtitle);
    header.append(&heading);

    let close_button = gtk::Button::new();
    close_button.add_css_class("okp-media-info-close");
    close_button.set_has_frame(false);
    close_button.set_tooltip_text(Some("Close Media Information"));
    close_button.set_child(Some(&gtk::Image::from_icon_name("window-close-symbolic")));
    header.append(&close_button);
    card.append(&header);

    let tab_strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tab_strip.add_css_class("okp-media-info-tab-strip");
    tab_strip.set_halign(gtk::Align::Start);
    tab_strip.set_size_request(280, -1);
    let streams_button = media_info_tab_button("Streams");
    let stats_button = media_info_tab_button("Stats for nerds");
    tab_strip.append(&streams_button);
    tab_strip.append(&stats_button);
    let tab_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tab_row.add_css_class("okp-media-info-tabs");
    tab_row.append(&tab_strip);
    card.append(&tab_row);

    let streams_scroller = media_info_scroller(&media_info_streams_content(media_info));
    let stats_scroller = media_info_scroller(&media_info_stats_content(media_info));
    let stack = gtk::Stack::new();
    stack.add_css_class("okp-media-info-stack");
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_hhomogeneous(true);
    stack.set_vhomogeneous(false);
    stack.set_transition_type(gtk::StackTransitionType::None);
    stack.add_named(&streams_scroller, Some("streams"));
    stack.add_named(&stats_scroller, Some("stats"));
    card.append(&stack);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("okp-media-info-footer");
    let path_label = gtk::Label::new(media_info.path.as_deref());
    path_label.add_css_class("okp-media-info-path");
    path_label.set_xalign(0.0);
    path_label.set_hexpand(true);
    path_label.set_width_chars(1);
    path_label.set_ellipsize(pango::EllipsizeMode::Middle);
    footer.append(&path_label);

    let copy_button = media_info_action_button("Copy all", "edit-copy-symbolic");
    copy_button.add_css_class("okp-media-info-copy");
    let copy_text = Rc::new(media_info_copy_text(media_info));
    let copy_toast = Rc::clone(&status_toast);
    copy_button.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(copy_text.as_str());
            copy_toast.show("Media information copied");
        }
    });
    footer.append(&copy_button);

    let done_button = gtk::Button::with_label("Done");
    done_button.add_css_class("okp-media-info-done");
    done_button.set_has_frame(false);
    footer.append(&done_button);
    card.append(&footer);
    let close_window = window.clone();
    close_button.connect_clicked(move |_| close_window.close());
    let done_window = window.clone();
    done_button.connect_clicked(move |_| done_window.close());

    let current_tab = Rc::new(Cell::new(media_info_preview_tab()));
    set_media_info_tab(current_tab.get(), &stack, &streams_button, &stats_button);
    let tab_state = Rc::clone(&current_tab);
    let streams_stack = stack.clone();
    let streams_tab = streams_button.clone();
    let streams_peer = stats_button.clone();
    streams_button.connect_clicked(move |_| {
        tab_state.set(MediaInfoTab::Streams);
        set_media_info_tab(
            MediaInfoTab::Streams,
            &streams_stack,
            &streams_tab,
            &streams_peer,
        );
    });
    let tab_state = Rc::clone(&current_tab);
    let stats_stack = stack.clone();
    let stats_peer = streams_button.clone();
    let stats_tab = stats_button.clone();
    stats_button.connect_clicked(move |_| {
        tab_state.set(MediaInfoTab::Stats);
        set_media_info_tab(MediaInfoTab::Stats, &stats_stack, &stats_peer, &stats_tab);
    });

    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let close_from_key = window.clone();
    let key_stack = stack.clone();
    let key_streams = streams_button.clone();
    let key_stats = stats_button.clone();
    key_controller.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gdk::Key::Escape
            || (key
                .to_unicode()
                .is_some_and(|value| matches!(value, 'i' | 'I'))
                && !modifiers.intersects(
                    gdk::ModifierType::CONTROL_MASK
                        | gdk::ModifierType::ALT_MASK
                        | gdk::ModifierType::SUPER_MASK,
                ))
        {
            close_from_key.close();
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Left || key == gdk::Key::Right {
            let tab = if key == gdk::Key::Left {
                MediaInfoTab::Streams
            } else {
                MediaInfoTab::Stats
            };
            set_media_info_tab(tab, &key_stack, &key_streams, &key_stats);
            if tab == MediaInfoTab::Streams {
                key_streams.grab_focus();
            } else {
                key_stats.grab_focus();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    let window_overlay = gtk::Overlay::new();
    window_overlay.set_child(Some(&card));
    let drag_layer = captionless_window_drag_layer(&window);
    drag_layer.set_margin_end(42);
    window_overlay.add_overlay(&drag_layer);
    add_companion_window_resize_zones(&window_overlay, &window);
    window.set_child(Some(&window_overlay));
    connect_companion_play_pause_space(&window, Rc::clone(state));
    window.present();
    if current_tab.get() == MediaInfoTab::Stats {
        stats_button.grab_focus();
    } else {
        streams_button.grab_focus();
    }

    if env::var_os("OKP_MEDIA_INFO_SCROLL_BOTTOM").is_some() {
        let scroller = if current_tab.get() == MediaInfoTab::Stats {
            stats_scroller
        } else {
            streams_scroller
        };
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            let adjustment = scroller.vadjustment();
            adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(0.0));
        });
    }
}

fn media_info_identity() -> gtk::DrawingArea {
    let identity = gtk::DrawingArea::new();
    identity.add_css_class("okp-media-info-identity");
    identity.set_size_request(
        MEDIA_INFO_IDENTITY_BADGE_SIZE,
        MEDIA_INFO_IDENTITY_BADGE_SIZE,
    );
    identity.set_halign(gtk::Align::Center);
    identity.set_valign(gtk::Align::Center);
    identity.set_draw_func(draw_media_info_identity);
    identity
}

fn draw_media_info_identity(area: &gtk::DrawingArea, cr: &cairo::Context, width: i32, height: i32) {
    let color = area.color();
    let scale = f64::min(width as f64, height as f64) / MEDIA_INFO_IDENTITY_BADGE_SIZE as f64;
    let rendered_size = MEDIA_INFO_IDENTITY_VIEWBOX_SIZE * scale;
    let _ = cr.save();
    cr.translate(
        (width as f64 - rendered_size) / 2.0,
        (height as f64 - rendered_size) / 2.0,
    );
    cr.scale(scale, scale);
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    cr.set_line_width(1.25);
    cr.arc(
        10.0,
        10.0,
        MEDIA_INFO_IDENTITY_RING_RADIUS,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = cr.stroke();

    cr.arc(10.0, 6.15, 0.8, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.set_line_width(1.5);
    cr.move_to(10.0, 9.2);
    cr.line_to(10.0, 13.35);
    let _ = cr.stroke();
    let _ = cr.restore();
}

fn media_info_tab_button(label: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("okp-media-info-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

fn set_media_info_tab(
    tab: MediaInfoTab,
    stack: &gtk::Stack,
    streams_button: &gtk::Button,
    stats_button: &gtk::Button,
) {
    let streams = tab == MediaInfoTab::Streams;
    stack.set_visible_child_name(if streams { "streams" } else { "stats" });
    if streams {
        streams_button.add_css_class("is-active");
        stats_button.remove_css_class("is-active");
    } else {
        stats_button.add_css_class("is-active");
        streams_button.remove_css_class("is-active");
    }
}

fn media_info_preview_tab() -> MediaInfoTab {
    if env::var("OKP_MEDIA_INFO_TAB")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("stats"))
    {
        MediaInfoTab::Stats
    } else {
        MediaInfoTab::Streams
    }
}

fn media_info_scroller(content: &gtk::Box) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-media-info-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_propagate_natural_width(false);
    scroller.set_propagate_natural_height(true);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(content));
    scroller
}

fn media_info_streams_content(media_info: &MediaInfo) -> gtk::Box {
    let content = media_info_content_box();
    for section in media_info_stream_sections(media_info) {
        content.append(&media_info_section_widget(&section));
    }
    for kind in [TrackKind::Audio, TrackKind::Subtitle] {
        if media_info.tracks.iter().any(|track| track.kind == kind) {
            content.append(&media_info_tracks_section(&media_info.tracks, kind));
        }
    }
    if content.first_child().is_none() {
        content.append(&media_info_empty_card("No stream information available"));
    }
    content
}

fn media_info_stats_content(media_info: &MediaInfo) -> gtk::Box {
    let content = media_info_content_box();
    for section in media_info_stats_sections(media_info) {
        content.append(&media_info_section_widget(&section));
    }
    if content.first_child().is_none() {
        content.append(&media_info_empty_card("No playback diagnostics available"));
    }
    content
}

fn media_info_content_box() -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("okp-media-info-content");
    content
}

fn media_info_empty_card(message: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("okp-info-section");
    let label = gtk::Label::new(Some(message));
    label.add_css_class("okp-media-info-empty");
    label.set_xalign(0.0);
    card.append(&label);
    card
}

pub(crate) fn media_info_stream_sections(media_info: &MediaInfo) -> Vec<InfoSection> {
    media_info
        .sections
        .iter()
        .filter(|section| !section.title.eq_ignore_ascii_case("Playback"))
        .filter_map(|section| media_info_display_section(section, true))
        .collect()
}

pub(crate) fn media_info_stats_sections(media_info: &MediaInfo) -> Vec<InfoSection> {
    let Some(playback) = media_info
        .sections
        .iter()
        .find(|section| section.title.eq_ignore_ascii_case("Playback"))
    else {
        return Vec::new();
    };

    let mut decode = InfoSection {
        title: "Decode · Render".to_owned(),
        rows: Vec::new(),
    };
    let mut live = InfoSection {
        title: "Live · Performance".to_owned(),
        rows: Vec::new(),
    };
    let mut display = InfoSection {
        title: "Display · Output".to_owned(),
        rows: Vec::new(),
    };
    let Some(playback) = media_info_display_section(playback, false) else {
        return Vec::new();
    };
    for row in playback.rows {
        let target = match row.label.as_str() {
            "Hardware Decode" | "Video Output" | "Scaler" | "Engine Tone Mapping" => &mut decode,
            "A/V Sync" | "Dropped Frames" | "Cache" => &mut live,
            _ => &mut display,
        };
        target.rows.push(row);
    }
    [decode, live, display]
        .into_iter()
        .filter(|section| !section.rows.is_empty())
        .collect()
}

fn media_info_display_section(section: &InfoSection, exclude_path: bool) -> Option<InfoSection> {
    let mut rows: Vec<InfoRow> = section
        .rows
        .iter()
        .filter(|row| !exclude_path || !row.label.eq_ignore_ascii_case("Path"))
        .cloned()
        .collect();

    if section.title.eq_ignore_ascii_case("Video") {
        let hdr_row = rows
            .iter()
            .position(|row| row.label.eq_ignore_ascii_case("Dynamic Range"))
            .filter(|index| {
                okp_core::hdr::DynamicRangeState::from_media_info_value(Some(
                    rows[*index].value.as_str(),
                )) == okp_core::hdr::DynamicRangeState::Hdr
            });
        if let Some(index) = hdr_row
            && !rows
                .iter()
                .any(|row| row.label.eq_ignore_ascii_case("HDR Handling"))
        {
            rows.insert(
                index + 1,
                InfoRow {
                    label: "HDR Handling".to_owned(),
                    value: LINUX_HDR_HANDLING.diagnostic_label().to_owned(),
                },
            );
        }
    }

    if section.title.eq_ignore_ascii_case("Playback") {
        for row in &mut rows {
            if row.label.eq_ignore_ascii_case("Tone Mapping") {
                row.label = "Engine Tone Mapping".to_owned();
            }
        }
    }

    (!rows.is_empty()).then(|| InfoSection {
        title: section.title.clone(),
        rows,
    })
}

/// Representative Media Information used by the visual smoke hook
/// (`OKP_OPEN_MEDIA_INFO_ON_STARTUP`). It is fixture data for screenshots and
/// tests only; the live modal always renders `Mpv::observed_media_info`.
pub(crate) fn media_info_preview_sample() -> MediaInfo {
    let row = |label: &str, value: &str| InfoRow {
        label: label.to_owned(),
        value: value.to_owned(),
    };
    MediaInfo {
        title: "Blade Runner 2049 (2017) — 2160p HDR".to_owned(),
        path: Some(
            "/media/films/Blade Runner 2049 (2017)/Blade.Runner.2049.2160p.HDR.mkv".to_owned(),
        ),
        sections: vec![
            InfoSection {
                title: "File".to_owned(),
                rows: vec![
                    row("Container", "Matroska (MKV)"),
                    row("Duration", "2:43:31"),
                    row("Size", "24.7 GiB"),
                    row("Overall bitrate", "21.6 Mb/s"),
                ],
            },
            InfoSection {
                title: "Video".to_owned(),
                rows: vec![
                    row("Codec", "HEVC (H.265) Main 10"),
                    row("Resolution", "3840 × 2160"),
                    row("Frame rate", "23.976 fps"),
                    row("Bit depth", "10-bit"),
                    row("Dynamic Range", "HDR (PQ / ST 2084, BT.2020)"),
                    row("Mastering display", "1000 cd/m² peak · 0.005 cd/m² black"),
                ],
            },
            InfoSection {
                title: "Audio".to_owned(),
                rows: vec![
                    row("Codec", "TrueHD + Atmos"),
                    row("Channels", "7.1 (8 ch)"),
                    row("Sample rate", "48.0 kHz"),
                ],
            },
            InfoSection {
                title: "Playback".to_owned(),
                rows: vec![
                    row("Hardware Decode", "vaapi"),
                    row("Video Output", "gpu-next"),
                    row("Scaler", "ewa_lanczossharp"),
                    row("Tone Mapping", "bt.2390"),
                    row("A/V Sync", "+0.003 s"),
                    row("Dropped Frames", "2"),
                    row("Cache", "12.0 s"),
                    row("Display FPS", "59.940 fps"),
                    row("Sync Mode", "display-resample"),
                ],
            },
        ],
        tracks: vec![
            InfoTrack {
                id: 1,
                kind: TrackKind::Audio,
                selected: true,
                external: false,
                default: true,
                title: "English · TrueHD Atmos".to_owned(),
                detail: "7.1 · 48 kHz · default".to_owned(),
            },
            InfoTrack {
                id: 2,
                kind: TrackKind::Audio,
                selected: false,
                external: false,
                default: false,
                title: "English · AC-3 Commentary".to_owned(),
                detail: "2.0 · 48 kHz".to_owned(),
            },
            InfoTrack {
                id: 3,
                kind: TrackKind::Subtitle,
                selected: true,
                external: false,
                default: false,
                title: "English (SDH)".to_owned(),
                detail: "Primary · PGS · Image · embedded".to_owned(),
            },
            InfoTrack {
                id: 4,
                kind: TrackKind::Subtitle,
                selected: true,
                external: true,
                default: false,
                title: "Spanish".to_owned(),
                detail: "Secondary · SubRip (SRT) · external".to_owned(),
            },
        ],
    }
}

pub(crate) fn media_info_preview_from_env() -> MediaInfo {
    match env::var("OKP_MEDIA_INFO_PREVIEW")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "long" => media_info_long_preview_sample(),
        "missing" => media_info_missing_preview_sample(),
        _ => media_info_preview_sample(),
    }
}

fn media_info_long_preview_sample() -> MediaInfo {
    let mut sample = media_info_preview_sample();
    sample.title = "Blade.Runner.2049.2017.Final.Cut.2160p.UHD.BluRay.REMUX.HEVC.TrueHD.Atmos.7.1-Long.Release.Group.mkv".to_owned();
    sample.path = Some(
        "/media/archive/cinema/science-fiction/Blade Runner 2049 (2017)/Final Cut/Blade.Runner.2049.2017.Final.Cut.2160p.UHD.BluRay.REMUX.HEVC.TrueHD.Atmos.7.1-Long.Release.Group.mkv".to_owned(),
    );
    if let Some(video) = sample
        .sections
        .iter_mut()
        .find(|section| section.title == "Video")
    {
        video.rows.push(InfoRow {
            label: "Mastering metadata".to_owned(),
            value: "Display P3 D65 · BT.2020 container · MaxCLL 1000 nits · MaxFALL 400 nits"
                .to_owned(),
        });
        video.rows.push(InfoRow {
            label: "Encoder settings".to_owned(),
            value: "x265 3.5+153 · 10-bit · slow preset · grain retention enabled".to_owned(),
        });
    }
    sample
}

fn media_info_missing_preview_sample() -> MediaInfo {
    MediaInfo {
        title: "Untitled network stream".to_owned(),
        path: None,
        sections: vec![InfoSection {
            title: "File".to_owned(),
            rows: vec![InfoRow {
                label: "Container".to_owned(),
                value: "Unknown".to_owned(),
            }],
        }],
        tracks: Vec::new(),
    }
}

pub(crate) fn media_info_action_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_has_frame(false);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));

    button
}

pub(crate) fn media_info_section_widget(section: &InfoSection) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("okp-info-section");

    let section_title = section.title.to_uppercase();
    let title = gtk::Label::new(Some(&section_title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    let grid = gtk::Grid::new();
    grid.add_css_class("okp-media-info-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(28);
    grid.set_row_spacing(9);
    for (index, row) in section.rows.iter().enumerate() {
        let row_widget = media_info_row(&row.label, &row.value);
        if media_info_row_is_highlight(&row.label, &row.value) {
            row_widget.add_css_class("is-highlight");
        }
        grid.attach(&row_widget, (index % 2) as i32, (index / 2) as i32, 1, 1);
    }
    content.append(&grid);

    content
}

/// Rows that carry a headline diagnostic (currently active HDR) get an accent
/// value so the most consequential capabilities stand out from the dense list.
pub(crate) fn media_info_row_is_highlight(label: &str, value: &str) -> bool {
    label.eq_ignore_ascii_case("Dynamic Range")
        && okp_core::hdr::DynamicRangeState::from_media_info_value(Some(value))
            == okp_core::hdr::DynamicRangeState::Hdr
}

pub(crate) fn media_info_row(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("okp-info-row");
    row.set_hexpand(true);

    let label_widget = gtk::Label::new(Some(label));
    label_widget.add_css_class("okp-info-label");
    label_widget.set_xalign(0.0);
    row.append(&label_widget);

    let value_widget = gtk::Label::new(Some(value));
    value_widget.add_css_class("okp-info-value");
    value_widget.set_xalign(1.0);
    value_widget.set_hexpand(true);
    value_widget.set_wrap(true);
    value_widget.set_wrap_mode(pango::WrapMode::WordChar);
    row.append(&value_widget);

    row
}

pub(crate) fn media_info_tracks_section(tracks: &[InfoTrack], kind: TrackKind) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 9);
    content.add_css_class("okp-info-section");

    let count = tracks.iter().filter(|track| track.kind == kind).count();
    let title_text = format!(
        "{} · {} TRACK{}",
        media_info_track_kind_label(kind).to_uppercase(),
        count,
        if count == 1 { "" } else { "S" }
    );
    let title = gtk::Label::new(Some(&title_text));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for track in tracks.iter().filter(|track| track.kind == kind) {
        content.append(&media_info_track_row(track));
    }

    content
}

pub(crate) fn media_info_track_row(track: &InfoTrack) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-info-track-row");
    if track.selected {
        row.add_css_class("is-selected");
    }

    let id_text = if track.external {
        "ext".to_owned()
    } else {
        format!("#0:{}", track.id)
    };
    let id = gtk::Label::new(Some(&id_text));
    id.add_css_class("okp-info-track-kind");
    id.set_xalign(0.0);
    row.append(&id);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
    body.set_hexpand(true);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    title_row.set_hexpand(true);

    let title = gtk::Label::new(Some(&track.title));
    title.add_css_class("okp-info-track-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title_row.append(&title);

    if track.selected {
        let current = gtk::Label::new(Some(if track.external {
            "EXT"
        } else if track.default {
            "DEFAULT"
        } else {
            "ON"
        }));
        current.add_css_class("okp-info-track-current");
        title_row.append(&current);
    }
    body.append(&title_row);

    if !track.detail.is_empty() {
        let detail = gtk::Label::new(Some(&track.detail));
        detail.add_css_class("okp-info-track-detail");
        detail.set_xalign(0.0);
        detail.set_wrap(true);
        detail.set_wrap_mode(pango::WrapMode::WordChar);
        body.append(&detail);
    }

    row.append(&body);
    row
}

pub(crate) fn media_info_track_kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitle",
    }
}

pub(crate) fn media_info_copy_text(media_info: &MediaInfo) -> String {
    let mut lines = vec![
        "OK Player Media Information".to_owned(),
        format!("App: OK Player {APP_BUILD_VERSION} ({APP_BUILD_SHA})"),
        "Platform: Linux GTK4 / libmpv".to_owned(),
        String::new(),
        media_info.title.clone(),
    ];
    if let Some(path) = media_info.path.as_deref() {
        lines.push(format!("Path: {path}"));
    }

    for section in media_info
        .sections
        .iter()
        .filter_map(|section| media_info_display_section(section, false))
    {
        lines.push(String::new());
        lines.push(section.title);
        for row in section.rows {
            lines.push(format!("{}: {}", row.label, row.value));
        }
    }

    if !media_info.tracks.is_empty() {
        lines.push(String::new());
        lines.push("Tracks".to_owned());
        for track in &media_info.tracks {
            let detail = if track.detail.is_empty() {
                String::new()
            } else {
                format!(" - {}", track.detail)
            };
            lines.push(format!(
                "{} #{}: {}{}",
                media_info_track_kind_label(track.kind),
                track.id,
                track.title,
                detail
            ));
        }
    }

    lines.join("\n")
}
