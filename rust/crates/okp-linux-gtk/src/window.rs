use super::*;

#[cfg(test)]
pub(crate) fn parse_launch_args_from(args: impl Iterator<Item = std::ffi::OsString>) -> LaunchArgs {
    let cwd = env::current_dir().ok();
    parse_launch_args_from_cwd(args, cwd.as_deref())
}

pub(crate) fn parse_launch_args_from_cwd(
    args: impl Iterator<Item = std::ffi::OsString>,
    cwd: Option<&Path>,
) -> LaunchArgs {
    let args = args.collect::<Vec<_>>();
    let utf8_args = args
        .iter()
        .filter_map(|arg| arg.to_str())
        .collect::<Vec<_>>();
    let parsed = launch_args::parse(&utf8_args);
    let mut launch = LaunchArgs {
        directives: LaunchDirectives {
            resume_seconds: parsed.resume_seconds,
            subtitle: parsed.sub,
            audio: parsed.audio,
        },
        ..LaunchArgs::default()
    };

    for text in parsed.files {
        add_launch_text_arg(&mut launch, &text, cwd);
    }

    // Preserve non-UTF-8 POSIX paths. The portable parser intentionally handles strings only;
    // raw filesystem arguments still follow the existing path classification in the shell.
    for arg in args.into_iter().filter(|arg| arg.to_str().is_none()) {
        add_launch_path_arg(&mut launch, launch_path_arg(arg, cwd));
    }

    launch
}

pub(crate) fn add_launch_text_arg(launch: &mut LaunchArgs, text: &str, cwd: Option<&Path>) {
    // The reserved ok-player:// scheme is checked before the URL/path branches so a control
    // request is never mistaken for a stream URL and handed to the engine.
    if let Some(notice) = reserved_uri_notice(text) {
        if !launch.reserved_notices.contains(&notice) {
            launch.reserved_notices.push(notice);
        }
        return;
    }

    if media_formats::is_playable_url(Some(text)) {
        push_unique_playlist_item(&mut launch.items, PlaylistItem::Url(text.to_owned()));
        return;
    }

    if let Some(path) = file_uri_path(text) {
        add_launch_path_arg(launch, path);
        return;
    }

    add_launch_path_arg(launch, launch_path_arg(text.into(), cwd));
}

