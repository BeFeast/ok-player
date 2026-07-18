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
    let defer_initial_map = launch_args.has_media_payload()
        && env::var_os("OKP_START_FULLSCREEN").is_none()
        && env::var_os("OKP_START_MAXIMIZED").is_none();
    let initial_map_pending = Rc::new(Cell::new(defer_initial_map));
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
    if env::var_os("OKP_NARROW_COMMAND_PREVIEW").is_some() {
        window.set_default_size(480, 270);
    }
    window.add_css_class("okp-player-window");
    apply_compact_accessibility_classes(&window);
    window.set_icon_name(Some(LINUX_ICON_NAME));
    let aspect_resize_state = AspectResizeState::new();
    let window_bounds = track_player_window_bounds(&window, &aspect_resize_state);
    connect_aspect_resize_shift(&window, &aspect_resize_state);

    let video_host = VideoHost::for_display(&gtk::prelude::WidgetExt::display(&window));
    if video_host.is_native() {
        window.add_css_class("okp-native-video");
    }

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-root");
    if video_host.is_native() {
        overlay.add_css_class("okp-native-video");
    }
    if !playback_animations_enabled() {
        overlay.add_css_class("is-reduced-motion");
    }

    let controls = build_controls(
        &window,
        Rc::clone(&window_bounds),
        Rc::clone(&state),
        Rc::clone(&updating_seek),
        Rc::clone(&updating_volume),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    let control_bar = controls_bar(&controls);
    let window_chrome = build_player_window_chrome(&window, Rc::clone(&status_toast));
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

    overlay.set_child(Some(video_host.widget()));
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
        let compact_preview = env::var_os("OKP_START_COMPACT").is_some();
        let narrow_command_preview = env::var_os("OKP_NARROW_COMMAND_PREVIEW").is_some();
        if !compact_preview && !narrow_command_preview {
            preview.set_size_request(1120, 680);
        }
        preview.set_can_target(false);
        overlay.add_overlay(&preview);
        overlay.set_measure_overlay(&preview, !compact_preview);
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
    let resize_handles = build_player_resize_handles(&window, &aspect_resize_state);
    let compact_mode = CompactMode::build(
        &window,
        CompactModeInputs {
            state: Rc::clone(&state),
            status_toast: Rc::clone(&status_toast),
            chrome: &chrome,
            window_chrome: &window_chrome,
            controls: &controls,
            empty_surface: &empty_surface,
            resize_handles: resize_handles.clone(),
        },
    );
    for compact_overlay in compact_mode.overlays() {
        overlay.add_overlay(compact_overlay);
    }
    overlay.add_overlay(status_toast.widget());
    for resize_handle in resize_handles {
        overlay.add_overlay(&resize_handle);
    }
    window.set_child(Some(&overlay));
    chrome.add_cursor_widget(&overlay);
    connect_chrome_activity(&overlay, Rc::clone(&chrome));

    let launch_reserved_notice = launch_args.reserved_notice().map(str::to_owned);
    state.borrow_mut().presentation_recorder = PresentationRecorder::from_env(video_host.backend());
    if env::var_os("OKP_PRESENT_EXERCISE").is_some() {
        state.borrow_mut().presentation_exercise = Some(Default::default());
    }
    connect_mpv(&video_host, Rc::clone(&state), launch_args);
    // Keep the fullscreen intent aligned with the compositor's authoritative
    // state so toggles driven outside the double-click path (Escape, a
    // window-manager shortcut) leave the next double-click pointing the right
    // way. See [`fullscreen_toggle`].
    {
        let notify_state = Rc::clone(&state);
        window.connect_notify_local(Some("fullscreened"), move |window, _| {
            notify_state
                .borrow_mut()
                .fullscreen_toggle
                .observe(window.is_fullscreen());
        });
    }
    connect_video_clicks(video_host.widget(), &window, Rc::clone(&state));
    connect_compact_video_interactions(
        video_host.widget(),
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    );
    connect_player_context_menu(
        &overlay,
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
        PlayerCommandReach {
            screenshot: controls.screenshot_button.clone(),
            fullscreen: controls.fullscreen_button.clone(),
            chapters: controls.chapters_button.clone(),
            window_bounds: Rc::clone(&window_bounds),
        },
    );
    connect_player_window_move(&overlay, &window);
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
    let more_popover_preview = env::var_os("OKP_OPEN_MORE_POPOVER_ON_STARTUP").map(|_| {
        chrome.set_has_media(true);
        chrome.show_persistently();
        controls.more_button.clone()
    });
    let volume_preview = env::var_os("OKP_VOLUME_PREVIEW")
        .map(|mode| (controls.volume.clone(), mode.to_string_lossy().into_owned()));
    connect_state_poll(
        &window,
        Rc::clone(&state),
        controls,
        StatePollContext {
            updating_seek: Rc::clone(&updating_seek),
            initial_map_pending: Rc::clone(&initial_map_pending),
            chrome: Rc::clone(&chrome),
            compact_mode,
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
    if defer_initial_map {
        gtk::prelude::WidgetExt::realize(&window);
        // Realizing a toplevel does not guarantee that its child DrawingArea is
        // realized before the first map. The native mpv bridge is connected to
        // the video host's realize signal, so realize it explicitly while the
        // root Wayland surface already exists. That starts the normal libmpv
        // load/event path early enough to obtain video dimensions for the
        // initial fit without first showing a default-sized window.
        video_host.realize_video_surface();
        let fallback_window = window.clone();
        let fallback_pending = Rc::clone(&initial_map_pending);
        // The real 4K HEVC acceptance file publishes its first usable video
        // dimensions at about 2.2 seconds on the target host. Mapping at 1.5s
        // raced that event and defeated the pre-map fit. Local media normally
        // maps as soon as dimensions arrive; five seconds is only the bounded
        // escape hatch for failed, slow network, or metadata-less loads.
        glib::timeout_add_local_once(Duration::from_secs(5), move || {
            if fallback_pending.get() {
                if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                    eprintln!("window fit fallback: presenting without dimensions");
                }
                fallback_window.present();
            }
        });
    } else {
        window.present();
    }
    if let Some(more_button) = more_popover_preview {
        // Test-only mapped-anchor poll, symmetric with the volume preview below.
        // Media launches may defer the initial map while dimensions settle, and
        // GtkMenuButton::popup is a no-op until its anchor is mapped.
        let preview_attempts = Rc::new(Cell::new(0_u8));
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if more_button.is_mapped() {
                more_button.popup();
                return glib::ControlFlow::Break;
            }
            let attempts = preview_attempts.get().saturating_add(1);
            preview_attempts.set(attempts);
            if attempts >= 50 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    if env::var_os("OKP_OSD_PREVIEW_ON_STARTUP").is_some() {
        let preview_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            preview_toast.show("Volume 72%");
        });
    }
    if let Some((volume, mode)) = volume_preview {
        // Media launches can deliberately defer the first map until video
        // dimensions are available. Calling GtkPopover::popup before its
        // anchor is mapped is a no-op, which made the deterministic volume
        // captures compare two closed states. Wait for the real anchor with a
        // bounded test-only poll instead of relying on launch timing.
        let preview_anchor = volume.widget().clone();
        let preview_attempts = Rc::new(Cell::new(0_u8));
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if preview_anchor.is_mapped() {
                volume.open_preview(&mode);
                return glib::ControlFlow::Break;
            }
            let attempts = preview_attempts.get().saturating_add(1);
            preview_attempts.set(attempts);
            if attempts >= 50 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
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
    // Deterministic visual smoke hook for the active-query and matching-cue
    // states. The production entry loads and indexes on its background path;
    // this hook supplies an in-memory fixture so screenshot tests do not depend
    // on a desktop file chooser or launch timing.
    if env::var_os("OKP_OPEN_SUBTITLE_SEARCH_ON_STARTUP").is_some() {
        let search_parent = window.clone();
        let search_state = Rc::clone(&state);
        let search_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            open_subtitle_search_preview(&search_parent, search_state, search_toast);
        });
    }
    // Visual smoke hook: render the Media Information companion window with
    // representative fixture data so it can be screenshot-tested without media.
    if env::var_os("OKP_OPEN_MEDIA_INFO_ON_STARTUP").is_some() {
        if let Some(substrate) = env::var_os("OKP_MEDIA_INFO_PREVIEW_SUBSTRATE") {
            empty_surface.set_preview_substrate(substrate.eq_ignore_ascii_case("bright"));
        }
        let info_parent = window.clone();
        let info_state = Rc::clone(&state);
        let info_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            show_media_info_window(
                &info_parent,
                &info_state,
                &media_info_preview_from_env(),
                info_toast,
            );
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

#[derive(Clone)]
pub(crate) enum VideoHost {
    Native {
        area: gtk::DrawingArea,
        container: gtk::Stack,
        auto_fallback: bool,
    },
    Gtk(gtk::GLArea),
}

impl VideoHost {
    fn for_display(display: &gdk::Display) -> Self {
        let requested = env::var("OKP_VIDEO_BACKEND").unwrap_or_else(|_| "auto".to_owned());
        let wayland = is_wayland_display(display.type_().name());
        let (use_native, auto_fallback) = match requested.as_str() {
            "gtk" | "gtk-glarea" => (false, false),
            "native" | "native-wayland-egl" if wayland => (true, false),
            "native" | "native-wayland-egl" => {
                eprintln!(
                    "Native Wayland/EGL video was requested on a non-Wayland display; using GtkGLArea"
                );
                (false, false)
            }
            "auto" | "" => (wayland, wayland),
            value => {
                eprintln!("Unknown OKP_VIDEO_BACKEND={value}; using the automatic backend");
                (wayland, wayland)
            }
        };

        if use_native {
            let area = gtk::DrawingArea::new();
            area.set_hexpand(true);
            area.set_vexpand(true);
            area.add_css_class("okp-video-plane");
            area.add_css_class("okp-native-video");
            let container = gtk::Stack::new();
            container.set_hexpand(true);
            container.set_vexpand(true);
            container.add_named(&area, Some("native-wayland-egl"));
            Self::Native {
                area,
                container,
                auto_fallback,
            }
        } else {
            Self::Gtk(gtk_video_area())
        }
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        match self {
            Self::Native { container, .. } => container.upcast_ref(),
            Self::Gtk(area) => area.upcast_ref(),
        }
    }

    pub(crate) fn realize_video_surface(&self) {
        match self {
            Self::Native { area, .. } => gtk::prelude::WidgetExt::realize(area),
            Self::Gtk(area) => gtk::prelude::WidgetExt::realize(area),
        }
    }

    pub(crate) fn is_native(&self) -> bool {
        matches!(self, Self::Native { .. })
    }

    pub(crate) fn backend(&self) -> okp_core::presentation_evidence::PresentationBackend {
        match self {
            Self::Native { .. } => {
                okp_core::presentation_evidence::PresentationBackend::NativeWaylandEgl
            }
            Self::Gtk(_) => okp_core::presentation_evidence::PresentationBackend::GtkGlArea,
        }
    }
}

pub(crate) fn gtk_video_area() -> gtk::GLArea {
    let area = gtk::GLArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_auto_render(false);
    area.set_required_version(3, 2);
    area.add_css_class("okp-video-plane");
    area
}

pub(crate) fn track_player_window_bounds(
    window: &gtk::ApplicationWindow,
    aspect_resize_state: &AspectResizeState,
) -> Rc<RefCell<Option<PlayerWindowBounds>>> {
    let bounds = Rc::new(RefCell::new(None));
    let realize_bounds = Rc::clone(&bounds);
    let realize_resize = aspect_resize_state.clone();
    window.connect_realize(move |window| {
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        let compute_bounds = Rc::clone(&realize_bounds);
        let compute_resize = realize_resize.clone();
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
                compute_resize.work_area.set(Some(work_area));
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
    deferred_launch_fit: bool,
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
    let was_mapped = window.is_mapped();
    window.set_default_size(placement.size.width, placement.size.height);
    move_resize_player_window_on_x11(window, placement.position, placement.size);
    // A launch with media is deliberately fitted before its first Wayland map.
    // Sampling the surface monitor immediately after that map observes GNOME's
    // transient placement negotiation (on the dual-monitor QA host it reports
    // the laptop twice before settling on the primary display) and can shrink a
    // correctly requested primary-monitor size. Keep the pre-map request,
    // complete that same transaction on the first map, and also guard against
    // Mutter's `auto-maximize` policy: a near-workarea 4K fit can be maximized
    // shortly after mapping even though the app never requested that state.
    // Explicit fullscreen/maximized launches never take this path.
    if !was_mapped {
        if debug {
            eprintln!("window fit launch: keeping initial compositor request mapped={was_mapped}");
        }
        if deferred_launch_fit {
            settle_deferred_launch_fit_on_map(
                window,
                state,
                reported_bounds,
                source_generation,
                video,
                work_area,
                placement,
            );
            restore_deferred_launch_fit_after_compositor(
                window,
                state,
                source_generation,
                placement,
            );
        }
        return;
    }
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

pub(crate) fn fit_player_window_to_current_media(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    reported_bounds: &Rc<RefCell<Option<PlayerWindowBounds>>>,
    status_toast: &StatusToast,
) {
    let (source_generation, video) = {
        let state = state.borrow();
        let Some(dimensions) = state.current_video_dimensions else {
            status_toast.show("Media dimensions are not available");
            return;
        };
        (
            state.source_generation,
            window_fit::WindowSize {
                width: dimensions.width,
                height: dimensions.height,
            },
        )
    };

    let compact = window_compact_mode_active(window);
    match window_fit::explicit_window_fit_action(
        true,
        window.is_fullscreen(),
        window.is_maximized() || compact,
    ) {
        window_fit::ExplicitWindowFitAction::Disabled => {
            status_toast.show("Media dimensions are not available");
        }
        window_fit::ExplicitWindowFitAction::FitWindowed => {
            fit_player_window_to_video(
                window,
                state,
                reported_bounds,
                source_generation,
                video,
                false,
            );
            status_toast.show("Window fitted to media");
        }
        window_fit::ExplicitWindowFitAction::RestoreWindowedAndFit => {
            state.borrow_mut().fullscreen_toggle.observe(false);
            window.unfullscreen();
            window.unmaximize();
            restore_compact_mode(window);
            schedule_explicit_player_window_fit(
                window,
                state,
                reported_bounds,
                source_generation,
                video,
            );
            status_toast.show("Restoring window and fitting to media");
        }
    }
}

fn schedule_explicit_player_window_fit(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    reported_bounds: &Rc<RefCell<Option<PlayerWindowBounds>>>,
    source_generation: u64,
    video: window_fit::WindowSize,
) {
    let window = window.clone();
    let state = Rc::clone(state);
    let reported_bounds = Rc::clone(reported_bounds);
    let attempts = Rc::new(Cell::new(0_u8));
    glib::timeout_add_local(Duration::from_millis(50), move || {
        if state.borrow().source_generation != source_generation {
            return glib::ControlFlow::Break;
        }

        if window.is_fullscreen() {
            window.unfullscreen();
        }
        if window.is_maximized() {
            window.unmaximize();
        }
        if window.is_fullscreen() || window.is_maximized() {
            let next = attempts.get().saturating_add(1);
            attempts.set(next);
            return if next < 40 {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            };
        }

        fit_player_window_to_video(
            &window,
            &state,
            &reported_bounds,
            source_generation,
            video,
            false,
        );
        glib::ControlFlow::Break
    });
}

fn settle_deferred_launch_fit_on_map(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    reported_bounds: &Rc<RefCell<Option<PlayerWindowBounds>>>,
    source_generation: u64,
    video: window_fit::WindowSize,
    requested_work_area: window_fit::WindowRect,
    requested: window_fit::WindowPlacement,
) {
    let mapped_state = Rc::clone(state);
    let mapped_bounds = Rc::clone(reported_bounds);
    let pending = Rc::new(Cell::new(true));
    window.connect_map(move |mapped_window| {
        if !pending.replace(false)
            || !window_fit_generation_is_current(mapped_window, &mapped_state, source_generation)
        {
            return;
        }

        // `GtkWindow::present` may restore the builder's default geometry when
        // a realized launch is mapped after the video dimensions arrive. This
        // is still the original load-time fit transaction: re-issue it once on
        // the mapped toplevel, then use the normal monitor-aware settle path.
        mapped_window.set_default_size(requested.size.width, requested.size.height);
        move_resize_player_window_on_x11(mapped_window, requested.position, requested.size);
        if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
            eprintln!(
                "window fit mapped launch: target={}x{}+{},{}",
                requested.size.width,
                requested.size.height,
                requested.position.x,
                requested.position.y,
            );
        }
        schedule_player_window_fit_settle(
            mapped_window,
            &mapped_state,
            &mapped_bounds,
            source_generation,
            video,
            requested_work_area,
            requested,
        );
    });
}

fn restore_deferred_launch_fit_after_compositor(
    window: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    source_generation: u64,
    target: window_fit::WindowPlacement,
) {
    let window = window.clone();
    let state = Rc::clone(state);
    glib::timeout_add_local_once(Duration::from_millis(1_200), move || {
        if state.borrow().source_generation != source_generation || window.is_fullscreen() {
            return;
        }
        if !window.is_maximized() {
            return;
        }

        window.unmaximize();
        window.set_default_size(target.size.width, target.size.height);
        if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
            eprintln!(
                "window fit restored after compositor auto-maximize: target={}x{}+{},{}",
                target.size.width, target.size.height, target.position.x, target.position.y,
            );
        }
    });
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
        let Some(first_work_area) = current_player_work_area(&settled_window, &settled_bounds)
        else {
            return;
        };
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if !window_fit_generation_is_current(&settled_window, &settled_state, source_generation)
            {
                return;
            }
            let Some(confirmed_work_area) =
                current_player_work_area(&settled_window, &settled_bounds)
            else {
                return;
            };
            let confirmed_scale = current_player_scale(&settled_window);
            let Some(confirmed) = window_fit::fit_physical_video_to_work_area(
                video.width,
                video.height,
                confirmed_scale,
                confirmed_work_area,
            ) else {
                return;
            };
            let changed = confirmed.size != requested.size;
            if changed {
                settled_window.set_default_size(confirmed.size.width, confirmed.size.height);
                move_resize_player_window_on_x11(
                    &settled_window,
                    confirmed.position,
                    confirmed.size,
                );
            }
            if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                eprintln!(
                    "window fit confirmed: requested-workarea={}x{}+{},{} first-workarea={}x{}+{},{} confirmed-workarea={}x{}+{},{} target={}x{} changed={}",
                    requested_work_area.width,
                    requested_work_area.height,
                    requested_work_area.x,
                    requested_work_area.y,
                    first_work_area.width,
                    first_work_area.height,
                    first_work_area.x,
                    first_work_area.y,
                    confirmed_work_area.width,
                    confirmed_work_area.height,
                    confirmed_work_area.x,
                    confirmed_work_area.y,
                    confirmed.size.width,
                    confirmed.size.height,
                    changed,
                );
            }
            finish_player_window_fit_after(
                settled_window,
                settled_state,
                settled_bounds,
                source_generation,
                video,
                confirmed,
                if changed {
                    Duration::from_millis(300)
                } else {
                    Duration::ZERO
                },
            );
        });
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

    pub(crate) fn always_on_top_state(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.always_on_top)
    }
}

