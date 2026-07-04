use super::*;

#[cfg(test)]
pub(crate) fn parse_launch_args_from(args: impl Iterator<Item = std::ffi::OsString>) -> LaunchArgs {
    let cwd = env::current_dir().ok();
    parse_launch_args_from_cwd(args, cwd.as_deref())
}

pub(crate) fn parse_launch_args_from_cwd(
    mut args: impl Iterator<Item = std::ffi::OsString>,
    cwd: Option<&Path>,
) -> LaunchArgs {
    let mut launch = LaunchArgs::default();
    while let Some(arg) = args.next() {
        if arg == "--sub" {
            if let Some(arg) = args.next() {
                add_launch_subtitle_arg(&mut launch, arg, cwd);
            }
            continue;
        }

        if let Some(text) = arg.to_str() {
            if media_formats::is_playable_url(Some(text)) {
                push_unique_playlist_item(&mut launch.items, PlaylistItem::Url(text.to_owned()));
                continue;
            }

            if let Some(path) = file_uri_path(text) {
                add_launch_path_arg(&mut launch, path);
                continue;
            }
        }

        add_launch_path_arg(&mut launch, launch_path_arg(arg, cwd));
    }

    launch
}

pub(crate) fn file_uri_path(text: &str) -> Option<PathBuf> {
    text.strip_prefix("file://")?;
    gtk::gio::File::for_uri(text).path()
}

pub(crate) fn add_launch_subtitle_arg(
    launch: &mut LaunchArgs,
    arg: std::ffi::OsString,
    cwd: Option<&Path>,
) {
    if let Some(text) = arg.to_str()
        && let Some(path) = file_uri_path(text)
    {
        add_unique_launch_subtitle(launch, path);
        return;
    }

    add_unique_launch_subtitle(launch, launch_path_arg(arg, cwd));
}

pub(crate) fn launch_path_arg(arg: std::ffi::OsString, cwd: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(arg);
    if path.is_relative()
        && let Some(cwd) = cwd
    {
        return cwd.join(path);
    }
    path
}

pub(crate) fn add_launch_path_arg(launch: &mut LaunchArgs, path: PathBuf) {
    if is_subtitle_path(&path) {
        add_unique_launch_subtitle(launch, path);
    } else if is_playlist_path(&path) {
        if !launch.playlists.iter().any(|existing| existing == &path) {
            launch.playlists.push(path);
        }
    } else if is_media_path(&path) {
        push_unique_playlist_item(&mut launch.items, PlaylistItem::Local(path));
    }
}

pub(crate) fn add_unique_launch_subtitle(launch: &mut LaunchArgs, path: PathBuf) {
    if !launch.subtitles.iter().any(|existing| existing == &path) {
        launch.subtitles.push(path);
    }
}

pub(crate) fn push_unique_playlist_item(items: &mut Vec<PlaylistItem>, item: PlaylistItem) {
    if !items.iter().any(|existing| existing == &item) {
        items.push(item);
    }
}

