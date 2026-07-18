use super::*;

#[derive(Clone)]
pub(crate) struct SettingsPageBuilder {
    parent: glib::WeakRef<gtk::ApplicationWindow>,
    window: glib::WeakRef<gtk::Window>,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    max_body_height: i32,
}

impl SettingsPageBuilder {
    pub(crate) fn ensure_page(&self, stack: &gtk::Stack, page: SettingsPage) {
        if stack.child_by_name(page.id()).is_some() {
            return;
        }
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let Some(window) = self.window.upgrade() else {
            return;
        };

        let content = match page {
            SettingsPage::About => {
                settings_about_section(Rc::clone(&self.state), Rc::clone(&self.status_toast))
            }
            SettingsPage::Appearance => {
                let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
                page.add_css_class("okp-settings-page");
                page.append(&settings_appearance_section(
                    &window,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                page
            }
            SettingsPage::Advanced => {
                settings_advanced_page(Rc::clone(&self.state), Rc::clone(&self.status_toast))
            }
            SettingsPage::Updates => {
                settings_updates_page(Rc::clone(&self.state), Rc::clone(&self.status_toast))
            }
            SettingsPage::Playback => {
                let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
                page.add_css_class("okp-settings-page");
                let playback = settings_section("Playback");
                playback.append(&settings_resume_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                playback.append(&settings_auto_advance_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                playback.append(&settings_repeat_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                playback.append(&settings_shuffle_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                playback.append(&settings_gapless_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                playback.append(&settings_volume_row(Rc::clone(&self.state)));
                page.append(&playback);
                page
            }
            SettingsPage::Subtitles => settings_subtitles_page(
                &parent,
                Rc::clone(&self.state),
                Rc::clone(&self.status_toast),
            ),
            SettingsPage::Video => {
                let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
                page.add_css_class("okp-settings-page");
                let video = settings_section("Video");
                video.append(&settings_hwdec_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                video.append(&settings_hdr_handling_row());
                video.append(&settings_video_adjustment_row(
                    VideoAdjustment::Brightness,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                video.append(&settings_video_adjustment_row(
                    VideoAdjustment::Contrast,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                video.append(&settings_video_adjustment_row(
                    VideoAdjustment::Saturation,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                video.append(&settings_video_adjustment_row(
                    VideoAdjustment::Gamma,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                page.append(&video);
                page.append(&settings_screenshot_section(
                    &window,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                page
            }
            SettingsPage::Audio => {
                settings_audio_page(Rc::clone(&self.state), Rc::clone(&self.status_toast))
            }
            SettingsPage::Shortcuts => {
                let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
                page.add_css_class("okp-settings-page");
                page.append(&settings_shortcuts_section(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                page
            }
            SettingsPage::Integration => {
                let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
                page.add_css_class("okp-settings-page");
                page.append(&settings_integration_section(Rc::clone(&self.status_toast)));

                let privacy = settings_section("Privacy");
                privacy.append(&settings_private_session_row(
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                privacy.append(&settings_clear_history_row(
                    &parent,
                    Rc::clone(&self.state),
                    Rc::clone(&self.status_toast),
                ));
                page.append(&privacy);

                let storage = settings_section("Storage");
                let settings_path = self
                    .state
                    .borrow()
                    .settings
                    .path()
                    .to_string_lossy()
                    .into_owned();
                storage.append(&settings_value_row("Settings file", &settings_path));
                page.append(&storage);
                page
            }
        };

        stack.add_named(
            &settings_scroller(&content, self.max_body_height),
            Some(page.id()),
        );
    }
}

pub(crate) fn open_settings_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let started = Instant::now();
    let initial_page = env::var("OKP_OPEN_SETTINGS_PAGE_ON_STARTUP")
        .ok()
        .and_then(|page| normalized_settings_page(&page))
        .unwrap_or(SettingsPage::About);
    let page_id = initial_page.id();

    if let Some(window) = existing_companion_window(&state, CompanionWindowKind::Settings) {
        let already_mapped = window.is_mapped();
        if !already_mapped {
            begin_companion_map_timing(
                &state,
                CompanionWindowKind::Settings,
                page_id,
                started,
                true,
            );
        }
        window.present();
        if already_mapped {
            eprintln!(
                "okp-companion-map: kind=settings page={page_id} warm=true ms={}",
                started.elapsed().as_millis()
            );
        }
        eprintln!(
            "okp-companion-present: kind=settings page={page_id} warm=true ms={}",
            started.elapsed().as_millis()
        );
        return;
    }

    let max_window_height = settings_window_height_cap(parent);
    let max_body_height = (max_window_height - SETTINGS_TITLEBAR_HEIGHT).max(1);
    let window = build_companion_window(parent, &state, CompanionWindowKind::Settings, "Settings");
    window.add_css_class("okp-settings-window");
    apply_settings_window_theme(&window, state.borrow().settings.appearance_theme());
    watch_system_settings_theme(&window, Rc::clone(&state));

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("okp-settings-root");

    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    titlebar.add_css_class("okp-settings-titlebar");
    // GTK adds the 1px bottom border outside the requested content box.
    titlebar.set_height_request(SETTINGS_TITLEBAR_HEIGHT - 1);
    let title = gtk::Label::new(Some("Settings"));
    title.add_css_class("okp-settings-titlebar-label");
    title.set_xalign(0.0);
    titlebar.append(&title);
    root.append(&titlebar);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.add_css_class("okp-settings-body");
    body.set_hexpand(true);

    let stack = gtk::Stack::new();
    stack.add_css_class("okp-settings-stack");
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let page_builder = Rc::new(SettingsPageBuilder {
        parent: parent.downgrade(),
        window: window.downgrade(),
        state: Rc::clone(&state),
        status_toast: Rc::clone(&status_toast),
        max_body_height,
    });
    page_builder.ensure_page(&stack, initial_page);

    stack.set_visible_child_name(initial_page.id());
    let (rail, search) = settings_nav_rail(&stack, initial_page, Rc::clone(&page_builder));
    connect_settings_search_shortcut(&window, &search);
    body.append(&settings_nav_rail_frame(rail, max_body_height));

    stack.set_size_request(SETTINGS_CONTENT_WIDTH, -1);
    stack.set_hexpand(true);
    stack.set_vexpand(false);
    let resize_stack = stack.clone();
    stack.connect_visible_child_name_notify(move |_| {
        resize_stack.queue_resize();
    });
    body.append(&stack);
    root.append(&body);

    let window_overlay = gtk::Overlay::new();
    window_overlay.set_child(Some(&root));
    window_overlay.add_overlay(&captionless_window_drag_layer(&window));
    add_companion_window_resize_zones(&window_overlay, &window);
    window_overlay.add_overlay(&settings_window_controls(&window));
    window.set_child(Some(&window_overlay));
    connect_companion_play_pause_space(&window, Rc::clone(&state));

    begin_companion_map_timing(
        &state,
        CompanionWindowKind::Settings,
        page_id,
        started,
        false,
    );
    window.present();
    eprintln!(
        "okp-companion-present: kind=settings page={page_id} warm=false ms={}",
        started.elapsed().as_millis()
    );
}

pub(crate) fn apply_settings_window_theme(window: &gtk::Window, theme: AppearanceTheme) {
    let dark = match settings_theme_override().unwrap_or(theme) {
        AppearanceTheme::Light => false,
        AppearanceTheme::Dark => true,
        AppearanceTheme::Auto => gtk::Settings::default()
            .map(|settings| settings.is_gtk_application_prefer_dark_theme())
            .unwrap_or(false),
    };
    if dark {
        window.add_css_class("is-dark");
    } else {
        window.remove_css_class("is-dark");
    }

    let high_contrast = env::var("GTK_THEME")
        .ok()
        .map(|name| name.to_ascii_lowercase().contains("highcontrast"))
        .unwrap_or(false)
        || gtk::Settings::default()
            .and_then(|settings| settings.gtk_theme_name())
            .map(|name| name.to_ascii_lowercase().contains("highcontrast"))
            .unwrap_or(false);
    if high_contrast {
        window.add_css_class("is-high-contrast");
    } else {
        window.remove_css_class("is-high-contrast");
    }
}

fn settings_theme_override() -> Option<AppearanceTheme> {
    match env::var("OKP_SETTINGS_COLOR_SCHEME")
        .ok()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "light" => Some(AppearanceTheme::Light),
        "dark" => Some(AppearanceTheme::Dark),
        _ => None,
    }
}

pub(crate) fn watch_system_settings_theme(window: &gtk::Window, state: Rc<RefCell<PlayerState>>) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let weak_window = window.downgrade();
    let color_state = Rc::clone(&state);
    settings.connect_gtk_application_prefer_dark_theme_notify(move |_| {
        if let Some(window) = weak_window.upgrade() {
            apply_settings_window_theme(&window, color_state.borrow().settings.appearance_theme());
        }
    });

    let weak_window = window.downgrade();
    settings.connect_gtk_theme_name_notify(move |_| {
        if let Some(window) = weak_window.upgrade() {
            apply_settings_window_theme(&window, state.borrow().settings.appearance_theme());
        }
    });
}

pub(crate) fn normalized_settings_page(page: &str) -> Option<SettingsPage> {
    SettingsPage::from_id(page)
}

pub(crate) fn settings_window_height_cap(parent: &gtk::ApplicationWindow) -> i32 {
    parent
        .surface()
        .and_then(|surface| surface.display().monitor_at_surface(&surface))
        .map(|monitor| settings_window_height_cap_for_monitor(monitor.geometry().height()))
        .unwrap_or(SETTINGS_REFERENCE_HEIGHT)
}

pub(crate) fn settings_window_height_cap_for_monitor(monitor_height: i32) -> i32 {
    monitor_height.saturating_sub(48).max(1)
}

pub(crate) fn settings_scroller<T: IsA<gtk::Widget>>(
    child: &T,
    max_content_height: i32,
) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-settings-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_min_content_width(SETTINGS_CONTENT_WIDTH);
    scroller.set_max_content_width(-1);
    scroller.set_propagate_natural_width(false);
    scroller.set_max_content_height(max_content_height);
    scroller.set_propagate_natural_height(true);
    scroller.set_hexpand(true);
    scroller.set_vexpand(false);
    scroller.set_child(Some(child));
    scroller
}

pub(crate) fn settings_nav_rail_frame(
    rail: gtk::Box,
    max_content_height: i32,
) -> gtk::ScrolledWindow {
    let frame = gtk::ScrolledWindow::new();
    frame.add_css_class("okp-settings-rail-frame");
    frame.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    frame.set_min_content_width(SETTINGS_RAIL_WIDTH);
    frame.set_max_content_width(SETTINGS_RAIL_WIDTH);
    frame.set_propagate_natural_width(false);
    frame.set_max_content_height(max_content_height);
    frame.set_propagate_natural_height(true);
    frame.set_child(Some(&rail));
    frame
}

pub(crate) fn settings_nav_rail(
    stack: &gtk::Stack,
    selected_page: SettingsPage,
    page_builder: Rc<SettingsPageBuilder>,
) -> (gtk::Box, gtk::SearchEntry) {
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 2);
    rail.add_css_class("okp-settings-rail");
    rail.set_size_request(SETTINGS_RAIL_WIDTH, -1);

    let buttons = Rc::new(RefCell::new(Vec::<(SettingsPage, gtk::Button)>::new()));

    let search = gtk::SearchEntry::new();
    search.add_css_class("okp-settings-search");
    search.set_size_request(171, 30);
    search.set_placeholder_text(Some("Search settings"));
    search.update_property(&[gtk::accessible::Property::Label("Search settings")]);
    rail.append(&search);

    let search_result = gtk::Button::new();
    search_result.add_css_class("okp-settings-search-result");
    search_result.set_has_frame(false);
    search_result.set_visible(false);
    let result_content = gtk::Box::new(gtk::Orientation::Vertical, 1);
    let result_label = gtk::Label::new(None);
    result_label.add_css_class("okp-settings-search-result-label");
    result_label.set_xalign(0.0);
    result_label.set_ellipsize(pango::EllipsizeMode::End);
    result_content.append(&result_label);
    let result_page = gtk::Label::new(None);
    result_page.add_css_class("okp-settings-search-result-page");
    result_page.set_xalign(0.0);
    result_content.append(&result_page);
    search_result.set_child(Some(&result_content));
    rail.append(&search_result);

    let active_result = Rc::new(Cell::new(None::<SettingsPage>));
    let result_active = Rc::clone(&active_result);
    let result_label_changed = result_label.clone();
    let result_page_changed = result_page.clone();
    let result_button_changed = search_result.clone();
    search.connect_search_changed(move |entry| {
        let result = search_settings(entry.text().as_str()).into_iter().next();
        result_active.set(result.map(|result| result.page));
        if let Some(result) = result {
            result_label_changed.set_text(result.label);
            result_page_changed.set_text(result.page.title());
            result_button_changed.update_property(&[gtk::accessible::Property::Label(&format!(
                "{} — {}",
                result.label,
                result.page.title()
            ))]);
            result_button_changed.set_visible(true);
        } else {
            result_button_changed.set_visible(false);
        }
    });

    let activate_stack = stack.clone();
    let activate_buttons = Rc::clone(&buttons);
    let activate_result = Rc::clone(&active_result);
    let activate_search = search.clone();
    let activate_builder = Rc::clone(&page_builder);
    search.connect_activate(move |_| {
        if let Some(page) = activate_result.get() {
            navigate_settings_page(page, &activate_stack, &activate_buttons, &activate_builder);
            activate_search.set_text("");
        }
    });

    let click_stack = stack.clone();
    let click_buttons = Rc::clone(&buttons);
    let click_result = Rc::clone(&active_result);
    let click_search = search.clone();
    let click_builder = Rc::clone(&page_builder);
    search_result.connect_clicked(move |_| {
        if let Some(page) = click_result.get() {
            navigate_settings_page(page, &click_stack, &click_buttons, &click_builder);
            click_search.set_text("");
        }
    });

    for page in SETTINGS_RAIL_ORDER {
        let row = settings_nav_row(
            page.title(),
            settings_nav_icon_for_page(page),
            page == selected_page,
        );
        connect_settings_nav_row(&row, page, stack, &buttons, &page_builder);
        buttons.borrow_mut().push((page, row.clone()));
        rail.append(&row);
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-settings-rail-divider");
    rail.append(&divider);

    let about = settings_nav_row(
        SettingsPage::About.title(),
        SettingsNavIcon::About,
        selected_page == SettingsPage::About,
    );
    connect_settings_nav_row(&about, SettingsPage::About, stack, &buttons, &page_builder);
    buttons
        .borrow_mut()
        .push((SettingsPage::About, about.clone()));
    rail.append(&about);

    (rail, search)
}

pub(crate) fn connect_settings_search_shortcut(window: &gtk::Window, search: &gtk::SearchEntry) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let search = search.clone();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::f {
            search.grab_focus();
            search.select_region(0, -1);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(controller);
}

pub(crate) fn settings_nav_icon_for_page(page: SettingsPage) -> SettingsNavIcon {
    match page {
        SettingsPage::Appearance => SettingsNavIcon::Appearance,
        SettingsPage::Playback => SettingsNavIcon::Playback,
        SettingsPage::Subtitles => SettingsNavIcon::Subtitles,
        SettingsPage::Video => SettingsNavIcon::Video,
        SettingsPage::Audio => SettingsNavIcon::Audio,
        SettingsPage::Shortcuts => SettingsNavIcon::Shortcuts,
        SettingsPage::Integration => SettingsNavIcon::Integration,
        SettingsPage::Updates => SettingsNavIcon::Updates,
        SettingsPage::Advanced => SettingsNavIcon::Advanced,
        SettingsPage::About => SettingsNavIcon::About,
    }
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
    let color = area.color();
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
    let color = area.color();
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
        SettingsNavIcon::Updates => {
            cr.move_to(8.0, 2.3);
            cr.line_to(8.0, 9.1);
            cr.move_to(5.2, 6.4);
            cr.line_to(8.0, 9.2);
            cr.line_to(10.8, 6.4);
            let _ = cr.stroke();
            cairo_rounded_rect(cr, 2.5, 10.5, 11.0, 3.0, 1.0);
            let _ = cr.stroke();
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
    page: SettingsPage,
    stack: &gtk::Stack,
    buttons: &Rc<RefCell<Vec<(SettingsPage, gtk::Button)>>>,
    page_builder: &Rc<SettingsPageBuilder>,
) {
    let stack = stack.clone();
    let buttons = Rc::clone(buttons);
    let page_builder = Rc::clone(page_builder);
    button.connect_clicked(move |_| {
        navigate_settings_page(page, &stack, &buttons, &page_builder);
    });
}

pub(crate) fn navigate_settings_page(
    page: SettingsPage,
    stack: &gtk::Stack,
    buttons: &Rc<RefCell<Vec<(SettingsPage, gtk::Button)>>>,
    page_builder: &SettingsPageBuilder,
) {
    page_builder.ensure_page(stack, page);
    stack.set_visible_child_name(page.id());
    for (row_page, row) in buttons.borrow().iter() {
        if *row_page == page {
            row.add_css_class("is-selected");
        } else {
            row.remove_css_class("is-selected");
        }
    }
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
