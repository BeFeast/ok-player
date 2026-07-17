//! Pure geometry for aspect-locked window resizing (the Shift-resize feature) — port of
//! `src/OkPlayer.Core/AspectResize.cs`; the C# suite in
//! `tests/OkPlayer.Tests/AspectResizeTests.cs` is the executable spec. Given a proposed OUTER
//! window rectangle from a live resize drag, the dragged edge, the target client aspect
//! ratio, and the non-client insets (outer minus client, i.e. the resize borders), it returns
//! a rectangle whose CLIENT area matches the aspect. The edge(s) the user is dragging stay
//! put; the free edge moves to compensate. Engine- and UI-free.

/// Which window edge or corner a resize drag grips. The discriminants are the Win32 `WMSZ_*`
/// edge codes (the `wParam` of `WM_SIZING`) so the Windows consumer can cast straight through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ResizeEdge {
    Left = 1,
    Right = 2,
    Top = 3,
    TopLeft = 4,
    TopRight = 5,
    Bottom = 6,
    BottomLeft = 7,
    BottomRight = 8,
}

/// Adjust a proposed outer `(left, top, right, bottom)` rect so its client area holds
/// `aspect` (width/height). `frame_w`/`frame_h` are the non-client insets (outer size −
/// client size). Returns the original rect unchanged when the inputs can't yield a valid
/// client box.
pub fn constrain(
    rect: (i32, i32, i32, i32),
    edge: ResizeEdge,
    aspect: f64,
    frame_w: i32,
    frame_h: i32,
) -> (i32, i32, i32, i32) {
    let (left, mut top, mut right, mut bottom) = rect;
    let client_w = (right - left) - frame_w;
    let client_h = (bottom - top) - frame_h;
    if aspect <= 0.0 || client_w <= 0 || client_h <= 0 {
        return rect;
    }

    // round_ties_even mirrors C# Math.Round (banker's rounding at .5 midpoints).
    match edge {
        // Dragging a vertical edge → width is the user's intent.
        // Width leads: derive height, grow/shrink downward (keep the top edge).
        ResizeEdge::Left | ResizeEdge::Right => {
            let new_client_h = (f64::from(client_w) / aspect).round_ties_even() as i32;
            bottom = top + new_client_h + frame_h;
        }
        // Dragging a horizontal edge → height is the user's intent.
        // Height leads: derive width, grow/shrink rightward (keep the left edge).
        ResizeEdge::Top | ResizeEdge::Bottom => {
            let new_client_w = (f64::from(client_h) * aspect).round_ties_even() as i32;
            right = left + new_client_w + frame_w;
        }
        // Corner: width leads height; move the vertical edge that belongs to the dragged corner.
        ResizeEdge::TopLeft | ResizeEdge::TopRight => {
            let new_client_h = (f64::from(client_w) / aspect).round_ties_even() as i32;
            top = bottom - new_client_h - frame_h;
        }
        ResizeEdge::BottomLeft | ResizeEdge::BottomRight => {
            let new_client_h = (f64::from(client_w) / aspect).round_ties_even() as i32;
            bottom = top + new_client_h + frame_h;
        }
    }
    (left, top, right, bottom)
}

// ---------------------------------------------------------------------------
// Linux interactive Shift-resize (issue #331)
//
// GTK's `compute-size` response is only a preferred-size hint during a Mutter
// interactive resize, so it cannot enforce a live aspect ratio. The Linux shell
// therefore owns pointer-to-size projection while a resize handle is dragged.
// This module keeps the pointer geometry, clamping, anchoring, and modifier
// transitions portable; the shell only feeds pointer offsets and applies the
// returned logical client geometry.
// ---------------------------------------------------------------------------

use crate::window_fit::{WindowPoint, WindowRect, WindowSize};

/// Logical pointer travel from the beginning of an app-owned resize gesture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerDelta {
    pub x: f64,
    pub y: f64,
}

