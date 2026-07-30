//! Report where the player's surfaces actually are, for input harnesses.
//!
//! A Wayland client cannot be asked where its window is - no client may query another
//! window's position and the shells refuse introspection to unprivileged callers - so
//! automation that drives real pointer input ends up guessing coordinates and missing the
//! window. The app knows, so under `OKP_DEBUG_INTERACTIONS` it says so on the same
//! `interaction:` stream every other diagnostic uses.
//!
//! This file only gathers GTK values and prints what `okp_core::interaction_geometry`
//! projects from them; the projection itself is portable and covered by tests there.

use super::*;

use okp_core::focus_trace;
use okp_core::interaction_geometry::{
    self as geometry, Monitor, Plane, Point, Rect, WindowGeometry,
};

/// Poll interval for the snapshot. Only a mapped, changed snapshot prints, so this is a
/// change detector rather than a log source, and it exists only with the diagnostic on.
const GEOMETRY_SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

/// Floor between pointer samples. Motion is continuous, and a harness needs only one
/// sample to resolve the origin it cannot query.
const POINTER_SAMPLE_INTERVAL: Duration = Duration::from_millis(150);

/// The window-local planes reported alongside the video surface.
///
/// `chrome` is ordered bottom to top, mirroring the order the root overlay stacks them,
/// so the plane that owns a point is the last one containing it - the same answer GTK
/// picking gives.
pub(crate) struct GeometrySurfaces {
    pub(crate) video: gtk::Widget,
    pub(crate) chrome: Vec<(String, gtk::Widget)>,
    pub(crate) nested: Vec<NestedPlanes>,
}

/// Planes inside a chrome surface that the shell rebuilds with it, so they cannot be
/// captured once at construction the way the fixed surfaces are.
///
/// They are resolved from the live widget tree on every snapshot and reported directly
/// above their host, which keeps the plane list in stacking order: a point inside one of
/// them is owned by the child, exactly as GTK picking would answer.
pub(crate) struct NestedPlanes {
    /// Name of the chrome plane these sit inside.
    pub(crate) host: &'static str,
    /// Reported plane name, suffixed with the child's index in on-screen order.
    pub(crate) prefix: &'static str,
    /// CSS class marking the widgets to report.
    pub(crate) css_class: &'static str,
}

struct GeometryReporter {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    surfaces: GeometrySurfaces,
    previous: RefCell<Option<WindowGeometry>>,
    seq: Cell<u64>,
    last_pointer: Cell<Option<Instant>>,
}

/// Emit the toplevel rectangle, the video plane, the chrome planes and the aim point
/// whenever any of them changes, plus a pointer sample a harness can calibrate with.
pub(crate) fn connect_geometry_diagnostics(
    window: &gtk::ApplicationWindow,
    surfaces: GeometrySurfaces,
) {
    if env::var_os("OKP_DEBUG_INTERACTIONS").is_none() {
        return;
    }

    let reporter = Rc::new(GeometryReporter {
        window: window.downgrade(),
        surfaces,
        previous: RefCell::new(None),
        seq: Cell::new(0),
        last_pointer: Cell::new(None),
    });

    let mapped = Rc::clone(&reporter);
    window.connect_map(move |_| mapped.publish());

    // Fullscreen, compact, monitor and layout changes all land in the same snapshot, so
    // one deduplicated poll covers every transition a harness cares about without wiring
    // a signal per property or holding the frame clock awake.
    let polled = Rc::clone(&reporter);
    glib::timeout_add_local(GEOMETRY_SAMPLE_INTERVAL, move || {
        if polled.window.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        polled.publish();
        glib::ControlFlow::Continue
    });

    let motion = gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_motion(move |_, x, y| reporter.publish_pointer(Point::new(x, y)));
    window.add_controller(motion);

    connect_focus_diagnostics(window);
}

/// Name each keyboard-focus stop, so a harness can tell traversal apart from keys that
/// went nowhere.
///
/// Tab is stock GTK behaviour with no shell code and no outcome anywhere else in the
/// diagnostics, so a check driving it could otherwise only assert that it sent keys - a
/// check that passes just as well when focus never moves, which is not evidence.
fn connect_focus_diagnostics(window: &gtk::ApplicationWindow) {
    let sequence = Cell::new(0u64);
    let previous = RefCell::new(String::new());
    window.connect_focus_widget_notify(move |window| {
        let token = gtk::prelude::GtkWindowExt::focus(window)
            .map(|widget| focus_widget_token(&widget))
            .unwrap_or_else(|| focus_trace::NO_FOCUS.to_owned());
        // Only a change is a stop. GTK renotifies while a widget rebuilds, and a repeated
        // name would let a harness read standing still as traversal.
        if *previous.borrow() == token {
            return;
        }
        previous.replace(token.clone());
        let next = sequence.get().wrapping_add(1);
        sequence.set(next);
        eprintln!("{}", focus_trace::focus_line(&token, next));
    });
}

fn focus_widget_token(widget: &gtk::Widget) -> String {
    let classes = widget.css_classes();
    // The tooltip is the label a user reads off this control, so it is the closest thing
    // to a name for the widgets the stylesheet does not claim with an `okp-` class.
    let label = widget.tooltip_text();
    focus_trace::focus_token(
        widget.type_().name(),
        classes.iter().map(|class| class.as_str()),
        label.as_deref(),
    )
}

