use super::*;

pub(crate) fn open_settings_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let initial_page = env::var("OKP_OPEN_SETTINGS_PAGE_ON_STARTUP")
        .ok()
        .and_then(|page| normalized_settings_page(&page))
        .unwrap_or("about");
    let window = captionless_transient_window(
        parent,
        "Settings",
        SETTINGS_REFERENCE_WIDTH,
        SETTINGS_REFERENCE_HEIGHT,
        false,
    );
    window.add_css_class("okp-settings-window");

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("okp-settings-root");

    let stack = gtk::Stack::new();
    stack.add_css_class("okp-settings-stack");
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let about_page = settings_scroller(&settings_about_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&about_page, Some("about"));

    let appearance_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    appearance_page.add_css_class("okp-settings-page");
    appearance_page.append(&settings_appearance_section());
    stack.add_named(&settings_scroller(&appearance_page), Some("appearance"));

    let advanced_page = settings_advanced_page(Rc::clone(&state), Rc::clone(&status_toast));
    stack.add_named(&settings_scroller(&advanced_page), Some("advanced"));

    let playback_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    playback_page.add_css_class("okp-settings-page");
    let playback = settings_section("Playback");
    playback.append(&settings_resume_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_auto_advance_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_repeat_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_shuffle_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    playback.append(&settings_volume_row(Rc::clone(&state)));
    playback_page.append(&playback);
    stack.add_named(&settings_scroller(&playback_page), Some("playback"));

    let subtitles_page =
        settings_subtitles_page(parent, Rc::clone(&state), Rc::clone(&status_toast));
    stack.add_named(&settings_scroller(&subtitles_page), Some("subtitles"));

    let video_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    video_page.add_css_class("okp-settings-page");
    let video = settings_section("Video");
    video.append(&settings_hwdec_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Brightness,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Contrast,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Saturation,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video.append(&settings_video_adjustment_row(
        VideoAdjustment::Gamma,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    video_page.append(&video);
    video_page.append(&settings_screenshot_section(
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&settings_scroller(&video_page), Some("video"));

    let audio_page = settings_audio_page(Rc::clone(&state), Rc::clone(&status_toast));
    stack.add_named(&settings_scroller(&audio_page), Some("audio"));

    let shortcuts_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    shortcuts_page.add_css_class("okp-settings-page");
    shortcuts_page.append(&settings_shortcuts_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&settings_scroller(&shortcuts_page), Some("shortcuts"));

    let integration_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    integration_page.add_css_class("okp-settings-page");
    integration_page.append(&settings_integration_section(Rc::clone(&status_toast)));

    let privacy = settings_section("Privacy");
    privacy.append(&settings_private_session_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    privacy.append(&settings_clear_history_row(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    integration_page.append(&privacy);

    let storage = settings_section("Storage");
    let settings_path = state
        .borrow()
        .settings
        .path()
        .to_string_lossy()
        .into_owned();
    storage.append(&settings_value_row("Settings file", &settings_path));
    integration_page.append(&storage);
    stack.add_named(&settings_scroller(&integration_page), Some("integration"));

    stack.set_visible_child_name(initial_page);
    root.append(&settings_nav_rail_frame(settings_nav_rail(
        &stack,
        initial_page,
    )));

    stack.set_size_request(SETTINGS_CONTENT_WIDTH, SETTINGS_REFERENCE_HEIGHT);
    root.append(&stack);

    let window_overlay = gtk::Overlay::new();
    window_overlay.set_child(Some(&root));
    window_overlay.add_overlay(&captionless_window_drag_layer(&window));
    window_overlay.add_overlay(&settings_window_controls(&window));
    window.set_child(Some(&window_overlay));
    window.present();
}

pub(crate) fn normalized_settings_page(page: &str) -> Option<&'static str> {
    match page.trim().to_ascii_lowercase().as_str() {
        "appearance" => Some("appearance"),
        "playback" => Some("playback"),
        "subtitles" => Some("subtitles"),
        "video" => Some("video"),
        "audio" => Some("audio"),
        "shortcuts" => Some("shortcuts"),
        "integration" => Some("integration"),
        "advanced" => Some("advanced"),
        "about" => Some("about"),
        _ => None,
    }
}

pub(crate) fn settings_scroller<T: IsA<gtk::Widget>>(child: &T) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-settings-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_min_content_width(SETTINGS_CONTENT_WIDTH);
    scroller.set_max_content_width(SETTINGS_CONTENT_WIDTH);
    scroller.set_propagate_natural_width(false);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(child));
    scroller
}

pub(crate) fn settings_nav_rail_frame(rail: gtk::Box) -> gtk::ScrolledWindow {
    let frame = gtk::ScrolledWindow::new();
    frame.add_css_class("okp-settings-rail-frame");
    frame.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    frame.set_min_content_width(SETTINGS_RAIL_WIDTH);
    frame.set_max_content_width(SETTINGS_RAIL_WIDTH);
    frame.set_propagate_natural_width(false);
    frame.set_size_request(SETTINGS_RAIL_WIDTH, SETTINGS_REFERENCE_HEIGHT);
    frame.set_child(Some(&rail));
    frame
}

pub(crate) fn settings_nav_rail(stack: &gtk::Stack, selected_page: &str) -> gtk::Box {
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 2);
    rail.add_css_class("okp-settings-rail");
    rail.set_size_request(SETTINGS_RAIL_WIDTH, SETTINGS_REFERENCE_HEIGHT);

    let title = gtk::Label::new(Some("Settings"));
    title.add_css_class("okp-settings-rail-title");
    title.set_xalign(0.0);
    rail.append(&title);

    let search = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    search.add_css_class("okp-settings-search");
    search.set_size_request(171, 30);
    let search_icon = gtk::Image::from_icon_name("system-search-symbolic");
    search_icon.set_pixel_size(14);
    search.append(&search_icon);
    let search_label = gtk::Label::new(Some("Search"));
    search_label.add_css_class("okp-settings-search-label");
    search_label.set_xalign(0.0);
    search.append(&search_label);
    rail.append(&search);

    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));
    let nav_items = [
        (
            "Appearance",
            SettingsNavIcon::Appearance,
            Some("appearance"),
        ),
        ("Playback", SettingsNavIcon::Playback, Some("playback")),
        ("Subtitles", SettingsNavIcon::Subtitles, Some("subtitles")),
        ("Video", SettingsNavIcon::Video, Some("video")),
        ("Audio", SettingsNavIcon::Audio, Some("audio")),
        ("Shortcuts", SettingsNavIcon::Shortcuts, Some("shortcuts")),
        (
            "Integration",
            SettingsNavIcon::Integration,
            Some("integration"),
        ),
        ("Advanced", SettingsNavIcon::Advanced, Some("advanced")),
    ];

    for (label, icon, page) in nav_items {
        let row = settings_nav_row(label, icon, page == Some(selected_page));
        if let Some(page) = page {
            connect_settings_nav_row(&row, page, stack, &buttons);
            buttons.borrow_mut().push(row.clone());
        }
        rail.append(&row);
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-settings-rail-divider");
    rail.append(&divider);

    let about = settings_nav_row("About", SettingsNavIcon::About, selected_page == "about");
    connect_settings_nav_row(&about, "about", stack, &buttons);
    buttons.borrow_mut().push(about.clone());
    rail.append(&about);

    rail
}

pub(crate) fn settings_window_controls(window: &gtk::Window) -> gtk::Box {
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-settings-window-controls");
    controls.set_halign(gtk::Align::End);
    controls.set_valign(gtk::Align::Start);

    let minimize = settings_window_control(WindowControlKind::Minimize, "Minimize");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    controls.append(&minimize);

    let maximize = settings_window_control(WindowControlKind::Maximize, "Maximize");
    sync_settings_maximize_icon(&maximize, window);
    let maximize_window = window.clone();
    let maximize_button = maximize.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
        sync_settings_maximize_icon(&maximize_button, &maximize_window);
    });
    let notify_button = maximize.clone();
    window.connect_maximized_notify(move |window| {
        sync_settings_maximize_icon(&notify_button, window);
    });
    controls.append(&maximize);

    let close = settings_window_control(WindowControlKind::Close, "Close");
    close.add_css_class("okp-settings-window-close");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    controls.append(&close);

    controls
}