pub(crate) fn build_window(app: &gtk::Application, launch_args: LaunchArgs) -> AppRuntime {
    install_css();

    let identity = AppIdentity::linux();
    let state = Rc::new(RefCell::new(PlayerState::default()));
    apply_playback_settings_defaults(&state);
    let auto_check_updates = state.borrow().settings.auto_check_updates();
    let updating_seek = Rc::new(Cell::new(false));
    let updating_volume = Rc::new(Cell::new(false));
    let status_toast = Rc::new(StatusToast::new());
    let chrome = Rc::new(ChromeVisibility::new());
    let (mpris_controller, mpris_commands, mpris_signals) = create_mpris_controller();
    start_mpris_service(mpris_controller.clone(), mpris_signals);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&identity.name)
        .default_width(1120)
        .default_height(680)
        .decorated(false)
        .build();
    window.add_css_class("okp-player-window");

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-root");

    let video_area = gtk::GLArea::new();
    video_area.set_hexpand(true);
    video_area.set_vexpand(true);
    video_area.set_auto_render(false);
    video_area.set_required_version(3, 2);
    video_area.add_css_class("okp-video-plane");

    let controls = build_controls(
        &window,
        Rc::clone(&state),
        Rc::clone(&updating_seek),
        Rc::clone(&updating_volume),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    let control_bar = controls_bar(&controls);
    let window_chrome = build_player_window_chrome(&window);
    sync_player_window_chrome_fullscreen(&window_chrome, &window);
    let empty_surface = build_empty_surface(&window, Rc::clone(&state), Rc::clone(&status_toast));
    chrome.set_child(&control_bar);
    chrome.add_linked_revealer(&window_chrome);
    chrome.add_linked_revealer(&controls.up_next_revealer);

    overlay.set_child(Some(&video_area));
    overlay.add_overlay(empty_surface.widget());
    overlay.add_overlay(&window_chrome);
    overlay.add_overlay(chrome.widget());
    overlay.add_overlay(&controls.up_next_revealer);
    overlay.add_overlay(status_toast.widget());
    for resize_handle in build_player_resize_handles(&window) {
        overlay.add_overlay(&resize_handle);
    }
    window.set_child(Some(&overlay));
    connect_chrome_activity(&overlay, Rc::clone(&chrome));

    connect_mpv(&video_area, Rc::clone(&state), launch_args);
    connect_video_clicks(
        &video_area,
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    );
    connect_drop(&window, Rc::clone(&state), empty_surface.clone());
    connect_keyboard(
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    connect_mpris_commands(
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        mpris_commands,
    );
    connect_progress_persistence(&window, Rc::clone(&state));
    // Visual smoke hook: render the Chapters/Up Next side panel with representative
    // fixture rows so its layout can be screenshot-tested without loaded media.
    // `OKP_OPEN_SIDE_PANEL_ON_STARTUP=up-next` previews the queue; any other value
    // previews Chapters.
    if let Some(value) = env::var_os("OKP_OPEN_SIDE_PANEL_ON_STARTUP") {
        let mode = if value.eq_ignore_ascii_case("up-next") {
            SidePanelMode::UpNext
        } else {
            SidePanelMode::Chapters
        };
        open_side_panel_preview(&controls, &state, &chrome, mode);
    }
    connect_state_poll(
        &window,
        Rc::clone(&state),
        controls,
        StatePollContext {
            updating_seek: Rc::clone(&updating_seek),
            updating_volume: Rc::clone(&updating_volume),
            chrome: Rc::clone(&chrome),
            empty_surface,
            mpris_snapshot: Arc::clone(&mpris_controller.snapshot),
            mpris_signals: mpris_controller.signals.clone(),
        },
    );

    window.present();
    if env::var_os("OKP_OPEN_SETTINGS_ON_STARTUP").is_some() {
        let settings_parent = window.clone();
        let settings_state = Rc::clone(&state);
        let settings_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            open_settings_window(&settings_parent, settings_state, settings_toast);
        });
    }
    // Visual smoke hook: render the Media Information window with representative
    // fixture data so its layout can be screenshot-tested without loaded media.
    if env::var_os("OKP_OPEN_MEDIA_INFO_ON_STARTUP").is_some() {
        let info_parent = window.clone();
        let info_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            show_media_info_window(&info_parent, &media_info_preview_sample(), info_toast);
        });
    }
    if auto_check_updates {
        check_updates_on_startup(Rc::clone(&state), Rc::clone(&status_toast));
    }

    AppRuntime { window, state }
}

pub(crate) fn open_runtime_launch_args(runtime: &AppRuntime, launch_args: &LaunchArgs) {
    runtime.window.present();
    if launch_args.has_payload() {
        apply_launch_args(&runtime.state, launch_args);
    }
}

pub(crate) fn sync_player_window_chrome_fullscreen(
    window_chrome: &gtk::Revealer,
    window: &gtk::ApplicationWindow,
) {
    window_chrome.set_visible(!window.is_fullscreen());

    let fullscreen_chrome = window_chrome.clone();
    window.connect_notify_local(Some("fullscreened"), move |window, _| {
        fullscreen_chrome.set_visible(!window.is_fullscreen());
    });
}