impl PointerDelta {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

/// Logical client geometry requested for one pointer update. `position_delta`
/// moves the toplevel origin relative to the drag start so X11 can keep the
/// opposite edge/corner fixed. Wayland intentionally applies `size` only because
/// clients cannot position normal toplevels there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeGeometry {
    pub size: WindowSize,
    pub position_delta: WindowPoint,
}

/// Which client dimension leads when an aspect-locked resolve derives the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leading {
    Width,
    Height,
}

impl ResizeEdge {
    /// The fixed leading axis for a straight edge. Corners return `None`; the
    /// leading axis for a corner depends on which way the pointer moved and is
    /// decided per resolve so the locked box tracks the pointer diagonally.
    fn straight_leading(self) -> Option<Leading> {
        match self {
            ResizeEdge::Left | ResizeEdge::Right => Some(Leading::Width),
            ResizeEdge::Top | ResizeEdge::Bottom => Some(Leading::Height),
            ResizeEdge::TopLeft
            | ResizeEdge::TopRight
            | ResizeEdge::BottomLeft
            | ResizeEdge::BottomRight => None,
        }
    }

    const fn moves_left(self) -> bool {
        matches!(
            self,
            ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft
        )
    }

    const fn moves_top(self) -> bool {
        matches!(
            self,
            ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight
        )
    }

    const fn horizontal_direction(self) -> i32 {
        if self.moves_left() {
            -1
        } else if matches!(
            self,
            ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight
        ) {
            1
        } else {
            0
        }
    }

    const fn vertical_direction(self) -> i32 {
        if self.moves_top() {
            -1
        } else if matches!(
            self,
            ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight
        ) {
            1
        } else {
            0
        }
    }
}

fn sane_size(size: WindowSize) -> WindowSize {
    WindowSize {
        width: size.width.max(1),
        height: size.height.max(1),
    }
}

/// Lock a proposed client size to `aspect` (client width / height) for a drag
/// gripping `edge` from `origin`, then clamp it — while staying on the aspect line — so it
/// never falls below `min` or exceeds `max` on either axis.
///
/// Straight edges have one meaningful axis (a vertical edge leads with width, a
/// horizontal edge with height). For corners the leading axis is whichever one
/// encloses the pointer, so a mostly-horizontal drag follows width and a
/// mostly-vertical drag follows height. `min` guards the minimum usable OSC size;
/// `max` is the monitor workarea (already reduced for chrome by the caller).
/// Returns `proposed` unchanged when `aspect` is not a positive, finite ratio.
pub fn lock_client_size(
    origin: WindowSize,
    proposed: WindowSize,
    edge: ResizeEdge,
    aspect: f64,
    min: WindowSize,
    max: WindowSize,
) -> WindowSize {
    if !aspect.is_finite() || aspect <= 0.0 {
        return proposed;
    }
    let proposed = sane_size(proposed);

    let leading = edge
        .straight_leading()
        .unwrap_or_else(|| corner_leading(sane_size(origin), proposed));

    let width_target = match leading {
        Leading::Width => f64::from(proposed.width),
        Leading::Height => f64::from(proposed.height) * aspect,
    };
    clamp_to_aspect(width_target, aspect, min, max)
}

