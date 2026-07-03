use super::*;

pub(crate) fn open_media_info_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };

        mpv.media_info(state.current_file.as_deref())
    };

    match result {
        Ok(media_info) => show_media_info_window(parent, &media_info, status_toast),
        Err(error) => {
            eprintln!("Failed to read media information: {error}");
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
    title.set_ellipsize(pango::EllipsizeMode::End);
    header.append(&title);

    if let Some(path) = media_info.path.as_deref() {
        let path_label = gtk::Label::new(Some(path));
        path_label.add_css_class("okp-info-path");
        path_label.set_xalign(0.0);
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

pub(crate) fn media_info_summary_widget(media_info: &MediaInfo) -> Option<gtk::Box> {
    let chips = media_info_summary_chips(media_info);
    if chips.is_empty() {
        return None;
    }

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    summary.add_css_class("okp-info-summary");
    summary.set_halign(gtk::Align::Start);
    for (label, value) in chips {
        summary.append(&media_info_summary_chip(label, &value));
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
    if let Some(codec) = media_info_value(media_info, "Video", "Codec") {
        chips.push(("Codec", codec.to_owned()));
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
    value.set_max_width_chars(18);
    chip.append(&value);

    chip
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
        content.append(&media_info_row(&row.label, &row.value));
    }

    content
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
