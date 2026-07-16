use super::*;

pub(crate) const VOLUME_RESTING_SIZE: i32 = 34;
pub(crate) const VOLUME_WICK_WIDTH: i32 = 18;
pub(crate) const VOLUME_WICK_HEIGHT: i32 = 3;
pub(crate) const VOLUME_TRACK_WIDTH: i32 = 122;
pub(crate) const VOLUME_TRACK_HEIGHT: i32 = 6;
pub(crate) const VOLUME_THUMB_SIZE: i32 = 14;
pub(crate) const VOLUME_CAPSULE_OFFSET: i32 = 10;
pub(crate) const VOLUME_COLLAPSE_MS: u64 = 120;
pub(crate) const VOLUME_HOVER_GRACE_MS: u64 = 220;
pub(crate) const TIMELINE_HEIGHT: i32 = 20;
pub(crate) const TIMELINE_RAIL_HEIGHT: f64 = 4.0;
pub(crate) const TIMELINE_THUMB_DIAMETER: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TimelineLayerGeometry {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl TimelineLayerGeometry {
    pub(crate) fn center_y(self) -> f64 {
        self.y + self.height / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TimelineGeometry {
    pub(crate) trough: TimelineLayerGeometry,
    pub(crate) buffered: TimelineLayerGeometry,
    pub(crate) played: TimelineLayerGeometry,
    pub(crate) thumb_center_x: f64,
    pub(crate) thumb_center_y: f64,
}

pub(crate) fn timeline_geometry(
    width: i32,
    height: i32,
    played_fraction: f64,
    buffered_fraction: f64,
) -> TimelineGeometry {
    let inset = TIMELINE_THUMB_DIAMETER / 2.0;
    let rail_width = (f64::from(width) - TIMELINE_THUMB_DIAMETER).max(0.0);
    let center_y = f64::from(height) / 2.0;
    let rail_y = center_y - TIMELINE_RAIL_HEIGHT / 2.0;
    let played_fraction = played_fraction.clamp(0.0, 1.0);
    let buffered_fraction = buffered_fraction.clamp(played_fraction, 1.0);
    let layer = |fraction: f64| TimelineLayerGeometry {
        x: inset,
        y: rail_y,
        width: rail_width * fraction,
        height: TIMELINE_RAIL_HEIGHT,
    };

    TimelineGeometry {
        trough: layer(1.0),
        buffered: layer(buffered_fraction),
        played: layer(played_fraction),
        thumb_center_x: inset + rail_width * played_fraction,
        thumb_center_y: center_y,
    }
}

#[derive(Clone)]
pub(crate) struct TimelineRail {
    area: gtk::DrawingArea,
    buffered_fraction: Rc<Cell<f64>>,
    loading: Rc<Cell<bool>>,
    loading_phase: Rc<Cell<f64>>,
    marks: Rc<RefCell<Vec<TimelineMark>>>,
}

impl TimelineRail {
    fn new(adjustment: &gtk::Adjustment) -> Self {
        let area = gtk::DrawingArea::new();
        area.add_css_class("okp-timeline-rail");
        area.set_content_width(120);
        area.set_content_height(TIMELINE_HEIGHT);
        area.set_hexpand(true);
        area.set_can_target(false);

        let buffered_fraction = Rc::new(Cell::new(0.0));
        let loading = Rc::new(Cell::new(false));
        let loading_phase = Rc::new(Cell::new(0.0));
        let marks = Rc::new(RefCell::new(Vec::new()));
        let draw_adjustment = adjustment.clone();
        let draw_buffered_fraction = Rc::clone(&buffered_fraction);
        let draw_loading = Rc::clone(&loading);
        let draw_loading_phase = Rc::clone(&loading_phase);
        let draw_marks = Rc::clone(&marks);
        area.set_draw_func(move |_, cr, width, height| {
            let marks = draw_marks.borrow();
            let snapshot = TimelineDrawSnapshot {
                adjustment: &draw_adjustment,
                buffered_fraction: draw_buffered_fraction.get(),
                loading: draw_loading.get(),
                loading_phase: draw_loading_phase.get(),
                marks: &marks,
            };
            draw_timeline_rail(cr, width, height, snapshot);
        });

        let redraw = area.clone();
        adjustment.connect_value_changed(move |_| redraw.queue_draw());
        let redraw = area.clone();
        adjustment.connect_changed(move |_| redraw.queue_draw());

        Self {
            area,
            buffered_fraction,
            loading,
            loading_phase,
            marks,
        }
    }

    fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub(crate) fn set_buffered_fraction(&self, fraction: f64) {
        self.buffered_fraction.set(fraction.clamp(0.0, 1.0));
        self.area.queue_draw();
    }

    pub(crate) fn set_loading(&self, loading: bool) {
        if self.loading.replace(loading) != loading {
            self.area.queue_draw();
        }
    }

    pub(crate) fn pulse(&self) {
        self.loading_phase
            .set((self.loading_phase.get() + 0.08).rem_euclid(1.0));
        self.area.queue_draw();
    }

    pub(crate) fn set_marks(&self, marks: Vec<TimelineMark>) {
        if *self.marks.borrow() == marks {
            return;
        }
        self.marks.replace(marks);
        self.area.queue_draw();
    }
}

struct TimelineDrawSnapshot<'a> {
    adjustment: &'a gtk::Adjustment,
    buffered_fraction: f64,
    loading: bool,
    loading_phase: f64,
    marks: &'a [TimelineMark],
}

fn draw_timeline_rail(
    cr: &cairo::Context,
    width: i32,
    height: i32,
    snapshot: TimelineDrawSnapshot<'_>,
) {
    let lower = snapshot.adjustment.lower();
    let upper = snapshot.adjustment.upper();
    let range = upper - lower;
    let played_fraction = if range.is_finite() && range > 0.0 {
        ((snapshot.adjustment.value() - lower) / range).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let geometry = timeline_geometry(width, height, played_fraction, snapshot.buffered_fraction);
    let rail_center_y = geometry.trough.center_y();

    fill_timeline_layer(cr, geometry.trough, (1.0, 1.0, 1.0, 0.22));
    if snapshot.loading {
        let segment_fraction = 0.22;
        let travel = geometry.trough.width * (1.0 + segment_fraction);
        let segment_x = geometry.trough.x - geometry.trough.width * segment_fraction
            + travel * snapshot.loading_phase;
        let segment = TimelineLayerGeometry {
            x: segment_x.max(geometry.trough.x),
            y: geometry.trough.y,
            width: (geometry.trough.width * segment_fraction)
                .min(geometry.trough.x + geometry.trough.width - segment_x)
                .max(0.0),
            height: geometry.trough.height,
        };
        fill_timeline_layer(cr, segment, (1.0, 1.0, 1.0, 0.62));
    } else {
        fill_timeline_layer(cr, geometry.buffered, (1.0, 1.0, 1.0, 0.46));
    }
    fill_timeline_layer(
        cr,
        geometry.played,
        (40.0 / 255.0, 179.0 / 255.0, 170.0 / 255.0, 1.0),
    );

    for mark in snapshot.marks {
        if !range.is_finite() || range <= 0.0 {
            continue;
        }
        let fraction = ((mark.time - lower) / range).clamp(0.0, 1.0);
        let x = geometry.trough.x + geometry.trough.width * fraction;
        let (mark_width, mark_height, color, label) = match mark.kind {
            TimelineMarkKind::Chapter => (2.0, 7.0, (1.0, 1.0, 1.0, 0.42), None),
            TimelineMarkKind::Interval => (1.5, 5.0, (1.0, 1.0, 1.0, 0.25), None),
            TimelineMarkKind::Bookmark => (3.0, 9.0, (0.91, 0.69, 0.29, 0.96), None),
            TimelineMarkKind::AbStart => (3.0, 9.0, (0.91, 0.69, 0.29, 0.96), Some("A")),
            TimelineMarkKind::AbEnd => (3.0, 9.0, (0.91, 0.69, 0.29, 0.96), Some("B")),
            TimelineMarkKind::AbLoop => (3.0, 9.0, (0.91, 0.69, 0.29, 0.96), Some("A-B")),
        };
        let marker = TimelineLayerGeometry {
            x: x - mark_width / 2.0,
            y: rail_center_y - mark_height / 2.0,
            width: mark_width,
            height: mark_height,
        };
        fill_timeline_layer(cr, marker, color);
        if let Some(label) = label {
            cr.set_source_rgba(0.91, 0.69, 0.29, 0.98);
            cr.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(8.0);
            cr.move_to(x + 3.0, f64::from(height) - 1.0);
            let _ = cr.show_text(label);
        }
    }

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.42);
    cr.arc(
        geometry.thumb_center_x,
        rail_center_y + 1.0,
        TIMELINE_THUMB_DIAMETER / 2.0 + 1.0,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = cr.fill();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.96);
    cr.arc(
        geometry.thumb_center_x,
        rail_center_y,
        TIMELINE_THUMB_DIAMETER / 2.0,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = cr.fill();
}

fn fill_timeline_layer(
    cr: &cairo::Context,
    layer: TimelineLayerGeometry,
    color: (f64, f64, f64, f64),
) {
    if layer.width <= 0.0 || layer.height <= 0.0 {
        return;
    }
    timeline_rounded_rect(
        cr,
        layer.x,
        layer.y,
        layer.width,
        layer.height,
        layer.height / 2.0,
    );
    cr.set_source_rgba(color.0, color.1, color.2, color.3);
    let _ = cr.fill();
}

fn timeline_rounded_rect(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let right = x + width;
    let bottom = y + height;
    cr.new_sub_path();
    cr.arc(
        right - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

#[derive(Clone)]
pub(crate) struct VolumeControl {
    root: gtk::Box,
    button: gtk::Button,
    icon: gtk::Image,
    wick: gtk::DrawingArea,
    scale: gtk::Scale,
    track: gtk::DrawingArea,
    readout: gtk::Button,
    readout_input: gtk::Entry,
    capsule: gtk::Box,
    popover: gtk::Popover,
    model: Rc<Cell<volume::VolumeState>>,
    state: Rc<RefCell<PlayerState>>,
    updating: Rc<Cell<bool>>,
    close_source: Rc<RefCell<Option<glib::SourceId>>>,
    preview_override: Rc<Cell<bool>>,
    exact_input_active: Rc<Cell<bool>>,
    exact_input_focused: Rc<Cell<bool>>,
    animations_enabled: bool,
}

impl VolumeControl {
    fn new(
        state: Rc<RefCell<PlayerState>>,
        updating: Rc<Cell<bool>>,
        chrome: Rc<ChromeVisibility>,
    ) -> Self {
        let initial_level = state.borrow().settings.volume();
        state.borrow_mut().volume_state.set_level(initial_level);
        let model = Rc::new(Cell::new(volume::VolumeState::new(initial_level)));

        let icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
        icon.add_css_class("okp-volume-icon");

        let wick = gtk::DrawingArea::new();
        wick.add_css_class("okp-volume-wick");
        wick.set_content_width(VOLUME_WICK_WIDTH);
        wick.set_content_height(VOLUME_WICK_HEIGHT);
        wick.set_can_target(false);
        let wick_model = Rc::clone(&model);
        wick.set_draw_func(move |_, cr, width, height| {
            draw_volume_wick(cr, width, height, wick_model.get());
        });

        let resting = gtk::Box::new(gtk::Orientation::Vertical, 0);
        resting.set_halign(gtk::Align::Center);
        resting.set_valign(gtk::Align::Center);
        resting.append(&icon);
        resting.append(&wick);

        let button = gtk::Button::builder().focus_on_click(false).build();
        button.add_css_class("okp-volume-button");
        button.set_has_frame(false);
        button.set_size_request(VOLUME_RESTING_SIZE, VOLUME_RESTING_SIZE);
        button.set_tooltip_text(Some("Mute (M) \u{00B7} Ctrl-click: 100%"));
        button.set_child(Some(&resting));

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("okp-volume-control");
        root.set_size_request(VOLUME_RESTING_SIZE, VOLUME_RESTING_SIZE);
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::Center);
        root.append(&button);

        let track = gtk::DrawingArea::new();
        track.add_css_class("okp-volume-track");
        track.set_content_width(VOLUME_TRACK_WIDTH);
        track.set_content_height(VOLUME_THUMB_SIZE);
        track.set_can_target(false);
        let track_model = Rc::clone(&model);
        track.set_draw_func(move |_, cr, width, height| {
            draw_volume_track(cr, width, height, track_model.get());
        });

        let scale = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            volume::MIN_VOLUME,
            volume::MAX_VOLUME,
            1.0,
        );
        scale.add_css_class("okp-volume-slider");
        scale.set_draw_value(false);
        scale.set_increments(1.0, 10.0);
        scale.set_round_digits(1);
        scale.set_size_request(VOLUME_TRACK_WIDTH, VOLUME_THUMB_SIZE);
        scale.set_value(initial_level);

        let track_stack = gtk::Overlay::new();
        track_stack.add_css_class("okp-volume-track-stack");
        track_stack.set_child(Some(&track));
        track_stack.add_overlay(&scale);
        track_stack.set_measure_overlay(&scale, true);

        let readout = gtk::Button::builder()
            .label("100%")
            .focus_on_click(true)
            .build();
        readout.add_css_class("okp-volume-readout");
        readout.set_has_frame(false);
        readout.set_tooltip_text(Some("Enter an exact volume"));

        let readout_input = gtk::Entry::new();
        readout_input.add_css_class("okp-volume-readout-input");
        readout_input.set_width_chars(5);
        readout_input.set_max_width_chars(6);
        readout_input.set_visible(false);
        readout_input.set_focusable(true);
        readout_input.set_input_purpose(gtk::InputPurpose::Number);

        let readout_stack = gtk::Overlay::new();
        readout_stack.add_css_class("okp-volume-readout-stack");
        readout_stack.set_child(Some(&readout));
        readout_stack.add_overlay(&readout_input);
        readout_stack.set_measure_overlay(&readout_input, true);

        let capsule = gtk::Box::new(gtk::Orientation::Horizontal, 11);
        capsule.add_css_class("okp-volume-capsule");
        capsule.append(&track_stack);
        capsule.append(&readout_stack);

        let popover = gtk::Popover::new();
        popover.add_css_class("okp-volume-popover");
        popover.set_autohide(false);
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Top);
        popover.set_offset(0, -VOLUME_CAPSULE_OFFSET);
        popover.set_child(Some(&capsule));
        popover.set_parent(&root);
        connect_popover_chrome_pin(&popover, chrome);

        let animations_enabled =
            gtk::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations());
        if !animations_enabled {
            capsule.add_css_class("reduce-motion");
        }

        let control = Self {
            root,
            button,
            icon,
            wick,
            scale,
            track,
            readout,
            readout_input,
            capsule,
            popover,
            model,
            state,
            updating,
            close_source: Rc::new(RefCell::new(None)),
            preview_override: Rc::new(Cell::new(false)),
            exact_input_active: Rc::new(Cell::new(false)),
            exact_input_focused: Rc::new(Cell::new(false)),
            animations_enabled,
        };
        control.connect_events();
        control.project();
        control
    }

    pub(crate) fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub(crate) fn sync_level(&self, level: f64) {
        if self.preview_override.get() {
            return;
        }
        let level = {
            let mut state = self.state.borrow_mut();
            let Some(level) = volume::reconcile_observed_level(&mut state.pending_volume, level)
            else {
                return;
            };
            state.volume_state.set_level(level)
        };
        let mut model = self.model.get();
        model.set_level(level);
        self.model.set(model);
        self.project();
    }

    pub(crate) fn open_preview(&self, mode: &str) {
        let level = match mode {
            "zero" | "muted" => 0.0,
            "normal" => 64.0,
            "unity" => 100.0,
            "boost" => 124.0,
            _ => 78.0,
        };
        self.preview_override.set(true);
        let mut model = volume::VolumeState::new(if mode == "muted" { 72.0 } else { level });
        if mode == "muted" {
            model.toggle_mute();
        }
        self.model.set(model);
        self.project();
        if mode != "rest" {
            self.open();
        }
    }

    fn connect_events(&self) {
        let clicked = self.clone();
        self.button.connect_clicked(move |_| clicked.toggle_mute());

        // Ctrl+primary-click is the canonical quick reset to exactly 100%
        // (matching the Windows control). The gesture lives on the ancestors
        // of the mute button and the slider: capture-phase claims only cancel
        // gestures further down the propagation chain, so claiming here (and
        // not on the widgets themselves) is what keeps the plain-click action
        // (mute toggle, slider jump-to-position) from also firing.
        let track_stack = self
            .scale
            .parent()
            .expect("volume scale is parented to its track overlay");
        for widget in [self.root.clone().upcast::<gtk::Widget>(), track_stack] {
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gdk::BUTTON_PRIMARY);
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            let reset = self.clone();
            gesture.connect_pressed(move |gesture, _, _, _| {
                if volume_click_resets(gesture.current_event_state()) {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    reset.reset_to_unity();
                }
            });
            widget.add_controller(gesture);
        }

        let changed = self.clone();
        self.scale.connect_change_value(move |_, _, value| {
            if !changed.updating.get() {
                changed.set_level_from_ui(value);
            }
            glib::Propagation::Proceed
        });

        let exact = self.clone();
        self.readout
            .connect_clicked(move |_| exact.begin_exact_input());
        let apply = self.clone();
        self.readout_input
            .connect_activate(move |_| apply.apply_exact_input());
        let exact_focus = gtk::EventControllerFocus::new();
        let focused = self.clone();
        exact_focus.connect_enter(move |_| {
            focused.exact_input_focused.set(true);
            focused.open();
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: volume-exact-focus=entry");
            }
        });
        let blurred = self.clone();
        exact_focus.connect_leave(move |_| {
            if blurred.exact_input_focused.replace(false) && blurred.exact_input_active.get() {
                blurred.commit_exact_input_on_blur();
            }
            blurred.schedule_close();
        });
        self.readout_input.add_controller(exact_focus);
        let dismiss = self.clone();
        self.popover.connect_closed(move |_| {
            if dismiss.exact_input_active.get() {
                dismiss.commit_exact_input_on_blur();
            }
        });

        let motion = gtk::EventControllerMotion::new();
        let enter = self.clone();
        motion.connect_enter(move |_, _, _| enter.open());
        let leave = self.clone();
        motion.connect_leave(move |_| leave.schedule_close());
        self.root.add_controller(motion);

        let capsule_motion = gtk::EventControllerMotion::new();
        let enter = self.clone();
        capsule_motion.connect_enter(move |_, _, _| enter.open());
        let leave = self.clone();
        capsule_motion.connect_leave(move |_| leave.schedule_close());
        self.capsule.add_controller(capsule_motion);

        for widget in [
            self.button.clone().upcast::<gtk::Widget>(),
            self.scale.clone().upcast::<gtk::Widget>(),
            self.readout.clone().upcast::<gtk::Widget>(),
        ] {
            let focus = self.clone();
            widget.connect_has_focus_notify(move |widget| {
                if widget.has_focus() {
                    focus.open();
                } else {
                    focus.schedule_close();
                }
            });
        }

        for widget in [
            self.root.clone().upcast::<gtk::Widget>(),
            self.capsule.clone().upcast::<gtk::Widget>(),
        ] {
            let scroll = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::VERTICAL
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            let scrolled = self.clone();
            scroll.connect_scroll(move |controller, _, dy| {
                let fine = controller
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK);
                let Some(delta) = volume_scroll_delta(dy, fine) else {
                    return glib::Propagation::Proceed;
                };
                scrolled.nudge(delta);
                glib::Propagation::Stop
            });
            widget.add_controller(scroll);
        }

        let keys = gtk::EventControllerKey::new();
        let keyed = self.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if let Some(delta) = volume_key_delta(key) {
                keyed.nudge(delta);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.button.add_controller(keys);
    }

    fn set_level_from_ui(&self, level: f64) {
        let mut model = self.model.get();
        let level = model.set_level(level);
        self.model.set(model);
        self.project();
        set_volume_from_ui(&self.state, level);
    }

    fn nudge(&self, delta: f64) {
        let mut model = self.model.get();
        let level = model.nudge(delta);
        self.model.set(model);
        self.project();
        set_volume_from_ui(&self.state, level);
    }

    fn toggle_mute(&self) {
        let mut model = self.model.get();
        let level = model.toggle_mute();
        self.model.set(model);
        self.project();
        set_volume_from_ui(&self.state, level);
    }

    fn reset_to_unity(&self) {
        let mut model = self.model.get();
        let level = model.reset_to_unity();
        self.model.set(model);
        self.project();
        set_volume_from_ui(&self.state, level);
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: volume-ctrl-click=unity");
        }
    }

    fn begin_exact_input(&self) {
        let level = self.model.get().level();
        self.readout_input.set_text(&format!("{level:.1}"));
        self.exact_input_active.set(true);
        self.exact_input_focused.set(false);
        self.readout.set_visible(false);
        self.readout_input.set_visible(true);
        self.open();
        let input = self.readout_input.clone();
        glib::idle_add_local_once(move || {
            // Use GtkEntry's focus path so focus lands inside its editable child.
            // Setting root focus directly targets only the composite entry widget,
            // which reports focus while text input and activation are lost.
            input.grab_focus_without_selecting();
            input.select_region(0, -1);
        });
    }

    fn apply_exact_input(&self) {
        if self.commit_exact_input() {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: volume-exact-commit=activate");
            }
            self.scale.grab_focus();
        }
    }

    fn commit_exact_input_on_blur(&self) {
        if self.commit_exact_input() && env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: volume-exact-commit=blur");
        }
    }

    fn commit_exact_input(&self) -> bool {
        if !self.exact_input_active.replace(false) {
            return false;
        }
        let text = self.readout_input.text();
        if let Some(level) = volume::parse_readout(text.as_str()) {
            self.set_level_from_ui(level);
        }
        self.readout_input.set_visible(false);
        self.readout.set_visible(true);
        self.exact_input_focused.set(false);
        true
    }

    fn project(&self) {
        let model = self.model.get();
        self.updating.set(true);
        self.scale.set_value(model.level());
        self.updating.set(false);

        self.icon.set_icon_name(Some(if model.is_muted() {
            "audio-volume-muted-symbolic"
        } else if model.level() < 50.0 {
            "audio-volume-low-symbolic"
        } else {
            "audio-volume-high-symbolic"
        }));
        self.readout.set_label(&model.readout());
        self.button.set_tooltip_text(Some(if model.is_muted() {
            "Unmute (M) \u{00B7} Ctrl-click: 100%"
        } else {
            "Mute (M) \u{00B7} Ctrl-click: 100%"
        }));
        set_state_class(&self.root, "is-muted", model.is_muted());
        set_state_class(&self.root, "is-boosted", model.is_boosted());
        set_state_class(&self.capsule, "is-muted", model.is_muted());
        set_state_class(&self.capsule, "is-boosted", model.is_boosted());
        self.wick.queue_draw();
        self.track.queue_draw();

        let value_text = model.readout();
        self.scale.update_property(&[
            gtk::accessible::Property::Label("Volume"),
            gtk::accessible::Property::ValueMin(volume::MIN_VOLUME),
            gtk::accessible::Property::ValueMax(volume::MAX_VOLUME),
            gtk::accessible::Property::ValueNow(model.level()),
            gtk::accessible::Property::ValueText(&value_text),
        ]);
        let button_label = if model.is_muted() {
            format!("Unmute volume to {:.1}%", model.remembered_nonzero())
        } else {
            format!("Mute volume at {:.1}%", model.level())
        };
        self.button
            .update_property(&[gtk::accessible::Property::Label(&button_label)]);
    }

    fn open(&self) {
        self.cancel_close();
        self.capsule.remove_css_class("is-closing");
        if !self.capsule.has_css_class("is-open") {
            self.capsule.remove_css_class("is-open");
            self.popover.popup();
            if self.animations_enabled {
                let capsule = self.capsule.clone();
                glib::timeout_add_local_once(Duration::from_millis(16), move || {
                    capsule.add_css_class("is-open");
                });
            } else {
                self.capsule.add_css_class("is-open");
            }
        } else {
            self.popover.popup();
        }
    }

    fn schedule_close(&self) {
        if self.preview_override.get() {
            return;
        }
        self.cancel_close();
        let control = self.clone();
        let source =
            glib::timeout_add_local(Duration::from_millis(VOLUME_HOVER_GRACE_MS), move || {
                control.close_source.borrow_mut().take();
                control.collapse();
                glib::ControlFlow::Break
            });
        self.close_source.borrow_mut().replace(source);
    }

    fn collapse(&self) {
        if self.exact_input_active.get() {
            return;
        }
        self.capsule.remove_css_class("is-open");
        self.capsule.add_css_class("is-closing");
        if !self.animations_enabled {
            self.popover.popdown();
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: volume-capsule=closed");
            }
            return;
        }
        let popover = self.popover.clone();
        let capsule = self.capsule.clone();
        let close_source = Rc::clone(&self.close_source);
        let source =
            glib::timeout_add_local(Duration::from_millis(VOLUME_COLLAPSE_MS), move || {
                close_source.borrow_mut().take();
                capsule.remove_css_class("is-closing");
                popover.popdown();
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: volume-capsule=closed");
                }
                glib::ControlFlow::Break
            });
        self.close_source.borrow_mut().replace(source);
    }

    fn cancel_close(&self) {
        if let Some(source) = self.close_source.borrow_mut().take() {
            source.remove();
        }
    }
}

