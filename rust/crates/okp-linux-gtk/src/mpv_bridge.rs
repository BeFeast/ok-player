use super::*;

#[cfg(target_os = "linux")]
use gtk::glib::translate::ToGlibPtr;
#[cfg(target_os = "linux")]
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct NativeRenderNotifier {
    alive: AtomicBool,
    pending: Mutex<bool>,
    wake: std::sync::Condvar,
}

impl NativeRenderNotifier {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            alive: AtomicBool::new(true),
            pending: Mutex::new(false),
            wake: std::sync::Condvar::new(),
        })
    }

    fn notify(&self) {
        if !self.alive.load(Ordering::Acquire) {
            return;
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *pending = true;
        self.wake.notify_one();
    }

    fn wait(&self) -> bool {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !*pending && self.alive.load(Ordering::Acquire) {
            pending = self
                .wake
                .wait(pending)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        *pending = false;
        true
    }

    fn disable(&self) {
        self.alive.store(false, Ordering::Release);
        self.wake.notify_all();
    }
}

pub(crate) struct NativeRenderLoop {
    notifier: Arc<NativeRenderNotifier>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl NativeRenderLoop {
    fn start(
        plane: Arc<NativeVideoPlane>,
        update_handle: okp_mpv::RenderUpdateHandle,
        recorder: Option<Arc<PresentationRecorder>>,
    ) -> Result<Self, String> {
        let notifier = NativeRenderNotifier::new();
        let thread_notifier = Arc::clone(&notifier);
        let join = std::thread::Builder::new()
            .name("okp-native-render".to_owned())
            .spawn(move || {
                if !plane.make_current() {
                    eprintln!("Failed to activate the native Wayland/EGL render thread");
                    return;
                }
                while thread_notifier.wait() {
                    if !update_handle.update_has_frame() {
                        continue;
                    }
                    let Some(size) = plane.prepare_frame() else {
                        break;
                    };
                    if let Err(error) = update_handle.render_current_frame(size.width, size.height)
                    {
                        eprintln!("mpv native render failed: {error}");
                        break;
                    }
                    if !plane.swap() {
                        eprintln!("Native Wayland/EGL video swap failed");
                        break;
                    }
                    update_handle.report_swap();
                    if let Some(recorder) = recorder.as_ref() {
                        recorder.record_present(size, "egl-swap-buffers");
                    }
                }
                let _ = plane.release_current();
            })
            .map_err(|error| format!("spawning the native render thread failed: {error}"))?;
        Ok(Self {
            notifier,
            join: Some(join),
        })
    }

    fn callback_context(&self) -> *mut libc::c_void {
        Arc::as_ptr(&self.notifier) as *mut libc::c_void
    }

    fn stop_and_join(&mut self) {
        self.notifier.disable();
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            eprintln!("Native Wayland/EGL render thread panicked");
        }
    }
}

impl Drop for NativeRenderLoop {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

unsafe extern "C" fn native_render_update_trampoline(ctx: *mut libc::c_void) {
    let Some(notifier) = (unsafe { (ctx as *const NativeRenderNotifier).as_ref() }) else {
        return;
    };
    notifier.notify();
}

pub(crate) fn is_wayland_display(display_type_name: &str) -> bool {
    display_type_name == "GdkWaylandDisplay"
}

#[cfg(target_os = "linux")]
fn wayland_display_resource(display: &gdk::Display) -> Option<NativeWaylandDisplay> {
    type GetWaylandDisplay =
        unsafe extern "C" fn(display: *mut gtk::gdk::ffi::GdkDisplay) -> *mut libc::c_void;

    if !is_wayland_display(display.type_().name()) {
        return None;
    }

    // Resolve the backend accessor at runtime: GTK builds without Wayland
    // support keep compiling and use the existing no-native-display path.
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"gdk_wayland_display_get_wl_display".as_ptr(),
        )
    };
    if symbol.is_null() {
        return None;
    }

    let get_wayland_display =
        unsafe { std::mem::transmute::<*mut libc::c_void, GetWaylandDisplay>(symbol) };
    let pointer = NonNull::new(unsafe { get_wayland_display(display.to_glib_none().0) })?;
    // GDK owns the wl_display. Its cloned GObject reference is retained by
    // okp-mpv until the render context has been freed.
    Some(unsafe { NativeWaylandDisplay::new(pointer, display.clone()) })
}

#[cfg(not(target_os = "linux"))]
fn wayland_display_resource(_display: &gdk::Display) -> Option<NativeWaylandDisplay> {
    None
}