pub(crate) fn build_player_window_chrome(
    window: &gtk::ApplicationWindow,
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
        persistent_widgets: Vec::new(),
        media_icon,
        title_label,
        always_on_top: pinned,
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
    fn XResizeWindow(
        display: *mut XDisplay,
        window: libc::c_ulong,
        width: libc::c_uint,
        height: libc::c_uint,
    ) -> libc::c_int;
    fn XTranslateCoordinates(
        display: *mut XDisplay,
        src_window: libc::c_ulong,
        dest_window: libc::c_ulong,
        src_x: libc::c_int,
        src_y: libc::c_int,
        dest_x: *mut libc::c_int,
        dest_y: *mut libc::c_int,
        child: *mut libc::c_ulong,
    ) -> libc::c_int;
    fn XQueryPointer(
        display: *mut XDisplay,
        window: libc::c_ulong,
        root_return: *mut libc::c_ulong,
        child_return: *mut libc::c_ulong,
        root_x: *mut libc::c_int,
        root_y: *mut libc::c_int,
        win_x: *mut libc::c_int,
        win_y: *mut libc::c_int,
        mask_return: *mut libc::c_uint,
    ) -> libc::c_int;
    fn XFlush(display: *mut XDisplay) -> libc::c_int;
}

/// Resize a mapped player toplevel where X11 permits a direct request. Wayland
/// uses the matching `GtkWindow::set_default_size` request made by the caller.
pub(crate) fn resize_player_window_on_x11(
    window: &gtk::ApplicationWindow,
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
    unsafe {
        let xdisplay = gdk_x11_display_get_xdisplay(display.to_glib_none().0);
        if xdisplay.is_null() {
            return false;
        }
        let xid = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        if xid == 0 {
            return false;
        }
        let resized = XResizeWindow(
            xdisplay,
            xid,
            size.width.max(1) as libc::c_uint,
            size.height.max(1) as libc::c_uint,
        );
        XFlush(xdisplay);
        resized != 0
    }
}

