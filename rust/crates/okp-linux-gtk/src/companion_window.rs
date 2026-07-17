use super::*;

const COMPANION_WORK_AREA_MARGIN: i32 = 48;
const COMPANION_RESIZE_EDGE: i32 = 7;
const COMPANION_RESIZE_CORNER: i32 = 12;

#[derive(Default)]
pub(crate) struct CompanionWindows {
    settings: CompanionWindowSlot,
    media_info: CompanionWindowSlot,
}

#[derive(Default)]
struct CompanionWindowSlot {
    window: glib::WeakRef<gtk::Window>,
    restored_size: Option<window_fit::WindowSize>,
}

impl CompanionWindows {
    fn slot(&self, kind: CompanionWindowKind) -> &CompanionWindowSlot {
        match kind {
            CompanionWindowKind::Settings => &self.settings,
            CompanionWindowKind::MediaInfo => &self.media_info,
        }
    }

    fn slot_mut(&mut self, kind: CompanionWindowKind) -> &mut CompanionWindowSlot {
        match kind {
            CompanionWindowKind::Settings => &mut self.settings,
            CompanionWindowKind::MediaInfo => &mut self.media_info,
        }
    }
}

pub(crate) fn companion_window_name(kind: CompanionWindowKind) -> &'static str {
    match kind {
        CompanionWindowKind::Settings => "settings",
        CompanionWindowKind::MediaInfo => "media-info",
    }
}

pub(crate) fn present_existing_companion_window(
    state: &Rc<RefCell<PlayerState>>,
    kind: CompanionWindowKind,
) -> bool {
    let window = state.borrow().companion_windows.slot(kind).window.upgrade();
    let Some(window) = window else {
        return false;
    };

    window.present();
    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
        eprintln!(
            "interaction: companion={} focus-existing",
            companion_window_name(kind)
        );
    }
    true
}

pub(crate) fn build_companion_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    kind: CompanionWindowKind,
    title: &str,
) -> gtk::Window {
    let policy = companion_window_core::companion_window_policy(kind);
    let restored_size = state.borrow().companion_windows.slot(kind).restored_size;
    let work_area = companion_window_work_area(parent);
    let size = companion_window_core::companion_window_size(kind, restored_size, work_area);
    let application = parent
        .application()
        .expect("player window has an application");
    let window = gtk::Window::builder()
        .application(&application)
        .title(title)
        .default_width(size.width)
        .default_height(size.height)
        .modal(policy.modal)
        .resizable(policy.resizable)
        .decorated(false)
        .build();
    window.add_css_class("okp-companion-window");
    window.set_size_request(
        policy.minimum_size.width.min(work_area.width),
        policy.minimum_size.height.min(work_area.height),
    );

    let placed = Rc::new(Cell::new(false));
    let place_once = Rc::clone(&placed);
    window.connect_map(move |window| {
        if place_once.replace(true) {
            return;
        }
        let position = window_fit::centered_position(size, work_area);
        move_resize_player_window_on_x11(window, position, size);
    });

    state
        .borrow_mut()
        .companion_windows
        .slot_mut(kind)
        .window
        .set(Some(&window));

    let close_state = Rc::clone(state);
    window.connect_close_request(move |window| {
        let width = window.width();
        let height = window.height();
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!(
                "interaction: companion={} close-size={}x{}",
                companion_window_name(kind),
                width,
                height
            );
        }
        if width > 0 && height > 0 {
            let mut state = close_state.borrow_mut();
            let slot = state.companion_windows.slot_mut(kind);
            slot.restored_size = Some(window_fit::WindowSize { width, height });
            slot.window.set(None);
        } else {
            close_state
                .borrow_mut()
                .companion_windows
                .slot_mut(kind)
                .window
                .set(None);
        }
        glib::Propagation::Proceed
    });

    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
        eprintln!(
            "interaction: companion={} created modal={} resizable={} always-on-top={} parent-input={} size={}x{}",
            companion_window_name(kind),
            policy.modal,
            policy.resizable,
            policy.always_on_top,
            policy.parent_input_enabled,
            size.width,
            size.height
        );
    }

    window
}

pub(crate) fn close_companion_windows(state: &Rc<RefCell<PlayerState>>) {
    let windows = {
        let state = state.borrow();
        [
            state.companion_windows.settings.window.upgrade(),
            state.companion_windows.media_info.window.upgrade(),
        ]
    };
    for window in windows.into_iter().flatten() {
        window.close();
    }
}

pub(crate) fn companion_window_work_area(
    parent: &gtk::ApplicationWindow,
) -> window_fit::WindowRect {
    parent
        .surface()
        .and_then(|surface| surface.display().monitor_at_surface(&surface))
        .map(|monitor| companion_window_work_area_for_monitor(monitor.geometry()))
        .unwrap_or(window_fit::WindowRect {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        })
}

