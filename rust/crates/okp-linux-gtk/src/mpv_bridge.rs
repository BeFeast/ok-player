use super::*;

#[cfg(target_os = "linux")]
use gtk::glib::translate::ToGlibPtr;
#[cfg(target_os = "linux")]
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn configure_linux_renderer_environment() -> LinuxRendererMode {
    let flatpak = flatpak_install_detected(
        env::var_os("FLATPAK_ID").as_deref(),
        Path::new("/.flatpak-info").is_file(),
    );
    let dri_root = env::var_os("OKP_TEST_DRI_DEVICE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev/dri"));
    let dri_accessible = accessible_dri_device_exists(&dri_root);
    let mode = okp_core::linux_renderer::select_linux_renderer(flatpak, dri_accessible);

    for (name, value) in mode.environment_overrides() {
        // SAFETY: this is the first statement in `main`, before GTK,
        // Velopack, or any application worker thread is initialized.
        unsafe {
            env::set_var(name, value);
        }
    }
    let _ = LINUX_RENDERER_MODE.set(mode);

    eprintln!(
        "Renderer policy: mode={} flatpak={} dri-accessible={} backend={} hwdec={} render-api={} gsk-renderer={}",
        mode.label(),
        flatpak,
        dri_accessible,
        if mode.requires_software_surface() {
            "libmpv-software"
        } else {
            "automatic"
        },
        mode.mpv_hwdec_override().unwrap_or("settings"),
        if mode == LinuxRendererMode::SoftwareNoDri {
            "sw"
        } else {
            "default"
        },
        if mode == LinuxRendererMode::SoftwareNoDri {
            "cairo"
        } else {
            "default"
        }
    );
    mode
}

pub(crate) fn configured_linux_renderer_mode() -> LinuxRendererMode {
    LINUX_RENDERER_MODE.get().copied().unwrap_or_default()
}

pub(crate) fn flatpak_install_detected(
    flatpak_id: Option<&std::ffi::OsStr>,
    sandbox_marker_exists: bool,
) -> bool {
    flatpak_id.is_some_and(|value| !value.is_empty()) || sandbox_marker_exists
}

pub(crate) fn accessible_dri_device_exists(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        (name.starts_with("renderD") || name.starts_with("card"))
            && fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(entry.path())
                .is_ok()
    })
}

