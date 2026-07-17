use super::*;

const COMPACT_ACTION: &str = "compact-mode";

pub(crate) fn apply_compact_accessibility_classes(window: &gtk::ApplicationWindow) {
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
    }
    if env::var_os("OKP_REDUCE_TRANSPARENCY").is_some() {
        window.add_css_class("is-reduced-transparency");
    }
}

#[derive(Debug, Clone, Copy)]
struct NormalWindowState {
    width: i32,
    height: i32,
    maximized: bool,
    always_on_top: bool,
}

#[derive(Clone)]
pub(crate) struct CompactMode {
    window: gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
    active: Rc<Cell<bool>>,
    normal_state: Rc<RefCell<Option<NormalWindowState>>>,
    startup_requested: Rc<Cell<bool>>,
    aspect_lock_after: Rc<Cell<Option<Instant>>>,
    surface_restore_pending: Rc<Cell<bool>>,
    top_bar: gtk::Box,
    play_button: gtk::Button,
    bottom_bar: gtk::Box,
    title_label: gtk::Label,
    elapsed_label: gtk::Label,
    seek: gtk::Scale,
    updating_seek: Rc<Cell<bool>>,
    standard_chrome: gtk::Revealer,
    side_panel: gtk::Revealer,
    empty_surface: gtk::Revealer,
    resize_handles: Vec<gtk::Box>,
    normal_always_on_top: Rc<Cell<bool>>,
}

pub(crate) struct CompactModeInputs<'a> {
    pub(crate) state: Rc<RefCell<PlayerState>>,
    pub(crate) status_toast: Rc<StatusToast>,
    pub(crate) chrome: &'a Rc<ChromeVisibility>,
    pub(crate) window_chrome: &'a PlayerWindowChrome,
    pub(crate) controls: &'a Controls,
    pub(crate) empty_surface: &'a EmptySurface,
    pub(crate) resize_handles: Vec<gtk::Box>,
}

impl CompactMode {
    pub(crate) fn build(window: &gtk::ApplicationWindow, inputs: CompactModeInputs<'_>) -> Self {
        let CompactModeInputs {
            state,
            status_toast,
            chrome,
            window_chrome,
            controls,
            empty_surface,
            resize_handles,
        } = inputs;
        let top_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top_bar.add_css_class("okp-compact-top-bar");
        top_bar.add_css_class("okp-compact-motion");
        top_bar.set_halign(gtk::Align::Fill);
        top_bar.set_valign(gtk::Align::Start);
        top_bar.set_margin_top(8);
        top_bar.set_margin_start(8);
        top_bar.set_margin_end(8);

        let restore_button = compact_button("view-restore-symbolic", "Restore player");
        restore_button.add_css_class("okp-compact-restore");

        let title_label = gtk::Label::new(None);
        title_label.add_css_class("okp-compact-title");
        title_label.set_hexpand(true);
        title_label.set_xalign(0.0);
        title_label.set_ellipsize(pango::EllipsizeMode::End);
        title_label.set_width_chars(1);
        title_label.set_max_width_chars(1);

        let close_button = compact_button("window-close-symbolic", "Close media");
        close_button.add_css_class("okp-compact-close");
        top_bar.append(&restore_button);
        top_bar.append(&title_label);
        top_bar.append(&close_button);

        let play_button = compact_button("media-playback-start-symbolic", "Play / Pause (Space)");
        play_button.add_css_class("okp-compact-play");
        play_button.add_css_class("okp-compact-motion");
        play_button.set_halign(gtk::Align::Center);
        play_button.set_valign(gtk::Align::Center);

        let bottom_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bottom_bar.add_css_class("okp-compact-bottom-bar");
        bottom_bar.add_css_class("okp-compact-motion");
        bottom_bar.set_halign(gtk::Align::Fill);
        bottom_bar.set_valign(gtk::Align::End);
        bottom_bar.set_margin_start(10);
        bottom_bar.set_margin_end(10);
        bottom_bar.set_margin_bottom(9);

        let elapsed_label = gtk::Label::new(Some("00:00"));
        elapsed_label.add_css_class("okp-compact-time");

        let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        seek.add_css_class("okp-compact-seek");
        seek.set_draw_value(false);
        seek.set_hexpand(true);
        seek.set_sensitive(false);
        bottom_bar.append(&elapsed_label);
        bottom_bar.append(&seek);

        chrome.add_linked_motion_widget(&top_bar);
        chrome.add_linked_motion_widget(&play_button);
        chrome.add_linked_motion_widget(&bottom_bar);

        let mode = Self {
            window: window.clone(),
            state: Rc::clone(&state),
            status_toast: Rc::clone(&status_toast),
            chrome: Rc::clone(chrome),
            active: Rc::new(Cell::new(false)),
            normal_state: Rc::new(RefCell::new(None)),
            startup_requested: Rc::new(Cell::new(env::var_os("OKP_START_COMPACT").is_some())),
            aspect_lock_after: Rc::new(Cell::new(None)),
            surface_restore_pending: Rc::new(Cell::new(false)),
            top_bar,
            play_button,
            bottom_bar,
            title_label,
            elapsed_label,
            seek,
            updating_seek: Rc::new(Cell::new(false)),
            standard_chrome: window_chrome.widget().clone(),
            side_panel: controls.up_next_revealer.clone(),
            empty_surface: empty_surface.widget().clone(),
            resize_handles,
            normal_always_on_top: window_chrome.always_on_top_state(),
        };

        let restore_window = window.clone();
        restore_button.connect_clicked(move |_| {
            toggle_compact_mode(&restore_window);
        });

        let close_state = Rc::clone(&state);
        let close_toast = Rc::clone(&status_toast);
        close_button.connect_clicked(move |_| {
            close_current_media(&close_state, &close_toast);
        });

        let play_state = Rc::clone(&state);
        mode.play_button.connect_clicked(move |_| {
            toggle_play_pause(&play_state);
        });

        let seek_state = Rc::clone(&state);
        let updating_seek = Rc::clone(&mode.updating_seek);
        mode.seek.connect_change_value(move |_, _, value| {
            if !updating_seek.get() {
                seek_absolute(&seek_state, value);
            }
            glib::Propagation::Proceed
        });

        mode.install_action();
        if mode.startup_requested.get() {
            mode.top_bar.set_visible(false);
            mode.play_button.set_visible(false);
            mode.bottom_bar.set_visible(false);
            mode.standard_chrome.set_visible(false);
            mode.side_panel.set_visible(false);
            mode.empty_surface.set_visible(false);
            mode.chrome.set_surface_suppressed(true);
        } else {
            mode.set_surfaces_visible(false);
        }
        mode
    }