pub(crate) fn current_player_position_on_x11(
    window: &gtk::ApplicationWindow,
) -> Option<window_fit::WindowPoint> {
    use gtk::glib::translate::ToGlibPtr;

    let display = gdk::Display::default()?;
    if display.type_().name() != "GdkX11Display" {
        return None;
    }
    let surface = window.surface()?;
    let scale = surface.scale_factor().max(1);
    unsafe {
        let xdisplay = gdk_x11_display_get_xdisplay(display.to_glib_none().0);
        if xdisplay.is_null() {
            return None;
        }
        let xid = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        let root = XDefaultRootWindow(xdisplay);
        let mut x = 0;
        let mut y = 0;
        let mut child = 0;
        (xid != 0
            && XTranslateCoordinates(xdisplay, xid, root, 0, 0, &mut x, &mut y, &mut child) != 0)
            .then_some(window_fit::WindowPoint {
                x: x / scale,
                y: y / scale,
            })
    }
}

pub(crate) fn current_pointer_position_on_x11(
    window: &gtk::ApplicationWindow,
) -> Option<window_fit::WindowPoint> {
    use gtk::glib::translate::ToGlibPtr;

    let display = gdk::Display::default()?;
    if display.type_().name() != "GdkX11Display" {
        return None;
    }
    let surface = window.surface()?;
    let scale = surface.scale_factor().max(1);
    unsafe {
        let xdisplay = gdk_x11_display_get_xdisplay(display.to_glib_none().0);
        if xdisplay.is_null() {
            return None;
        }
        let xid = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        let mut root = 0;
        let mut child = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;
        (xid != 0
            && XQueryPointer(
                xdisplay,
                xid,
                &mut root,
                &mut child,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            ) != 0)
            .then_some(window_fit::WindowPoint {
                x: root_x / scale,
                y: root_y / scale,
            })
    }
}