/// Clamp a target width (on the `aspect` line) to the width range that keeps
/// both axes within `min`/`max`, then derive the height. When `min` exceeds
/// `max` (a workarea smaller than the minimum OSC), the workarea wins so the
/// window can never be pushed off-screen.
fn clamp_to_aspect(width_target: f64, aspect: f64, min: WindowSize, max: WindowSize) -> WindowSize {
    let min = sane_size(min);
    // A non-positive max means "unbounded"; fall back to the target itself.
    let width_ceiling = if max.width > 0 && max.height > 0 {
        f64::from(max.width).min(f64::from(max.height) * aspect)
    } else {
        width_target.max(1.0)
    };
    let width_floor = f64::from(min.width).max(f64::from(min.height) * aspect);

    let width = if width_ceiling < width_floor {
        width_ceiling
    } else {
        width_target.clamp(width_floor, width_ceiling)
    };

    let mut width = width.round_ties_even().max(1.0) as i32;
    let mut height = (f64::from(width) / aspect).round_ties_even().max(1.0) as i32;

    // Floating projection followed by integer rounding must never cross the
    // accepted workarea ceiling. Tighten the leading dimension once if the
    // derived axis rounded upward beyond it.
    if max.width > 0 && width > max.width {
        width = max.width.max(1);
        height = (f64::from(width) / aspect).round_ties_even().max(1.0) as i32;
    }
    if max.height > 0 && height > max.height {
        height = max.height.max(1);
        width = (f64::from(height) * aspect).floor().max(1.0) as i32;
        height = (f64::from(width) / aspect).round_ties_even().max(1.0) as i32;
    }
    WindowSize { width, height }
}

fn clamp_freeform(size: WindowSize, min: WindowSize, max: WindowSize) -> WindowSize {
    fn axis(value: i32, floor: i32, ceiling: i32) -> i32 {
        let floor = floor.max(1);
        if ceiling > 0 {
            value.max(1).clamp(floor.min(ceiling), ceiling)
        } else {
            value.max(floor)
        }
    }

    WindowSize {
        width: axis(size.width, min.width, max.width),
        height: axis(size.height, min.height, max.height),
    }
}

fn pointer_proposal(origin: ResizeFrame, pointer: PointerDelta, edge: ResizeEdge) -> WindowSize {
    let delta_x = (pointer.x - origin.pointer.x).round_ties_even() as i32;
    let delta_y = (pointer.y - origin.pointer.y).round_ties_even() as i32;
    WindowSize {
        width: origin
            .geometry
            .size
            .width
            .saturating_add(edge.horizontal_direction().saturating_mul(delta_x)),
        height: origin
            .geometry
            .size
            .height
            .saturating_add(edge.vertical_direction().saturating_mul(delta_y)),
    }
}

fn corner_leading(origin: WindowSize, proposed: WindowSize) -> Leading {
    let width_fraction = (f64::from(proposed.width) - f64::from(origin.width)).abs()
        / f64::from(origin.width.max(1));
    let height_fraction = (f64::from(proposed.height) - f64::from(origin.height)).abs()
        / f64::from(origin.height.max(1));
    if width_fraction >= height_fraction {
        Leading::Width
    } else {
        Leading::Height
    }
}

fn locked_size(
    origin: WindowSize,
    proposed: WindowSize,
    edge: ResizeEdge,
    aspect: f64,
    min: WindowSize,
    max: WindowSize,
) -> WindowSize {
    lock_client_size(origin, proposed, edge, aspect, min, max)
}