pub(crate) fn connect_mpv(
    video_host: &VideoHost,
    state: Rc<RefCell<PlayerState>>,
    launch_args: LaunchArgs,
) {
    match video_host {
        VideoHost::Native {
            area,
            container,
            auto_fallback,
        } => connect_native_mpv(area, container, *auto_fallback, state, launch_args),
        VideoHost::Gtk(area) => connect_gtk_mpv(area, state, launch_args),
    }
}

fn connect_native_mpv(
    video_area: &gtk::DrawingArea,
    container: &gtk::Stack,
    auto_fallback: bool,
    state: Rc<RefCell<PlayerState>>,
    launch_args: LaunchArgs,
) {
    let realize_state = Rc::clone(&state);
    let fallback_container = container.clone();
    let fallback_started = Rc::new(Cell::new(false));
    let realize_fallback_started = Rc::clone(&fallback_started);
    video_area.connect_realize(move |area| {
        let plane = match NativeVideoPlane::create(area) {
            Ok(plane) => plane,
            Err(error) => {
                schedule_gtk_mpv_fallback(
                    &fallback_container,
                    &realize_fallback_started,
                    &realize_state,
                    &launch_args,
                    auto_fallback,
                    format!("Failed to create the native Wayland/EGL video plane: {error}"),
                );
                return;
            }
        };
        if !plane.make_current() {
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &launch_args,
                auto_fallback,
                "Failed to activate the native Wayland/EGL video context".to_owned(),
            );
            return;
        }
        let Some(mut mpv) = create_configured_mpv(&realize_state) else {
            return;
        };
        let display = area.display();
        let native_wayland_display = wayland_display_resource(&display);
        if native_wayland_display.is_none() {
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &launch_args,
                auto_fallback,
                "GDK Wayland native display is unavailable for direct VAAPI interop".to_owned(),
            );
            return;
        }
        if let Err(error) = mpv.create_render_context(native_wayland_display) {
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &launch_args,
                auto_fallback,
                format!("Failed to create mpv native render context: {error}"),
            );
            return;
        }
        let update_handle = match mpv.render_update_handle() {
            Ok(handle) => handle,
            Err(error) => {
                mpv.destroy_render_context();
                schedule_gtk_mpv_fallback(
                    &fallback_container,
                    &realize_fallback_started,
                    &realize_state,
                    &launch_args,
                    auto_fallback,
                    format!("Failed to capture the mpv native render handle: {error}"),
                );
                return;
            }
        };
        let recorder = realize_state.borrow().presentation_recorder.clone();
        if !plane.release_current() {
            mpv.destroy_render_context();
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &launch_args,
                auto_fallback,
                "Failed to transfer the native Wayland/EGL context to the render thread".to_owned(),
            );
            return;
        }
        let render_loop = match NativeRenderLoop::start(Arc::clone(&plane), update_handle, recorder)
        {
            Ok(render_loop) => render_loop,
            Err(error) => {
                let _ = plane.make_current();
                mpv.destroy_render_context();
                schedule_gtk_mpv_fallback(
                    &fallback_container,
                    &realize_fallback_started,
                    &realize_state,
                    &launch_args,
                    auto_fallback,
                    error,
                );
                return;
            }
        };
        if let Err(error) = unsafe {
            mpv.set_render_update_callback(
                Some(native_render_update_trampoline),
                render_loop.callback_context(),
            )
        } {
            eprintln!("Failed to install the mpv native render callback: {error}");
            drop(render_loop);
            let _ = plane.make_current();
            mpv.destroy_render_context();
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &launch_args,
                auto_fallback,
                format!("Failed to install the mpv native render callback: {error}"),
            );
            return;
        }

        mpv.start_event_pump();
        {
            let mut state = realize_state.borrow_mut();
            state.native_video_plane = Some(plane);
            state.native_render_loop = Some(render_loop);
            state.mpv = Some(mpv);
        }
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);
        apply_launch_args(&realize_state, &launch_args);
        area.queue_draw();
    });

    let resize_state = Rc::clone(&state);
    video_area.connect_resize(move |area, width, height| {
        if let Some(plane) = resize_state.borrow().native_video_plane.as_ref() {
            plane.resize(width, height, area.scale_factor());
        }
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |_| {
        let mut state = unrealize_state.borrow_mut();
        if let Some(mpv) = state.mpv.as_mut() {
            let _ = unsafe { mpv.set_render_update_callback(None, std::ptr::null_mut()) };
        }
        if let Some(mut render_loop) = state.native_render_loop.take() {
            render_loop.stop_and_join();
        }
        if let Some(plane) = state.native_video_plane.as_ref() {
            let _ = plane.make_current();
        }
        if let Some(mpv) = state.mpv.as_mut() {
            mpv.destroy_render_context();
        }
        if let Some(plane) = state.native_video_plane.as_ref() {
            plane.disable();
        }
        state.native_video_plane = None;
    });
}