pub(crate) fn volume_scroll_delta(dy: f64, fine: bool) -> Option<f64> {
    if !dy.is_finite() || dy == 0.0 {
        return None;
    }
    let step = if fine { 0.1 } else { 1.0 };
    Some(if dy < 0.0 { step } else { -step })
}

/// A primary click resets to unity whenever Ctrl is held, matching the
/// Windows control's `CtrlDown()` policy (other concurrent modifiers are
/// ignored rather than cancelling the reset).
pub(crate) fn volume_click_resets(state: gdk::ModifierType) -> bool {
    state.contains(gdk::ModifierType::CONTROL_MASK)
}

pub(crate) fn volume_key_delta(key: gdk::Key) -> Option<f64> {
    match key {
        gdk::Key::Up | gdk::Key::Right => Some(1.0),
        gdk::Key::Down | gdk::Key::Left => Some(-1.0),
        _ => None,
    }
}

fn set_state_class(widget: &impl IsA<gtk::Widget>, class: &str, enabled: bool) {
    if enabled {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

fn draw_volume_wick(cr: &cairo::Context, width: i32, height: i32, state: volume::VolumeState) {
    let y = f64::from(height) / 2.0;
    cr.set_line_width(f64::from(VOLUME_WICK_HEIGHT));
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.19);
    cr.move_to(1.5, y);
    cr.line_to(f64::from(width) - 1.5, y);
    let _ = cr.stroke();

    if !state.is_muted() {
        let fill = (f64::from(width) - 3.0) * state.level_fraction();
        let color = if state.is_boosted() {
            (0.941, 0.722, 0.251)
        } else {
            (0.157, 0.702, 0.667)
        };
        cr.set_source_rgb(color.0, color.1, color.2);
        cr.move_to(1.5, y);
        cr.line_to(1.5 + fill, y);
        let _ = cr.stroke();
    }
}

fn draw_volume_track(cr: &cairo::Context, width: i32, height: i32, state: volume::VolumeState) {
    let width = f64::from(width);
    let y = f64::from(height) / 2.0;
    cr.set_line_width(f64::from(VOLUME_TRACK_HEIGHT));
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.20);
    cr.move_to(3.0, y);
    cr.line_to(width - 3.0, y);
    let _ = cr.stroke();

    let track_span = width - 6.0;
    let teal_end = 3.0 + track_span * state.teal_fraction();
    if teal_end > 3.0 {
        cr.set_source_rgba(
            0.157,
            0.702,
            0.667,
            if state.is_muted() { 0.38 } else { 1.0 },
        );
        cr.move_to(3.0, y);
        cr.line_to(teal_end, y);
        let _ = cr.stroke();
    }
    if state.is_boosted() && !state.is_muted() {
        let boost_start = 3.0 + track_span * volume::VolumeState::unity_fraction();
        let boost_end = boost_start + track_span * state.boost_fraction();
        cr.set_source_rgb(0.941, 0.722, 0.251);
        cr.move_to(boost_start, y);
        cr.line_to(boost_end, y);
        let _ = cr.stroke();
    }

    let marker_x = 3.0 + track_span * volume::VolumeState::unity_fraction();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.50);
    cr.rectangle(marker_x - 1.0, 1.0, 2.0, f64::from(height - 2));
    let _ = cr.fill();
}