pub(crate) struct NativeRenderNotifier {
    alive: AtomicBool,
    pending: Mutex<NativeRenderRequest>,
    wake: std::sync::Condvar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct NativeRenderRequest {
    pending: bool,
    force: bool,
}

impl NativeRenderNotifier {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            alive: AtomicBool::new(true),
            pending: Mutex::new(NativeRenderRequest::default()),
            wake: std::sync::Condvar::new(),
        })
    }

    fn notify(&self) {
        self.queue(false);
    }

    fn force_render(&self) {
        self.queue(true);
    }

    fn queue(&self, force: bool) {
        if !self.alive.load(Ordering::Acquire) {
            return;
        }
        let mut request = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        request.pending = true;
        request.force |= force;
        self.wake.notify_one();
    }

    fn wait(&self) -> Option<NativeRenderRequest> {
        let mut request = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !request.pending && self.alive.load(Ordering::Acquire) {
            request = self
                .wake
                .wait(request)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if !self.alive.load(Ordering::Acquire) {
            return None;
        }
        let next = *request;
        *request = NativeRenderRequest::default();
        Some(next)
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
                while let Some(request) = thread_notifier.wait() {
                    let has_frame = update_handle.update_has_frame();
                    if !request.force && !has_frame {
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

    pub(crate) fn request_render(&self) {
        self.notifier.force_render();
    }

    pub(crate) fn render_for_screenshot(&self) {
        // The native Wayland path is callback-driven, unlike GtkGLArea's 16 ms
        // tick. A paused video may otherwise have no render after mpv accepts
        // screenshot-to-file, starving the capture of the current libmpv frame.
        self.request_render();
    }

    /// Stops the notifier and waits briefly for the render thread.
    ///
    /// Returns `true` when the thread has fully joined and it is safe to free
    /// the EGL plane / mpv render context. On timeout, the `JoinHandle` is
    /// forgotten (not dropped/detached) so the thread keeps its `Arc`s; the
    /// caller must skip render teardown and avoid `Drop`-destroying `Mpv`
    /// until process exit (`Application::quit` is imminent on the close path).
    fn stop_and_join(&mut self) -> bool {
        self.notifier.disable();
        let Some(join) = self.join.take() else {
            return true;
        };
        // Never block window destroy/unrealize on a stuck render thread — that left the
        // candidate headless close waiter staring at an IsViewable shell forever.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
        while !join.is_finished() {
            if std::time::Instant::now() >= deadline {
                eprintln!(
                    "Native Wayland/EGL render thread exceeded the close join deadline; \
                     leaking render resources until process exit"
                );
                // Do not drop the JoinHandle: that would detach a worker that still
                // owns RenderUpdateHandle / plane while unrealize frees them (UAF).
                std::mem::forget(join);
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if join.join().is_err() {
            eprintln!("Native Wayland/EGL render thread panicked");
        }
        true
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

fn start_event_pump_for_session(mpv: &mut Mpv) {
    let headless_fit_session = std::env::var_os("OKP_MAIN_WINDOW_FIT_ONLY")
        .is_some_and(|value| value == std::ffi::OsStr::new("1"));
    if headless_fit_session {
        eprintln!("headless fit smoke: audio-device observation disabled");
        mpv.start_event_pump_without_audio_devices();
    } else {
        mpv.start_event_pump();
    }
}

pub(crate) fn connect_mpv(
    video_host: &VideoHost,
    state: Rc<RefCell<PlayerState>>,
    startup_launch: StartupLaunchGate,
) {
    match video_host {
        VideoHost::Native {
            area,
            container,
            auto_fallback,
        } => connect_native_mpv(area, container, *auto_fallback, state, startup_launch),
        VideoHost::Gtk(area) => connect_gtk_mpv(area, state, startup_launch),
        VideoHost::Software(area) => connect_software_mpv(area, state, startup_launch),
    }
}

pub(crate) fn mark_startup_window_mapped(
    startup_launch: &StartupLaunchGate,
    state: &Rc<RefCell<PlayerState>>,
) {
    if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
        eprintln!("startup launch lifecycle: window mapped");
    }
    let launch_args = startup_launch.borrow_mut().mark_window_mapped();
    apply_ready_startup_launch(state, launch_args);
}

fn mark_startup_player_ready(startup_launch: &StartupLaunchGate, state: &Rc<RefCell<PlayerState>>) {
    if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
        eprintln!("startup launch lifecycle: player ready");
    }
    let launch_args = startup_launch.borrow_mut().mark_player_ready();
    apply_ready_startup_launch(state, launch_args);
}

fn apply_ready_startup_launch(state: &Rc<RefCell<PlayerState>>, launch_args: Option<LaunchArgs>) {
    let Some(launch_args) = launch_args else {
        return;
    };
    if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
        eprintln!("startup launch lifecycle: delivering after map and player readiness");
    }
    apply_launch_args(state, &launch_args);
}

fn connect_native_mpv(
    video_area: &gtk::DrawingArea,
    container: &gtk::Stack,
    auto_fallback: bool,
    state: Rc<RefCell<PlayerState>>,
    startup_launch: StartupLaunchGate,
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
                    &startup_launch,
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
                &startup_launch,
                auto_fallback,
                "Failed to activate the native Wayland/EGL video context".to_owned(),
            );
            return;
        }
        let dmabuf_target = wayland_dmabuf_target(area).ok();
        let render_size =
            native_render_size(area.width(), area.height(), native_surface_scale(area));
        let Some(mut mpv) = create_configured_native_mpv(
            &realize_state,
            dmabuf_target,
            render_size,
            wayland_scale_units(area),
        ) else {
            return;
        };
        let display = area.display();
        let native_wayland_display = wayland_display_resource(&display);
        if native_wayland_display.is_none() {
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &startup_launch,
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
                &startup_launch,
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
                    &startup_launch,
                    auto_fallback,
                    format!("Failed to capture the mpv native render handle: {error}"),
                );
                return;
            }
        };
        if !plane.release_current() {
            mpv.destroy_render_context();
            schedule_gtk_mpv_fallback(
                &fallback_container,
                &realize_fallback_started,
                &realize_state,
                &startup_launch,
                auto_fallback,
                "Failed to transfer the native Wayland/EGL context to the render thread".to_owned(),
            );
            return;
        }
        let render_loop = match NativeRenderLoop::start(Arc::clone(&plane), update_handle) {
            Ok(render_loop) => render_loop,
            Err(error) => {
                let _ = plane.make_current();
                mpv.destroy_render_context();
                schedule_gtk_mpv_fallback(
                    &fallback_container,
                    &realize_fallback_started,
                    &realize_state,
                    &startup_launch,
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
                &startup_launch,
                auto_fallback,
                format!("Failed to install the mpv native render callback: {error}"),
            );
            return;
        }

        start_event_pump_for_session(&mut mpv);
        {
            let mut state = realize_state.borrow_mut();
            state.native_video_plane = Some(plane);
            state.native_render_loop = Some(render_loop);
            state.mpv = Some(mpv);
        }
        if let Some(surface) = area.native().and_then(|native| native.surface())
            && surface.find_property("scale").is_some()
        {
            let scale_state = Rc::clone(&realize_state);
            let scale_area = area.clone();
            surface.connect_notify_local(Some("scale"), move |_, _| {
                let state = scale_state.borrow();
                if let Some(plane) = state.native_video_plane.as_ref() {
                    plane.resize(
                        scale_area.width(),
                        scale_area.height(),
                        native_surface_scale(&scale_area),
                    );
                }
                if let Some(mpv) = state.mpv.as_ref() {
                    let size = native_render_size(
                        scale_area.width(),
                        scale_area.height(),
                        native_surface_scale(&scale_area),
                    );
                    if let Err(error) =
                        mpv.set_wayland_dmabuf_geometry(size, wayland_scale_units(&scale_area))
                    {
                        eprintln!("Failed to update the embedded Wayland video scale: {error}");
                    }
                }
                if let Some(render_loop) = state.native_render_loop.as_ref() {
                    render_loop.request_render();
                }
            });
        }
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);
        mark_startup_player_ready(&startup_launch, &realize_state);
        area.queue_draw();
    });

    let resize_state = Rc::clone(&state);
    video_area.connect_resize(move |area, width, height| {
        let state = resize_state.borrow();
        if let Some(plane) = state.native_video_plane.as_ref() {
            plane.resize(width, height, native_surface_scale(area));
        }
        if let Some(mpv) = state.mpv.as_ref() {
            let size = native_render_size(width, height, native_surface_scale(area));
            if let Err(error) = mpv.set_wayland_dmabuf_geometry(size, wayland_scale_units(area)) {
                eprintln!("Failed to resize the embedded Wayland video surface: {error}");
            }
        }
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |_| {
        let mut state = unrealize_state.borrow_mut();
        let uses_wayland_dmabuf = state
            .mpv
            .as_ref()
            .is_some_and(okp_mpv::Mpv::uses_wayland_dmabuf);
        if let Some(mpv) = state.mpv.as_mut() {
            let _ = unsafe { mpv.set_render_update_callback(None, std::ptr::null_mut()) };
        }
        let render_joined = if let Some(mut render_loop) = state.native_render_loop.take() {
            render_loop.stop_and_join()
        } else {
            true
        };
        if !render_joined {
            // Render thread may still call into the mpv render context / EGL
            // plane. Skip teardown and leak until process exit (close → quit).
            if let Some(mpv) = state.mpv.take() {
                std::mem::forget(mpv);
            }
            state.native_video_plane = None;
            return;
        }
        if let Some(plane) = state.native_video_plane.as_ref() {
            let _ = plane.make_current();
        }
        if let Some(mpv) = state.mpv.as_mut() {
            mpv.destroy_render_context();
        }
        if uses_wayland_dmabuf {
            state.mpv.take();
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
    startup_launch: &StartupLaunchGate,
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
    let startup_launch = Rc::clone(startup_launch);
    glib::idle_add_local_once(move || {
        state.borrow_mut().presentation_recorder = PresentationRecorder::from_env(
            okp_core::presentation_evidence::PresentationBackend::GtkGlArea,
        );
        let area = gtk_video_area();
        connect_gtk_mpv(&area, Rc::clone(&state), startup_launch);
        container.add_named(&area, Some("gtk-glarea-fallback"));
        container.set_visible_child(&area);
        // A child added during native fallback may not realize until a later layout pass. Start
        // Gtk/libmpv now; the shared startup gate still prevents payload delivery until the main
        // window's map edge has been observed.
        gtk::prelude::WidgetExt::realize(&area);
    });
}

fn connect_gtk_mpv(
    video_area: &gtk::GLArea,
    state: Rc<RefCell<PlayerState>>,
    startup_launch: StartupLaunchGate,
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
        start_event_pump_for_session(&mut mpv);

        realize_state.borrow_mut().mpv = Some(mpv);
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);
        mark_startup_player_ready(&startup_launch, &realize_state);
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

fn connect_software_mpv(
    video_area: &gtk::DrawingArea,
    state: Rc<RefCell<PlayerState>>,
    startup_launch: StartupLaunchGate,
) {
    let realize_state = Rc::clone(&state);
    video_area.connect_realize(move |_| {
        let Some(mut mpv) = create_configured_mpv(&realize_state) else {
            fail_software_renderer(
                &realize_state,
                "libmpv software player initialization failed",
            );
            return;
        };
        if let Err(error) = mpv.create_software_render_context() {
            eprintln!("Failed to create libmpv software render context: {error}");
            fail_software_renderer(
                &realize_state,
                format!("libmpv software render context initialization failed: {error}"),
            );
            return;
        }
        eprintln!(
            "Software renderer: backend=libmpv-software format={} scene-renderer=cairo",
            okp_mpv::software_render_format()
        );
        start_event_pump_for_session(&mut mpv);
        realize_state.borrow_mut().mpv = Some(mpv);
        schedule_audio_device_restore(&realize_state);
        try_pending_audio_device_restore(&realize_state);
        mark_startup_player_ready(&startup_launch, &realize_state);
    });

    let frame = Rc::new(RefCell::new(None::<cairo::ImageSurface>));
    let draw_frame = Rc::clone(&frame);
    let draw_state = Rc::clone(&state);
    let render_failed = Rc::new(Cell::new(false));
    let draw_render_failed = Rc::clone(&render_failed);
    video_area.set_draw_func(move |_, context, width, height| {
        if width <= 0 || height <= 0 || draw_render_failed.get() {
            return;
        }

        let mut frame = draw_frame.borrow_mut();
        let recreate = frame
            .as_ref()
            .is_none_or(|surface| surface.width() != width || surface.height() != height);
        if recreate {
            match cairo::ImageSurface::create(cairo::Format::Rgb24, width, height) {
                Ok(surface) => *frame = Some(surface),
                Err(error) => {
                    draw_render_failed.set(true);
                    fail_software_renderer(
                        &draw_state,
                        format!("software video surface allocation failed: {error}"),
                    );
                    return;
                }
            }
        }

        let surface = frame.as_mut().expect("software surface was created");
        let stride = surface.stride() as usize;
        surface.flush();
        let render_result = match surface.data() {
            Ok(mut pixels) => draw_state
                .borrow_mut()
                .mpv
                .as_mut()
                .map(|mpv| mpv.render_software(width, height, stride, pixels.as_mut()))
                .transpose(),
            Err(error) => {
                draw_render_failed.set(true);
                fail_software_renderer(
                    &draw_state,
                    format!("software video pixel access failed: {error}"),
                );
                return;
            }
        };
        if let Err(error) = render_result {
            draw_render_failed.set(true);
            fail_software_renderer(
                &draw_state,
                format!("libmpv software frame render failed: {error}"),
            );
            return;
        }
        surface.mark_dirty();
        if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
            let _ = context.paint();
        }
        let state = draw_state.borrow();
        if let Some(recorder) = state.presentation_recorder.as_ref()
            && state.mpv.is_some()
        {
            recorder.record_present(
                okp_mpv::RenderTargetSize { width, height },
                "libmpv-software-cairo",
            );
        }
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |_| {
        if let Some(mpv) = unrealize_state.borrow_mut().mpv.as_mut() {
            mpv.destroy_render_context();
        }
    });

    let tick_area = video_area.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_area.queue_draw();
        glib::ControlFlow::Continue
    });
}

pub(crate) fn fail_software_renderer(state: &Rc<RefCell<PlayerState>>, detail: impl Into<String>) {
    let detail = detail.into();
    eprintln!("Software renderer unavailable: {detail}");
    if let Some(mpv) = state.borrow().mpv.as_ref()
        && let Err(error) = mpv.stop()
    {
        eprintln!("Failed to stop playback after software renderer failure: {error}");
    }
    let mut state = state.borrow_mut();
    state.media_load_state = network_media::MediaLoadState::Failed;
    state.last_load_diagnostic = Some(
        okp_core::playback_failure::PlaybackFailureDiagnostic::flatpak_dri_unavailable(detail),
    );
}

fn create_configured_mpv(state: &Rc<RefCell<PlayerState>>) -> Option<Mpv> {
    let (hwdec, raw_mpv_options) = mpv_creation_options(state);
    let mpv = create_regular_mpv(&hwdec, &raw_mpv_options)?;
    Some(finish_configured_mpv(state, mpv))
}

fn create_configured_native_mpv(
    state: &Rc<RefCell<PlayerState>>,
    target: Option<okp_mpv::WaylandDmabufTarget>,
    size: okp_mpv::RenderTargetSize,
    scale: i32,
) -> Option<Mpv> {
    let (hwdec, raw_mpv_options) = mpv_creation_options(state);
    let presentation_log = state.borrow().presentation_recorder.is_some();
    if let Some(target) = target {
        let attempt = Mpv::try_new_with_wayland_dmabuf(
            &hwdec,
            &raw_mpv_options,
            target.clone(),
            size,
            scale,
            presentation_log,
        );
        match attempt {
            Ok(Some(mpv)) => return Some(finish_configured_mpv(state, mpv)),
            Ok(None) => {}
            Err(error) if !raw_mpv_options.is_empty() => {
                eprintln!(
                    "Failed to create the Wayland DMA-BUF player with custom mpv.conf options: {error}; retrying without them"
                );
                match Mpv::try_new_with_wayland_dmabuf(
                    &hwdec,
                    &[],
                    target,
                    size,
                    scale,
                    presentation_log,
                ) {
                    Ok(Some(mpv)) => return Some(finish_configured_mpv(state, mpv)),
                    Ok(None) => {}
                    Err(error) => eprintln!(
                        "Wayland DMA-BUF initialization failed: {error}; using the OpenGL render API"
                    ),
                }
            }
            Err(error) => eprintln!(
                "Wayland DMA-BUF initialization failed: {error}; using the OpenGL render API"
            ),
        }
    }

    let mpv = create_regular_mpv(&hwdec, &raw_mpv_options)?;
    Some(finish_configured_mpv(state, mpv))
}

fn mpv_creation_options(state: &Rc<RefCell<PlayerState>>) -> (String, Vec<(String, String)>) {
    let (hwdec, raw_mpv_config) = {
        let state = state.borrow();
        (
            state.settings.hardware_decode_mpv_option().to_owned(),
            state.settings.raw_mpv_config().to_owned(),
        )
    };
    let options = match parse_raw_mpv_config(&raw_mpv_config) {
        Ok(options) => options,
        Err(error) => {
            eprintln!(
                "Ignoring custom mpv.conf option at line {}: {}",
                error.line, error.message
            );
            Vec::new()
        }
    };
    let hwdec = configured_linux_renderer_mode()
        .mpv_hwdec_override()
        .map(str::to_owned)
        .unwrap_or(hwdec);
    (hwdec, options)
}

fn create_regular_mpv(hwdec: &str, raw_mpv_options: &[(String, String)]) -> Option<Mpv> {
    Some(match Mpv::new_with_options(hwdec, raw_mpv_options) {
        Ok(mpv) => mpv,
        Err(error) if !raw_mpv_options.is_empty() => {
            eprintln!(
                "Failed to create mpv with custom mpv.conf options: {error}; retrying without them"
            );
            match Mpv::new_with_hwdec(hwdec) {
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
    })
}

fn finish_configured_mpv(state: &Rc<RefCell<PlayerState>>, mpv: Mpv) -> Mpv {
    let (subtitle_scale, subtitle_position, subtitle_style) = {
        let state = state.borrow();
        (
            state.settings.subtitle_scale(),
            state.settings.subtitle_position(),
            state.settings.subtitle_style(),
        )
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
    mpv
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
        chrome,
        compact_mode,
        window_chrome,
        root_surface,
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
        drain_wayland_presentation_feedback(&state);
        apply_pending_nfo_titles(&state);
        observe_initial_window_fit(&state, auto_fit_dimensions);
        // Startup payload delivery is gated on the map edge, so initial fitting uses an
        // already-visible toplevel with compositor context. Wayland may publish desktop-wide
        // configure bounds before its output-enter event; retain the one-shot request until the
        // bounds can be tied to one monitor instead of consuming a spanning fit. Fullscreen and
        // maximized loads still consume their deliberate no-resize skip immediately.
        if window.is_mapped()
            && (window.is_fullscreen()
                || window.is_maximized()
                || player_window_fit_area_available(&window, &window_bounds))
            && let Some(request) = take_initial_window_fit(&state)
        {
            fit_player_window_to_video(
                &window,
                &window_bounds,
                request.video,
                PlayerWindowFitRequest::Initial,
            );
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
        sync_native_video_background(&window, &root_surface, has_media);
        let seek_preview = env::var_os("OKP_OPEN_SEEK_PREVIEW_ON_STARTUP").is_some();
        let command_preview = env::var_os("OKP_OPEN_MORE_POPOVER_ON_STARTUP").is_some();
        let has_chapters = state
            .borrow()
            .mpv
            .as_ref()
            .is_some_and(|mpv| !mpv.observed_chapters().is_empty());
        chrome.set_has_media(has_media || seek_preview || command_preview);
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
        let failed = state.borrow().media_load_state == network_media::MediaLoadState::Failed;
        let idle_surface_hidden = has_media || failed || lyrics_surface.is_preview_frozen();
        // Welcome/History owns its poster controller only while it owns the window. Continuing
        // to refresh it behind loading or playback can leave ffmpeg work (and its pipes) alive
        // through a local open and the following EOF transition.
        if idle_surface_hidden {
            thumbnails::suspend_poster_generation();
        } else {
            empty_surface.refresh(&window, &state, Rc::clone(&status_toast));
        }
        // Hide the welcome surface behind an active lyrics preview so the fixture reads cleanly;
        // in production the loaded audio already hides it (`is_preview_frozen` stays false).
        empty_surface.set_has_media(idle_surface_hidden);
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

fn sync_native_video_background(
    window: &gtk::ApplicationWindow,
    root_surface: &gtk::Overlay,
    has_media: bool,
) {
    for widget in [
        window.upcast_ref::<gtk::Widget>(),
        root_surface.upcast_ref(),
    ] {
        if has_media {
            widget.add_css_class("has-active-video-plane");
        } else {
            widget.remove_css_class("has-active-video-plane");
        }
    }
}

fn drain_wayland_presentation_feedback(state: &Rc<RefCell<PlayerState>>) {
    let (recorder, dmabuf_feedback, egl_feedback) = {
        let state = state.borrow();
        let dmabuf_feedback = state
            .mpv
            .as_ref()
            .map(Mpv::take_wayland_presentation_feedback)
            .unwrap_or_default();
        let egl_feedback = state
            .native_video_plane
            .as_ref()
            .map(|plane| plane.take_presentation_feedback())
            .unwrap_or_default();
        (
            state.presentation_recorder.clone(),
            dmabuf_feedback,
            egl_feedback,
        )
    };
    if let Some(recorder) = recorder {
        for feedback in dmabuf_feedback {
            recorder.record_wayland_feedback(
                okp_core::presentation_evidence::PresentationBackend::NativeWaylandDmabuf,
                feedback,
            );
        }
        for feedback in egl_feedback {
            recorder.record_wayland_feedback(
                okp_core::presentation_evidence::PresentationBackend::NativeWaylandEgl,
                feedback,
            );
        }
    }
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
    let (load_state, can_retry, failure) = {
        let state = state.borrow();
        (
            state.media_load_state,
            state.retry_load_source.is_some(),
            state.last_load_diagnostic.clone(),
        )
    };
    overlay.update(
        load_state,
        has_media,
        playback.is_some_and(|playback| playback.paused),
        can_retry,
        failure.as_ref(),
    );
}

pub(crate) fn connect_video_clicks(
    video_area: &gtk::Widget,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    suppress_video_click: Rc<Cell<bool>>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    let reset_suppression = Rc::clone(&suppress_video_click);
    click.connect_pressed(move |_, _, _, _| reset_suppression.set(false));

    let click_window = window.clone();
    let click_state = Rc::clone(&state);
    let pending_single_click = Rc::new(RefCell::new(None::<glib::SourceId>));
    let pending_click = Rc::clone(&pending_single_click);
    click.connect_released(move |_, press_count, _, _| {
        if suppress_video_click.replace(false) {
            if let Some(source_id) = pending_click.borrow_mut().take() {
                source_id.remove();
            }
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: video-click-suppressed-by-window-drag");
            }
            return;
        }
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

/// Restore #281's whole-surface behavior: a left-drag that clears the movement
/// threshold anywhere on a non-interactive, non-OSC surface (video, letterbox,
/// empty/title background, idle canvas) begins a compositor-native window move.
/// A click that stays under the threshold falls through to the play/pause and
/// double-click-fullscreen gestures untouched, and interactive surfaces keep
/// their own input.
pub(crate) fn connect_player_window_move(
    player_root: &gtk::Overlay,
    window: &gtk::ApplicationWindow,
) -> Rc<Cell<bool>> {
    let suppress_video_click = Rc::new(Cell::new(false));
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Bubble);

    let move_root = player_root.clone();
    let move_window = window.clone();
    let already_moving = Rc::new(Cell::new(false));
    let update_moving = Rc::clone(&already_moving);
    let update_suppression = Rc::clone(&suppress_video_click);
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        // Compact mode owns its own drag-to-move (and snap) gesture; leave it be
        // so a single drag never begins two moves.
        if window_compact_mode_active(&move_window) {
            return;
        }

        // The press target decides interactivity, so classify the drag's start
        // point with the same rules the right-click menu uses. A missing pick is
        // treated as interactive to fail safe and preserve the click.
        let over_interactive = gesture
            .start_point()
            .and_then(|(x, y)| move_root.pick(x, y, gtk::PickFlags::INSENSITIVE))
            .map(|target| player_context_menu_target_is_interactive(&move_root, &target))
            .unwrap_or(true);

        let context = video_click::WindowDragContext {
            fullscreen: move_window.is_fullscreen(),
            maximized: move_window.is_maximized(),
            over_interactive,
            already_moving: update_moving.get(),
        };
        match video_click::window_drag_action(context, offset_x, offset_y) {
            video_click::WindowDragAction::Hold => {}
            video_click::WindowDragAction::BeginMove => {
                update_moving.set(true);
                update_suppression.set(true);
                if !begin_native_window_move_from_drag(gesture, &move_root, &move_window) {
                    update_moving.set(false);
                    update_suppression.set(false);
                    return;
                }
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: player-window-move");
                }
            }
        }
    });
    let end_moving = Rc::clone(&already_moving);
    drag.connect_drag_end(move |_, _, _| end_moving.set(false));
    let cancel_moving = Rc::clone(&already_moving);
    drag.connect_cancel(move |_, _| cancel_moving.set(false));

    player_root.add_controller(drag);
    suppress_video_click
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
    popover.set_position(if x < f64::from(parent.width()) / 2.0 {
        gtk::PositionType::Right
    } else {
        gtk::PositionType::Left
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

#[cfg(test)]
mod native_render_notifier_tests {
    use super::*;

    #[test]
    fn screenshot_render_upgrades_a_coalesced_update() {
        let notifier = NativeRenderNotifier::new();

        notifier.notify();
        notifier.force_render();

        assert_eq!(
            notifier.wait(),
            Some(NativeRenderRequest {
                pending: true,
                force: true,
            })
        );
    }
}