fn schedule_gtk_mpv_fallback(
    container: &gtk::Stack,
    fallback_started: &Rc<Cell<bool>>,
    state: &Rc<RefCell<PlayerState>>,
    launch_args: &LaunchArgs,
    auto_fallback: bool,
    reason: String,
) {
    if !auto_fallback {
        eprintln!("{reason}. Set OKP_VIDEO_BACKEND=gtk to use the compatibility path.");
        return;
    }
    if fallback_started.replace(true) {
        return;
    }

    eprintln!("{reason}; falling back to GtkGLArea");
    let container = container.clone();
    let state = Rc::clone(state);
    let launch_args = launch_args.clone();
    glib::idle_add_local_once(move || {
        state.borrow_mut().presentation_recorder = PresentationRecorder::from_env(
            okp_core::presentation_evidence::PresentationBackend::GtkGlArea,
        );
        let area = gtk_video_area();
        connect_gtk_mpv(&area, Rc::clone(&state), launch_args);
        container.add_named(&area, Some("gtk-glarea-fallback"));
        container.set_visible_child(&area);
        // The media-launch window is intentionally realized but not mapped
        // until dimensions arrive. A child added during native fallback does
        // not realize automatically in that state, so start Gtk/libmpv now
        // instead of waiting for the five-second map escape hatch.
        gtk::prelude::WidgetExt::realize(&area);
    });
}

fn connect_gtk_mpv(
    video_area: &gtk::GLArea,
    state: Rc<RefCell<PlayerState>>,
    launch_args: LaunchArgs,
) {
    let realize_state = Rc::clone(&state);
    video_area.connect_realize(move |area| {
        area.make_current();
        if let Some(error) = area.error() {
            eprintln!("GTK GLArea error: {error}");
            return;
        }

        let Some(mut mpv) = create_configured_mpv(&realize_state) else {
            return;
        };

        let display = area.display();
        let native_wayland_display = wayland_display_resource(&display);
        if is_wayland_display(display.type_().name()) && native_wayland_display.is_none() {
            eprintln!(
                "GDK Wayland native display is unavailable; continuing without zero-copy display interop"
            );
        }
        if let Err(error) = mpv.create_render_context(native_wayland_display) {
            eprintln!("Failed to create mpv render context: {error}");
            return;
        }
        if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
            eprintln!("mpv render context initialized before source load");
        }

        // Start the background event pump: from here on the shell reads playback
        // state from its observed snapshot rather than polling mpv from this
        // (GLib main-context) thread, so the tripwire armed above stays green.
        mpv.start_event_pump();

        realize_state.borrow_mut().mpv = Some(mpv);
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);

        apply_launch_args(&realize_state, &launch_args);
    });

    let resize_state = Rc::clone(&state);
    video_area.connect_resize(move |_, width, height| {
        resize_state.borrow_mut().render_target_size =
            (width > 0 && height > 0).then_some(okp_mpv::RenderTargetSize { width, height });
    });

    let render_state = Rc::clone(&state);
    video_area.connect_render(move |area, _context| {
        area.make_current();
        area.attach_buffers();
        let viewport_size = current_render_target_size();
        let widget_width = area.width();
        let widget_height = area.height();
        let scale_factor = area.scale_factor();
        let mut state = render_state.borrow_mut();
        let target_size = resolve_render_target_size(
            viewport_size,
            state.render_target_size,
            widget_width,
            widget_height,
            scale_factor,
        );
        if let Some(mpv) = state.mpv.as_mut()
            && let Err(error) = mpv.render(target_size.width, target_size.height)
        {
            eprintln!("mpv render failed: {error}");
        }
        if let Some(recorder) = state.presentation_recorder.as_ref() {
            recorder.record_present(target_size, "gtk-glarea-render");
        }

        glib::Propagation::Stop
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |area| {
        area.make_current();
        if let Some(mpv) = unrealize_state.borrow_mut().mpv.as_mut() {
            mpv.destroy_render_context();
        }
    });

    let tick_area = video_area.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_area.queue_render();
        glib::ControlFlow::Continue
    });
}