pub(crate) fn build_controls(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) -> Controls {
    let play_button = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .build();
    play_button.set_has_frame(false);
    play_button.add_css_class("okp-control-button");
    play_button.add_css_class("okp-play-button");
    play_button.set_tooltip_text(Some("Play / Pause (Space)"));
    play_button.set_sensitive(false);

    let subtitle_button = gtk::MenuButton::builder()
        .icon_name("media-view-subtitles-symbolic")
        .build();
    subtitle_button.set_has_frame(false);
    subtitle_button.add_css_class("okp-control-button");
    subtitle_button.add_css_class("okp-icon-button");
    subtitle_button.add_css_class("okp-utility-button");
    subtitle_button.set_tooltip_text(Some("Subtitles"));
    subtitle_button.set_sensitive(false);

    let audio_button = gtk::MenuButton::builder()
        .icon_name("audio-speakers-symbolic")
        .build();
    audio_button.set_has_frame(false);
    audio_button.add_css_class("okp-control-button");
    audio_button.add_css_class("okp-icon-button");
    audio_button.add_css_class("okp-utility-button");
    audio_button.set_tooltip_text(Some("Audio"));
    audio_button.set_sensitive(false);

    let speed_button = gtk::MenuButton::builder().label("1.00×").build();
    speed_button.set_has_frame(false);
    speed_button.add_css_class("okp-control-button");
    speed_button.add_css_class("okp-speed-chip");
    speed_button.set_tooltip_text(Some("Playback speed"));
    speed_button.set_sensitive(false);

    let previous_button = gtk::Button::builder()
        .icon_name("media-skip-backward-symbolic")
        .build();
    previous_button.set_has_frame(false);
    previous_button.add_css_class("okp-control-button");
    previous_button.add_css_class("okp-transport-button");
    previous_button.set_tooltip_text(Some("Previous chapter"));
    previous_button.set_sensitive(false);

    let elapsed_label = gtk::Label::new(Some("00:00"));
    elapsed_label.add_css_class("okp-time-label");
    elapsed_label.add_css_class("okp-elapsed-time");

    let next_button = gtk::Button::builder()
        .icon_name("media-skip-forward-symbolic")
        .build();
    next_button.set_has_frame(false);
    next_button.add_css_class("okp-control-button");
    next_button.add_css_class("okp-transport-button");
    next_button.set_tooltip_text(Some("Next chapter"));
    next_button.set_sensitive(false);

    let chapters_button = gtk::Button::builder()
        .icon_name("view-list-symbolic")
        .build();
    chapters_button.set_has_frame(false);
    chapters_button.add_css_class("okp-control-button");
    chapters_button.add_css_class("okp-icon-button");
    chapters_button.add_css_class("okp-utility-button");
    chapters_button.set_tooltip_text(Some("Chapters / Up Next"));
    chapters_button.set_sensitive(false);

    let screenshot_button = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .build();
    screenshot_button.set_has_frame(false);
    screenshot_button.add_css_class("okp-control-button");
    screenshot_button.add_css_class("okp-icon-button");
    screenshot_button.add_css_class("okp-utility-button");
    screenshot_button.set_tooltip_text(Some("Save frame to Pictures/OK Player (C)"));
    screenshot_button.set_sensitive(false);

    let fullscreen_button = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .build();
    fullscreen_button.set_has_frame(false);
    fullscreen_button.add_css_class("okp-control-button");
    fullscreen_button.add_css_class("okp-icon-button");
    fullscreen_button.add_css_class("okp-utility-button");
    fullscreen_button.set_tooltip_text(Some("Enter Fullscreen (F)"));
    fullscreen_button.set_sensitive(false);

    let more_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build();
    more_button.set_has_frame(false);
    more_button.add_css_class("okp-control-button");
    more_button.add_css_class("okp-icon-button");
    more_button.add_css_class("okp-utility-button");
    more_button.set_tooltip_text(Some("More commands"));

    let duration_label = gtk::Label::new(Some("-00:00"));
    duration_label.add_css_class("okp-time-label");
    duration_label.add_css_class("okp-remaining-time");
    duration_label.set_tooltip_text(Some("Show total time"));
    duration_label.set_can_target(true);
    let trailing_time_mode = Rc::new(Cell::new(time_code::TrailingTimeMode::Remaining));
    let time_click = gtk::GestureClick::new();
    time_click.set_button(gdk::BUTTON_PRIMARY);
    let time_mode = Rc::clone(&trailing_time_mode);
    let time_label = duration_label.clone();
    time_click.connect_released(move |_, press_count, _, _| {
        if press_count != 1 {
            return;
        }
        let mode = time_mode.get().toggled();
        time_mode.set(mode);
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: time-label={mode:?}");
        }
        time_label.set_tooltip_text(Some(match mode {
            time_code::TrailingTimeMode::Total => "Show remaining time",
            time_code::TrailingTimeMode::Remaining => "Show total time",
        }));
    });
    duration_label.add_controller(time_click);

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    seek.set_draw_value(false);
    seek.set_hexpand(true);
    seek.set_sensitive(false);
    seek.set_size_request(120, TIMELINE_HEIGHT);
    seek.add_css_class("okp-seek");

    let timeline_rail = TimelineRail::new(&seek.adjustment());

    let timeline = gtk::Overlay::new();
    timeline.add_css_class("okp-timeline");
    timeline.set_hexpand(true);
    timeline.set_valign(gtk::Align::Center);
    timeline.set_size_request(120, TIMELINE_HEIGHT);
    timeline.set_child(Some(timeline_rail.widget()));
    timeline.add_overlay(&seek);
    timeline.set_measure_overlay(&seek, true);

    let volume = VolumeControl::new(
        Rc::clone(&state),
        Rc::clone(&updating_volume),
        Rc::clone(&chrome),
    );

    let chapters_tab = side_panel_segment_button("Chapters", true);
    let up_next_tab = side_panel_segment_button("Up Next", false);
    let side_panel_mode = Rc::new(Cell::new(SidePanelMode::Chapters));
    let side_panel_tabs = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    side_panel_tabs.add_css_class("okp-side-panel-tabs");
    side_panel_tabs.set_halign(gtk::Align::Start);
    side_panel_tabs.set_hexpand(true);
    side_panel_tabs.append(&chapters_tab);
    side_panel_tabs.append(&up_next_tab);

    let side_panel_close = gtk::Button::from_icon_name("window-close-symbolic");
    side_panel_close.add_css_class("okp-side-panel-close");
    side_panel_close.set_has_frame(false);
    side_panel_close.set_tooltip_text(Some("Close panel"));

    let up_next_list = gtk::ListBox::new();
    up_next_list.add_css_class("okp-up-next-list");
    up_next_list.set_selection_mode(gtk::SelectionMode::None);

    let up_next_scroller = gtk::ScrolledWindow::new();
    up_next_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    up_next_scroller.set_child(Some(&up_next_list));
    up_next_scroller.set_vexpand(true);

    let up_next_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    up_next_header.add_css_class("okp-side-panel-header");
    up_next_header.append(&side_panel_tabs);
    up_next_header.append(&side_panel_close);

    let up_next_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    up_next_panel.add_css_class("okp-up-next-panel");
    up_next_panel.set_width_request(SIDE_PANEL_WIDTH);
    up_next_panel.append(&up_next_header);
    up_next_panel.append(&up_next_scroller);

    let side_panel_fade_revealer = gtk::Revealer::new();
    side_panel_fade_revealer.set_transition_duration(if playback_animations_enabled() {
        SIDE_PANEL_TRANSITION_MS
    } else {
        0
    });
    side_panel_fade_revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    side_panel_fade_revealer.set_reveal_child(false);
    side_panel_fade_revealer.set_child(Some(&up_next_panel));

    let up_next_revealer = gtk::Revealer::new();
    up_next_revealer.set_halign(gtk::Align::End);
    up_next_revealer.set_valign(gtk::Align::Fill);
    up_next_revealer.set_margin_top(SIDE_PANEL_TOP_INSET);
    up_next_revealer.set_margin_end(0);
    up_next_revealer.set_margin_bottom(SIDE_PANEL_BOTTOM_INSET);
    up_next_revealer.set_transition_duration(if playback_animations_enabled() {
        SIDE_PANEL_TRANSITION_MS
    } else {
        0
    });
    up_next_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    up_next_revealer.set_reveal_child(false);
    up_next_revealer.set_can_target(false);
    up_next_revealer.set_child(Some(&side_panel_fade_revealer));

    let side_panel_user_visible = Rc::new(Cell::new(false));
    let side_panel_pinned = Rc::new(Cell::new(false));
    let side_panel_manual_mode = Rc::new(Cell::new(false));
    let side_panel_snapshot = Rc::new(RefCell::new(SidePanelSnapshot::default()));

    let close_panel = up_next_revealer.clone();
    let close_fade = side_panel_fade_revealer.clone();
    let close_toggle = chapters_button.clone();
    let close_visible = Rc::clone(&side_panel_user_visible);
    let close_pinned = Rc::clone(&side_panel_pinned);
    let close_chrome = Rc::clone(&chrome);
    side_panel_close.connect_clicked(move |_| {
        set_side_panel_user_visible(
            &close_panel,
            &close_fade,
            &close_toggle,
            &close_visible,
            &close_pinned,
            &close_chrome,
            false,
        );
    });

    let up_next_state = Rc::clone(&state);
    let up_next_actions = Rc::new(RefCell::new(Vec::<SidePanelAction>::new()));
    let chapter_detection = Rc::new(Cell::new(chapter_math::ChapterDetection::default()));
    let row_actions = Rc::clone(&up_next_actions);
    let row_toast = Rc::clone(&status_toast);
    let row_parent = window.clone();
    let row_detection = Rc::clone(&chapter_detection);
    let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
    up_next_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }

        match row_actions
            .borrow()
            .get(index as usize)
            .copied()
            .unwrap_or(SidePanelAction::None)
        {
            SidePanelAction::None => {}
            SidePanelAction::Chapter(time) => seek_to_chapter(&up_next_state, time),
            SidePanelAction::Playlist(index) => {
                jump_playlist_index(&up_next_state, index);
            }
            SidePanelAction::AddBookmark => add_bookmark_at_position(&up_next_state, &row_toast),
            // The Up Next short-queue state's "Add files" affordance: opens the
            // same multi-select media dialog the overflow menu's "Add to Queue"
            // uses, so a single-URL / no-folder session can grow a queue without
            // leaving the panel (PRD §2.6).
            SidePanelAction::AddFiles => open_queue_media_dialog(
                &row_parent,
                Rc::clone(&up_next_state),
                Rc::clone(&row_toast),
                QueueInsertMode::Append,
            ),
            SidePanelAction::DetectChapters => detect_chapters(&row_detection, &row_toast),
        }
    });

    let chapters_tab_mode = Rc::clone(&side_panel_mode);
    let chapters_tab_manual_mode = Rc::clone(&side_panel_manual_mode);
    let chapters_tab_snapshot = Rc::clone(&side_panel_snapshot);
    let chapters_tab_button = chapters_tab.clone();
    let chapters_peer_tab = up_next_tab.clone();
    chapters_tab.connect_clicked(move |_| {
        chapters_tab_manual_mode.set(true);
        chapters_tab_mode.set(SidePanelMode::Chapters);
        chapters_tab_snapshot.borrow_mut().has_media = false;
        update_side_panel_tab_state(
            &chapters_tab_button,
            &chapters_peer_tab,
            SidePanelMode::Chapters,
        );
    });

    let up_next_tab_mode = Rc::clone(&side_panel_mode);
    let up_next_tab_manual_mode = Rc::clone(&side_panel_manual_mode);
    let up_next_tab_snapshot = Rc::clone(&side_panel_snapshot);
    let up_next_tab_button = up_next_tab.clone();
    let up_next_peer_tab = chapters_tab.clone();
    up_next_tab.connect_clicked(move |_| {
        up_next_tab_manual_mode.set(true);
        up_next_tab_mode.set(SidePanelMode::UpNext);
        up_next_tab_snapshot.borrow_mut().has_media = false;
        update_side_panel_tab_state(
            &up_next_peer_tab,
            &up_next_tab_button,
            SidePanelMode::UpNext,
        );
    });

    let subtitle_popover = gtk::Popover::new();
    prepare_track_popover(&subtitle_popover, PlayerPopoverKind::Subtitles);
    connect_popover_chrome_pin(&subtitle_popover, Rc::clone(&chrome));
    connect_popover_focus_return(&subtitle_popover, &subtitle_button);
    subtitle_button.set_popover(Some(&subtitle_popover));
    let subtitle_state = Rc::clone(&state);
    subtitle_popover.connect_show(move |popover| {
        populate_subtitle_popover(popover, Rc::clone(&subtitle_state));
    });

    let audio_popover = gtk::Popover::new();
    prepare_track_popover(&audio_popover, PlayerPopoverKind::Audio);
    connect_popover_chrome_pin(&audio_popover, Rc::clone(&chrome));
    connect_popover_focus_return(&audio_popover, &audio_button);
    audio_button.set_popover(Some(&audio_popover));
    let audio_state = Rc::clone(&state);
    audio_popover.connect_show(move |popover| {
        populate_audio_popover(popover, Rc::clone(&audio_state));
    });

    let speed_popover = gtk::Popover::new();
    prepare_track_popover(&speed_popover, PlayerPopoverKind::Speed);
    connect_popover_chrome_pin(&speed_popover, Rc::clone(&chrome));
    connect_popover_focus_return(&speed_popover, &speed_button);
    speed_button.set_popover(Some(&speed_popover));
    let speed_state = Rc::clone(&state);
    speed_popover.connect_show(move |popover| {
        populate_speed_popover(popover, Rc::clone(&speed_state));
    });

    let more_popover = gtk::Popover::new();
    prepare_track_popover(&more_popover, PlayerPopoverKind::More);
    connect_popover_chrome_pin(&more_popover, Rc::clone(&chrome));
    connect_popover_focus_return(&more_popover, &more_button);
    more_button.set_popover(Some(&more_popover));
    let more_parent = window.clone();
    let more_state = Rc::clone(&state);
    let more_toast = Rc::clone(&status_toast);
    more_popover.connect_show(move |popover| {
        populate_command_popover(
            popover,
            &more_parent,
            Rc::clone(&more_state),
            Rc::clone(&more_toast),
        );
    });

    let previous_state = Rc::clone(&state);
    previous_button.connect_clicked(move |_| {
        jump_chapter(&previous_state, -1);
    });

    let play_state = Rc::clone(&state);
    play_button.connect_clicked(move |_| {
        if let Some(mpv) = play_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.cycle_pause()
        {
            eprintln!("Failed to toggle playback: {error}");
        }
    });
    play_button.connect_has_focus_notify(|button| {
        if button.has_focus() && env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: outside-target=play-focused");
        }
    });

    let next_state = Rc::clone(&state);
    next_button.connect_clicked(move |_| {
        jump_chapter(&next_state, 1);
    });

    let chapters_panel = up_next_revealer.clone();
    let chapters_fade = side_panel_fade_revealer.clone();
    let chapters_toggle = chapters_button.clone();
    let chapters_visible = Rc::clone(&side_panel_user_visible);
    let chapters_pinned = Rc::clone(&side_panel_pinned);
    let chapters_chrome = Rc::clone(&chrome);
    let chapters_state = Rc::clone(&state);
    let chapters_mode = Rc::clone(&side_panel_mode);
    let chapters_manual_mode = Rc::clone(&side_panel_manual_mode);
    let chapters_tab_for_toggle = chapters_tab.clone();
    let up_next_tab_for_toggle = up_next_tab.clone();
    let chapters_snapshot_for_toggle = Rc::clone(&side_panel_snapshot);
    chapters_button.connect_clicked(move |_| {
        let next_visible = !chapters_visible.get();
        if next_visible {
            let preferred_mode = preferred_side_panel_mode(&chapters_state);
            chapters_manual_mode.set(false);
            chapters_mode.set(preferred_mode);
            chapters_snapshot_for_toggle.borrow_mut().has_media = false;
            update_side_panel_tab_state(
                &chapters_tab_for_toggle,
                &up_next_tab_for_toggle,
                preferred_mode,
            );
        }
        set_side_panel_user_visible(
            &chapters_panel,
            &chapters_fade,
            &chapters_toggle,
            &chapters_visible,
            &chapters_pinned,
            &chapters_chrome,
            next_visible,
        );
    });

    let screenshot_state = Rc::clone(&state);
    let screenshot_toast = Rc::clone(&status_toast);
    screenshot_button
        .connect_clicked(move |_| save_screenshot(&screenshot_state, &screenshot_toast, false));

    let fullscreen_parent = window.clone();
    fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&fullscreen_parent));

    let seek_state = Rc::clone(&state);
    seek.connect_change_value(move |_, _, value| {
        if !updating_seek.get() {
            seek_absolute(&seek_state, value);
        }

        glib::Propagation::Proceed
    });
    let seek_hover_preview = connect_seek_hover(&seek, Rc::clone(&state), thumbnail_sender.clone());

    Controls {
        subtitle_button,
        audio_button,
        speed_button,
        previous_button,
        play_button,
        next_button,
        chapters_button,
        screenshot_button,
        fullscreen_button,
        more_button,
        timeline,
        seek,
        timeline_rail,
        elapsed_label,
        duration_label,
        trailing_time_mode,
        volume,
        status_toast,
        up_next_revealer,
        side_panel_fade_revealer,
        chapters_tab,
        up_next_tab,
        up_next_list,
        side_panel_user_visible,
        side_panel_pinned,
        side_panel_mode,
        side_panel_manual_mode,
        side_panel_snapshot,
        side_panel_actions: up_next_actions,
        chapter_detection,
        side_panel_preview_frozen: Rc::new(Cell::new(false)),
        seek_hover_preview,
        thumbnail_sender,
        thumbnail_events: RefCell::new(thumbnail_receiver),
    }
}