pub(crate) fn primary_pointer_down_on_x11(window: &gtk::ApplicationWindow) -> Option<bool> {
    use gtk::glib::translate::ToGlibPtr;

    let display = gdk::Display::default()?;
    if display.type_().name() != "GdkX11Display" {
        return None;
    }
    let surface = window.surface()?;
    unsafe {
        let xdisplay = gdk_x11_display_get_xdisplay(display.to_glib_none().0);
        if xdisplay.is_null() {
            return None;
        }
        let xid = gdk_x11_surface_get_xid(surface.to_glib_none().0);
        let mut root = 0;
        let mut child = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;
        (xid != 0
            && XQueryPointer(
                xdisplay,
                xid,
                &mut root,
                &mut child,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            ) != 0)
            .then_some(mask & (1 << 8) != 0)
    }
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
pub(crate) fn move_resize_player_window_on_x11(
    window: &impl IsA<gtk::Window>,
    position: window_fit::WindowPoint,
    size: window_fit::WindowSize,
) -> bool {
    use gtk::glib::translate::ToGlibPtr;

    let window = window.upcast_ref::<gtk::Window>();
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

/// Smallest client the interactive resize will settle on, keeping the OSC
/// transport usable on either orientation. The core clamp grows the derived axis
/// as needed to keep both dimensions above this floor while holding the aspect.
const MIN_RESIZE_CLIENT: window_fit::WindowSize = window_fit::WindowSize {
    width: 320,
    height: 180,
};

/// Shared live state for the Shift-locked interactive resize (issue #331).
///
/// One drag session at a time is created by a resize handle and resolved from
/// its pointer updates; `shift_held` is tracked by a window-level modifier
/// controller so pressing or releasing either Shift key mid-drag is
/// deterministic. All geometry/state decisions live in `okp_core::aspect_resize`;
/// this only shuttles observations in and applies the answer.
#[derive(Clone)]
pub(crate) struct AspectResizeState {
    session: Rc<RefCell<Option<aspect_resize::AspectResize>>>,
    shift_held: Rc<Cell<bool>>,
    work_area: Rc<Cell<Option<window_fit::WindowRect>>>,
    start_position_x11: Rc<Cell<Option<window_fit::WindowPoint>>>,
    last_requested: Rc<Cell<Option<window_fit::WindowSize>>>,
}

impl AspectResizeState {
    pub(crate) fn new() -> Self {
        Self {
            session: Rc::new(RefCell::new(None)),
            shift_held: Rc::new(Cell::new(false)),
            work_area: Rc::new(Cell::new(None)),
            start_position_x11: Rc::new(Cell::new(None)),
            last_requested: Rc::new(Cell::new(None)),
        }
    }
}

fn current_client_size(window: &gtk::ApplicationWindow) -> window_fit::WindowSize {
    window_fit::WindowSize {
        width: window.width().max(1),
        height: window.height().max(1),
    }
}

/// GTK reports the compositor's logical toplevel bounds through `compute-size`;
/// on GNOME those bounds are the usable workarea rather than the full monitor.
/// Fall back to monitor geometry only before the first bounds report arrives.
fn current_resize_work_area(
    window: &gtk::ApplicationWindow,
    state: &AspectResizeState,
) -> window_fit::WindowRect {
    state.work_area.get().unwrap_or_else(|| {
        window
            .surface()
            .and_then(|surface| surface.display().monitor_at_surface(&surface))
            .map(|monitor| monitor.geometry())
            .filter(|geometry| geometry.width() > 0 && geometry.height() > 0)
            .map(|geometry| window_fit::WindowRect {
                x: geometry.x(),
                y: geometry.y(),
                width: geometry.width(),
                height: geometry.height(),
            })
            .unwrap_or(window_fit::WindowRect {
                x: 0,
                y: 0,
                width: i32::MAX,
                height: i32::MAX,
            })
    })
}

fn aspect_resize_edge(edge: gdk::SurfaceEdge) -> aspect_resize::ResizeEdge {
    match edge {
        gdk::SurfaceEdge::North => aspect_resize::ResizeEdge::Top,
        gdk::SurfaceEdge::South => aspect_resize::ResizeEdge::Bottom,
        gdk::SurfaceEdge::West => aspect_resize::ResizeEdge::Left,
        gdk::SurfaceEdge::East => aspect_resize::ResizeEdge::Right,
        gdk::SurfaceEdge::NorthWest => aspect_resize::ResizeEdge::TopLeft,
        gdk::SurfaceEdge::NorthEast => aspect_resize::ResizeEdge::TopRight,
        gdk::SurfaceEdge::SouthWest => aspect_resize::ResizeEdge::BottomLeft,
        // SurfaceEdge is non-exhaustive; SouthEast and any future variant fall
        // through to the bottom-right grip, the safest default anchor.
        _ => aspect_resize::ResizeEdge::BottomRight,
    }
}

/// Track the aggregate modifier mask so left and right Shift remain independent:
/// releasing one key while the other is held leaves the lock active. The resize
/// gesture remains app-owned, so modifier transitions and pointer updates share
/// one deterministic core session.
pub(crate) fn connect_aspect_resize_shift(
    window: &gtk::ApplicationWindow,
    state: &AspectResizeState,
) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let modifiers_state = state.clone();
    keys.connect_modifiers(move |_, modifiers| {
        apply_shift(
            &modifiers_state,
            modifiers.contains(gdk::ModifierType::SHIFT_MASK),
        );
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    // GestureDrag normally ends the session. Keep a raw release failsafe for a
    // compositor/window-state interruption so stale geometry cannot affect a
    // later drag.
    let legacy = gtk::EventControllerLegacy::new();
    legacy.set_propagation_phase(gtk::PropagationPhase::Capture);
    let end_state = state.clone();
    legacy.connect_event(move |_, event| {
        if event.event_type() == gdk::EventType::ButtonRelease {
            clear_aspect_resize_session(&end_state);
        }
        glib::Propagation::Proceed
    });
    window.add_controller(legacy);

    // Entering maximize or fullscreen supersedes any interactive resize; drop a
    // lingering session so its lock cannot reshape the maximized/fullscreen size.
    let maximized_state = state.clone();
    window.connect_maximized_notify(move |_| clear_aspect_resize_session(&maximized_state));
    let fullscreen_state = state.clone();
    window.connect_notify_local(Some("fullscreened"), move |_, _| {
        clear_aspect_resize_session(&fullscreen_state)
    });
}

/// Abandon any in-flight aspect-lock session. Called when the window enters a
/// state that supersedes an interactive resize (maximize/fullscreen), so a
/// lingering lock can never distort a later drag.
pub(crate) fn clear_aspect_resize_session(state: &AspectResizeState) {
    state.session.borrow_mut().take();
    state.start_position_x11.set(None);
    state.last_requested.set(None);
}

fn apply_shift(state: &AspectResizeState, pressed: bool) {
    if state.shift_held.get() == pressed {
        return;
    }
    state.shift_held.set(pressed);
    if let Some(session) = state.session.borrow_mut().as_mut() {
        session.set_shift(pressed);
    }
}

pub(crate) fn build_player_resize_handles(
    window: &gtk::ApplicationWindow,
    aspect_resize_state: &AspectResizeState,
) -> Vec<gtk::Box> {
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
                connect_player_window_resize(&handle, window, edge, aspect_resize_state);
                handle
            },
        )
        .collect()
}

/// The current pointer position in the stable drag-origin coordinate space.
/// On X11 this is the root-coordinate pointer position so opposite-edge/corner
/// anchoring can be computed; on Wayland it is the GDK surface-event position of
/// the active drag event, which stays in the same toplevel surface and is not
/// rebased by the resize handle's allocation.
fn current_drag_pointer(
    gesture: &gtk::GestureDrag,
    window: &gtk::ApplicationWindow,
) -> Option<aspect_resize::PointerDelta> {
    current_pointer_position_on_x11(window)
        .map(|p| aspect_resize::PointerDelta {
            x: f64::from(p.x),
            y: f64::from(p.y),
        })
        .or_else(|| current_event_surface_position(gesture))
}

fn current_event_surface_position(
    gesture: &gtk::GestureDrag,
) -> Option<aspect_resize::PointerDelta> {
    let event = gesture.current_event()?;
    let (x, y) = event.position()?;
    Some(aspect_resize::PointerDelta { x, y })
}

pub(crate) fn connect_player_window_resize(
    widget: &gtk::Box,
    window: &gtk::ApplicationWindow,
    edge: gdk::SurfaceEdge,
    aspect_resize_state: &AspectResizeState,
) {
    let gesture = gtk::GestureDrag::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let resize_window = window.clone();
    let resize_state = aspect_resize_state.clone();
    gesture.connect_drag_begin(move |gesture, x, y| {
        let debug_resize = env::var_os("OKP_DEBUG_WINDOW_RESIZE").is_some();
        if debug_resize {
            eprintln!("resize drag begin edge={edge:?} local=({x:.1},{y:.1})");
        }

        if resize_window.is_fullscreen() || resize_window.is_maximized() {
            if debug_resize {
                eprintln!("resize ignored: fullscreen/maximized");
            }
            return;
        }
        apply_shift(
            &resize_state,
            gesture
                .current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK),
        );
        let start_size = current_client_size(&resize_window);
        let start_position = current_player_position_on_x11(&resize_window);
        let max = aspect_resize::client_max_for_anchor(
            aspect_resize_edge(edge),
            start_size,
            start_position,
            current_resize_work_area(&resize_window, &resize_state),
        );
        let Some(start_pointer) = current_drag_pointer(gesture, &resize_window) else {
            if debug_resize {
                eprintln!("resize drag begin edge={edge:?}: no stable pointer origin, aborting");
            }
            return;
        };
        resize_state
            .session
            .replace(Some(aspect_resize::AspectResize::begin(
                aspect_resize_edge(edge),
                start_size,
                MIN_RESIZE_CLIENT,
                max,
                resize_state.shift_held.get(),
                start_pointer,
            )));
        resize_state.start_position_x11.set(start_position);
        resize_state.last_requested.set(Some(start_size));
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if debug_resize {
            eprintln!(
                "resize session begin edge={edge:?} locked={} start={}x{} max={}x{} x11_anchor={} pointer=({:.1},{:.1})",
                resize_state.shift_held.get(),
                start_size.width,
                start_size.height,
                max.width,
                max.height,
                start_position.is_some(),
                start_pointer.x,
                start_pointer.y
            );
        }
    });

    let update_window = window.clone();
    let update_state = aspect_resize_state.clone();
    gesture.connect_drag_update(move |gesture, _offset_x, _offset_y| {
        apply_shift(
            &update_state,
            gesture
                .current_event_state()
                .contains(gdk::ModifierType::SHIFT_MASK),
        );
        let Some(pointer) = current_drag_pointer(gesture, &update_window) else {
            return;
        };
        let Some(resolved) = update_state
            .session
            .borrow_mut()
            .as_mut()
            .map(|session| session.resolve(pointer))
        else {
            return;
        };
        if update_state.last_requested.get() == Some(resolved.size) {
            return;
        }
        update_state.last_requested.set(Some(resolved.size));

        update_window.set_default_size(resolved.size.width, resolved.size.height);
        if let Some(start) = update_state.start_position_x11.get() {
            move_resize_player_window_on_x11(
                &update_window,
                window_fit::WindowPoint {
                    x: start.x.saturating_add(resolved.position_delta.x),
                    y: start.y.saturating_add(resolved.position_delta.y),
                },
                resolved.size,
            );
        }
        if env::var_os("OKP_DEBUG_WINDOW_RESIZE").is_some() {
            eprintln!(
                "resize update edge={edge:?} shift={} pointer=({:.1},{:.1}) size={}x{} anchor=({}, {})",
                update_state.shift_held.get(),
                pointer.x,
                pointer.y,
                resolved.size.width,
                resolved.size.height,
                resolved.position_delta.x,
                resolved.position_delta.y
            );
        }
    });

    let end_state = aspect_resize_state.clone();
    gesture.connect_drag_end(move |_, _, _| clear_aspect_resize_session(&end_state));
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
    if idle_theme_is_high_contrast() {
        root.add_css_class("is-high-contrast");
    }
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

pub(crate) fn idle_theme_is_dark() -> bool {
    match env::var("OKP_IDLE_THEME").ok().as_deref() {
        Some("light") => false,
        Some("dark") => true,
        _ => gtk::Settings::default()
            .map(|settings| settings.property::<bool>("gtk-application-prefer-dark-theme"))
            .unwrap_or(false),
    }
}

pub(crate) fn idle_theme_is_high_contrast() -> bool {
    env::var("GTK_THEME")
        .ok()
        .map(|name| name.to_ascii_lowercase().contains("highcontrast"))
        .unwrap_or(false)
        || gtk::Settings::default()
            .and_then(|settings| settings.gtk_theme_name())
            .map(|name| name.to_ascii_lowercase().contains("highcontrast"))
            .unwrap_or(false)
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