pub(crate) fn build_player_window_chrome(window: &gtk::ApplicationWindow) -> gtk::Revealer {
    let revealer = gtk::Revealer::new();
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Start);
    revealer.set_transition_duration(140);
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_reveal_child(true);
    revealer.set_can_target(true);

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("okp-window-chrome");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::Start);
    bar.set_margin_top(0);

    let drag_zone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_zone.add_css_class("okp-window-drag-zone");
    drag_zone.set_hexpand(true);
    drag_zone.set_can_target(true);
    connect_player_window_drag(&drag_zone, window);
    bar.append(&drag_zone);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-player-window-controls");
    controls.set_halign(gtk::Align::End);
    controls.set_margin_top(4);
    controls.set_margin_end(6);

    let minimize = player_window_control(WindowControlKind::Minimize, "Minimize");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    controls.append(&minimize);

    let maximize = player_window_control(WindowControlKind::Maximize, "Maximize");
    sync_player_maximize_icon(&maximize, window);
    let maximize_window = window.clone();
    let maximize_button = maximize.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
        sync_player_maximize_icon(&maximize_button, &maximize_window);
    });
    let notify_button = maximize.clone();
    window.connect_maximized_notify(move |window| {
        sync_player_maximize_icon(&notify_button, window);
    });
    controls.append(&maximize);

    let close = player_window_control(WindowControlKind::Close, "Close");
    close.add_css_class("okp-player-window-close");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    controls.append(&close);

    bar.append(&controls);
    revealer.set_child(Some(&bar));
    revealer
}

pub(crate) fn player_window_control(kind: WindowControlKind, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-player-window-control");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button.set_child(Some(&window_control_icon(
        kind,
        "okp-player-window-control-glyph",
    )));
    button
}

pub(crate) fn sync_player_maximize_icon(button: &gtk::Button, window: &gtk::ApplicationWindow) {
    if window.is_maximized() {
        set_player_window_control_kind(button, WindowControlKind::Restore);
        button.set_tooltip_text(Some("Restore"));
    } else {
        set_player_window_control_kind(button, WindowControlKind::Maximize);
        button.set_tooltip_text(Some("Maximize"));
    }
}

pub(crate) fn set_player_window_control_kind(button: &gtk::Button, kind: WindowControlKind) {
    if let Some(icon) = button.child().and_downcast::<gtk::DrawingArea>() {
        icon.set_draw_func(move |area, cr, width, height| {
            draw_window_control_icon(area, cr, width, height, kind);
        });
        icon.queue_draw();
    }
}