pub(crate) fn controls_bar(controls: &Controls) -> gtk::Overlay {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    bar.add_css_class("okp-controls");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_start(16);
    bar.set_margin_end(16);
    bar.set_margin_bottom(18);

    bar.append(&controls.play_button);
    bar.append(&controls.previous_button);
    bar.append(&controls.next_button);
    bar.append(&controls.elapsed_label);
    bar.append(&controls.timeline);
    bar.append(&controls.duration_label);
    bar.append(controls.volume.widget());
    bar.append(&controls.speed_button);
    bar.append(&controls.subtitle_button);
    bar.append(&controls.audio_button);
    bar.append(&controls.chapters_button);
    bar.append(&controls.screenshot_button);
    bar.append(&controls.fullscreen_button);
    bar.append(&controls.more_button);

    let scrim = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    scrim.add_css_class("okp-bottom-scrim");
    scrim.set_halign(gtk::Align::Fill);
    scrim.set_valign(gtk::Align::End);
    scrim.set_height_request(220);
    scrim.set_can_target(false);

    let chrome = gtk::Overlay::new();
    chrome.add_css_class("okp-bottom-chrome");
    chrome.set_halign(gtk::Align::Fill);
    chrome.set_valign(gtk::Align::End);
    chrome.set_child(Some(&scrim));
    chrome.add_overlay(&bar);
    chrome
}

