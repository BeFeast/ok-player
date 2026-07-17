//! Adaptive OSC control bar container (issue #328).
//!
//! The bottom control strip has to keep its primary transport, timeline,
//! volume, and the `…` overflow entry usable at every window width, folding
//! lower-priority actions into the overflow menu *before* any two controls
//! overlap. A plain `GtkBox` cannot do this: its horizontal minimum is the sum
//! of every child, so when the window is narrower the box is still allocated
//! its full minimum and simply clips the trailing controls — which is exactly
//! why the previous layout floated the overflow button as a separate overlay
//! that then painted over its neighbour.
//!
//! This custom widget reports a *low* horizontal minimum (only the never-
//! collapsing floor) so GTK hands it the real, narrow allocation, and then in
//! `size_allocate` it runs the pure [`okp_core::osc_overflow`] policy to decide
//! which controls stay and where each one sits. Collapsed controls are marked
//! child-invisible, so they are unmapped — not painted, not focusable, not
//! hit-testable — and the overflow entry keeps an exclusive, reserved band as
//! the final in-flow action. The collapse decision itself lives in okp-core;
//! this shell widget only measures children and performs the allocation.

use super::*;
use gtk::graphene;
use gtk::gsk;
use gtk::subclass::prelude::*;
use okp_core::osc_overflow::{self, OscControlId, OscSlot};

/// Horizontal inset of the control content from the pill edge. Matches the
/// `.okp-controls` design padding; kept here (with CSS padding zeroed) so the
/// custom allocation owns the geometry rather than fighting GTK's CSS gadget.
pub(crate) const PAD_HORIZONTAL: i32 = 14;
/// Vertical inset of the control content from the pill edge.
pub(crate) const PAD_VERTICAL: i32 = 7;
/// Gap between adjacent visible controls.
pub(crate) const SPACING: i32 = 16;

/// Measure each child's baseline width. Fixed controls render at their natural
/// width; the flexible timeline reports its minimum so the policy can grow it
/// into the leftover slack.
fn measure_slots(children: &[(gtk::Widget, OscControlId)]) -> Vec<OscSlot> {
    children
        .iter()
        .map(|(child, id)| {
            let (min, nat, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            let baseline = if id.is_flexible() {
                min.max(1)
            } else {
                nat.max(min)
            };
            OscSlot::new(*id, baseline.max(0))
        })
        .collect()
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub(crate) struct OscBar {
        pub(super) children: RefCell<Vec<(gtk::Widget, OscControlId)>>,
        /// Shared sink for the controls the policy folded into the overflow
        /// menu at the current width. The overflow popover reads it to surface
        /// the collapsed actions. Written only when the set actually changes.
        pub(super) collapsed_sink: RefCell<Option<Rc<RefCell<Vec<OscControlId>>>>>,
        pub(super) debug_last_size: Cell<Option<(i32, i32)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OscBar {
        const NAME: &'static str = "OkpOscBar";
        type Type = super::OscBar;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for OscBar {
        fn dispose(&self) {
            for (child, _) in self.children.borrow_mut().drain(..) {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for OscBar {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let children = self.children.borrow();
            match orientation {
                gtk::Orientation::Horizontal => {
                    let slots = measure_slots(&children);
                    // Report only the floor as the minimum. That is what lets a
                    // narrower window hand this widget its true allocation
                    // instead of forcing the full-width minimum and clipping.
                    let min = osc_overflow::floor_min_width(&slots, SPACING) + PAD_HORIZONTAL * 2;
                    let nat = osc_overflow::natural_min_width(&slots, SPACING) + PAD_HORIZONTAL * 2;
                    (min, nat.max(min), -1, -1)
                }
                gtk::Orientation::Vertical => {
                    let mut height = 0;
                    for (child, _) in children.iter() {
                        let (child_min, child_nat, _, _) =
                            child.measure(gtk::Orientation::Vertical, -1);
                        height = height.max(child_nat.max(child_min));
                    }
                    let total = height + PAD_VERTICAL * 2;
                    (total, total, -1, -1)
                }
                _ => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let children = self.children.borrow();
            if children.is_empty() {
                return;
            }
            let slots = measure_slots(&children);
            let layout = osc_overflow::plan(&slots, width, SPACING, PAD_HORIZONTAL, PAD_HORIZONTAL);
            let content_height = (height - PAD_VERTICAL * 2).max(0);
            let log_layout = env::var_os("OKP_DEBUG_OSC_LAYOUT").is_some()
                && self.debug_last_size.replace(Some((width, height))) != Some((width, height));

            if let Some(sink) = self.collapsed_sink.borrow().as_ref() {
                let collapsed = layout.collapsed();
                if *sink.borrow() != collapsed {
                    sink.replace(collapsed);
                }
            }
            // Mirror the whole row for right-to-left locales so the visual order
            // (play … overflow) reverses, keeping the layout RTL-safe.
            let rtl = self.obj().direction() == gtk::TextDirection::Rtl;
            for ((child, id), placement) in children.iter().zip(layout.placements.iter()) {
                debug_assert_eq!(
                    *id, placement.id,
                    "child order diverged from the policy plan"
                );
                if !placement.visible {
                    // Unmaps the control: it stops being painted, focusable, or
                    // hit-testable, and its action lives on in the overflow menu.
                    child.set_child_visible(false);
                    if log_layout
                        && matches!(
                            id,
                            OscControlId::Volume | OscControlId::Audio | OscControlId::Overflow
                        )
                    {
                        eprintln!(
                            "osc-layout: id={id:?} visible=false bar_width={width} bar_height={height}"
                        );
                    }
                    continue;
                }
                child.set_child_visible(true);
                let x = if rtl {
                    width - PAD_HORIZONTAL - placement.x - placement.width
                } else {
                    PAD_HORIZONTAL + placement.x
                };
                let transform = gsk::Transform::new()
                    .translate(&graphene::Point::new(x as f32, PAD_VERTICAL as f32));
                // Each control is allocated the content height and centres
                // itself with its own valign, matching the previous GtkBox.
                child.allocate(placement.width, content_height, -1, Some(transform));
                if log_layout
                    && matches!(
                        id,
                        OscControlId::Volume | OscControlId::Audio | OscControlId::Overflow
                    )
                {
                    eprintln!(
                        "osc-layout: id={id:?} visible=true x={} y={} width={} height={} bar_width={width} bar_height={height}",
                        PAD_HORIZONTAL + placement.x,
                        PAD_VERTICAL,
                        placement.width,
                        content_height
                    );
                }
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            for (child, _) in self.children.borrow().iter() {
                widget.snapshot_child(child, snapshot);
            }
        }
    }
}

glib::wrapper! {
    pub(crate) struct OscBar(ObjectSubclass<imp::OscBar>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for OscBar {
    fn default() -> Self {
        Self::new()
    }
}

impl OscBar {
    pub(crate) fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Append a control in canonical order, tagged with the policy id that
    /// governs its collapse priority.
    pub(crate) fn push(&self, child: &impl IsA<gtk::Widget>, id: OscControlId) {
        let child = child.clone().upcast::<gtk::Widget>();
        child.set_parent(self);
        self.imp().children.borrow_mut().push((child, id));
    }

    /// Point the bar at a shared vec that mirrors, at the live window width,
    /// the controls folded into the overflow menu, so the `…` popover can
    /// surface their actions.
    pub(crate) fn set_collapsed_sink(&self, sink: Rc<RefCell<Vec<OscControlId>>>) {
        self.imp().collapsed_sink.replace(Some(sink));
    }
}
