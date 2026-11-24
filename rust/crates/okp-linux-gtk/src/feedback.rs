use super::*;
use okp_core::feedback::{Diagnostics, Kind, Metadata, Outbox, Playback, Report, SendFailure};

pub(crate) fn open(parent: &gtk::ApplicationWindow, state: &Rc<RefCell<PlayerState>>) {
    if let Some(window) = state.borrow().feedback_window.upgrade() {
        window.present();
        return;
    }

    let source = state.borrow();
    let path = source
        .settings
        .path()
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty());
    let Some(path) = path else {
        show_error(parent, "Could not locate the feedback settings directory.");
        return;
    };
    let outbox = Arc::new(Outbox::new(path));
    let form = match outbox.form() {
        Ok(form) => form,
        Err(message) => {
            show_error(parent, message);
            return;
        }
    };
    let playback = match source.media_load_state {
        network_media::MediaLoadState::Idle => Playback::Idle,
        network_media::MediaLoadState::Loading => Playback::Loading,
        network_media::MediaLoadState::Playing => Playback::Playing,
        network_media::MediaLoadState::Failed => Playback::Error,
    };
    let own_video_plane = source.native_video_plane.is_some();
    let meta = Metadata {
        version: about_display_version(APP_BUILD_VERSION),
        build: APP_BUILD_VERSION.into(),
        source: APP_BUILD_SHA.into(),
        os: "linux".into(),
        os_version: linux_os_label(),
        arch: std::env::consts::ARCH.into(),
        channel: about_display_channel(APP_BUILD_VERSION),
    };
    let diagnostics = Diagnostics {
        playback,
        fullscreen: parent.is_fullscreen(),
        width: parent.width().max(0) as u32,
        height: parent.height().max(0) as u32,
    };
    let appearance = source.settings.appearance_theme();
    drop(source);

    let window = gtk::Window::builder()
        .title("Send Feedback")
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .default_width(520)
        .build();
    state.borrow().feedback_window.set(Some(&window));
    apply_settings_window_theme(&window, appearance);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.set_margin_top(16);
    body.set_margin_bottom(16);
    body.set_margin_start(16);
    body.set_margin_end(16);
    window.set_child(Some(&body));

    let controls = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.append(&controls);
    let text = gtk::TextView::new();
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_height(100)
        .has_frame(true)
        .child(&text)
        .build();
    controls.append(&scroll);
    let description = label("Description");
    description.set_mnemonic_widget(Some(&text));
    controls.prepend(&description);

    let kind = gtk::DropDown::from_strings(&["Bug report", "Feature request"]);
    controls.append(&kind);
    let name = gtk::Entry::builder()
        .placeholder_text("Name or GitHub (optional)")
        .max_length(128)
        .build();
    controls.append(&name);
    let include_diagnostics = gtk::CheckButton::with_label("Include private diagnostics");
    include_diagnostics.set_active(true);
    controls.append(&include_diagnostics);
    let attachment = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let capture = gtk::Button::with_label("Capture Player Window");
    let remove = gtk::Button::with_label("Remove Screenshot");
    let thumbnail = gtk::Picture::new();
    thumbnail.set_size_request(112, 64);
    thumbnail.set_content_fit(gtk::ContentFit::Contain);
    thumbnail.set_visible(false);
    remove.set_visible(false);
    attachment.append(&capture);
    attachment.append(&thumbnail);
    attachment.append(&remove);
    controls.append(&attachment);
    controls.append(&label(form.privacy));

    let status = label("");
    status.set_selectable(true);
    body.append(&status);
    let link = gtk::LinkButton::with_label("", "Open Issue");
    link.set_halign(gtk::Align::Start);
    link.set_visible(false);
    body.append(&link);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let discard = gtk::Button::with_label("Discard Saved Report");
    let close = gtk::Button::with_label("Close");
    let send = gtk::Button::with_label("Send");
    send.add_css_class("suggested-action");
    buttons.append(&discard);
    buttons.append(&close);
    buttons.append(&send);
    body.append(&buttons);

    let token = Rc::new(Cell::new(form.draft.as_ref().map(|draft| draft.token)));
    let screenshot = Rc::new(RefCell::new(None::<String>));
    let mut report_meta = meta;
    let mut report_diagnostics = Some(diagnostics.clone());
    name.set_text(&form.reporter_name);
    discard.set_visible(token.get().is_some());
    if let Some(draft) = form.draft {
        text.buffer().set_text(&draft.report.text);
        name.set_text(&draft.report.reporter_name);
        kind.set_selected(u32::from(matches!(draft.report.kind, Kind::Feature)));
        include_diagnostics.set_active(draft.report.diagnostics.is_some());
        status.set_text(&draft.message);
        send.set_label("Retry");
        if let Ok(Some(png)) = draft.report.screenshot_bytes()
            && let Ok(image) = gdk::Texture::from_bytes(&glib::Bytes::from_owned(png))
        {
            screenshot.replace(draft.report.screenshot);
            thumbnail.set_paintable(Some(&image));
            thumbnail.set_visible(true);
            remove.set_visible(true);
        }
        report_meta = draft.report.meta;
        report_diagnostics = draft.report.diagnostics;
    }

    let owner = parent.downgrade();
    capture.connect_clicked({
        let (status, screenshot, thumbnail, remove) = (
            status.clone(),
            screenshot.clone(),
            thumbnail.clone(),
            remove.clone(),
        );
        move |_| {
            let Some(player) = owner.upgrade() else {
                return;
            };
            match capture_window(&player) {
                Ok((image, data)) => {
                    screenshot.replace(Some(data));
                    thumbnail.set_paintable(Some(&image));
                    thumbnail.set_visible(true);
                    remove.set_visible(true);
                    status.set_text(if own_video_plane {
                        "Screenshot attached. The separate video layer is omitted."
                    } else {
                        "Player window screenshot attached."
                    });
                }
                Err(message) => status.set_text(message),
            }
        }
    });
    remove.connect_clicked({
        let (screenshot, thumbnail) = (screenshot.clone(), thumbnail.clone());
        move |button| {
            screenshot.replace(None);
            thumbnail.set_paintable(None::<&gdk::Paintable>);
            thumbnail.set_visible(false);
            button.set_visible(false);
        }
    });
    let busy = Rc::new(Cell::new(false));
    window.connect_close_request({
        let busy = busy.clone();
        move |_| {
            if busy.get() {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    close.connect_clicked({
        let window = window.downgrade();
        move |_| {
            if let Some(window) = window.upgrade() {
                window.close()
            }
        }
    });
    discard.connect_clicked({
        let window = window.downgrade();
        let (outbox, token, status) = (outbox.clone(), token.clone(), status.clone());
        move |_| match token.get().map(|id| outbox.discard(id)) {
            Some(Ok(())) => {
                if let Some(window) = window.upgrade() {
                    window.close()
                }
            }
            Some(Err(message)) => status.set_text(message),
            None => {}
        }
    });
    send.connect_clicked({
        let (send, discard, close) = (send.downgrade(), discard.downgrade(), close.downgrade());
        let text = text.buffer();
        move |_| {
            let (Some(send), Some(discard), Some(close)) =
                (send.upgrade(), discard.upgrade(), close.upgrade())
            else {
                return;
            };
            if busy.replace(true) {
                return;
            }
            controls.set_sensitive(false);
            send.set_sensitive(false);
            close.set_sensitive(false);
            discard.set_sensitive(false);
            status.set_text("Sending…");
            let buffer = &text;
            let report = Report {
                text: buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), true)
                    .to_string(),
                kind: if kind.selected() == 0 {
                    Kind::Bug
                } else {
                    Kind::Feature
                },
                reporter_name: name.text().to_string(),
                meta: report_meta.clone(),
                diagnostics: include_diagnostics.is_active().then(|| {
                    report_diagnostics
                        .clone()
                        .unwrap_or_else(|| diagnostics.clone())
                }),
                screenshot: screenshot.borrow().clone(),
            };
            let (outbox, previous) = (outbox.clone(), token.get());
            let (busy, status, link, token, controls) = (
                busy.clone(),
                status.clone(),
                link.clone(),
                token.clone(),
                controls.clone(),
            );
            glib::spawn_future_local(async move {
                let result = gtk::gio::spawn_blocking(move || outbox.submit(previous, report))
                    .await
                    .unwrap_or_else(|_| {
                        Err(SendFailure::from(
                            "Sending was interrupted. Reopen Feedback to check the saved report.",
                        ))
                    });
                busy.set(false);
                close.set_sensitive(true);
                match result {
                    Ok(receipt) => {
                        status.set_text("Feedback sent.");
                        link.set_uri(&receipt.issue_url);
                        link.set_visible(true);
                        send.set_visible(false);
                        discard.set_visible(false);
                    }
                    Err(failure) => {
                        status.set_text(failure.message);
                        if let Some(id) = failure.token {
                            token.set(Some(id));
                            discard.set_visible(true);
                            send.set_label("Retry");
                        }
                        controls.set_sensitive(true);
                        send.set_sensitive(true);
                        discard.set_sensitive(true);
                    }
                }
            });
        }
    });
    window.present();
    text.grab_focus();
}

fn label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

fn show_error(parent: &gtk::ApplicationWindow, message: &str) {
    gtk::AlertDialog::builder()
        .message(message)
        .build()
        .show(Some(parent));
}

fn capture_window(parent: &gtk::ApplicationWindow) -> Result<(gdk::Texture, String), &'static str> {
    const UNAVAILABLE: &str = "The player window is not available to capture.";
    let width = parent.width();
    let height = parent.height();
    if width <= 0 || height <= 0 || width > 4096 || height > 4096 {
        return Err("Resize the player window before capturing a screenshot.");
    }
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(parent)).snapshot(
        &snapshot,
        f64::from(width),
        f64::from(height),
    );
    let node = snapshot.to_node().ok_or(UNAVAILABLE)?;
    let renderer = parent.renderer().ok_or(UNAVAILABLE)?;
    let texture = renderer.render_texture(&node, None);
    let bytes = texture.save_to_png_bytes();
    let encoded = Report::encode_screenshot(bytes.as_ref())?;
    Ok((texture, encoded))
}