pub(crate) fn connect_chrome_activity(overlay: &gtk::Overlay, chrome: Rc<ChromeVisibility>) {
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, _, _| {
        chrome.show_for_activity();
    });
    overlay.add_controller(motion);
}

pub(crate) fn connect_popover_chrome_pin(popover: &gtk::Popover, chrome: Rc<ChromeVisibility>) {
    let show_chrome = Rc::clone(&chrome);
    popover.connect_show(move |_| {
        show_chrome.pin();
    });

    popover.connect_closed(move |_| {
        chrome.unpin();
    });
}

pub(crate) fn prepare_track_popover(popover: &gtk::Popover, kind: PlayerPopoverKind) {
    popover.add_css_class("okp-track-popover");
    popover.add_css_class(kind.css_class());
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Top);
}

pub(crate) fn connect_popover_focus_return(popover: &gtk::Popover, source: &gtk::MenuButton) {
    let source = source.clone();
    popover.connect_closed(move |_| {
        source.grab_focus();
    });
}

pub(crate) fn side_panel_segment_button(label: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("okp-side-panel-tab");
    button.set_has_frame(false);
    if selected {
        button.add_css_class("is-selected");
    }
    button
}

pub(crate) fn preferred_side_panel_mode(state: &Rc<RefCell<PlayerState>>) -> SidePanelMode {
    let state = state.borrow();
    let source = state.mpv.as_ref().and_then(|mpv| {
        let has_embedded = !mpv.observed_chapters().is_empty();
        let duration = mpv.observed_playback_state().duration.unwrap_or(0.0);
        chapter_math::active_chapter_source(has_embedded, duration)
    });
    if source.is_some() {
        SidePanelMode::Chapters
    } else {
        SidePanelMode::UpNext
    }
}