fn create_configured_mpv(state: &Rc<RefCell<PlayerState>>) -> Option<Mpv> {
    let (hwdec, raw_mpv_config, subtitle_scale, subtitle_position, subtitle_style) = {
        let state = state.borrow();
        (
            state.settings.hardware_decode_mpv_option().to_owned(),
            state.settings.raw_mpv_config().to_owned(),
            state.settings.subtitle_scale(),
            state.settings.subtitle_position(),
            state.settings.subtitle_style(),
        )
    };
    let raw_mpv_options = match parse_raw_mpv_config(&raw_mpv_config) {
        Ok(options) => options,
        Err(error) => {
            eprintln!(
                "Ignoring custom mpv.conf option at line {}: {}",
                error.line, error.message
            );
            Vec::new()
        }
    };

    let mpv = match Mpv::new_with_options(&hwdec, &raw_mpv_options) {
        Ok(mpv) => mpv,
        Err(error) if !raw_mpv_options.is_empty() => {
            eprintln!(
                "Failed to create mpv with custom mpv.conf options: {error}; retrying without them"
            );
            match Mpv::new_with_hwdec(&hwdec) {
                Ok(mpv) => mpv,
                Err(error) => {
                    eprintln!("Failed to create mpv: {error}");
                    return None;
                }
            }
        }
        Err(error) => {
            eprintln!("Failed to create mpv: {error}");
            return None;
        }
    };
    mpv.mark_ui_thread();
    let saved_volume = state.borrow().settings.volume();
    if let Err(error) = mpv.set_volume(saved_volume) {
        eprintln!("Failed to restore saved volume: {error}");
    }
    let video_adjustments = state.borrow().settings.video_adjustments();
    if let Err(error) = mpv.set_video_adjustments(
        video_adjustments.brightness,
        video_adjustments.contrast,
        video_adjustments.saturation,
        video_adjustments.gamma,
    ) {
        eprintln!("Failed to restore video adjustments: {error}");
    }
    let audio_normalization = state.borrow().settings.audio_normalization_enabled();
    if let Err(error) = mpv.set_audio_normalization(audio_normalization) {
        eprintln!("Failed to restore audio normalization: {error}");
    }
    let downmix_surround = state.borrow().settings.downmix_surround_to_stereo_enabled();
    if let Err(error) = mpv.set_downmix_surround_to_stereo(downmix_surround) {
        eprintln!("Failed to restore surround downmix: {error}");
    }
    if let Err(error) = mpv.set_subtitle_scale(subtitle_scale) {
        eprintln!("Failed to restore subtitle size: {error}");
    }
    if let Err(error) = mpv.set_subtitle_position(subtitle_position as f64) {
        eprintln!("Failed to restore subtitle position: {error}");
    }
    if let Err(error) = mpv.set_subtitle_style(subtitle_style.options) {
        eprintln!("Failed to restore subtitle style: {error}");
    }
    Some(mpv)
}

pub(crate) fn parse_raw_mpv_config(text: &str) -> Result<Vec<(String, String)>, RawMpvConfigError> {
    let mut options = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let option = trimmed.strip_prefix("--").unwrap_or(trimmed);
        let Some((name, value)) = option.split_once('=') else {
            return Err(raw_mpv_config_error(
                line_number,
                "Use key=value syntax, one option per line.",
            ));
        };
        let name = name.trim();
        let value = value.trim();

        if name.is_empty() {
            return Err(raw_mpv_config_error(line_number, "Option name is empty."));
        }
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(raw_mpv_config_error(
                line_number,
                "Option names can use letters, numbers, hyphen, or underscore.",
            ));
        }
        if name.contains('\0') || value.contains('\0') {
            return Err(raw_mpv_config_error(
                line_number,
                "NUL bytes are not valid in mpv options.",
            ));
        }
        if PROTECTED_MPV_OPTIONS
            .iter()
            .any(|protected| name.eq_ignore_ascii_case(protected))
            || okp_core::subtitle_style::is_managed_option(name)
        {
            return Err(raw_mpv_config_error(
                line_number,
                &format!("{name} is managed by OK Player."),
            ));
        }

        options.push((name.to_owned(), value.to_owned()));
    }

    Ok(options)
}

pub(crate) fn raw_mpv_config_error(line: usize, message: &str) -> RawMpvConfigError {
    RawMpvConfigError {
        line,
        message: message.to_owned(),
    }
}