pub(crate) fn file_uri_path(text: &str) -> Option<PathBuf> {
    text.strip_prefix("file://")?;
    gtk::gio::File::for_uri(text).path()
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
    apply_gtk_theme_preview();
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
    window.set_icon_name(Some(LINUX_ICON_NAME));
    let window_bounds = track_player_window_bounds(&window);

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-root");
    if !playback_animations_enabled() {
        overlay.add_css_class("is-reduced-motion");
    }

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
    let window_chrome =
        build_player_window_chrome(&window, Rc::clone(&state), Rc::clone(&status_toast));
    sync_player_window_chrome_fullscreen(&window_chrome, &window);
    let empty_surface = build_empty_surface(&window, Rc::clone(&state), Rc::clone(&status_toast));
    let lyrics_surface = build_lyrics_surface();
    let media_state_overlay =
        MediaStateOverlay::new(&window, Rc::clone(&state), Rc::clone(&status_toast));
    chrome.set_child(&control_bar);
    for widget in window_chrome.auto_hide_widgets() {
        chrome.add_linked_motion_widget(widget);
    }
    for widget in window_chrome.persistent_widgets() {
        chrome.add_persistent_widget(widget);
    }
    chrome.add_linked_revealer(&controls.up_next_revealer);

    overlay.set_child(Some(&video_area));
    // Visual smoke hook: Xvfb cannot present the libmpv GL frame reliably on every
    // renderer, so deterministic bright/dark planes exercise chrome legibility while
    // real media remains loaded underneath to drive the production visibility state.
    if let Some(mode) = env::var_os("OKP_PLAYBACK_FRAME_PREVIEW") {
        let preview = gtk::DrawingArea::new();
        let color = if mode.eq_ignore_ascii_case("bright") {
            (0.957, 0.961, 0.965)
        } else if mode.eq_ignore_ascii_case("light") {
            (0.55, 0.59, 0.64)
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
    // The lyrics surface sits above the (audio-black) video plane but below the window chrome, the
    // OSC, and the side panel, so those stay on top and interactive while lyrics play underneath.
    overlay.add_overlay(lyrics_surface.widget());
    // Playback state sits above video/lyrics but below chrome and toasts. Only
    // the non-modal error card captures input; paused/loading leave the canvas usable.
    overlay.add_overlay(media_state_overlay.widget());
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
    connect_video_clicks(&video_area, &window, Rc::clone(&state));
    connect_player_context_menu(
        &overlay,
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
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
    // "Add files" affordance); `=chapters-empty` previews the no-duration empty
    // state; `=intervals` previews the metadata-less interval fallback and explicit
    // Detect chapters action; `=intervals-unavailable` previews its resolved no-engine
    // state; any other value previews embedded Chapters.
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
        } else if value.eq_ignore_ascii_case("intervals-unavailable") {
            let mut snapshot = side_panel_interval_preview_sample();
            snapshot.detection = chapter_math::ChapterDetection::Unavailable;
            (SidePanelMode::Chapters, snapshot)
        } else if value.eq_ignore_ascii_case("intervals") {
            (
                SidePanelMode::Chapters,
                side_panel_interval_preview_sample(),
            )
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
        empty_surface.set_preview_substrate(false);
        chrome.set_has_media(true);
        chrome.show_persistently();
        open_seek_preview(&controls);
    }
    let volume_preview = env::var_os("OKP_VOLUME_PREVIEW")
        .map(|mode| (controls.volume.clone(), mode.to_string_lossy().into_owned()));
    connect_state_poll(
        &window,
        Rc::clone(&state),
        controls,
        StatePollContext {
            updating_seek: Rc::clone(&updating_seek),
            chrome: Rc::clone(&chrome),
            window_chrome,
            subtitle_position_snapshot: Rc::new(Cell::new(None)),
            empty_surface: empty_surface.clone(),
            lyrics_surface,
            media_state_overlay,
            window_bounds,
            mpris_snapshot: Arc::clone(&mpris_controller.snapshot),
            mpris_signals: mpris_controller.signals.clone(),
        },
    );

    // Deterministic smoke hook for the load-time resize guard. The production
    // path reaches the same state through the caption button/window manager.
    if env::var_os("OKP_START_FULLSCREEN").is_some() {
        window.fullscreen();
    } else if env::var_os("OKP_START_MAXIMIZED").is_some() {
        window.maximize();
    }
    window.present();
    if env::var_os("OKP_OSD_PREVIEW_ON_STARTUP").is_some() {
        let preview_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            preview_toast.show("Volume 72%");
        });
    }
    if let Some((volume, mode)) = volume_preview {
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            volume.open_preview(&mode);
        });
    }
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
    // Visual smoke hook: render the in-player Media Information modal with
    // representative fixture data so it can be screenshot-tested without media.
    if env::var_os("OKP_OPEN_MEDIA_INFO_ON_STARTUP").is_some() {
        if let Some(substrate) = env::var_os("OKP_MEDIA_INFO_PREVIEW_SUBSTRATE") {
            empty_surface.set_preview_substrate(substrate.eq_ignore_ascii_case("bright"));
        }
        let info_parent = window.clone();
        let info_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            show_media_info_modal(&info_parent, &media_info_preview_from_env(), info_toast);
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

pub(crate) fn track_player_window_bounds(
    window: &gtk::ApplicationWindow,
) -> Rc<RefCell<Option<PlayerWindowBounds>>> {
    let bounds = Rc::new(RefCell::new(None));
    let realize_bounds = Rc::clone(&bounds);
    window.connect_realize(move |window| {
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        let compute_bounds = Rc::clone(&realize_bounds);
        toplevel.connect_compute_size(move |toplevel, size| {
            let (width, height) = size.bounds();
            if width > 0 && height > 0 {
                let monitor = toplevel.display().monitor_at_surface(toplevel);
                let work_area = monitor
                    .as_ref()
                    .map(|monitor| bounded_monitor_work_area(monitor.geometry(), width, height))
                    .unwrap_or(window_fit::WindowRect {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    });
                compute_bounds.replace(Some(PlayerWindowBounds { monitor, work_area }));
            }
        });
    });
    bounds
}

pub(crate) fn current_player_work_area(
    window: &gtk::ApplicationWindow,
    reported_bounds: &RefCell<Option<PlayerWindowBounds>>,
) -> Option<window_fit::WindowRect> {
    let current_monitor = window.surface().and_then(|surface| {
        let monitor = surface.display().monitor_at_surface(&surface)?;
        let geometry = monitor.geometry();
        (geometry.width() > 0 && geometry.height() > 0).then_some((
            monitor,
            window_fit::WindowRect {
                x: geometry.x(),
                y: geometry.y(),
                width: geometry.width(),
                height: geometry.height(),
            },
        ))
    });

    let reported = reported_bounds.borrow();
    match (reported.as_ref(), current_monitor) {
        (Some(bounds), Some((monitor, monitor_size)))
            if bounds.monitor.as_ref() == Some(&monitor) =>
        {
            Some(bounded_monitor_work_area(
                monitor.geometry(),
                bounds.work_area.width.min(monitor_size.width),
                bounds.work_area.height.min(monitor_size.height),
            ))
        }
        (_, Some((_, monitor_size))) => Some(monitor_size),
        (Some(bounds), None) => Some(bounds.work_area),
        (None, None) => None,
    }
}

fn bounded_monitor_work_area(
    geometry: gdk::Rectangle,
    bounds_width: i32,
    bounds_height: i32,
) -> window_fit::WindowRect {
    let width = bounds_width.min(geometry.width()).max(1);
    let height = bounds_height.min(geometry.height()).max(1);
    window_fit::WindowRect {
        // GDK's toplevel bounds expose only a size. Centering a smaller bound
        // within monitor geometry is the least-assumptive logical origin; on
        // Wayland Mutter remains authoritative for final placement.
        x: geometry.x() + (geometry.width() - width).max(0) / 2,
        y: geometry.y() + (geometry.height() - height).max(0) / 2,
        width,
        height,
    }
}

fn current_player_scale(window: &gtk::ApplicationWindow) -> f64 {
    window
        .surface()
        .map(|surface| {
            // GTK 4.14+ exposes fractional surface scale as a property. Keep
            // the crate's GTK 4.10 floor by discovering it dynamically, with
            // the integer ceiling as the older-runtime fallback.
            let scale = if surface.find_property("scale").is_some() {
                surface.property::<f64>("scale")
            } else {
                f64::from(surface.scale_factor())
            };
            if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            }
        })
        .unwrap_or(1.0)
}