pub(crate) fn update_side_panel_tab_state(
    chapters_tab: &gtk::Button,
    up_next_tab: &gtk::Button,
    mode: SidePanelMode,
) {
    match mode {
        SidePanelMode::Chapters => {
            chapters_tab.add_css_class("is-selected");
            up_next_tab.remove_css_class("is-selected");
        }
        SidePanelMode::UpNext => {
            up_next_tab.add_css_class("is-selected");
            chapters_tab.remove_css_class("is-selected");
        }
    }
}

pub(crate) fn set_side_panel_user_visible(
    revealer: &gtk::Revealer,
    fade_revealer: &gtk::Revealer,
    toggle: &gtk::Button,
    user_visible: &Rc<Cell<bool>>,
    pinned: &Rc<Cell<bool>>,
    chrome: &ChromeVisibility,
    visible: bool,
) {
    user_visible.set(visible);
    revealer.set_can_target(visible);
    fade_revealer.set_reveal_child(visible);
    revealer.set_reveal_child(visible);

    if visible {
        toggle.add_css_class("is-selected");
        if pinned.get() {
            chrome.show_persistently();
        } else {
            chrome.pin();
            pinned.set(true);
        }
    } else {
        toggle.remove_css_class("is-selected");
        if pinned.replace(false) {
            chrome.unpin();
        }
    }
}