pub(crate) fn apply_launch_args(
    state: &Rc<RefCell<PlayerState>>,
    launch_args: &LaunchArgs,
) -> bool {
    if launch_args.has_payload() {
        eprintln!(
            "Launch request: {} item(s), {} playlist(s), {} subtitle(s)",
            launch_args.items.len(),
            launch_args.playlists.len(),
            launch_args.subtitles.len()
        );
    }

    if launch_args.has_media_payload() {
        state.borrow_mut().next_launch_directives = Some(launch_args.directives);
    }

    let loaded = load_launch_args(state, launch_args);
    if !loaded {
        state.borrow_mut().next_launch_directives = None;
    }
    let subtitles_loaded = apply_launch_subtitles(state, &launch_args.subtitles);
    loaded || subtitles_loaded
}

pub(crate) fn load_launch_args(state: &Rc<RefCell<PlayerState>>, launch_args: &LaunchArgs) -> bool {
    match launch_args.items.as_slice() {
        [PlaylistItem::Local(path)] => {
            load_media_path(state, path.clone());
            true
        }
        [PlaylistItem::Url(url)] => {
            load_media_url(state, url.clone());
            true
        }
        [] => launch_args
            .playlists
            .first()
            .is_some_and(|path| load_m3u_playlist_silent(state, path)),
        items => {
            let playlist = items.to_vec();
            let Some(first_item) = playlist.first().cloned() else {
                return false;
            };
            load_playlist_item_with_playlist(state, first_item, playlist, true)
        }
    }
}

pub(crate) fn apply_launch_subtitles(
    state: &Rc<RefCell<PlayerState>>,
    subtitles: &[PathBuf],
) -> bool {
    let mut applied = false;
    for path in subtitles {
        if load_subtitle_path(state, path.clone()) {
            applied = true;
        } else if !has_loaded_media(state) {
            let mut state = state.borrow_mut();
            if !state
                .pending_subtitles
                .iter()
                .any(|existing| existing == path)
            {
                state.pending_subtitles.push(path.clone());
            }
        }
    }
    applied
}