pub(crate) fn fit_player_window_to_video(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    reported_bounds: &Rc<RefCell<Option<PlayerWindowBounds>>>,
    source_generation: u64,
    video: window_fit::WindowSize,
) {
    let debug = env::var_os("OKP_DEBUG_WINDOW_FIT").is_some();
    // Existing pixel-redline smokes intentionally hold the canonical 1120x680
    // viewport. The dedicated main-window fit smoke leaves this unset and
    // exercises the production behavior with real media dimensions.
    if env::var_os("OKP_FIXED_VIEWPORT_SMOKE").is_some() {
        return;
    }
    if window.is_fullscreen() || window.is_maximized() {
        if debug {
            eprintln!(
                "window fit skipped: fullscreen={} maximized={}",
                window.is_fullscreen(),
                window.is_maximized()
            );
        }
        return;
    }

    let Some(work_area) = current_player_work_area(window, reported_bounds) else {
        if debug {
            eprintln!("window fit skipped: workarea unavailable");
        }
        return;
    };
    let monitor_scale = current_player_scale(window);
    let Some(placement) = window_fit::fit_physical_video_to_work_area(
        video.width,
        video.height,
        monitor_scale,
        work_area,
    ) else {
        return;
    };

    if debug {
        eprintln!(
            "window fit request: video={}x{} scale={:.2} workarea={}x{}+{},{} target={}x{}+{},{}",
            video.width,
            video.height,
            monitor_scale,
            work_area.width,
            work_area.height,
            work_area.x,
            work_area.y,
            placement.size.width,
            placement.size.height,
            placement.position.x,
            placement.position.y,
        );
    }
    window.set_default_size(placement.size.width, placement.size.height);
    move_resize_player_window_on_x11(window, placement.position, placement.size);
    schedule_player_window_fit_settle(
        window,
        state,
        reported_bounds,
        source_generation,
        video,
        work_area,
        placement,
    );
}

