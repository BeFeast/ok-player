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
            // The reserved ok-player:// scheme is checked before the URL/path branches so a
            // control request is never mistaken for a stream URL and handed to the engine.
            if let Some(notice) = reserved_uri_notice(text) {
                if !launch.reserved_notices.contains(&notice) {
                    launch.reserved_notices.push(notice);
                }
                continue;
            }

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
    // Visual smoke hook for the private welcome state. Private session is transient by
    // design, so this changes only the in-memory session and never writes a setting.
    if env::var_os("OKP_PRIVATE_SESSION_ON_STARTUP").is_some() {
        state.borrow_mut().private_session = true;
    }
    apply_playback_settings_defaults(&state);
    let auto_check_updates = state.borrow().settings.auto_check_updates()
        && env::var_os("OKP_SKIP_UPDATE_CHECK").is_none();
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
    let lyrics_surface = build_lyrics_surface();
    let media_state_overlay = MediaStateOverlay::new();
    chrome.set_child(&control_bar);
    chrome.add_linked_motion_widget(window_chrome.widget());
    chrome.add_linked_revealer(&controls.up_next_revealer);

    overlay.set_child(Some(&video_area));
    // Visual smoke hook: Xvfb cannot present the libmpv GL frame reliably on every
    // renderer, so deterministic bright/dark planes exercise chrome legibility while
    // real media remains loaded underneath to drive the production visibility state.
    if let Some(mode) = env::var_os("OKP_PLAYBACK_FRAME_PREVIEW") {
        let preview = gtk::DrawingArea::new();
        let color = if mode.eq_ignore_ascii_case("bright") {
            (0.957, 0.961, 0.965)
        } else {
            (0.031, 0.035, 0.043)
        };
        preview.set_draw_func(move |_, cr, width, height| {
            cr.set_source_rgb(color.0, color.1, color.2);
            cr.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
            let _ = cr.fill();
        });
        preview.set_halign(gtk::Align::Fill);
        preview.set_valign(gtk::Align::Fill);
        preview.set_hexpand(true);
        preview.set_vexpand(true);
        preview.set_size_request(1120, 680);
        preview.set_can_target(false);
        overlay.add_overlay(&preview);
        overlay.set_measure_overlay(&preview, true);
    }
    overlay.add_overlay(empty_surface.widget());
    // The loading / buffering / error overlay sits just above the empty surface and
    // lyrics — below the chrome and toast — so the transport stays on top while a
    // stream's loading or failure status reads over the black video plane.
    overlay.add_overlay(media_state_overlay.widget());
    // The lyrics surface sits above the (audio-black) video plane but below the window chrome, the
    // OSC, and the side panel, so those stay on top and interactive while lyrics play underneath.
    overlay.add_overlay(lyrics_surface.widget());
    overlay.add_overlay(window_chrome.widget());
    overlay.add_overlay(chrome.widget());
    overlay.add_overlay(&controls.up_next_revealer);
    overlay.add_overlay(status_toast.widget());
    for resize_handle in build_player_resize_handles(&window) {
        overlay.add_overlay(&resize_handle);
    }
    window.set_child(Some(&overlay));
    chrome.add_cursor_widget(&overlay);
    connect_chrome_activity(&overlay, Rc::clone(&chrome));

    let launch_reserved_notice = launch_args.reserved_notice().map(str::to_owned);
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
    // `OKP_OPEN_SIDE_PANEL_ON_STARTUP=up-next` previews the queue; `=bookmarks`
    // previews the user-authored section; `=up-next-empty`
    // previews the PRD §2.6 single-URL / short-queue state (now-playing pin + the
    // "Add files" affordance); `=chapters-empty` previews the no-chapters stream
    // state; any other value previews the full Chapters fixture.
    if let Some(value) = env::var_os("OKP_OPEN_SIDE_PANEL_ON_STARTUP") {
        let bright_substrate = env::var_os("OKP_SIDE_PANEL_PREVIEW_SUBSTRATE")
            .is_some_and(|value| value.eq_ignore_ascii_case("bright"));
        empty_surface.set_preview_substrate(bright_substrate);
        let (mode, snapshot) = if value.eq_ignore_ascii_case("up-next") {
            (SidePanelMode::UpNext, side_panel_preview_sample())
        } else if value.eq_ignore_ascii_case("bookmarks") {
            (SidePanelMode::Chapters, side_panel_bookmarks_sample())
        } else if value.eq_ignore_ascii_case("up-next-empty") {
            (SidePanelMode::UpNext, side_panel_empty_up_next_sample())
        } else if value.eq_ignore_ascii_case("chapters-empty") {
            (SidePanelMode::Chapters, side_panel_empty_chapters_sample())
        } else {
            (SidePanelMode::Chapters, side_panel_preview_sample())
        };
        open_side_panel_preview(&controls, &state, &chrome, mode, snapshot);
    }
    // Visual smoke hook: render the audio lyrics overlay with a representative sheet so its layout
    // and active-line state can be screenshot-tested without loaded media.
    // `OKP_OPEN_LYRICS_ON_STARTUP=plain` previews untimed lyrics and `=empty` the no-lyrics state;
    // any other value previews the synced sheet with a live highlight.
    if let Some(value) = env::var_os("OKP_OPEN_LYRICS_ON_STARTUP") {
        let mode = if value.eq_ignore_ascii_case("plain") {
            LyricsPreviewMode::Plain
        } else if value.eq_ignore_ascii_case("empty") {
            LyricsPreviewMode::Empty
        } else {
            LyricsPreviewMode::Synced
        };
        lyrics_surface.open_preview(&state, mode);
    }
    // Visual smoke hook: pop the seek-bar hover tooltip with a representative timecode
    // and chapter so the timeline tooltip's timecode-only fallback can be screenshot-
    // tested without loaded media or a generated thumbnail.
    if env::var_os("OKP_OPEN_SEEK_PREVIEW_ON_STARTUP").is_some() {
        open_seek_preview(&controls);
    }
    connect_state_poll(
        &window,
        Rc::clone(&state),
        controls,
        StatePollContext {
            updating_seek: Rc::clone(&updating_seek),
            updating_volume: Rc::clone(&updating_volume),
            chrome: Rc::clone(&chrome),
            window_chrome,
            subtitle_position_snapshot: Rc::new(Cell::new(None)),
            empty_surface: empty_surface.clone(),
            lyrics_surface,
            media_state_overlay,
            mpris_snapshot: Arc::clone(&mpris_controller.snapshot),
            mpris_signals: mpris_controller.signals.clone(),
        },
    );

    window.present();
    if env::var_os("OKP_OPEN_HISTORY_ON_STARTUP").is_some() {
        let history_surface = empty_surface.clone();
        let history_parent = window.clone();
        let history_state = Rc::clone(&state);
        let history_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            history_surface.show_history(&history_parent, history_state, history_toast);
        });
    }
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
    if let Some(notice) = launch_reserved_notice {
        eprintln!("Ignoring reserved ok-player:// request: {notice}");
        let notice_toast = Rc::clone(&status_toast);
        glib::idle_add_local_once(move || notice_toast.show(&notice));
    }

    AppRuntime {
        window,
        state,
        status_toast,
    }
}