pub(crate) fn captionless_window_drag_layer(window: &gtk::Window) -> gtk::Box {
    let drag_layer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_layer.add_css_class("okp-captionless-window-drag-layer");
    drag_layer.set_halign(gtk::Align::Fill);
    drag_layer.set_valign(gtk::Align::Start);
    drag_layer.set_can_target(true);
    drag_layer.set_height_request(CAPTIONLESS_DRAG_HEIGHT);
    connect_captionless_window_drag(&drag_layer, window);
    drag_layer
}

pub(crate) fn connect_captionless_window_drag(
    widget: &impl IsA<gtk::Widget>,
    window: &gtk::Window,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let drag_window = window.clone();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if n_press == 2 {
            if drag_window.is_maximized() {
                drag_window.unmaximize();
            } else {
                drag_window.maximize();
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(surface) = drag_window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };

        toplevel.begin_move(
            &device,
            gesture.current_button() as i32,
            x,
            y,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

pub(crate) fn settings_window_control(kind: WindowControlKind, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-window-control");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));

    let glyph = window_control_icon(kind, "okp-settings-window-control-glyph");
    button.set_child(Some(&glyph));
    button
}

pub(crate) fn sync_settings_maximize_icon(button: &gtk::Button, window: &gtk::Window) {
    if window.is_maximized() {
        set_settings_window_control_kind(button, WindowControlKind::Restore);
        button.set_tooltip_text(Some("Restore"));
    } else {
        set_settings_window_control_kind(button, WindowControlKind::Maximize);
        button.set_tooltip_text(Some("Maximize"));
    }
}

pub(crate) fn window_control_icon(kind: WindowControlKind, css_class: &str) -> gtk::DrawingArea {
    let icon = gtk::DrawingArea::new();
    icon.add_css_class(css_class);
    icon.set_size_request(10, 10);
    icon.set_draw_func(move |area, cr, width, height| {
        draw_window_control_icon(area, cr, width, height, kind);
    });
    icon
}

pub(crate) fn set_settings_window_control_kind(button: &gtk::Button, kind: WindowControlKind) {
    if let Some(icon) = button.child().and_downcast::<gtk::DrawingArea>() {
        icon.set_draw_func(move |area, cr, width, height| {
            draw_window_control_icon(area, cr, width, height, kind);
        });
        icon.queue_draw();
    }
}

pub(crate) fn draw_window_control_icon(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    kind: WindowControlKind,
) {
    let color = area.style_context().color();
    let _ = cr.save();
    cr.translate(
        ((width as f64) - 10.0) / 2.0,
        ((height as f64) - 10.0) / 2.0,
    );
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.set_line_width(1.0);
    cr.set_line_cap(cairo::LineCap::Square);

    match kind {
        WindowControlKind::Minimize => {
            cr.move_to(1.0, 5.0);
            cr.line_to(9.0, 5.0);
            let _ = cr.stroke();
        }
        WindowControlKind::Maximize => {
            cr.rectangle(1.5, 1.5, 7.0, 7.0);
            let _ = cr.stroke();
        }
        WindowControlKind::Restore => {
            cr.rectangle(2.7, 1.5, 5.8, 5.8);
            let _ = cr.stroke();
            cr.move_to(1.5, 2.8);
            cr.line_to(1.5, 8.5);
            cr.line_to(7.2, 8.5);
            let _ = cr.stroke();
        }
        WindowControlKind::Close => {
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(2.0, 2.0);
            cr.line_to(8.0, 8.0);
            cr.move_to(8.0, 2.0);
            cr.line_to(2.0, 8.0);
            let _ = cr.stroke();
        }
    }

    let _ = cr.restore();
}

pub(crate) fn settings_nav_row(label: &str, icon: SettingsNavIcon, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-nav-row");
    button.set_has_frame(false);
    button.set_size_request(171, 36);
    if selected {
        button.add_css_class("is-selected");
    }

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_halign(gtk::Align::Fill);
    content.append(&settings_nav_icon(icon));
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    content.append(&text);
    button.set_child(Some(&content));
    button
}

pub(crate) fn settings_nav_icon(icon: SettingsNavIcon) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class("okp-settings-nav-icon");
    area.set_size_request(16, 16);
    area.set_draw_func(move |area, cr, width, height| {
        draw_settings_nav_icon(area, cr, width, height, icon);
    });
    area
}

pub(crate) fn draw_settings_nav_icon(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    icon: SettingsNavIcon,
) {
    let color = area.style_context().color();
    let scale = f64::min(width as f64, height as f64) / 16.0;
    let _ = cr.save();
    cr.translate(
        ((width as f64) - (16.0 * scale)) / 2.0,
        ((height as f64) - (16.0 * scale)) / 2.0,
    );
    cr.scale(scale, scale);
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.set_line_width(1.25);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    match icon {
        SettingsNavIcon::Appearance => {
            cr.arc(8.0, 8.0, 3.0, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            for index in 0..8 {
                let angle = (index as f64) * std::f64::consts::FRAC_PI_4;
                cr.move_to(8.0 + angle.cos() * 5.2, 8.0 + angle.sin() * 5.2);
                cr.line_to(8.0 + angle.cos() * 6.7, 8.0 + angle.sin() * 6.7);
            }
            let _ = cr.stroke();
        }
        SettingsNavIcon::Playback => {
            cr.move_to(5.25, 3.35);
            cr.line_to(12.3, 8.0);
            cr.line_to(5.25, 12.65);
            cr.close_path();
            let _ = cr.stroke();
        }
        SettingsNavIcon::Subtitles => {
            cairo_rounded_rect(cr, 2.5, 4.0, 11.0, 8.0, 1.2);
            let _ = cr.stroke();
            cr.move_to(5.0, 8.8);
            cr.line_to(7.3, 8.8);
            cr.move_to(8.7, 8.8);
            cr.line_to(11.0, 8.8);
            cr.move_to(5.0, 10.7);
            cr.line_to(10.2, 10.7);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Video => {
            cairo_rounded_rect(cr, 2.5, 3.5, 11.0, 8.2, 1.1);
            let _ = cr.stroke();
            cr.move_to(8.0, 11.7);
            cr.line_to(8.0, 13.2);
            cr.move_to(5.7, 13.2);
            cr.line_to(10.3, 13.2);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Audio => {
            cr.move_to(2.6, 6.2);
            cr.line_to(5.0, 6.2);
            cr.line_to(8.6, 3.7);
            cr.line_to(8.6, 12.3);
            cr.line_to(5.0, 9.8);
            cr.line_to(2.6, 9.8);
            cr.close_path();
            let _ = cr.stroke();
            cr.arc(8.7, 8.0, 3.3, -0.72, 0.72);
            let _ = cr.stroke();
            cr.arc(8.7, 8.0, 5.1, -0.62, 0.62);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Shortcuts => {
            cairo_rounded_rect(cr, 2.2, 4.1, 11.6, 7.8, 1.1);
            let _ = cr.stroke();
            for y in [6.7, 9.2] {
                for x in [4.5, 6.8, 9.1, 11.4] {
                    cairo_rounded_rect(cr, x - 0.45, y - 0.35, 0.9, 0.7, 0.2);
                    let _ = cr.fill();
                }
            }
        }
        SettingsNavIcon::Integration => {
            let _ = cr.save();
            cr.translate(8.0, 8.0);
            cr.rotate(-std::f64::consts::FRAC_PI_4);
            cairo_rounded_rect(cr, -6.0, -2.2, 7.3, 4.4, 2.2);
            let _ = cr.stroke();
            cairo_rounded_rect(cr, -1.3, -2.2, 7.3, 4.4, 2.2);
            let _ = cr.stroke();
            let _ = cr.restore();
        }
        SettingsNavIcon::Advanced => {
            cr.move_to(6.6, 2.6);
            cr.curve_to(4.6, 2.6, 5.2, 5.2, 4.0, 6.2);
            cr.curve_to(3.4, 6.8, 3.4, 7.2, 4.0, 7.8);
            cr.curve_to(5.2, 8.8, 4.6, 13.4, 6.6, 13.4);
            cr.move_to(9.4, 2.6);
            cr.curve_to(11.4, 2.6, 10.8, 5.2, 12.0, 6.2);
            cr.curve_to(12.6, 6.8, 12.6, 7.2, 12.0, 7.8);
            cr.curve_to(10.8, 8.8, 11.4, 13.4, 9.4, 13.4);
            let _ = cr.stroke();
        }
        SettingsNavIcon::About => {
            cr.arc(8.0, 8.0, 5.8, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.arc(8.0, 5.2, 0.55, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.move_to(8.0, 7.4);
            cr.line_to(8.0, 11.0);
            let _ = cr.stroke();
        }
    }

    let _ = cr.restore();
}

pub(crate) fn cairo_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let right = x + w;
    let bottom = y + h;
    cr.new_sub_path();
    cr.arc(right - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(right - r, bottom - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        bottom - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

pub(crate) fn connect_settings_nav_row(
    button: &gtk::Button,
    page: &str,
    stack: &gtk::Stack,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let page = page.to_owned();
    let stack = stack.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |button| {
        stack.set_visible_child_name(&page);
        for row in buttons.borrow().iter() {
            row.remove_css_class("is-selected");
        }
        button.add_css_class("is-selected");
    });
}

pub(crate) fn settings_section(title: &str) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
    section.add_css_class("okp-info-section");

    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    section.append(&title);
    section
}