    pub(crate) fn overlays(&self) -> [&gtk::Widget; 3] {
        [
            self.top_bar.upcast_ref(),
            self.play_button.upcast_ref(),
            self.bottom_bar.upcast_ref(),
        ]
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    pub(crate) fn sync_surface_visibility(&self) {
        if (self.startup_requested.get() && !self.is_active()) || self.surface_restore_pending.get()
        {
            return;
        }
        self.set_surfaces_visible(self.is_active());
    }

    pub(crate) fn update(&self, has_media: bool, playback: Option<PlaybackState>, title: &str) {
        if self.startup_requested.get()
            && !self.is_active()
            && has_media
            && self.window.is_mapped()
            && self.window.width() > 1
            && self.window.height() > 1
            && self.state.borrow().current_video_dimensions.is_some()
        {
            self.startup_requested.set(false);
            toggle_compact_mode(&self.window);
        }

        if self.is_active() && !has_media {
            toggle_compact_mode(&self.window);
            return;
        }
        if !self.is_active() {
            return;
        }

        self.correct_aspect();

        self.title_label.set_text(title);
        let Some(playback) = playback else {
            self.play_button
                .set_icon_name("media-playback-start-symbolic");
            self.elapsed_label.set_text("00:00");
            self.seek.set_sensitive(false);
            return;
        };

        self.play_button.set_icon_name(if playback.paused {
            "media-playback-start-symbolic"
        } else {
            "media-playback-pause-symbolic"
        });
        let duration = playback.duration.unwrap_or(0.0).max(0.0);
        let raw_time = playback.time_pos.unwrap_or(0.0).max(0.0);
        let time_pos = if duration > 0.0 {
            raw_time.min(duration)
        } else {
            raw_time
        };
        self.elapsed_label
            .set_text(&time_code::format_clock(time_pos));
        self.seek.set_sensitive(duration > 0.0);
        self.updating_seek.set(true);
        self.seek.set_range(0.0, duration.max(1.0));
        self.seek.set_value(time_pos);
        self.updating_seek.set(false);
    }

    fn install_action(&self) {
        let action =
            gtk::gio::SimpleAction::new_stateful(COMPACT_ACTION, None, &false.to_variant());
        let mode = self.clone();
        action.connect_activate(move |action, _| {
            let enabled = !mode.is_active();
            if mode.set_active(enabled) {
                action.set_state(&enabled.to_variant());
            } else {
                action.set_state(&mode.is_active().to_variant());
            }
        });
        self.window.add_action(&action);
    }

    fn set_active(&self, enabled: bool) -> bool {
        if enabled == self.is_active() {
            return true;
        }
        if enabled {
            self.enter()
        } else {
            self.restore();
            true
        }
    }

    fn enter(&self) -> bool {
        if !has_loaded_media(&self.state) {
            self.status_toast.show("Open media first");
            return false;
        }
        let dimensions = self
            .state
            .borrow()
            .current_video_dimensions
            .unwrap_or(VideoDimensions {
                width: 16,
                height: 9,
            });
        let default_short_edge = if env::var_os("OKP_COMPACT_START_AT_FLOOR").is_some() {
            window_fit::COMPACT_MIN_SHORT_EDGE
        } else {
            window_fit::COMPACT_DEFAULT_SHORT_EDGE
        };
        let Some(size) = window_fit::compact_size_for_video(
            dimensions.width,
            dimensions.height,
            default_short_edge,
        ) else {
            return false;
        };
        let Some(floor) = window_fit::compact_size_for_video(
            dimensions.width,
            dimensions.height,
            window_fit::COMPACT_MIN_SHORT_EDGE,
        ) else {
            return false;
        };

        let normal_state = NormalWindowState {
            width: self.window.width().max(1),
            height: self.window.height().max(1),
            maximized: self.window.is_maximized(),
            always_on_top: self.normal_always_on_top.get(),
        };
        self.normal_state.borrow_mut().replace(normal_state);
        if self.window.is_fullscreen() {
            self.window.unfullscreen();
        }
        if self.window.is_maximized() {
            self.window.unmaximize();
        }

        self.active.set(true);
        self.window.add_css_class("is-compact-mode");
        self.window.set_size_request(floor.width, floor.height);
        self.window.set_default_size(size.width, size.height);
        resize_player_window_on_x11(&self.window, size);
        self.aspect_lock_after
            .set(Some(Instant::now() + Duration::from_millis(500)));
        if set_window_always_on_top(&self.window, true) == AlwaysOnTopResult::Unavailable {
            self.status_toast
                .show("Always on top is unavailable on this desktop");
        }
        self.set_surfaces_visible(true);
        self.chrome.show_for_activity();
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!(
                "interaction: compact-mode-enter size={}x{} floor={}x{}",
                size.width, size.height, floor.width, floor.height
            );
            eprintln!(
                "interaction: compact-mode-widgets top={}/{} play={}/{} bottom={}/{}",
                self.top_bar.is_visible(),
                self.top_bar.is_mapped(),
                self.play_button.is_visible(),
                self.play_button.is_mapped(),
                self.bottom_bar.is_visible(),
                self.bottom_bar.is_mapped(),
            );
        }
        true
    }