pub(crate) fn open_runtime_launch_args(runtime: &AppRuntime, launch_args: &LaunchArgs) {
    runtime.window.present();
    if let Some(notice) = launch_args.reserved_notice() {
        eprintln!("Ignoring reserved ok-player:// request: {notice}");
        runtime.status_toast.show(notice);
    }
    if launch_args.has_payload() {
        apply_launch_args(&runtime.state, launch_args);
    }
}

pub(crate) fn sync_player_window_chrome_fullscreen(
    window_chrome: &PlayerWindowChrome,
    window: &gtk::ApplicationWindow,
) {
    window_chrome.widget().set_visible(!window.is_fullscreen());

    let fullscreen_chrome = window_chrome.widget().clone();
    window.connect_notify_local(Some("fullscreened"), move |window, _| {
        fullscreen_chrome.set_visible(!window.is_fullscreen());
    });
}

impl PlayerWindowChrome {
    pub(crate) fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    pub(crate) fn set_title(&self, title: &str) {
        self.title_label.set_text(title);
        self.media_icon.set_visible(!title.is_empty());
        self.title_label.set_visible(!title.is_empty());
    }
}

pub(crate) fn build_player_window_chrome(window: &gtk::ApplicationWindow) -> PlayerWindowChrome {
    let revealer = gtk::Revealer::new();
    revealer.add_css_class("okp-top-chrome-motion");
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Start);
    revealer.set_transition_duration(0);
    revealer.set_transition_type(gtk::RevealerTransitionType::None);
    revealer.set_reveal_child(true);
    revealer.set_can_target(true);

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("okp-window-chrome");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::Start);
    bar.set_margin_top(0);

    let drag_zone = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    drag_zone.add_css_class("okp-window-drag-zone");
    drag_zone.set_hexpand(true);
    drag_zone.set_can_target(true);
    drag_zone.set_margin_start(14);

    let media_icon = gtk::Image::from_icon_name("media-playback-start-symbolic");
    media_icon.add_css_class("okp-window-media-icon");
    media_icon.set_visible(false);
    let title_label = gtk::Label::new(None);
    title_label.add_css_class("okp-window-media-title");
    title_label.set_ellipsize(pango::EllipsizeMode::End);
    title_label.set_xalign(0.0);
    title_label.set_visible(false);
    drag_zone.append(&media_icon);
    drag_zone.append(&title_label);
    connect_player_window_drag(&drag_zone, window);
    bar.append(&drag_zone);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-player-window-controls");
    controls.set_halign(gtk::Align::End);

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
    PlayerWindowChrome {
        revealer,
        media_icon,
        title_label,
    }
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
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("okp-idle-canvas");
    root.add_css_class(if idle_theme_is_dark() {
        "is-dark"
    } else {
        "is-light"
    });
    root.append(&idle_titlebar());

    let stack = gtk::Stack::new();
    stack.add_css_class("okp-idle-stack");
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    let animations_enabled = env::var_os("OKP_REDUCED_MOTION").is_none()
        && gtk::Settings::default()
            .map(|settings| settings.property::<bool>("gtk-enable-animations"))
            .unwrap_or(true);
    stack.set_transition_type(if animations_enabled {
        gtk::StackTransitionType::Crossfade
    } else {
        gtk::StackTransitionType::None
    });
    stack.set_transition_duration(if animations_enabled { 180 } else { 0 });

    let welcome_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
    welcome_host.set_hexpand(true);
    welcome_host.set_vexpand(true);
    let history_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
    history_host.set_hexpand(true);
    history_host.set_vexpand(true);
    stack.add_named(&welcome_host, Some("welcome"));
    stack.add_named(&history_host, Some("history"));
    stack.set_visible_child_name("welcome");
    root.append(&stack);

    let (footer, footer_left, footer_left_icon, footer_left_label, footer_status) =
        idle_footer_widgets();
    idle_footer_settings_button(&footer, window, Rc::clone(&state), Rc::clone(&status_toast));
    root.append(&footer);

    let revealer = gtk::Revealer::new();
    revealer.add_css_class("okp-empty-surface");
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Fill);
    // The idle surface is present before the window maps. A crossfade started in that
    // pre-map state can remain partially opaque on GTK's software renderer, dimming the
    // entire welcome canvas. Media loads already replace it atomically, so no transition
    // is preferable to an unreadable first frame.
    revealer.set_transition_duration(0);
    revealer.set_transition_type(gtk::RevealerTransitionType::None);
    revealer.set_reveal_child(true);
    revealer.set_child(Some(&root));

    let surface = EmptySurface {
        revealer,
        stack,
        welcome_host,
        history_host,
        footer,
        footer_left_icon,
        footer_left_label,
        footer_status,
        page: Rc::new(Cell::new(IdlePage::Welcome)),
        model: Rc::new(RefCell::new(None)),
        history_model: Rc::new(RefCell::new(None)),
        opened_context_bucket: Rc::new(Cell::new(None)),
        is_preview_substrate: Rc::new(Cell::new(false)),
    };
    let toggle_surface = surface.clone();
    let toggle_parent = window.clone();
    let toggle_state = Rc::clone(&state);
    let toggle_toast = Rc::clone(&status_toast);
    footer_left.connect_clicked(move |_| match toggle_surface.page.get() {
        IdlePage::Welcome => toggle_surface.show_history(
            &toggle_parent,
            Rc::clone(&toggle_state),
            Rc::clone(&toggle_toast),
        ),
        IdlePage::History => toggle_surface.show_welcome(),
    });
    surface.refresh(window, &state, Rc::clone(&status_toast));
    surface
}

fn idle_theme_is_dark() -> bool {
    match env::var("OKP_IDLE_THEME").ok().as_deref() {
        Some("light") => false,
        Some("dark") => true,
        _ => gtk::Settings::default()
            .map(|settings| settings.property::<bool>("gtk-application-prefer-dark-theme"))
            .unwrap_or(false),
    }
}

#[cfg(test)]
pub(crate) fn app_icon_path() -> Option<PathBuf> {
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