fn schedule_player_window_fit_settle(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    reported_bounds: &Rc<RefCell<Option<PlayerWindowBounds>>>,
    source_generation: u64,
    video: window_fit::WindowSize,
    requested_work_area: window_fit::WindowRect,
    requested: window_fit::WindowPlacement,
) {
    let settled_window = window.clone();
    let settled_state = Rc::clone(state);
    let settled_bounds = Rc::clone(reported_bounds);
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        if !window_fit_generation_is_current(&settled_window, &settled_state, source_generation) {
            return;
        }
        let Some(current_work_area) = current_player_work_area(&settled_window, &settled_bounds)
        else {
            return;
        };
        let current_scale = current_player_scale(&settled_window);
        let retargeted = window_fit::smaller_fit_for_work_area(
            video.width,
            video.height,
            current_scale,
            current_work_area,
            requested,
        );
        if let Some(retargeted) = retargeted {
            settled_window.set_default_size(retargeted.size.width, retargeted.size.height);
            move_resize_player_window_on_x11(&settled_window, retargeted.position, retargeted.size);
            if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                eprintln!(
                    "window fit retargeted: previous-workarea={}x{}+{},{} current-workarea={}x{}+{},{} target={}x{}",
                    requested_work_area.width,
                    requested_work_area.height,
                    requested_work_area.x,
                    requested_work_area.y,
                    current_work_area.width,
                    current_work_area.height,
                    current_work_area.x,
                    current_work_area.y,
                    retargeted.size.width,
                    retargeted.size.height,
                );
            }
            finish_player_window_fit_after(
                settled_window,
                settled_state,
                settled_bounds,
                source_generation,
                video,
                retargeted,
                Duration::from_millis(300),
            );
        } else {
            finish_player_window_fit_after(
                settled_window,
                settled_state,
                settled_bounds,
                source_generation,
                video,
                requested,
                Duration::ZERO,
            );
        }
    });
}

fn finish_player_window_fit_after(
    window: gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    reported_bounds: Rc<RefCell<Option<PlayerWindowBounds>>>,
    source_generation: u64,
    video: window_fit::WindowSize,
    target: window_fit::WindowPlacement,
    delay: Duration,
) {
    glib::timeout_add_local_once(delay, move || {
        if !window_fit_generation_is_current(&window, &state, source_generation) {
            return;
        }
        let actual_width = window.width();
        let actual_height = window.height();
        let mut position_delay = Duration::ZERO;
        if actual_width <= target.size.width
            && actual_height <= target.size.height
            && let Some(corrected) = window_fit::fill_client(
                video.width,
                video.height,
                actual_width,
                actual_height,
                target.size.width,
                target.size.height,
            )
        {
            window.set_default_size(corrected.width, corrected.height);
            position_delay = Duration::from_millis(150);
        }
        glib::timeout_add_local_once(position_delay, move || {
            if !window_fit_generation_is_current(&window, &state, source_generation) {
                return;
            }
            let Some(work_area) = current_player_work_area(&window, &reported_bounds) else {
                return;
            };
            let final_size = window_fit::WindowSize {
                width: window.width(),
                height: window.height(),
            };
            let position = window_fit::centered_position(final_size, work_area);
            let moved = move_resize_player_window_on_x11(&window, position, final_size);
            if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                eprintln!(
                    "window fit settled: client={}x{} workarea={}x{}+{},{} target=+{},{} backend={}",
                    final_size.width,
                    final_size.height,
                    work_area.width,
                    work_area.height,
                    work_area.x,
                    work_area.y,
                    position.x,
                    position.y,
                    if moved { "x11" } else { "compositor" }
                );
            }
        });
    });
}

fn window_fit_generation_is_current(
    window: &gtk::ApplicationWindow,
    state: &RefCell<PlayerState>,
    source_generation: u64,
) -> bool {
    state.borrow().source_generation == source_generation
        && !window.is_fullscreen()
        && !window.is_maximized()
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

    pub(crate) fn auto_hide_widgets(&self) -> &[gtk::Widget] {
        &self.auto_hide_widgets
    }

    pub(crate) fn persistent_widgets(&self) -> &[gtk::Widget] {
        &self.persistent_widgets
    }
}

