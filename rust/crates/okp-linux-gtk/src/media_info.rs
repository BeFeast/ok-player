use super::*;

pub(crate) fn open_media_info_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let media_info = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };

        mpv.observed_media_info()
    };

    match media_info {
        Some(media_info) => show_media_info_window(parent, &media_info, status_toast),
        None => {
            status_toast.show("Media information unavailable");
        }
    }
}

pub(crate) fn show_media_info_window(
    parent: &gtk::ApplicationWindow,
    media_info: &MediaInfo,
    status_toast: Rc<StatusToast>,
) {
    let window = captionless_transient_window(parent, "Media Information", 680, 820, true);
    window.add_css_class("okp-info-window");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("okp-info-root");

    let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    page.add_css_class("okp-info-page");
    page.set_margin_top(54);
    page.set_margin_end(36);
    page.set_margin_bottom(24);
    page.set_margin_start(36);

    let header = gtk::Box::new(gtk::Orientation::Vertical, 5);
    header.add_css_class("okp-info-hero");
    let eyebrow = gtk::Label::new(Some("MEDIA INFO"));
    eyebrow.add_css_class("okp-info-eyebrow");
    eyebrow.set_xalign(0.0);
    header.append(&eyebrow);

    let title = gtk::Label::new(Some(&media_info.title));
    title.add_css_class("okp-info-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    // Bound the natural width so a long title cannot stretch the transient
    // window past its reference size; it fills the content column and ellipsizes.
    title.set_width_chars(1);
    title.set_ellipsize(pango::EllipsizeMode::End);
    header.append(&title);

    if let Some(path) = media_info.path.as_deref() {
        let path_label = gtk::Label::new(Some(path));
        path_label.add_css_class("okp-info-path");
        path_label.set_xalign(0.0);
        path_label.set_hexpand(true);
        path_label.set_width_chars(1);
        path_label.set_ellipsize(pango::EllipsizeMode::Middle);
        header.append(&path_label);
    }
    page.append(&header);

    if let Some(summary) = media_info_summary_widget(media_info) {
        page.append(&summary);
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("okp-info-content");
    for section in &media_info.sections {
        content.append(&media_info_section_widget(section));
    }
    if !media_info.tracks.is_empty() {
        content.append(&media_info_tracks_section(&media_info.tracks));
    }

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&content));
    page.append(&scroller);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    footer.add_css_class("okp-info-footer");

    let copy_button = media_info_action_button("Copy info", "edit-copy-symbolic");
    copy_button.add_css_class("okp-info-footer-button");
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
    done_button.add_css_class("okp-info-footer-button");
    done_button.set_has_frame(false);
    done_button.set_halign(gtk::Align::End);
    done_button.set_hexpand(true);
    let close_window = window.clone();
    done_button.connect_clicked(move |_| close_window.close());
    footer.append(&done_button);
    page.append(&footer);
    root.append(&page);

    let content_overlay = gtk::Overlay::new();
    content_overlay.set_child(Some(&root));
    content_overlay.add_overlay(&captionless_window_drag_layer(&window));
    content_overlay.add_overlay(&settings_window_controls(&window));
    window.set_child(Some(&content_overlay));
    window.present();
}

/// Representative Media Information used by the visual smoke hook
/// (`OKP_OPEN_MEDIA_INFO_ON_STARTUP`). It is fixture data for screenshots and
/// tests only; the live window always renders `Mpv::observed_media_info`.
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
                    row("HDR", "HDR10 · BT.2020 · SMPTE ST 2084 (PQ)"),
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
                detail: "PGS · embedded".to_owned(),
            },
            InfoTrack {
                id: 4,
                kind: TrackKind::Subtitle,
                selected: false,
                external: true,
                default: false,
                title: "English".to_owned(),
                detail: "SubRip (SRT) · external".to_owned(),
            },
        ],
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

pub(crate) fn media_info_summary_widget(media_info: &MediaInfo) -> Option<gtk::FlowBox> {
    let chips = media_info_summary_chips(media_info);
    if chips.is_empty() {
        return None;
    }

    // A flow layout keeps the at-a-glance strip on one row when it fits and
    // wraps to a second row on narrow widths instead of clipping chip values.
    let summary = gtk::FlowBox::new();
    summary.add_css_class("okp-info-summary");
    summary.set_selection_mode(gtk::SelectionMode::None);
    summary.set_activate_on_single_click(false);
    summary.set_max_children_per_line(chips.len() as u32);
    summary.set_min_children_per_line(1);
    summary.set_column_spacing(8);
    summary.set_row_spacing(8);
    summary.set_homogeneous(false);
    summary.set_halign(gtk::Align::Start);
    for (label, value) in chips {
        summary.insert(&media_info_summary_chip(label, &value), -1);
    }
    Some(summary)
}