    fn restore(&self) {
        self.active.set(false);
        self.surface_restore_pending.set(true);
        self.window.remove_css_class("is-compact-mode");
        self.window.set_size_request(-1, -1);
        self.aspect_lock_after.set(None);
        self.top_bar.set_visible(false);
        self.play_button.set_visible(false);
        self.bottom_bar.set_visible(false);
        self.standard_chrome.set_visible(false);
        self.side_panel.set_visible(false);
        self.empty_surface.set_visible(false);
        self.chrome.set_surface_suppressed(true);
        if let Some(normal) = self.normal_state.borrow_mut().take() {
            let size = window_fit::WindowSize {
                width: normal.width,
                height: normal.height,
            };
            self.window.set_default_size(size.width, size.height);
            resize_player_window_on_x11(&self.window, size);
            let _ = set_window_always_on_top(&self.window, normal.always_on_top);
            if normal.maximized {
                let window = self.window.clone();
                glib::idle_add_local_once(move || window.maximize());
            }
        }
        let restored_mode = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            restored_mode.surface_restore_pending.set(false);
            restored_mode.set_surfaces_visible(false);
        });
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: compact-mode-restore");
        }
    }

    fn correct_aspect(&self) {
        if self
            .aspect_lock_after
            .get()
            .is_none_or(|deadline| Instant::now() < deadline)
        {
            return;
        }
        let Some(dimensions) = self.state.borrow().current_video_dimensions else {
            return;
        };
        let max_size = self
            .window
            .surface()
            .and_then(|surface| surface.display().monitor_at_surface(&surface))
            .map(|monitor| monitor.geometry())
            .map(|geometry| (geometry.width(), geometry.height()))
            .unwrap_or((i32::MAX, i32::MAX));
        if let Some(corrected) = window_fit::fill_client(
            dimensions.width,
            dimensions.height,
            self.window.width(),
            self.window.height(),
            max_size.0,
            max_size.1,
        ) {
            self.window
                .set_default_size(corrected.width, corrected.height);
            resize_player_window_on_x11(&self.window, corrected);
        }
    }

    fn set_surfaces_visible(&self, compact: bool) {
        self.top_bar.set_visible(compact);
        self.play_button.set_visible(compact);
        self.bottom_bar.set_visible(compact);
        self.standard_chrome
            .set_visible(!compact && !self.window.is_fullscreen());
        self.side_panel.set_visible(!compact);
        self.empty_surface.set_visible(!compact);
        self.chrome.set_surface_suppressed(compact);
        for handle in &self.resize_handles {
            if compact {
                handle.add_css_class("is-compact");
            } else {
                handle.remove_css_class("is-compact");
            }
        }
    }
}