pub(crate) fn build_player_window_chrome(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> PlayerWindowChrome {
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

    let drag_zone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_zone.add_css_class("okp-window-drag-zone");
    drag_zone.set_hexpand(true);
    drag_zone.set_can_target(true);
    drag_zone.set_margin_start(14);

    let title_content = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    title_content.add_css_class("okp-top-chrome-motion");
    title_content.add_css_class("okp-window-title-content");
    let media_icon = canonical_brand_mark(20, 11, "okp-window-media-icon");
    media_icon.set_visible(false);
    let title_label = gtk::Label::new(None);
    title_label.add_css_class("okp-window-media-title");
    title_label.set_ellipsize(pango::EllipsizeMode::End);
    title_label.set_xalign(0.0);
    title_label.set_visible(false);
    title_content.append(&media_icon);
    title_content.append(&title_label);
    drag_zone.append(&title_content);
    connect_player_window_drag(&drag_zone, window);
    bar.append(&drag_zone);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-player-window-controls");
    controls.set_halign(gtk::Align::End);

    let settings = gtk::Button::from_icon_name("emblem-system-symbolic");
    settings.add_css_class("okp-player-window-control");
    settings.add_css_class("okp-player-settings-control");
    settings.set_has_frame(false);
    settings.set_tooltip_text(Some("Settings"));
    settings.update_property(&[gtk::accessible::Property::Label("Settings")]);
    settings.connect_has_focus_notify(|button| {
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() && button.has_focus() {
            eprintln!("interaction: player-settings-focus=true");
        }
    });
    let settings_window = window.clone();
    let settings_state = Rc::clone(&state);
    let settings_toast = Rc::clone(&status_toast);
    settings.connect_clicked(move |_| {
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: player-settings-clicked=true");
        }
        open_settings_window(
            &settings_window,
            Rc::clone(&settings_state),
            Rc::clone(&settings_toast),
        );
    });
    controls.append(&settings);

    let transient_controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    transient_controls.add_css_class("okp-top-chrome-motion");
    transient_controls.add_css_class("okp-player-transient-window-controls");

    let pin = gtk::Button::from_icon_name("view-pin-symbolic");
    pin.add_css_class("okp-player-window-control");
    pin.add_css_class("okp-player-window-pin");
    pin.set_has_frame(false);
    pin.set_tooltip_text(Some("Always on top"));
    let pin_window = window.clone();
    let pin_button = pin.clone();
    let pin_toast = Rc::clone(&status_toast);
    let pinned = Rc::new(Cell::new(false));
    let pin_state = Rc::clone(&pinned);
    pin.connect_clicked(move |_| {
        let enabled = !pin_state.get();
        match set_window_always_on_top(&pin_window, enabled) {
            AlwaysOnTopResult::Applied => {
                pin_state.set(enabled);
                if enabled {
                    pin_button.add_css_class("is-selected");
                    pin_button.set_tooltip_text(Some("Disable always on top"));
                } else {
                    pin_button.remove_css_class("is-selected");
                    pin_button.set_tooltip_text(Some("Always on top"));
                }
            }
            AlwaysOnTopResult::Unavailable => {
                pin_toast.show("Always on top is unavailable on this desktop");
            }
        }
    });
    transient_controls.append(&pin);

    let minimize = player_window_control(WindowControlKind::Minimize, "Minimize");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    transient_controls.append(&minimize);

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
    transient_controls.append(&maximize);

    let close = player_window_control(WindowControlKind::Close, "Close");
    close.add_css_class("okp-player-window-close");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    transient_controls.append(&close);
    controls.append(&transient_controls);

    bar.append(&controls);

    let scrim = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    scrim.add_css_class("okp-top-chrome-motion");
    scrim.add_css_class("okp-window-title-scrim");
    scrim.set_can_target(false);

    let chrome_surface = gtk::Overlay::new();
    chrome_surface.set_child(Some(&scrim));
    chrome_surface.add_overlay(&bar);
    revealer.set_child(Some(&chrome_surface));
    PlayerWindowChrome {
        revealer,
        auto_hide_widgets: vec![
            scrim.upcast(),
            title_content.upcast(),
            transient_controls.upcast(),
        ],
        persistent_widgets: vec![settings.upcast()],
        media_icon,
        title_label,
    }
}