pub(crate) fn connect_state_poll(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    controls: Controls,
    context: StatePollContext,
) {
    let window = window.clone();
    let status_toast = controls.status_toast.clone();
    let StatePollContext {
        updating_seek,
        initial_map_pending,
        chrome,
        compact_mode,
        window_chrome,
        subtitle_position_snapshot,
        empty_surface,
        lyrics_surface,
        media_state_overlay,
        window_bounds,
        mpris_snapshot,
        mpris_signals,
    } = context;
    glib::timeout_add_local(Duration::from_millis(200), move || {
        let auto_fit_dimensions = drain_mpv_events(&state, &status_toast);
        apply_pending_nfo_titles(&state);
        observe_initial_window_fit(&state, auto_fit_dimensions);
        // A normally realized-but-not-yet-mapped surface may already expose
        // its compositor-selected bounds. Use that opportunity without forcing
        // realization; otherwise keep the core request pending until mapping.
        if (window.is_mapped() || current_player_work_area(&window, &window_bounds).is_some())
            && let Some(request) = take_initial_window_fit(&state)
        {
            let deferred_launch_fit = initial_map_pending.get();
            fit_player_window_to_video(
                &window,
                &state,
                &window_bounds,
                request.source_generation,
                request.video,
                deferred_launch_fit,
            );
            if initial_map_pending.replace(false) {
                window.present();
            }
        }
        drain_screenshot_jobs(&state, &status_toast);
        try_pending_audio_device_restore(&state);

        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .map(|mpv| mpv.observed_playback_state());
        if let Some(playback) = playback {
            let state = state.borrow();
            if let (Some(recorder), Some(mpv)) =
                (state.presentation_recorder.as_ref(), state.mpv.as_ref())
            {
                recorder.record_playback(playback, mpv.observed_playback_diagnostics());
            }
        }
        run_presentation_exercise(&state, playback);
        let has_media = has_loaded_media(&state);
        let seek_preview = env::var_os("OKP_OPEN_SEEK_PREVIEW_ON_STARTUP").is_some();
        let has_chapters = state
            .borrow()
            .mpv
            .as_ref()
            .is_some_and(|mpv| !mpv.observed_chapters().is_empty());
        chrome.set_has_media(has_media || seek_preview);
        let media_title = if has_media {
            let state = state.borrow();
            let base = current_media_title(&state);
            let chapter = playback
                .and_then(|playback| playback.time_pos)
                .and_then(|position| {
                    let times = state
                        .chapters_snapshot
                        .iter()
                        .map(|chapter| chapter.time)
                        .collect::<Vec<_>>();
                    chapter_math::current_index(&times, position, chapter_math::DEFAULT_EPSILON)
                        .and_then(|index| state.chapters_snapshot.get(index))
                })
                .and_then(|chapter| chapter.title.as_deref())
                .filter(|title| !title.trim().is_empty());
            chapter
                .map(|chapter| format!("{base} · {chapter}"))
                .unwrap_or(base)
        } else {
            String::new()
        };
        compact_mode.update(has_media, playback, &media_title);
        window_chrome.set_title(&media_title);
        if has_media {
            let lift = if chrome.is_revealed() {
                okp_core::subtitle_lift::for_surface(
                    f64::from(window.height()),
                    OSC_CLEARANCE_DIP,
                    OSC_SUBTITLE_LIFT_PERCENT,
                )
            } else {
                0.0
            };
            let base_position = state.borrow().settings.subtitle_position() as f64;
            let subtitle_position = okp_core::subtitle_lift::apply_to_position(base_position, lift);
            let position_key = (subtitle_position * 1000.0).round() as i64;
            if subtitle_position_snapshot.replace(Some(position_key)) != Some(position_key)
                && let Some(mpv) = state.borrow().mpv.as_ref()
                && let Err(error) = mpv.set_subtitle_position(subtitle_position)
            {
                eprintln!("Failed to position subtitles above playback chrome: {error}");
            }
        } else {
            subtitle_position_snapshot.set(None);
        }
        {
            let state = state.borrow();
            update_mpris_snapshot(&mpris_snapshot, &mpris_signals, &state, playback);
        }
        sync_ab_loop_state(&state, has_media);
        if has_media {
            empty_surface.clear_preview_substrate();
        }
        // Hide the welcome surface behind an active lyrics preview so the fixture reads cleanly;
        // in production the loaded audio already hides it (`is_preview_frozen` stays false).
        empty_surface.refresh(&window, &state, Rc::clone(&status_toast));
        let failed = state.borrow().media_load_state == network_media::MediaLoadState::Failed;
        empty_surface.set_has_media(has_media || failed || lyrics_surface.is_preview_frozen());
        lyrics_surface.update(&state);
        drain_thumbnail_events(&controls, &state);
        update_up_next_panel(&controls, &state, &chrome);

        if let Some(playback) = playback {
            try_pending_subtitles(&state);
            let load_state = state.borrow().media_load_state;
            chrome.set_auto_hide_enabled(
                has_media
                    && load_state == network_media::MediaLoadState::Playing
                    && !playback.paused,
            );

            let duration = playback.duration.unwrap_or(0.0).max(0.0);
            let raw_time = playback.time_pos.unwrap_or(0.0).max(0.0);
            let time_pos = if duration > 0.0 {
                raw_time.min(duration)
            } else {
                raw_time
            };
            try_pending_resume(&state, duration);

            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_chapters);
            controls.next_button.set_sensitive(has_chapters);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls.play_button.set_icon_name(if playback.paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            });
            controls
                .play_button
                .set_tooltip_text(Some(if playback.paused {
                    "Play (Space)"
                } else {
                    "Pause (Space)"
                }));
            controls
                .speed_button
                .set_label(&format_speed(playback.speed.unwrap_or(1.0)));
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(has_media && duration > 0.0);

            updating_seek.set(true);
            controls.seek.set_range(0.0, duration.max(1.0));
            controls.seek.set_value(time_pos);
            updating_seek.set(false);

            if load_state == network_media::MediaLoadState::Loading {
                controls.timeline_rail.set_loading(true);
                controls.timeline_rail.pulse();
            } else {
                controls.timeline_rail.set_loading(false);
                let fraction = timeline_buffer::fraction(
                    playback.time_pos,
                    playback.cache_duration,
                    playback.duration,
                );
                controls.timeline_rail.set_buffered_fraction(fraction);
            }

            if let Some(volume) = playback.volume {
                controls.volume.sync_level(volume);
            }

            controls
                .elapsed_label
                .set_text(&time_code::format_clock(time_pos));
            // Unknown duration shows the live `--:--` sentinel only for a network source;
            // local loading remains `-00:00`. The pure core helper owns that distinction
            // and the remaining-time clamp so the shell only projects the value.
            // The seek range still clamps to 0 so the bar stays progress-only /
            // disabled rather than running broken timeline math.
            let is_url = state.borrow().current_url.is_some();
            controls
                .duration_label
                .set_text(&time_code::format_trailing(
                    controls.trailing_time_mode.get(),
                    is_url,
                    time_pos,
                    playback.duration,
                ));
        } else {
            chrome.set_auto_hide_enabled(false);
            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_chapters);
            controls.next_button.set_sensitive(has_chapters);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls
                .play_button
                .set_icon_name("media-playback-start-symbolic");
            controls.play_button.set_tooltip_text(Some("Play (Space)"));
            controls.speed_button.set_label("1.00×");
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(false);
            updating_seek.set(true);
            controls.seek.set_range(0.0, 1.0);
            controls.seek.set_value(0.0);
            updating_seek.set(false);
            controls.timeline_rail.set_buffered_fraction(0.0);
            controls.timeline_rail.set_loading(false);
            controls.elapsed_label.set_text("00:00");
            controls.duration_label.set_text("-00:00");
        }

        update_media_state_surface(&state, playback, has_media, &media_state_overlay);
        compact_mode.sync_surface_visibility();

        glib::ControlFlow::Continue
    });
}