pub(crate) fn window_compact_mode_active(window: &gtk::ApplicationWindow) -> bool {
    window
        .lookup_action(COMPACT_ACTION)
        .and_then(|action| action.state())
        .and_then(|state| state.get::<bool>())
        .unwrap_or(false)
}

pub(crate) fn restore_compact_mode(window: &gtk::ApplicationWindow) -> bool {
    if !window_compact_mode_active(window) {
        return false;
    }
    toggle_compact_mode(window);
    true
}

pub(crate) fn toggle_compact_mode(window: &gtk::ApplicationWindow) {
    gtk::prelude::ActionGroupExt::activate_action(window, COMPACT_ACTION, None);
}

fn compact_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("okp-compact-button");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    button
}

pub(crate) fn connect_compact_video_interactions(
    video: &gtk::Widget,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    let drag_window = window.clone();
    let drag_started = Rc::new(Cell::new(false));
    let update_started = Rc::clone(&drag_started);
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        if update_started.get()
            || !window_compact_mode_active(&drag_window)
            || !video_click::drag_exceeds_move_threshold(offset_x, offset_y, 6.0)
        {
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
        let Some((x, y)) = gesture.bounding_box_center() else {
            return;
        };
        update_started.set(true);
        gesture.set_state(gtk::EventSequenceState::Claimed);
        toplevel.begin_move(
            &device,
            gesture.current_button() as i32,
            x,
            y,
            gesture.current_event_time(),
        );
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: compact-mode-drag");
        }
        schedule_compact_snap(drag_window.clone(), Rc::clone(&update_started));
    });
    drag.connect_drag_end(move |_, _, _| drag_started.set(false));
    video.add_controller(drag);

    let scroll = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
    );
    let scroll_window = window.clone();
    scroll.connect_scroll(move |controller, _, dy| {
        if !window_compact_mode_active(&scroll_window) {
            return glib::Propagation::Proceed;
        }
        let fine = controller
            .current_event_state()
            .contains(gdk::ModifierType::CONTROL_MASK);
        if let Some(delta) = volume_scroll_delta(dy, fine) {
            adjust_volume(&state, &status_toast, delta);
        }
        glib::Propagation::Stop
    });
    video.add_controller(scroll);
}

fn snap_compact_window_on_x11(window: &gtk::ApplicationWindow) {
    let Some(position) = current_player_position_on_x11(window) else {
        return;
    };
    let Some(surface) = window.surface() else {
        return;
    };
    let Some(monitor) = surface.display().monitor_at_surface(&surface) else {
        return;
    };
    let geometry = monitor.geometry();
    let work_area = window_fit::WindowRect {
        x: geometry.x(),
        y: geometry.y(),
        width: geometry.width(),
        height: geometry.height(),
    };
    let size = window_fit::WindowSize {
        width: window.width(),
        height: window.height(),
    };
    let Some(target) = window_fit::compact_corner_snap(
        position,
        size,
        work_area,
        window_fit::COMPACT_SNAP_INSET,
        window_fit::COMPACT_SNAP_THRESHOLD,
    ) else {
        return;
    };
    move_resize_player_window_on_x11(window, target, size);
    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
        eprintln!(
            "interaction: compact-mode-snap x={} y={}",
            target.x, target.y
        );
    }
}

fn schedule_compact_snap(window: gtk::ApplicationWindow, drag_started: Rc<Cell<bool>>) {
    let attempts = Rc::new(Cell::new(0_u16));
    glib::timeout_add_local(Duration::from_millis(50), move || {
        let attempt = attempts.get().saturating_add(1);
        attempts.set(attempt);
        match primary_pointer_down_on_x11(&window) {
            Some(true) if attempt < 600 => glib::ControlFlow::Continue,
            Some(false) => {
                drag_started.set(false);
                snap_compact_window_on_x11(&window);
                glib::ControlFlow::Break
            }
            _ => {
                drag_started.set(false);
                glib::ControlFlow::Break
            }
        }
    });
}
