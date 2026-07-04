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
    let lyrics_surface = build_lyrics_surface();
    chrome.set_child(&control_bar);
    chrome.add_linked_revealer(&window_chrome);
    chrome.add_linked_revealer(&controls.up_next_revealer);

    overlay.set_child(Some(&video_area));
    overlay.add_overlay(empty_surface.widget());
    // The lyrics surface sits above the (audio-black) video plane but below the window chrome, the
    // OSC, and the side panel, so those stay on top and interactive while lyrics play underneath.
    overlay.add_overlay(lyrics_surface.widget());
    overlay.add_overlay(&window_chrome);
    overlay.add_overlay(chrome.widget());
    overlay.add_overlay(&controls.up_next_revealer);
    overlay.add_overlay(status_toast.widget());
    for resize_handle in build_player_resize_handles(&window) {
        overlay.add_overlay(&resize_handle);
    }
    window.set_child(Some(&overlay));
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
            empty_surface,
            lyrics_surface,
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

    EmptySurface { revealer, panel }
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