pub(crate) fn observe_initial_window_fit(
    state: &Rc<RefCell<PlayerState>>,
    video_dimensions: Option<VideoDimensions>,
) -> bool {
    let Some(video_dimensions) = video_dimensions else {
        return false;
    };
    let mut state = state.borrow_mut();
    state.current_video_dimensions = Some(video_dimensions);
    let source_generation = state.source_generation;
    state.initial_window_fit.observe_dimensions(
        source_generation,
        video_dimensions.width,
        video_dimensions.height,
    )
}

pub(crate) fn take_initial_window_fit(
    state: &Rc<RefCell<PlayerState>>,
) -> Option<window_fit::InitialFitRequest> {
    let mut state = state.borrow_mut();
    let source_generation = state.source_generation;
    state.initial_window_fit.take(source_generation)
}

fn run_presentation_exercise(state: &Rc<RefCell<PlayerState>>, playback: Option<PlaybackState>) {
    use okp_core::presentation_evidence::PresentationAction;

    let playing = playback.is_some_and(|playback| !playback.paused && playback.time_pos.is_some());
    let action = {
        let mut state = state.borrow_mut();
        state
            .presentation_exercise
            .as_mut()
            .and_then(|exercise| exercise.poll(monotonic_ns(), playing))
    };
    let Some(action) = action else {
        return;
    };
    let state = state.borrow();
    let Some(mpv) = state.mpv.as_ref() else {
        return;
    };
    let result = match action {
        PresentationAction::SeekForward | PresentationAction::SeekBackward => {
            mpv.seek_relative(action.seek_seconds().unwrap_or_default())
        }
        PresentationAction::SpeedDouble | PresentationAction::SpeedNormal => {
            mpv.set_speed(action.speed().unwrap_or(1.0))
        }
    };
    if let Err(error) = result {
        eprintln!("Presentation exercise action {action:?} failed: {error}");
        return;
    }
    if let Some(recorder) = state.presentation_recorder.as_ref() {
        recorder.record_action(action);
    }
}

/// Project the shared load state and observed pause flag onto the in-canvas
/// paused, loading, and recovery surfaces. Raw engine detail stays behind the
/// error card's explicit Copy details action.
fn update_media_state_surface(
    state: &Rc<RefCell<PlayerState>>,
    playback: Option<PlaybackState>,
    has_media: bool,
    overlay: &MediaStateOverlay,
) {
    let (load_state, can_retry) = {
        let state = state.borrow();
        (state.media_load_state, state.retry_load_source.is_some())
    };
    overlay.update(
        load_state,
        has_media,
        playback.is_some_and(|playback| playback.paused),
        can_retry,
    );
}