pub(crate) fn connect_player_window_drag(
    widget: &impl IsA<gtk::Widget>,
    window: &gtk::ApplicationWindow,
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

pub(crate) fn build_player_resize_handles(window: &gtk::ApplicationWindow) -> Vec<gtk::Box> {
    let specs = [
        (
            gdk::SurfaceEdge::NorthWest,
            gtk::Align::Start,
            gtk::Align::Start,
            16,
            16,
            "nwse-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::North,
            gtk::Align::Fill,
            gtk::Align::Start,
            -1,
            6,
            "ns-resize",
            "okp-resize-edge-horizontal",
        ),
        (
            gdk::SurfaceEdge::NorthEast,
            gtk::Align::End,
            gtk::Align::Start,
            16,
            16,
            "nesw-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::West,
            gtk::Align::Start,
            gtk::Align::Fill,
            6,
            -1,
            "ew-resize",
            "okp-resize-edge-vertical",
        ),
        (
            gdk::SurfaceEdge::East,
            gtk::Align::End,
            gtk::Align::Fill,
            6,
            -1,
            "ew-resize",
            "okp-resize-edge-vertical",
        ),
        (
            gdk::SurfaceEdge::SouthWest,
            gtk::Align::Start,
            gtk::Align::End,
            16,
            16,
            "nesw-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::South,
            gtk::Align::Fill,
            gtk::Align::End,
            -1,
            6,
            "ns-resize",
            "okp-resize-edge-horizontal",
        ),
        (
            gdk::SurfaceEdge::SouthEast,
            gtk::Align::End,
            gtk::Align::End,
            16,
            16,
            "nwse-resize",
            "okp-resize-corner",
        ),
    ];

    specs
        .into_iter()
        .map(
            |(edge, halign, valign, width, height, cursor, class_name)| {
                let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                handle.add_css_class("okp-resize-handle");
                handle.add_css_class(class_name);
                handle.set_halign(halign);
                handle.set_valign(valign);
                handle.set_can_target(true);
                handle.set_cursor_from_name(Some(cursor));
                if width > 0 {
                    handle.set_width_request(width);
                }
                if height > 0 {
                    handle.set_height_request(height);
                }
                connect_player_window_resize(&handle, window, edge);
                handle
            },
        )
        .collect()
}

pub(crate) fn connect_player_window_resize(
    widget: &gtk::Box,
    window: &gtk::ApplicationWindow,
    edge: gdk::SurfaceEdge,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let resize_window = window.clone();
    let resize_widget = widget.clone();
    gesture.connect_pressed(move |gesture, _, x, y| {
        let debug_resize = env::var_os("OKP_DEBUG_WINDOW_RESIZE").is_some();
        if debug_resize {
            eprintln!("resize press edge={edge:?} local=({x:.1},{y:.1})");
        }

        if resize_window.is_fullscreen() || resize_window.is_maximized() {
            if debug_resize {
                eprintln!("resize ignored: fullscreen/maximized");
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            if debug_resize {
                eprintln!("resize ignored: no device");
            }
            return;
        };
        let Some(surface) = resize_window.surface() else {
            if debug_resize {
                eprintln!("resize ignored: no surface");
            }
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            if debug_resize {
                eprintln!("resize ignored: surface is not a toplevel");
            }
            return;
        };
        let window_point = resize_widget
            .compute_point(
                &resize_window,
                &gtk::graphene::Point::new(x as f32, y as f32),
            )
            .map(|point| (f64::from(point.x()), f64::from(point.y())))
            .unwrap_or((x, y));
        if debug_resize {
            eprintln!(
                "resize begin edge={edge:?} window=({:.1},{:.1}) button={}",
                window_point.0,
                window_point.1,
                gesture.current_button()
            );
        }

        toplevel.begin_resize(
            edge,
            Some(&device),
            gesture.current_button() as i32,
            window_point.0,
            window_point.1,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

pub(crate) fn build_empty_surface(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> EmptySurface {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("okp-empty-panel");
    panel.set_halign(gtk::Align::Center);
    panel.set_valign(gtk::Align::Center);

    let logo = empty_surface_logo();
    panel.append(&logo);

    let wordmark = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    wordmark.add_css_class("okp-empty-wordmark");
    wordmark.set_halign(gtk::Align::Center);
    let wordmark_ok = gtk::Label::new(Some("OK"));
    wordmark_ok.add_css_class("okp-empty-wordmark-ok");
    let wordmark_player = gtk::Label::new(Some(" Player"));
    wordmark_player.add_css_class("okp-empty-wordmark-player");
    wordmark.append(&wordmark_ok);
    wordmark.append(&wordmark_player);
    panel.append(&wordmark);

    let tagline = gtk::Label::new(Some("Open a file to start playing."));
    tagline.add_css_class("okp-empty-tagline");
    tagline.set_justify(gtk::Justification::Center);
    tagline.set_wrap(true);
    tagline.set_max_width_chars(34);
    panel.append(&tagline);

    let recents = build_welcome_recents();
    panel.append(&recents.section);
    panel.append(&recents.private_note);

    let actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
    actions.add_css_class("okp-empty-actions");

    let open_button = gtk::Button::with_label("Open media");
    open_button.add_css_class("okp-empty-primary-button");
    open_button.set_hexpand(true);
    open_button.set_halign(gtk::Align::Fill);
    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    open_button.connect_clicked(move |_| open_media_dialog(&open_parent, Rc::clone(&open_state)));
    actions.append(&open_button);

    let secondary_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    secondary_row.set_homogeneous(true);

    let folder_button = gtk::Button::with_label("Open folder");
    folder_button.add_css_class("okp-empty-secondary-button");
    let folder_parent = window.clone();
    let folder_state = Rc::clone(&state);
    let folder_toast = Rc::clone(&status_toast);
    folder_button.connect_clicked(move |_| {
        open_folder_dialog(
            &folder_parent,
            Rc::clone(&folder_state),
            Rc::clone(&folder_toast),
        );
    });
    secondary_row.append(&folder_button);

    let url_button = gtk::Button::with_label("Open URL");
    url_button.add_css_class("okp-empty-secondary-button");
    let url_parent = window.clone();
    let url_state = Rc::clone(&state);
    let url_toast = Rc::clone(&status_toast);
    url_button.connect_clicked(move |_| {
        open_url_dialog(&url_parent, Rc::clone(&url_state), Rc::clone(&url_toast));
    });
    secondary_row.append(&url_button);

    actions.append(&secondary_row);
    panel.append(&actions);

    let hint = gtk::Label::new(Some("Drop media here · press O to open"));
    hint.add_css_class("okp-empty-hint");
    hint.set_justify(gtk::Justification::Center);
    hint.set_wrap(true);
    hint.set_max_width_chars(40);
    panel.append(&hint);

    let revealer = gtk::Revealer::new();
    revealer.add_css_class("okp-empty-surface");
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Fill);
    revealer.set_transition_duration(180);
    revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    revealer.set_reveal_child(true);
    revealer.set_child(Some(&panel));

    let surface = EmptySurface {
        revealer,
        panel,
        tagline,
        secondary_row,
        hint,
        recents,
        state,
        signature: Rc::new(RefCell::new(None)),
    };
    // Populate the shelf from existing history so a returning user sees "Continue watching"
    // immediately, before the first idle poll tick.
    surface.refresh_recents();
    surface
}

/// Build the (initially hidden) welcome recents widgets: the "Continue watching" section — a
/// heading over a reflowing card row — and the private-session note that stands in for it while a
/// private session is active. Both start collapsed; [`EmptySurface::refresh_recents`] reveals the
/// right one.
pub(crate) fn build_welcome_recents() -> WelcomeRecents {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 12);
    section.add_css_class("okp-recents-section");
    section.set_halign(gtk::Align::Center);
    section.set_visible(false);

    let header = gtk::Label::new(Some("Continue watching"));
    header.add_css_class("okp-recents-header");
    header.set_halign(gtk::Align::Start);
    section.append(&header);

    let cards = gtk::FlowBox::new();
    cards.add_css_class("okp-recents-row");
    cards.set_selection_mode(gtk::SelectionMode::None);
    cards.set_homogeneous(true);
    cards.set_max_children_per_line(WELCOME_RECENTS_MAX_CARDS as u32);
    cards.set_min_children_per_line(1);
    cards.set_row_spacing(14);
    cards.set_column_spacing(14);
    // The card row must never own the horizontal scroll; when the window is too narrow for the
    // full row the FlowBox reflows the cards onto a second line rather than overlapping them.
    cards.set_halign(gtk::Align::Center);
    section.append(&cards);

    let private_note = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    private_note.add_css_class("okp-recents-private");
    private_note.set_halign(gtk::Align::Center);
    private_note.set_visible(false);
    let private_icon = gtk::Image::from_icon_name("view-conceal-symbolic");
    private_icon.add_css_class("okp-recents-private-icon");
    private_note.append(&private_icon);
    let private_text = gtk::Label::new(Some("Private session — recent activity is hidden."));
    private_text.add_css_class("okp-recents-private-text");
    private_text.set_wrap(true);
    private_text.set_max_width_chars(36);
    private_note.append(&private_text);

    WelcomeRecents {
        section,
        header,
        cards,
        private_note,
    }
}

/// Build one "Continue watching" card: a clickable poster tile (placeholder gradient + progress
/// fill + a "time left" badge) over the title, folder breadcrumb, and last-opened context. Clicking
/// resumes the file. Purely presentational — every string and fraction comes from the core
/// [`recents_shelf::ContinueWatchingCard`].
pub(crate) fn build_recent_card(
    state: &Rc<RefCell<PlayerState>>,
    card: &recents_shelf::ContinueWatchingCard,
) -> gtk::Widget {
    let button = gtk::Button::new();
    button.add_css_class("okp-recents-card");
    button.set_tooltip_text(Some(&card.path));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let thumb = gtk::Overlay::new();
    thumb.add_css_class("okp-recents-thumb");
    thumb.add_css_class(&format!("okp-recents-thumb-{}", card.palette_index % 5));
    thumb.set_size_request(194, 96);

    let glyph = gtk::Image::from_icon_name(if card.is_audio {
        "audio-x-generic-symbolic"
    } else {
        "media-playback-start-symbolic"
    });
    glyph.add_css_class("okp-recents-thumb-glyph");
    glyph.set_halign(gtk::Align::Center);
    glyph.set_valign(gtk::Align::Center);
    glyph.set_pixel_size(26);
    thumb.set_child(Some(&glyph));

    let badge = gtk::Label::new(Some(&card.time_left_label));
    badge.add_css_class("okp-recents-badge");
    badge.set_halign(gtk::Align::End);
    badge.set_valign(gtk::Align::Start);
    thumb.add_overlay(&badge);

    let progress = gtk::ProgressBar::new();
    progress.add_css_class("okp-recents-progress");
    progress.set_fraction(card.progress.clamp(0.0, 1.0));
    progress.set_halign(gtk::Align::Fill);
    progress.set_valign(gtk::Align::End);
    thumb.add_overlay(&progress);
    content.append(&thumb);

    let title = gtk::Label::new(Some(&card.title));
    title.add_css_class("okp-recents-title");
    title.set_halign(gtk::Align::Start);
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_max_width_chars(20);
    content.append(&title);

    let breadcrumb = history_format::folder_label(&card.path);
    let meta_text = if breadcrumb.is_empty() {
        card.runtime_label.clone()
    } else {
        format!("{breadcrumb} · {}", card.runtime_label)
    };
    let meta = gtk::Label::new(Some(&meta_text));
    meta.add_css_class("okp-recents-meta");
    meta.set_halign(gtk::Align::Start);
    meta.set_xalign(0.0);
    meta.set_ellipsize(pango::EllipsizeMode::End);
    meta.set_max_width_chars(20);
    content.append(&meta);

    let when_text = when_label_for(card.updated_at_unix);
    if !when_text.is_empty() {
        let when = gtk::Label::new(Some(&when_text));
        when.add_css_class("okp-recents-when");
        when.set_halign(gtk::Align::Start);
        when.set_xalign(0.0);
        content.append(&when);
    }

    button.set_child(Some(&content));

    let click_state = Rc::clone(state);
    let path = card.path.clone();
    button.connect_clicked(move |_| open_recent_media(&click_state, &path));

    button.upcast()
}

/// Open a recents card's target: a stream URL through the URL loader, a local path through the file
/// loader. Both route through the same resume/progress path the open dialogs use.
pub(crate) fn open_recent_media(state: &Rc<RefCell<PlayerState>>, path: &str) {
    if media_formats::is_playable_url(Some(path)) {
        load_media_url(state, path.to_owned());
    } else {
        load_media_path(state, PathBuf::from(path));
    }
}

/// The local "when" label for a last-opened Unix timestamp (e.g. "Today 21:14", "Tue 20:03",
/// "12 Jun"), formatted by the shared [`history_format`] rule. Empty when the clock is unavailable.
pub(crate) fn when_label_for(updated_at_unix: i64) -> String {
    let Ok(when) = glib::DateTime::from_unix_local(updated_at_unix) else {
        return String::new();
    };
    let Ok(now) = glib::DateTime::now_local() else {
        return String::new();
    };
    history_format::when_label(local_date_time(&when), local_date_time(&now))
}

fn local_date_time(value: &glib::DateTime) -> history_format::LocalDateTime {
    history_format::LocalDateTime::new(
        value.year(),
        value.month() as u32,
        value.day_of_month() as u32,
        value.hour() as u32,
        value.minute() as u32,
    )
}

/// A stable fingerprint of the shelf's inputs, so the idle poll only rebuilds the cards when the
/// resumable set (or the private flag) changed — not on every 200ms tick.
pub(crate) fn welcome_recents_signature(
    private: bool,
    cards: &[recents_shelf::ContinueWatchingCard],
) -> String {
    if private {
        return "private".to_owned();
    }
    if cards.is_empty() {
        return "empty".to_owned();
    }
    let mut signature = String::new();
    for card in cards {
        signature.push_str(&card.path);
        signature.push('|');
        signature.push_str(&format!("{:.4}", card.progress));
        signature.push('|');
        signature.push_str(&card.updated_at_unix.to_string());
        signature.push(';');
    }
    signature
}

/// Remove every child from a FlowBox (its children are the auto-inserted `FlowBoxChild` wrappers).
pub(crate) fn clear_flow_box(flow_box: &gtk::FlowBox) {
    while let Some(child) = flow_box.first_child() {
        flow_box.remove(&child);
    }
}

/// The screenshot/smoke override for the welcome shelf, mirroring the other preview-on-startup
/// hooks: `OKP_WELCOME_RECENTS_PREVIEW=private` renders the private-session note; any other value
/// renders a fixed fixture card set. `None` (unset) means read real history. Returns the
/// `(private, cards)` pair the shell would otherwise compute, so the surface renders
/// deterministically without seeded history.
pub(crate) fn welcome_recents_preview() -> Option<(bool, Vec<recents_shelf::ContinueWatchingCard>)>
{
    let value = env::var_os("OKP_WELCOME_RECENTS_PREVIEW")?;
    if value
        .to_str()
        .is_some_and(|value| value.eq_ignore_ascii_case("private"))
    {
        Some((true, Vec::new()))
    } else {
        Some((false, welcome_recents_preview_sample()))
    }
}

/// A representative "Continue watching" card set for the screenshot/smoke preview, run through the
/// real core selection so the preview exercises the shipping projection.
pub(crate) fn welcome_recents_preview_sample() -> Vec<recents_shelf::ContinueWatchingCard> {
    use okp_core::history::FileEntry;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let entry = |position: f64, duration: f64, opened_ago: i64| FileEntry {
        position,
        duration,
        updated_at_unix: now - opened_ago,
        ..FileEntry::default()
    };
    let entries = [
        (
            "/home/media/Movies/Dune Part Two/Dune.Part.Two.2160p.mkv".to_owned(),
            entry(4200.0, 9660.0, 90 * 60),
        ),
        (
            "/home/media/Series/Severance/Season 02/S02E05.mkv".to_owned(),
            entry(1180.0, 3300.0, 26 * 60 * 60),
        ),
        (
            "/home/media/Talks/Design/interview-raw-take3.mov".to_owned(),
            entry(300.0, 2400.0, 3 * 24 * 60 * 60),
        ),
    ];
    recents_shelf::select_continue_watching(
        entries.iter().map(|(path, record)| (path.as_str(), record)),
        false,
        WELCOME_RECENTS_MAX_CARDS,
    )
}

/// The welcome surface anchors the OK Player identity with the app icon tile.
/// It loads the bundled SVG directly so the mark renders crisply in development
/// and packaged builds alike, falling back to the themed icon when the asset is
/// not on disk (mirrors `about_illustration`).
pub(crate) fn empty_surface_logo() -> gtk::Image {
    if let Some(path) = empty_surface_logo_path() {
        let image = gtk::Image::from_file(path);
        image.add_css_class("okp-empty-logo");
        image.set_size_request(64, 64);
        image.set_pixel_size(64);
        return image;
    }

    let image = gtk::Image::from_icon_name("com.befeast.okplayer");
    image.add_css_class("okp-empty-logo");
    image.set_pixel_size(64);
    image
}

pub(crate) fn empty_surface_logo_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(
        "/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg",
    ));
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("com.befeast.okplayer.svg"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/linux/com.befeast.okplayer.svg"),
    );

    candidates.into_iter().find(|path| path.is_file())
}