pub(crate) fn update_fullscreen_button(button: &gtk::Button, is_fullscreen: bool) {
    if is_fullscreen {
        button.set_icon_name("view-restore-symbolic");
        button.set_tooltip_text(Some("Exit Fullscreen (F / Esc)"));
        button.add_css_class("is-selected");
    } else {
        button.set_icon_name("view-fullscreen-symbolic");
        button.set_tooltip_text(Some("Enter Fullscreen (F)"));
        button.remove_css_class("is-selected");
    }
}

pub(crate) fn connect_seek_hover(
    seek: &gtk::Scale,
    state: Rc<RefCell<PlayerState>>,
    thumbnail_sender: mpsc::Sender<String>,
) -> Rc<SeekHoverPreview> {
    let preview = Rc::new(SeekHoverPreview::new(seek));
    let motion = gtk::EventControllerMotion::new();

    let motion_seek = seek.clone();
    let motion_state = Rc::clone(&state);
    let motion_preview = Rc::clone(&preview);
    motion.connect_motion(move |_, x, _| {
        let Some((media_path, duration, chapters)) = seek_hover_snapshot(&motion_state) else {
            motion_preview.hide();
            return;
        };

        let width = f64::from(motion_seek.width().max(1));
        let time = (x.clamp(0.0, width) / width * duration).clamp(0.0, duration);
        // Only a local file can be sampled for a hover thumbnail; a stream (or any
        // source without a file on disk) still gets the timecode + chapter preview,
        // just with no thumbnail — the deliberate timecode-only fallback.
        let thumbnail = media_path.as_deref().and_then(|path| {
            hover_thumbnail_for_time(&motion_state, path, time, duration, &thumbnail_sender)
        });
        motion_preview.show(
            &motion_seek,
            x,
            time,
            chapter_at_time(&chapters, time),
            thumbnail,
        );
    });

    let leave_preview = Rc::clone(&preview);
    motion.connect_leave(move |_| {
        leave_preview.hide();
    });

    seek.add_controller(motion);
    preview
}