fn anchored_geometry(origin: ResizeGeometry, size: WindowSize, edge: ResizeEdge) -> ResizeGeometry {
    ResizeGeometry {
        size,
        position_delta: WindowPoint {
            x: if edge.moves_left() {
                origin
                    .position_delta
                    .x
                    .saturating_add(origin.size.width.saturating_sub(size.width))
            } else {
                origin.position_delta.x
            },
            y: if edge.moves_top() {
                origin
                    .position_delta
                    .y
                    .saturating_add(origin.size.height.saturating_sub(size.height))
            } else {
                origin.position_delta.y
            },
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ResizeFrame {
    pointer: PointerDelta,
    geometry: ResizeGeometry,
}

/// The live state of one app-owned interactive resize drag. Created when a
/// resize handle press begins, resolved for each pointer update, and dropped
/// when the drag ends.
///
/// It owns the deterministic answer to "what happens when Shift is pressed or
/// released mid-drag" (issue #331): pressing Shift locks the aspect to the
/// window's proportions *at that instant*; releasing Shift returns to freeform
/// and keeps the current size. Modifier transitions rebase the pointer origin,
/// so the next motion continues from the size already reached without snap-back.
#[derive(Debug, Clone, PartialEq)]
pub struct AspectResize {
    edge: ResizeEdge,
    min: WindowSize,
    max: WindowSize,
    locked_aspect: Option<f64>,
    origin: ResizeFrame,
    last: ResizeFrame,
}

impl AspectResize {
    /// Begin a drag from `edge` at the current client size `start`. When `shift`
    /// is already held the aspect locks immediately to `start`'s proportions.
    pub fn begin(
        edge: ResizeEdge,
        start: WindowSize,
        min: WindowSize,
        max: WindowSize,
        shift: bool,
    ) -> Self {
        let mut session = Self {
            edge,
            min,
            max,
            locked_aspect: None,
            origin: ResizeFrame {
                pointer: PointerDelta::ZERO,
                geometry: ResizeGeometry {
                    size: sane_size(start),
                    position_delta: WindowPoint { x: 0, y: 0 },
                },
            },
            last: ResizeFrame {
                pointer: PointerDelta::ZERO,
                geometry: ResizeGeometry {
                    size: sane_size(start),
                    position_delta: WindowPoint { x: 0, y: 0 },
                },
            },
        };
        if shift {
            session.lock_to_last();
        }
        session
    }

    fn lock_to_last(&mut self) {
        let aspect =
            f64::from(self.last.geometry.size.width) / f64::from(self.last.geometry.size.height);
        if aspect.is_finite() && aspect > 0.0 {
            self.locked_aspect = Some(aspect);
        }
    }

    /// Report a Shift press (`true`) or release (`false`) during the drag. The
    /// current pointer/geometry frame becomes the new origin, so neither
    /// transition can jump back to the drag's initial size.
    pub fn set_shift(&mut self, pressed: bool) {
        self.origin = self.last;
        if pressed {
            self.lock_to_last();
        } else {
            self.locked_aspect = None;
        }
    }

    /// Whether the aspect is currently locked (Shift held).
    pub fn is_locked(&self) -> bool {
        self.locked_aspect.is_some()
    }

    /// The edge this drag grips.
    pub fn edge(&self) -> ResizeEdge {
        self.edge
    }

    /// Resolve one pointer update into a bounded logical client size and an
    /// opposite-edge anchor delta. Exactly one result is produced per input;
    /// compositor configure sizes never feed back into this state machine.
    pub fn resolve(&mut self, pointer: PointerDelta) -> ResizeGeometry {
        let proposed = pointer_proposal(self.origin, pointer, self.edge);
        let size = match self.locked_aspect {
            Some(aspect) => locked_size(
                self.origin.geometry.size,
                proposed,
                self.edge,
                aspect,
                self.min,
                self.max,
            ),
            None => clamp_freeform(proposed, self.min, self.max),
        };
        let geometry = anchored_geometry(self.origin.geometry, size, self.edge);
        self.last = ResizeFrame { pointer, geometry };
        geometry
    }
}

/// Maximum client size that keeps the fixed edge/corner inside `work_area` when
/// the platform exposes the starting toplevel position (X11). Wayland callers
/// pass no position and use the full logical workarea dimensions instead.
pub fn client_max_for_anchor(
    edge: ResizeEdge,
    start_size: WindowSize,
    start_position: Option<WindowPoint>,
    work_area: WindowRect,
) -> WindowSize {
    let fallback = WindowSize {
        width: work_area.width.max(1),
        height: work_area.height.max(1),
    };
    let Some(position) = start_position else {
        return fallback;
    };
    let work_right = work_area.x.saturating_add(work_area.width);
    let work_bottom = work_area.y.saturating_add(work_area.height);
    let fixed_right = position.x.saturating_add(start_size.width);
    let fixed_bottom = position.y.saturating_add(start_size.height);
    WindowSize {
        width: if edge.moves_left() {
            fixed_right.saturating_sub(work_area.x)
        } else {
            work_right.saturating_sub(position.x)
        }
        .max(1),
        height: if edge.moves_top() {
            fixed_bottom.saturating_sub(work_area.y)
        } else {
            work_bottom.saturating_sub(position.y)
        }
        .max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: f64 = 16.0 / 9.0;
    /// Typical resize-border insets.
    const FRAME_W: i32 = 16;
    const FRAME_H: i32 = 8;

    #[test]
    fn horizontal_drag_width_leads_derives_height() {
        // Drag the right edge to a 1600-wide client at a stale 500 height; height should snap
        // to 900.
        let rect = (100, 100, 100 + 1600 + FRAME_W, 100 + 500 + FRAME_H);
        let (l, t, r, b) = constrain(rect, ResizeEdge::Right, WIDE, FRAME_W, FRAME_H);
        assert_eq!(l, 100);
        assert_eq!(t, 100); // top edge stays
        assert_eq!((r - l) - FRAME_W, 1600);
        assert_eq!((b - t) - FRAME_H, 900);
    }

    #[test]
    fn vertical_drag_height_leads_derives_width() {
        // Drag the bottom edge to a 900-tall client at a stale 500 width; width should snap
        // to 1600.
        let rect = (100, 100, 100 + 500 + FRAME_W, 100 + 900 + FRAME_H);
        let (l, t, r, b) = constrain(rect, ResizeEdge::Bottom, WIDE, FRAME_W, FRAME_H);
        assert_eq!(l, 100); // left edge stays
        assert_eq!((r - l) - FRAME_W, 1600);
        assert_eq!((b - t) - FRAME_H, 900);
        assert_eq!(t, 100);
    }

    #[test]
    fn bottom_right_corner_moves_bottom_keeps_top_and_left() {
        let rect = (100, 100, 100 + 1600 + FRAME_W, 100 + 500 + FRAME_H);
        let (l, t, r, b) = constrain(rect, ResizeEdge::BottomRight, WIDE, FRAME_W, FRAME_H);
        assert_eq!(l, 100);
        assert_eq!(t, 100);
        assert_eq!((r - l) - FRAME_W, 1600);
        assert_eq!((b - t) - FRAME_H, 900);
    }

    #[test]
    fn top_left_corner_moves_top_keeps_bottom_and_right() {
        let bottom = 100 + 500 + FRAME_H; // this edge must be preserved
        let right_start = 100 + 1600 + FRAME_W;
        let rect = (100, 100, right_start, bottom);
        let (l, t, r, b) = constrain(rect, ResizeEdge::TopLeft, WIDE, FRAME_W, FRAME_H);
        assert_eq!(r, right_start); // right edge stays
        assert_eq!(b, bottom); // bottom edge stays
        assert_eq!((r - l) - FRAME_W, 1600);
        assert_eq!((b - t) - FRAME_H, 900); // height grew upward to satisfy the aspect
    }

    #[test]
    fn non_positive_aspect_leaves_rect_untouched() {
        for aspect in [0.0, -2.0] {
            // No aspect known / nonsense.
            let rect = constrain(
                (10, 20, 410, 320),
                ResizeEdge::Right,
                aspect,
                FRAME_W,
                FRAME_H,
            );
            assert_eq!(rect, (10, 20, 410, 320), "aspect {aspect}");
        }
    }

    #[test]
    fn degenerate_client_leaves_rect_untouched() {
        // Proposed box smaller than the frame insets → no valid client; return as-is rather
        // than invert.
        let proposed = (0, 0, FRAME_W - 2, FRAME_H - 2);
        let rect = constrain(proposed, ResizeEdge::Right, WIDE, FRAME_W, FRAME_H);
        assert_eq!(rect, proposed);
    }

    // --- Linux/Wayland client-size aspect lock (issue #331) --------------------

    const TALL: f64 = 9.0 / 16.0;
    const SQUARE: f64 = 1.0;
    /// A workarea large enough not to bind in the shape tests below.
    const OPEN: WindowSize = WindowSize {
        width: 100_000,
        height: 100_000,
    };
    const NO_MIN: WindowSize = WindowSize {
        width: 1,
        height: 1,
    };

    fn size(width: i32, height: i32) -> WindowSize {
        WindowSize { width, height }
    }

    fn pointer(x: f64, y: f64) -> PointerDelta {
        PointerDelta { x, y }
    }

    #[test]
    fn vertical_edges_lead_with_width() {
        // Dragging Left or Right expresses width intent; height follows the ratio
        // from a stale height, and the fixed edge is the compositor's concern.
        for edge in [ResizeEdge::Left, ResizeEdge::Right] {
            let locked =
                lock_client_size(size(1280, 720), size(1600, 137), edge, WIDE, NO_MIN, OPEN);
            assert_eq!(locked, size(1600, 900), "{edge:?}");
        }
    }

    #[test]
    fn horizontal_edges_lead_with_height() {
        // Dragging Top or Bottom expresses height intent; width follows.
        for edge in [ResizeEdge::Top, ResizeEdge::Bottom] {
            let locked =
                lock_client_size(size(1280, 720), size(137, 900), edge, WIDE, NO_MIN, OPEN);
            assert_eq!(locked, size(1600, 900), "{edge:?}");
        }
    }

    #[test]
    fn corner_follows_the_dominant_pointer_axis() {
        // Mostly-horizontal pointer motion at a corner → width leads.
        for edge in [
            ResizeEdge::TopLeft,
            ResizeEdge::TopRight,
            ResizeEdge::BottomLeft,
            ResizeEdge::BottomRight,
        ] {
            assert_eq!(
                lock_client_size(size(1280, 720), size(1600, 760), edge, WIDE, NO_MIN, OPEN,),
                size(1600, 900),
                "{edge:?} wide drag"
            );
            // Mostly-vertical pointer motion at a corner → height leads.
            assert_eq!(
                lock_client_size(size(1280, 720), size(1300, 900), edge, WIDE, NO_MIN, OPEN,),
                size(1600, 900),
                "{edge:?} tall drag"
            );
        }
    }

    #[test]
    fn portrait_and_square_media_lock_cleanly() {
        assert_eq!(
            lock_client_size(
                size(900, 1600),
                size(500, 1600),
                ResizeEdge::Bottom,
                TALL,
                NO_MIN,
                OPEN,
            ),
            size(900, 1600)
        );
        assert_eq!(
            lock_client_size(
                size(640, 640),
                size(640, 480),
                ResizeEdge::Right,
                SQUARE,
                NO_MIN,
                OPEN,
            ),
            size(640, 640)
        );
    }

    #[test]
    fn minimum_osc_size_is_respected_on_both_axes() {
        // A tiny drag cannot shrink below the OSC floor; the derived axis is
        // grown to keep the floor on the height axis too.
        let min = size(320, 180);
        let locked = lock_client_size(
            size(1280, 720),
            size(40, 30),
            ResizeEdge::BottomRight,
            WIDE,
            min,
            OPEN,
        );
        assert!(locked.width >= min.width, "width {} < 320", locked.width);
        assert!(
            locked.height >= min.height,
            "height {} < 180",
            locked.height
        );
        // Still on the aspect line within rounding tolerance.
        assert!((f64::from(locked.width) / f64::from(locked.height) - WIDE).abs() < 0.01);
    }

    #[test]
    fn workarea_clamp_keeps_both_axes_on_screen() {
        // A drag larger than the workarea is clamped so neither axis overflows,
        // and the aspect is preserved (height binds here: 800 * 16/9 > 1200).
        let max = size(1200, 800);
        let locked = lock_client_size(
            size(1280, 720),
            size(5000, 5000),
            ResizeEdge::BottomRight,
            WIDE,
            NO_MIN,
            max,
        );
        assert!(locked.width <= max.width, "width {}", locked.width);
        assert!(locked.height <= max.height, "height {}", locked.height);
        assert_eq!(locked, size(1200, 675));
    }

    #[test]
    fn workarea_wins_when_smaller_than_minimum() {
        // Degenerate: a workarea below the OSC floor must still fit on screen
        // rather than honor the minimum and push the window off the monitor.
        let min = size(1000, 1000);
        let max = size(400, 300);
        let locked = lock_client_size(
            size(1280, 720),
            size(4000, 4000),
            ResizeEdge::BottomRight,
            WIDE,
            min,
            max,
        );
        assert!(locked.width <= max.width && locked.height <= max.height);
    }

    #[test]
    fn lock_is_scale_invariant() {
        // Proportions, not native pixels: the same proposed *logical* size locks
        // to the same result no matter the fractional monitor scale, because the
        // shell negotiates compute-size in logical pixels. Both a 1.0 and a 1.5
        // scale reach compute-size with the same logical proposal.
        let a = lock_client_size(
            size(1280, 720),
            size(1000, 137),
            ResizeEdge::Right,
            WIDE,
            NO_MIN,
            OPEN,
        );
        let b = lock_client_size(
            size(1280, 720),
            size(1000, 137),
            ResizeEdge::Right,
            WIDE,
            NO_MIN,
            OPEN,
        );
        assert_eq!(a, b);
        // A fractional workarea floor still clamps predictably.
        let max = size(1503, 987);
        let locked = lock_client_size(
            size(1280, 720),
            size(9000, 9000),
            ResizeEdge::Right,
            WIDE,
            NO_MIN,
            max,
        );
        assert!(locked.width <= max.width && locked.height <= max.height);
    }

    #[test]
    fn non_positive_aspect_passes_the_proposal_through() {
        for aspect in [0.0, -1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(
                lock_client_size(
                    size(1280, 720),
                    size(800, 450),
                    ResizeEdge::Right,
                    aspect,
                    NO_MIN,
                    OPEN,
                ),
                size(800, 450),
                "aspect {aspect}"
            );
        }
    }

    #[test]
    fn lock_helper_preserves_inward_corner_direction() {
        assert_eq!(
            lock_client_size(
                size(1600, 900),
                size(1500, 900),
                ResizeEdge::BottomRight,
                WIDE,
                NO_MIN,
                OPEN,
            ),
            size(1500, 844)
        );
    }

    #[test]
    fn freeform_session_passes_sizes_through_untouched() {
        let mut session = AspectResize::begin(
            ResizeEdge::BottomRight,
            size(1280, 720),
            NO_MIN,
            OPEN,
            false,
        );
        assert!(!session.is_locked());
        assert_eq!(session.resolve(pointer(-640.0, 279.0)).size, size(640, 999));
        assert_eq!(session.edge(), ResizeEdge::BottomRight);
    }

    #[test]
    fn session_locked_from_start_holds_aspect() {
        let mut session =
            AspectResize::begin(ResizeEdge::Right, size(1280, 720), NO_MIN, OPEN, true);
        assert!(session.is_locked());
        assert_eq!(session.resolve(pointer(320.0, 0.0)).size, size(1600, 900));
    }

    #[test]
    fn shift_pressed_mid_drag_locks_the_current_proportions() {
        // Freeform to a fresh shape, then Shift engages: the lock must capture the
        // proportions on screen at that instant, not the drag's start shape.
        let mut session = AspectResize::begin(
            ResizeEdge::BottomRight,
            size(1280, 720),
            NO_MIN,
            OPEN,
            false,
        );
        let freeform = session.resolve(pointer(-280.0, -220.0)); // 2:1 now on screen
        assert_eq!(freeform.size, size(1000, 500));
        session.set_shift(true);
        assert!(session.is_locked());
        assert_eq!(
            session.resolve(pointer(120.0, -220.0)).size,
            size(1400, 700)
        );
    }

    #[test]
    fn shift_released_mid_drag_returns_to_freeform_and_keeps_size() {
        let mut session =
            AspectResize::begin(ResizeEdge::BottomRight, size(1280, 720), NO_MIN, OPEN, true);
        let locked = session.resolve(pointer(320.0, 0.0));
        assert_eq!(locked.size, size(1600, 900));
        session.set_shift(false);
        assert!(!session.is_locked());
        // Freeform resumes from the kept size; further motion is unconstrained.
        assert_eq!(session.resolve(pointer(321.0, 1.0)).size, size(1601, 901));
    }

    #[test]
    fn shift_toggled_twice_relocks_to_the_latest_shape() {
        let mut session =
            AspectResize::begin(ResizeEdge::BottomRight, size(1280, 720), NO_MIN, OPEN, true);
        session.set_shift(false);
        let reshaped = session.resolve(pointer(-480.0, 80.0)); // freeform to square
        assert_eq!(reshaped.size, size(800, 800));
        session.set_shift(true);
        // Re-locked to 1:1 now.
        assert_eq!(
            session.resolve(pointer(-780.0, -220.0)).size,
            size(500, 500)
        );
    }

    #[test]
    fn pointer_projection_covers_every_edge_and_corner() {
        let cases = [
            (ResizeEdge::Left, pointer(-320.0, 0.0), (-320, 0)),
            (ResizeEdge::Right, pointer(320.0, 0.0), (0, 0)),
            (ResizeEdge::Top, pointer(0.0, -180.0), (0, -180)),
            (ResizeEdge::Bottom, pointer(0.0, 180.0), (0, 0)),
            (ResizeEdge::TopLeft, pointer(-320.0, -180.0), (-320, -180)),
            (ResizeEdge::TopRight, pointer(320.0, -180.0), (0, -180)),
            (ResizeEdge::BottomLeft, pointer(-320.0, 180.0), (-320, 0)),
            (ResizeEdge::BottomRight, pointer(320.0, 180.0), (0, 0)),
        ];
        for (edge, delta, position) in cases {
            let mut session = AspectResize::begin(edge, size(1280, 720), NO_MIN, OPEN, true);
            let resolved = session.resolve(delta);
            assert_eq!(resolved.size, size(1600, 900), "{edge:?}");
            assert_eq!(
                resolved.position_delta,
                WindowPoint {
                    x: position.0,
                    y: position.1,
                },
                "{edge:?}"
            );
        }
    }

    #[test]
    fn inward_corner_motion_uses_signed_delta_without_reversing() {
        let mut session =
            AspectResize::begin(ResizeEdge::TopLeft, size(1600, 900), NO_MIN, OPEN, true);
        let resolved = session.resolve(pointer(400.0, 10.0));
        assert_eq!(resolved.size, size(1200, 675));
        assert_eq!(resolved.position_delta, WindowPoint { x: 400, y: 225 });
    }

    #[test]
    fn fractional_pointer_offsets_round_once_in_logical_pixels() {
        let mut session =
            AspectResize::begin(ResizeEdge::BottomRight, size(1280, 720), NO_MIN, OPEN, true);
        assert_eq!(session.resolve(pointer(319.5, 179.5)).size, size(1600, 900));
    }

    #[test]
    fn post_rounding_result_never_crosses_workarea_ceiling() {
        let max = size(1000, 562);
        let mut session =
            AspectResize::begin(ResizeEdge::BottomRight, size(800, 450), NO_MIN, max, true);
        let resolved = session.resolve(pointer(10_000.0, 10_000.0)).size;
        assert!(resolved.width <= max.width && resolved.height <= max.height);
    }

    #[test]
    fn x11_anchor_ceiling_respects_workarea_and_fixed_edges() {
        let work_area = WindowRect {
            x: 100,
            y: 50,
            width: 1600,
            height: 900,
        };
        let start = size(800, 450);
        let position = Some(WindowPoint { x: 500, y: 250 });
        assert_eq!(
            client_max_for_anchor(ResizeEdge::BottomRight, start, position, work_area),
            size(1200, 700)
        );
        assert_eq!(
            client_max_for_anchor(ResizeEdge::TopLeft, start, position, work_area),
            size(1200, 650)
        );
    }
}