#[repr(C)]
struct XDisplay {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
union XClientMessageData {
    longs: [libc::c_long; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XClientMessageEvent {
    type_: libc::c_int,
    serial: libc::c_ulong,
    send_event: libc::c_int,
    display: *mut XDisplay,
    window: libc::c_ulong,
    message_type: libc::c_ulong,
    format: libc::c_int,
    data: XClientMessageData,
}

#[repr(C)]
union XEvent {
    client_message: XClientMessageEvent,
    pad: [libc::c_long; 24],
}

unsafe extern "C" {
    fn gdk_x11_display_get_xdisplay(display: *mut gtk::gdk::ffi::GdkDisplay) -> *mut XDisplay;
    fn gdk_x11_surface_get_xid(surface: *mut gtk::gdk::ffi::GdkSurface) -> libc::c_ulong;
    fn XDefaultRootWindow(display: *mut XDisplay) -> libc::c_ulong;
    fn XInternAtom(
        display: *mut XDisplay,
        atom_name: *const libc::c_char,
        only_if_exists: libc::c_int,
    ) -> libc::c_ulong;
    fn XSendEvent(
        display: *mut XDisplay,
        window: libc::c_ulong,
        propagate: libc::c_int,
        event_mask: libc::c_long,
        event: *mut XEvent,
    ) -> libc::c_int;
    fn XMoveResizeWindow(
        display: *mut XDisplay,
        window: libc::c_ulong,
        x: libc::c_int,
        y: libc::c_int,
        width: libc::c_uint,
        height: libc::c_uint,
    ) -> libc::c_int;
    fn XFlush(display: *mut XDisplay) -> libc::c_int;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlwaysOnTopBackend {
    X11Ewmh,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlwaysOnTopResult {
    Applied,
    Unavailable,
}

pub(crate) fn always_on_top_backend(display_type_name: &str) -> AlwaysOnTopBackend {
    if display_type_name == "GdkX11Display" {
        AlwaysOnTopBackend::X11Ewmh
    } else {
        AlwaysOnTopBackend::Unavailable
    }
}

/// Center the fitted window where X11 permits explicit toplevel positioning.
/// Wayland intentionally exposes no client-controlled global coordinates, so
/// Mutter remains responsible for keeping the already-bounded surface visible.
fn move_resize_player_window_on_x11(
    window: &gtk::ApplicationWindow,
    position: window_fit::WindowPoint,
    size: window_fit::WindowSize,
) -> bool {
    use gtk::glib::translate::ToGlibPtr;

    let Some(display) = gdk::Display::default() else {
        return false;
    };
    if display.type_().name() != "GdkX11Display" {
        return false;
    }
    let Some(surface) = window.surface() else {
        return false;
    };
    let scale_factor = surface.scale_factor().max(1);
    let x = position.x.saturating_mul(scale_factor);
    let y = position.y.saturating_mul(scale_factor);

    unsafe {
        let xdisplay = gdk_x11_display_get_xdisplay(display.to_glib_none().0);
        if xdisplay.is_null() {
            return false;
        }
        let xid = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        if xid == 0 {
            return false;
        }
        let move_resize = XInternAtom(xdisplay, c"_NET_MOVERESIZE_WINDOW".as_ptr(), 0);
        let moved = if move_resize != 0 {
            let client_message = XClientMessageEvent {
                type_: 33,
                serial: 0,
                send_event: 1,
                display: xdisplay,
                window: xid,
                message_type: move_resize,
                format: 32,
                // EWMH: x/y/width/height are present (bits 8-11), source is a
                // normal application (1 in bits 12-15).
                data: XClientMessageData {
                    longs: [
                        (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11) | (1 << 12),
                        libc::c_long::from(x),
                        libc::c_long::from(y),
                        libc::c_long::from(size.width),
                        libc::c_long::from(size.height),
                    ],
                },
            };
            let mut event = XEvent { client_message };
            let root = XDefaultRootWindow(xdisplay);
            let mask = (1 << 20) | (1 << 19);
            XSendEvent(xdisplay, root, 0, mask, &mut event)
        } else {
            XMoveResizeWindow(
                xdisplay,
                xid,
                x,
                y,
                size.width.max(1) as libc::c_uint,
                size.height.max(1) as libc::c_uint,
            )
        };
        XFlush(xdisplay);
        moved != 0
    }
}

/// Ask an EWMH X11 window manager to add/remove `_NET_WM_STATE_ABOVE`.
/// Wayland returns [`AlwaysOnTopResult::Unavailable`]: GTK exposes no portable
/// protocol for clients to force this state, and modal/transient flags are not
/// substitutes.
pub(crate) fn set_window_always_on_top(
    window: &gtk::ApplicationWindow,
    enabled: bool,
) -> AlwaysOnTopResult {
    use gtk::glib::translate::ToGlibPtr;

    let Some(display) = gdk::Display::default() else {
        return AlwaysOnTopResult::Unavailable;
    };
    if always_on_top_backend(display.type_().name()) != AlwaysOnTopBackend::X11Ewmh {
        return AlwaysOnTopResult::Unavailable;
    }
    let Some(surface) = window.surface() else {
        return AlwaysOnTopResult::Unavailable;
    };

    let state_name = c"_NET_WM_STATE";
    let above_name = c"_NET_WM_STATE_ABOVE";
    unsafe {
        let xdisplay = gdk_x11_display_get_xdisplay(display.to_glib_none().0);
        if xdisplay.is_null() {
            return AlwaysOnTopResult::Unavailable;
        }
        let xid = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        let message_type = XInternAtom(xdisplay, state_name.as_ptr(), 0);
        let above = XInternAtom(xdisplay, above_name.as_ptr(), 0);
        if xid == 0 || message_type == 0 || above == 0 {
            return AlwaysOnTopResult::Unavailable;
        }

        let client_message = XClientMessageEvent {
            type_: 33,
            serial: 0,
            send_event: 1,
            display: xdisplay,
            window: xid,
            message_type,
            format: 32,
            data: XClientMessageData {
                longs: [if enabled { 1 } else { 0 }, above as libc::c_long, 0, 1, 0],
            },
        };
        let mut event = XEvent { client_message };
        let root = XDefaultRootWindow(xdisplay);
        let mask = (1 << 20) | (1 << 19);
        let sent = XSendEvent(xdisplay, root, 0, mask, &mut event);
        XFlush(xdisplay);
        if sent != 0 {
            AlwaysOnTopResult::Applied
        } else {
            AlwaysOnTopResult::Unavailable
        }
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
    let animations_enabled = playback_animations_enabled();
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
        canvas: root,
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
        welcome_history_button: Rc::new(RefCell::new(None)),
        is_preview_substrate: Rc::new(Cell::new(false)),
    };
    // FlowBox can paint the margin-sized arrow column beyond its pointer allocation. Keep the
    // native button for keyboard/accessibility activation, and route pointer presses through the
    // window using the button's live bounds so wrapped narrow layouts remain correct.
    let history_arrow_click = gtk::GestureClick::new();
    history_arrow_click.set_button(gdk::BUTTON_PRIMARY);
    history_arrow_click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let history_arrow_surface = surface.clone();
    let history_arrow_parent = window.clone();
    let history_arrow_state = Rc::clone(&state);
    let history_arrow_toast = Rc::clone(&status_toast);
    history_arrow_click.connect_pressed(move |_, _, x, y| {
        if history_arrow_surface.page.get() != IdlePage::Welcome
            || !history_arrow_surface.revealer.can_target()
        {
            return;
        }
        let history_button = history_arrow_surface.welcome_history_button.borrow();
        let Some(history_button) = history_button.as_ref() else {
            return;
        };
        let Some(bounds) = history_button.compute_bounds(&history_arrow_parent) else {
            return;
        };
        if x >= f64::from(bounds.x())
            && x <= f64::from(bounds.x() + bounds.width())
            && y >= f64::from(bounds.y())
            && y <= f64::from(bounds.y() + bounds.height())
        {
            history_arrow_surface.show_history(
                &history_arrow_parent,
                Rc::clone(&history_arrow_state),
                Rc::clone(&history_arrow_toast),
            );
        }
    });
    window.add_controller(history_arrow_click);
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

pub(crate) fn playback_animations_enabled() -> bool {
    env::var_os("OKP_REDUCED_MOTION").is_none()
        && gtk::Settings::default()
            .map(|settings| settings.property::<bool>("gtk-enable-animations"))
            .unwrap_or(true)
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

fn apply_gtk_theme_preview() {
    // Drive the same GtkSettings property GNOME uses so visual smoke exercises
    // production theme resolution instead of only swapping app CSS classes.
    let prefer_dark = match env::var("OKP_GTK_THEME_PREVIEW").ok().as_deref() {
        Some("light") => Some(false),
        Some("dark") => Some(true),
        _ => None,
    };
    if let (Some(settings), Some(prefer_dark)) = (gtk::Settings::default(), prefer_dark) {
        settings.set_gtk_application_prefer_dark_theme(prefer_dark);
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