pub(crate) fn seek_hover_snapshot(
    state: &Rc<RefCell<PlayerState>>,
) -> Option<(Option<PathBuf>, f64, Vec<Chapter>)> {
    let state = state.borrow();
    let thumbnail_source =
        seek_hover_source(state.current_file.clone(), state.current_url.as_deref())?;

    state
        .mpv
        .as_ref()
        .map(|mpv| mpv.observed_playback_state())
        .and_then(|playback| playback.duration)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (thumbnail_source, duration, state.chapters_snapshot.clone()))
}

/// Resolve the hover-preview source for the loaded media: `None` when nothing is
/// loaded (no preview at all), `Some(Some(file))` for a local file that can be
/// sampled for a thumbnail, and `Some(None)` for a stream — which still previews the
/// timecode and chapter but has no on-disk file to thumbnail.
pub(crate) fn seek_hover_source(
    current_file: Option<PathBuf>,
    current_url: Option<&str>,
) -> Option<Option<PathBuf>> {
    if current_file.is_none() && current_url.is_none() {
        return None;
    }
    Some(current_file)
}

pub(crate) fn chapter_at_time(chapters: &[Chapter], time: f64) -> Option<&Chapter> {
    let mut current = None;
    for chapter in chapters {
        if chapter.time.is_finite() && chapter.time <= time {
            current = Some(chapter);
        } else {
            break;
        }
    }

    current
}

pub(crate) fn hover_thumbnail_for_time(
    state: &Rc<RefCell<PlayerState>>,
    media_path: &Path,
    time: f64,
    duration: f64,
    sender: &mpsc::Sender<String>,
) -> Option<PathBuf> {
    let thumbnail_time = thumbnails::hover_thumbnail_time(time, duration);
    if let Some(path) = thumbnails::existing_hover_thumbnail_path(media_path, thumbnail_time) {
        return Some(path);
    }

    let request_key = thumbnails::hover_request_key(media_path, thumbnail_time);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.hover_thumbnail_request_key.as_deref() == Some(request_key.as_str()) {
            false
        } else {
            state.hover_thumbnail_request_key = Some(request_key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_hover_thumbnail(
            media_path.to_path_buf(),
            thumbnail_time,
            request_key,
            sender.clone(),
        );
    }

    None
}

/// Visual smoke hook: pop the seek hover tooltip over the timeline with a
/// representative timecode and chapter and no thumbnail — the deliberate
/// timecode-only fallback the tooltip shows for a stream, a not-yet-generated frame,
/// or an unavailable source. Presentational only; production code never calls this.
/// The pop is deferred so the seek scale has a real allocation to anchor against.
pub(crate) fn open_seek_preview(controls: &Controls) {
    let seek = controls.seek.clone();
    let preview = Rc::clone(&controls.seek_hover_preview);
    glib::timeout_add_local_once(Duration::from_millis(300), move || {
        let width = f64::from(seek.width().max(1));
        let chapter = Chapter {
            index: 2,
            time: 933.0,
            title: Some("The Long Walk Home".to_owned()),
        };
        preview.show(&seek, width / 2.0, chapter.time, Some(&chapter), None);
    });
}