impl GeometryReporter {
    fn publish(&self) {
        let Some(current) = self.snapshot() else {
            return;
        };
        let mut previous = self.previous.borrow_mut();
        if previous.as_ref() == Some(&current) {
            return;
        }
        let reason = geometry::reason_between(previous.as_ref(), &current);
        let seq = self.seq.get().wrapping_add(1);
        self.seq.set(seq);
        for line in current.record(reason, seq) {
            eprintln!("{line}");
        }
        *previous = Some(current);
    }

    fn publish_pointer(&self, local: Point) {
        let now = Instant::now();
        if let Some(last) = self.last_pointer.get()
            && now.duration_since(last) < POINTER_SAMPLE_INTERVAL
        {
            return;
        }
        self.last_pointer.set(Some(now));
        let sample = self
            .previous
            .borrow()
            .as_ref()
            .map(|current| current.pointer_sample(local, self.seq.get()));
        if let Some(sample) = sample {
            eprintln!("{sample}");
        }
    }

    fn snapshot(&self) -> Option<WindowGeometry> {
        let window = self.window.upgrade()?;
        if !window.is_mapped() {
            return None;
        }
        let width = f64::from(window.width());
        let height = f64::from(window.height());
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        let display = gtk::prelude::WidgetExt::display(&window);
        let monitor = window
            .surface()
            .and_then(|surface| display.monitor_at_surface(&surface))
            .map(|monitor| monitor_geometry_info(&monitor));

        let mut planes = Vec::with_capacity(self.surfaces.chrome.len() + 1);
        if let Some(bounds) = plane_bounds(&self.surfaces.video, &window) {
            planes.push(Plane::new(
                geometry::VIDEO_PLANE,
                bounds,
                self.surfaces.video.can_target(),
            ));
        }
        for (name, widget) in &self.surfaces.chrome {
            if let Some(bounds) = plane_bounds(widget, &window) {
                planes.push(Plane::new(name.clone(), bounds, widget.can_target()));
            }
            for nested in self
                .surfaces
                .nested
                .iter()
                .filter(|nested| nested.host == name)
            {
                for (index, child) in
                    descendants_with_css_class(widget, nested.css_class).enumerate()
                {
                    if let Some(bounds) = plane_bounds(&child, &window) {
                        planes.push(Plane::new(
                            format!("{}-{index}", nested.prefix),
                            bounds,
                            child.can_target(),
                        ));
                    }
                }
            }
        }

        Some(WindowGeometry {
            client: Rect::new(0.0, 0.0, width, height),
            scale: current_player_scale(&window),
            fullscreen: window.is_fullscreen(),
            maximized: window.is_maximized(),
            compact: window.has_css_class("is-compact-mode"),
            // Only X11 publishes a toplevel position, and the shell never reads it back
            // there either. Wayland has no such call at all, and inventing a value would
            // be worse than reporting that the origin needs a pointer sample.
            position: None,
            monitor,
            monitors: display_monitor_geometry(&display),
            planes,
        })
    }
}

/// Every descendant of `root` carrying `css_class`, in document order.
///
/// Document order is on-screen order for the surfaces this reports, so the index a plane
/// name carries is the position a reader sees - the first card is `-0`.
fn descendants_with_css_class(
    root: &gtk::Widget,
    css_class: &str,
) -> impl Iterator<Item = gtk::Widget> {
    let mut found = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(widget) = pending.pop() {
        if widget.has_css_class(css_class) {
            found.push(widget.clone());
        }
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            children.push(current);
        }
        pending.extend(children.into_iter().rev());
    }
    found.into_iter()
}

fn plane_bounds(widget: &gtk::Widget, window: &gtk::ApplicationWindow) -> Option<Rect> {
    if !widget.is_mapped() {
        return None;
    }
    let bounds = widget.compute_bounds(window)?;
    let rect = Rect::new(
        f64::from(bounds.x()),
        f64::from(bounds.y()),
        f64::from(bounds.width()),
        f64::from(bounds.height()),
    );
    (!rect.is_empty()).then_some(rect)
}

fn monitor_geometry_info(monitor: &gdk::Monitor) -> Monitor {
    let bounds = monitor.geometry();
    Monitor {
        connector: monitor_log_token(monitor),
        bounds: Rect::new(
            f64::from(bounds.x()),
            f64::from(bounds.y()),
            f64::from(bounds.width()),
            f64::from(bounds.height()),
        ),
        scale: monitor_report_scale(monitor),
    }
}

fn monitor_report_scale(monitor: &gdk::Monitor) -> f64 {
    // GTK 4.14+ publishes fractional monitor scale; keep the crate's GTK 4.10 floor by
    // discovering the property, with the integer ceiling as the older-runtime fallback.
    let scale = if monitor.find_property("scale").is_some() {
        monitor.property::<f64>("scale")
    } else {
        f64::from(monitor.scale_factor())
    };
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn display_monitor_geometry(display: &gdk::Display) -> Vec<Monitor> {
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index))
        .filter_map(|item| item.downcast::<gdk::Monitor>().ok())
        .map(|monitor| monitor_geometry_info(&monitor))
        .collect()
}