pub(crate) fn companion_window_work_area_for_monitor(
    geometry: gdk::Rectangle,
) -> window_fit::WindowRect {
    window_fit::WindowRect {
        x: geometry.x(),
        y: geometry.y(),
        width: geometry.width().max(1),
        height: geometry
            .height()
            .saturating_sub(COMPANION_WORK_AREA_MARGIN)
            .max(1),
    }
}

pub(crate) fn captionless_window_drag_layer(window: &gtk::Window) -> gtk::Box {
    let drag_layer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_layer.add_css_class("okp-captionless-window-drag-layer");
    drag_layer.set_halign(gtk::Align::Fill);
    drag_layer.set_valign(gtk::Align::Start);
    drag_layer.set_can_target(true);
    drag_layer.set_height_request(CAPTIONLESS_DRAG_HEIGHT);
    connect_captionless_window_drag(&drag_layer, window);
    drag_layer
}

pub(crate) fn connect_captionless_window_drag(
    widget: &impl IsA<gtk::Widget>,
    window: &gtk::Window,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let drag_window = window.clone();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: companion-drag pressed");
        }
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

pub(crate) fn add_companion_window_resize_zones(overlay: &gtk::Overlay, window: &gtk::Window) {
    for (edge, horizontal, vertical, width, height, cursor) in [
        (
            gdk::SurfaceEdge::North,
            gtk::Align::Fill,
            gtk::Align::Start,
            -1,
            COMPANION_RESIZE_EDGE,
            "n-resize",
        ),
        (
            gdk::SurfaceEdge::South,
            gtk::Align::Fill,
            gtk::Align::End,
            -1,
            COMPANION_RESIZE_EDGE,
            "s-resize",
        ),
        (
            gdk::SurfaceEdge::West,
            gtk::Align::Start,
            gtk::Align::Fill,
            COMPANION_RESIZE_EDGE,
            -1,
            "w-resize",
        ),
        (
            gdk::SurfaceEdge::East,
            gtk::Align::End,
            gtk::Align::Fill,
            COMPANION_RESIZE_EDGE,
            -1,
            "e-resize",
        ),
        (
            gdk::SurfaceEdge::NorthWest,
            gtk::Align::Start,
            gtk::Align::Start,
            COMPANION_RESIZE_CORNER,
            COMPANION_RESIZE_CORNER,
            "nw-resize",
        ),
        (
            gdk::SurfaceEdge::NorthEast,
            gtk::Align::End,
            gtk::Align::Start,
            COMPANION_RESIZE_CORNER,
            COMPANION_RESIZE_CORNER,
            "ne-resize",
        ),
        (
            gdk::SurfaceEdge::SouthWest,
            gtk::Align::Start,
            gtk::Align::End,
            COMPANION_RESIZE_CORNER,
            COMPANION_RESIZE_CORNER,
            "sw-resize",
        ),
        (
            gdk::SurfaceEdge::SouthEast,
            gtk::Align::End,
            gtk::Align::End,
            COMPANION_RESIZE_CORNER,
            COMPANION_RESIZE_CORNER,
            "se-resize",
        ),
    ] {
        let zone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        zone.add_css_class("okp-companion-resize-zone");
        zone.set_halign(horizontal);
        zone.set_valign(vertical);
        zone.set_size_request(width, height);
        zone.set_can_target(true);
        zone.set_cursor_from_name(Some(cursor));
        connect_companion_window_resize(&zone, window, edge);
        overlay.add_overlay(&zone);
    }
}

fn connect_companion_window_resize(
    widget: &impl IsA<gtk::Widget>,
    window: &gtk::Window,
    edge: gdk::SurfaceEdge,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let resize_window = window.clone();
    gesture.connect_pressed(move |gesture, _, x, y| {
        if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
            eprintln!("interaction: companion-resize edge={edge:?}");
        }
        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(surface) = resize_window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        let surface_x = match edge {
            gdk::SurfaceEdge::NorthEast | gdk::SurfaceEdge::East | gdk::SurfaceEdge::SouthEast => {
                f64::from(resize_window.width().saturating_sub(1))
            }
            gdk::SurfaceEdge::NorthWest | gdk::SurfaceEdge::West | gdk::SurfaceEdge::SouthWest => {
                0.0
            }
            _ => x,
        };
        let surface_y = match edge {
            gdk::SurfaceEdge::SouthWest | gdk::SurfaceEdge::South | gdk::SurfaceEdge::SouthEast => {
                f64::from(resize_window.height().saturating_sub(1))
            }
            gdk::SurfaceEdge::NorthWest | gdk::SurfaceEdge::North | gdk::SurfaceEdge::NorthEast => {
                0.0
            }
            _ => y,
        };
        toplevel.begin_resize(
            edge,
            Some(&device),
            gesture.current_button() as i32,
            surface_x,
            surface_y,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}