pub(crate) fn media_info_summary_chips(media_info: &MediaInfo) -> Vec<(&'static str, String)> {
    let mut chips = Vec::new();

    if let Some(container) = media_info_value(media_info, "File", "Container") {
        chips.push(("Container", container.to_owned()));
    }
    if let Some(duration) = media_info_value(media_info, "File", "Duration") {
        chips.push(("Duration", duration.to_owned()));
    }
    if let Some(resolution) = media_info_value(media_info, "Video", "Resolution") {
        chips.push(("Video", resolution.to_owned()));
    }
    if let Some(hdr) = media_info_value(media_info, "Video", "HDR") {
        chips.push(("HDR", media_info_hdr_summary(hdr)));
    }

    let audio_count = media_info
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .count();
    if audio_count > 0 {
        chips.push(("Audio", audio_count.to_string()));
    }

    let subtitle_count = media_info
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .count();
    if subtitle_count > 0 {
        chips.push(("Subtitles", subtitle_count.to_string()));
    }

    chips
}

pub(crate) fn media_info_value<'a>(
    media_info: &'a MediaInfo,
    section_title: &str,
    row_label: &str,
) -> Option<&'a str> {
    media_info
        .sections
        .iter()
        .find(|section| section.title == section_title)?
        .rows
        .iter()
        .find(|row| row.label == row_label)
        .map(|row| row.value.as_str())
}

pub(crate) fn media_info_summary_chip(label: &str, value: &str) -> gtk::Box {
    let chip = gtk::Box::new(gtk::Orientation::Vertical, 2);
    chip.add_css_class("okp-info-chip");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-chip-label");
    label.set_xalign(0.0);
    chip.append(&label);

    let value = gtk::Label::new(Some(value));
    value.add_css_class("okp-info-chip-value");
    value.set_xalign(0.0);
    value.set_ellipsize(pango::EllipsizeMode::End);
    value.set_max_width_chars(22);
    chip.append(&value);

    chip
}

/// Condense a verbose HDR descriptor into a chip-sized token, keeping the
/// leading format name (e.g. "HDR10 · BT.2020 · PQ" -> "HDR10").
pub(crate) fn media_info_hdr_summary(hdr: &str) -> String {
    hdr.split('·')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(hdr)
        .to_owned()
}

pub(crate) fn media_info_section_widget(section: &InfoSection) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("okp-info-section");

    let section_title = section.title.to_uppercase();
    let title = gtk::Label::new(Some(&section_title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for row in &section.rows {
        let row_widget = media_info_row(&row.label, &row.value);
        if media_info_row_is_highlight(&row.label, &row.value) {
            row_widget.add_css_class("is-highlight");
        }
        content.append(&row_widget);
    }

    content
}

/// Rows that carry a headline diagnostic (currently active HDR) get an accent
/// value so the most consequential capabilities stand out from the dense list.
pub(crate) fn media_info_row_is_highlight(label: &str, value: &str) -> bool {
    label.eq_ignore_ascii_case("HDR")
        && !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "no" | "none" | "off" | "sdr"
        )
}

pub(crate) fn media_info_row(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("okp-info-row");
    row.set_hexpand(true);

    let label_widget = gtk::Label::new(Some(label));
    label_widget.add_css_class("okp-info-label");
    label_widget.set_xalign(0.0);
    label_widget.set_width_chars(15);
    row.append(&label_widget);

    let value_widget = gtk::Label::new(Some(value));
    value_widget.add_css_class("okp-info-value");
    value_widget.set_xalign(0.0);
    value_widget.set_hexpand(true);
    value_widget.set_wrap(true);
    value_widget.set_wrap_mode(pango::WrapMode::WordChar);
    row.append(&value_widget);

    row
}

pub(crate) fn media_info_tracks_section(tracks: &[InfoTrack]) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("okp-info-section");

    let title = gtk::Label::new(Some("Tracks"));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for track in tracks {
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

    let kind_text = media_info_track_kind_label(track.kind).to_uppercase();
    let kind = gtk::Label::new(Some(&kind_text));
    kind.add_css_class("okp-info-track-kind");
    kind.set_width_chars(8);
    kind.set_xalign(0.0);
    row.append(&kind);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
    body.set_hexpand(true);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    title_row.set_hexpand(true);

    let title = gtk::Label::new(Some(&format!("#{} {}", track.id, track.title)));
    title.add_css_class("okp-info-track-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title_row.append(&title);

    if track.selected {
        let current = gtk::Label::new(Some("CURRENT"));
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

    for section in &media_info.sections {
        lines.push(String::new());
        lines.push(section.title.clone());
        for row in &section.rows {
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