pub(crate) fn connect_video_clicks(
    video_area: &gtk::Widget,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);

    let click_window = window.clone();
    let click_state = Rc::clone(&state);
    let pending_single_click = Rc::new(RefCell::new(None::<glib::SourceId>));
    let pending_click = Rc::clone(&pending_single_click);
    click.connect_released(move |_, press_count, _, _| {
        match video_click::release_intent(press_count) {
            video_click::Intent::Ignore => {}
            video_click::Intent::SchedulePlayPause => {
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: video-single-click-scheduled");
                }
                if let Some(source_id) = pending_click.borrow_mut().take() {
                    source_id.remove();
                }
                let delay_ms = gtk::Settings::default()
                    .map(|settings| settings.property::<i32>("gtk-double-click-time").max(1) as u32)
                    .unwrap_or(250);
                let delayed_state = Rc::clone(&click_state);
                let delayed_pending = Rc::clone(&pending_click);
                let source_id = glib::timeout_add_local(
                    Duration::from_millis(u64::from(delay_ms)),
                    move || {
                        delayed_pending.borrow_mut().take();
                        if has_loaded_media(&delayed_state)
                            && let Some(mpv) = delayed_state.borrow().mpv.as_ref()
                            && let Err(error) = mpv.cycle_pause()
                        {
                            eprintln!("Failed to toggle playback: {error}");
                        }
                        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                            eprintln!("interaction: video-single-click-committed");
                        }
                        glib::ControlFlow::Break
                    },
                );
                pending_click.borrow_mut().replace(source_id);
            }
            video_click::Intent::CancelPlayPauseAndToggleFullscreen => {
                if let Some(source_id) = pending_click.borrow_mut().take() {
                    source_id.remove();
                }
                if restore_compact_mode(&click_window) {
                    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                        eprintln!("interaction: video-double-click-compact-restore");
                    }
                } else {
                    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                        eprintln!("interaction: video-double-click-fullscreen");
                    }
                    toggle_fullscreen(&click_window, &click_state);
                }
            }
        }
    });

    video_area.add_controller(click);
}

pub(crate) fn connect_player_context_menu(
    player_root: &gtk::Overlay,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
    reach: PlayerCommandReach,
) {
    let context_click = gtk::GestureClick::new();
    context_click.set_button(gdk::BUTTON_SECONDARY);
    context_click.set_propagation_phase(gtk::PropagationPhase::Bubble);

    let context_root = player_root.clone();
    let context_window = window.clone();
    let context_state = Rc::clone(&state);
    let context_toast = Rc::clone(&status_toast);
    let context_chrome = Rc::clone(&chrome);
    let context_reach = reach.clone();
    context_click.connect_pressed(move |gesture, _, x, y| {
        let Some(target) = context_root.pick(x, y, gtk::PickFlags::INSENSITIVE) else {
            return;
        };
        if player_context_menu_target_is_interactive(&context_root, &target) {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: player-context-menu-suppressed");
            }
            return;
        }

        gesture.set_state(gtk::EventSequenceState::Claimed);
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: player-context-menu-open x={x:.0} y={y:.0}");
        }
        show_player_context_menu(
            &context_root,
            &context_window,
            Rc::clone(&context_state),
            Rc::clone(&context_toast),
            Rc::clone(&context_chrome),
            &context_reach,
            (x, y),
        );
    });

    player_root.add_controller(context_click);
}

pub(crate) fn player_context_menu_target_is_interactive(
    player_root: &gtk::Overlay,
    target: &gtk::Widget,
) -> bool {
    const BLOCKING_CSS_CLASSES: [&str; 5] = [
        "okp-time-label",
        "okp-timeline",
        "okp-volume-control",
        "okp-up-next-panel",
        "okp-resize-handle",
    ];

    let mut current = Some(target.clone());
    while let Some(widget) = current {
        if widget == *player_root {
            return false;
        }
        if widget.is::<gtk::Button>()
            || widget.is::<gtk::MenuButton>()
            || widget.is::<gtk::Scale>()
            || widget.is::<gtk::Scrollbar>()
            || widget.is::<gtk::Entry>()
            || widget.is::<gtk::TextView>()
            || widget.is::<gtk::SpinButton>()
            || widget.is::<gtk::DropDown>()
            || widget.is::<gtk::Switch>()
            || widget.is::<gtk::ListBoxRow>()
            || widget.is::<gtk::Popover>()
            || BLOCKING_CSS_CLASSES
                .iter()
                .any(|class| widget.has_css_class(class))
        {
            return true;
        }
        current = widget.parent();
    }
    false
}

pub(crate) fn show_player_context_menu(
    player_root: &gtk::Overlay,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
    reach: &PlayerCommandReach,
    point: (f64, f64),
) {
    let (x, y) = point;
    let popover = gtk::Popover::new();
    prepare_track_popover(&popover, PlayerPopoverKind::AdvancedCommands);
    popover.set_position(if y < f64::from(parent.height()) / 2.0 {
        gtk::PositionType::Bottom
    } else {
        gtk::PositionType::Top
    });
    connect_popover_chrome_pin(&popover, chrome);
    popover.set_parent(player_root);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    populate_command_popover(
        &popover,
        parent,
        state,
        status_toast,
        reach,
        PlayerCommandSurface::ContextMenu,
    );
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}
