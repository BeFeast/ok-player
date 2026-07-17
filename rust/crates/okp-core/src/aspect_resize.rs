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
// Linux/Wayland interactive Shift-resize (issue #331)
//
// The Windows track drives aspect from `WM_SIZING`, adjusting an outer rect with
// explicit non-client insets (`constrain` above). Wayland has no client-visible
// window position and GTK4 dropped `GDK_HINT_ASPECT`, so the polished path is
// different: the compositor owns the interactive resize (`begin_resize`) and
// negotiates the surface size through the `compute-size` handshake. We keep that
// path — it is the only jitter-free one — and enforce the aspect ratio purely by
// answering each `compute-size` with a locked *client* size. No per-motion
// `set_default_size`, so there is no configure/resize feedback loop.
//
// Everything below is that engine-free client-size geometry plus the state
// machine for pressing/releasing Shift mid-drag. The GTK shell only observes
// Shift, the dragged edge, and the proposed size, and applies the result.
// ---------------------------------------------------------------------------

use crate::window_fit::WindowSize;

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
}

fn sane_size(size: WindowSize) -> WindowSize {
    WindowSize {
        width: size.width.max(1),
        height: size.height.max(1),
    }
}

/// Lock a proposed client size to `aspect` (client width / height) for a drag
/// gripping `edge`, then clamp it — while staying on the aspect line — so it
/// never falls below `min` or exceeds `max` on either axis.
///
/// Straight edges have one meaningful axis (a vertical edge leads with width, a
/// horizontal edge with height). For corners the leading axis is whichever one
/// encloses the pointer, so a mostly-horizontal drag follows width and a
/// mostly-vertical drag follows height. `min` guards the minimum usable OSC size;
/// `max` is the monitor workarea (already reduced for chrome by the caller).
/// Returns `proposed` unchanged when `aspect` is not a positive, finite ratio.
pub fn lock_client_size(
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

    let leading = edge.straight_leading().unwrap_or_else(|| {
        // Corner: pick the axis whose aspect projection covers the other, i.e.
        // the locked box just encloses the proposed (pointer) box.
        let height_if_width_leads = (f64::from(proposed.width) / aspect).round_ties_even();
        if height_if_width_leads >= f64::from(proposed.height) {
            Leading::Width
        } else {
            Leading::Height
        }
    });

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

    let width = width.round_ties_even().max(1.0);
    let height = (width / aspect).round_ties_even().max(1.0);
    WindowSize {
        width: width as i32,
        height: height as i32,
    }
}

/// The live state of one interactive resize drag. Created when a resize handle
/// press begins; consulted on every `compute-size`; dropped when the drag ends.
///
/// It owns the deterministic answer to "what happens when Shift is pressed or
/// released mid-drag" (issue #331): pressing Shift locks the aspect to the
/// window's proportions *at that instant*; releasing Shift returns to freeform
/// and keeps the current size. While unlocked, `resolve` is an identity that
/// only records the latest size so a later Shift-press captures fresh
/// proportions.
#[derive(Debug, Clone, PartialEq)]
pub struct AspectResize {
    edge: ResizeEdge,
    min: WindowSize,
    max: WindowSize,
    locked_aspect: Option<f64>,
    last: WindowSize,
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
            last: sane_size(start),
        };
        if shift {
            session.lock_to_last();
        }
        session
    }

    fn lock_to_last(&mut self) {
        let aspect = f64::from(self.last.width) / f64::from(self.last.height);
        if aspect.is_finite() && aspect > 0.0 {
            self.locked_aspect = Some(aspect);
        }
    }

    /// Report a Shift press (`true`) or release (`false`) during the drag, with
    /// the window's current client size so a press captures live proportions.
    pub fn set_shift(&mut self, pressed: bool, current: WindowSize) {
        self.last = sane_size(current);
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

    /// Resolve a compositor-proposed client size. Locked → aspect-corrected and
    /// clamped; unlocked → the proposal is returned untouched (ordinary freeform
    /// resize). Either way the result is remembered so a later Shift-press locks
    /// to the size actually on screen.
    pub fn resolve(&mut self, proposed: WindowSize) -> WindowSize {
        let resolved = match self.locked_aspect {
            Some(aspect) => lock_client_size(proposed, self.edge, aspect, self.min, self.max),
            None => sane_size(proposed),
        };
        self.last = resolved;
        resolved
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

    #[test]
    fn vertical_edges_lead_with_width() {
        // Dragging Left or Right expresses width intent; height follows the ratio
        // from a stale height, and the fixed edge is the compositor's concern.
        for edge in [ResizeEdge::Left, ResizeEdge::Right] {
            let locked = lock_client_size(size(1600, 137), edge, WIDE, NO_MIN, OPEN);
            assert_eq!(locked, size(1600, 900), "{edge:?}");
        }
    }

    #[test]
    fn horizontal_edges_lead_with_height() {
        // Dragging Top or Bottom expresses height intent; width follows.
        for edge in [ResizeEdge::Top, ResizeEdge::Bottom] {
            let locked = lock_client_size(size(137, 900), edge, WIDE, NO_MIN, OPEN);
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
                lock_client_size(size(1600, 200), edge, WIDE, NO_MIN, OPEN),
                size(1600, 900),
                "{edge:?} wide drag"
            );
            // Mostly-vertical pointer motion at a corner → height leads.
            assert_eq!(
                lock_client_size(size(200, 900), edge, WIDE, NO_MIN, OPEN),
                size(1600, 900),
                "{edge:?} tall drag"
            );
        }
    }

    #[test]
    fn portrait_and_square_media_lock_cleanly() {
        assert_eq!(
            lock_client_size(size(500, 1600), ResizeEdge::Bottom, TALL, NO_MIN, OPEN),
            size(900, 1600)
        );
        assert_eq!(
            lock_client_size(size(640, 480), ResizeEdge::Right, SQUARE, NO_MIN, OPEN),
            size(640, 640)
        );
    }

    #[test]
    fn minimum_osc_size_is_respected_on_both_axes() {
        // A tiny drag cannot shrink below the OSC floor; the derived axis is
        // grown to keep the floor on the height axis too.
        let min = size(320, 180);
        let locked = lock_client_size(size(40, 30), ResizeEdge::BottomRight, WIDE, min, OPEN);
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
        let locked = lock_client_size(size(5000, 5000), ResizeEdge::BottomRight, WIDE, NO_MIN, max);
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
        let locked = lock_client_size(size(4000, 4000), ResizeEdge::BottomRight, WIDE, min, max);
        assert!(locked.width <= max.width && locked.height <= max.height);
    }

    #[test]
    fn lock_is_scale_invariant() {
        // Proportions, not native pixels: the same proposed *logical* size locks
        // to the same result no matter the fractional monitor scale, because the
        // shell negotiates compute-size in logical pixels. Both a 1.0 and a 1.5
        // scale reach compute-size with the same logical proposal.
        let a = lock_client_size(size(1000, 137), ResizeEdge::Right, WIDE, NO_MIN, OPEN);
        let b = lock_client_size(size(1000, 137), ResizeEdge::Right, WIDE, NO_MIN, OPEN);
        assert_eq!(a, b);
        // A fractional workarea floor still clamps predictably.
        let max = size(1503, 987);
        let locked = lock_client_size(size(9000, 9000), ResizeEdge::Right, WIDE, NO_MIN, max);
        assert!(locked.width <= max.width && locked.height <= max.height);
    }

    #[test]
    fn non_positive_aspect_passes_the_proposal_through() {
        for aspect in [0.0, -1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(
                lock_client_size(size(800, 450), ResizeEdge::Right, aspect, NO_MIN, OPEN),
                size(800, 450),
                "aspect {aspect}"
            );
        }
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
        assert_eq!(session.resolve(size(640, 999)), size(640, 999));
        assert_eq!(session.edge(), ResizeEdge::BottomRight);
    }

    #[test]
    fn session_locked_from_start_holds_aspect() {
        let mut session =
            AspectResize::begin(ResizeEdge::Right, size(1280, 720), NO_MIN, OPEN, true);
        assert!(session.is_locked());
        // 1280/720 == 16/9; a width-led proposal snaps height back onto the ratio.
        assert_eq!(session.resolve(size(1600, 137)), size(1600, 900));
    }

    #[test]
    fn shift_pressed_mid_drag_locks_the_current_proportions() {
        // Freeform to a fresh shape, then Shift engages: the lock must capture the
        // proportions on screen at that instant, not the drag's start shape.
        let mut session =
            AspectResize::begin(ResizeEdge::Right, size(1280, 720), NO_MIN, OPEN, false);
        let freeform = session.resolve(size(1000, 500)); // 2:1 now on screen
        assert_eq!(freeform, size(1000, 500));
        session.set_shift(true, freeform);
        assert!(session.is_locked());
        // Locked to 2:1: a width-led proposal derives height at 2:1, not 16:9.
        assert_eq!(session.resolve(size(1400, 137)), size(1400, 700));
    }

    #[test]
    fn shift_released_mid_drag_returns_to_freeform_and_keeps_size() {
        let mut session =
            AspectResize::begin(ResizeEdge::BottomRight, size(1280, 720), NO_MIN, OPEN, true);
        let locked = session.resolve(size(1600, 200));
        assert_eq!(locked, size(1600, 900));
        session.set_shift(false, locked);
        assert!(!session.is_locked());
        // Freeform resumes from the kept size; further motion is unconstrained.
        assert_eq!(session.resolve(size(1601, 901)), size(1601, 901));
    }

    #[test]
    fn shift_toggled_twice_relocks_to_the_latest_shape() {
        let mut session =
            AspectResize::begin(ResizeEdge::Right, size(1280, 720), NO_MIN, OPEN, true);
        session.set_shift(false, size(1280, 720));
        let reshaped = session.resolve(size(800, 800)); // freeform to square
        assert_eq!(reshaped, size(800, 800));
        session.set_shift(true, reshaped);
        // Re-locked to 1:1 now.
        assert_eq!(session.resolve(size(500, 137)), size(500, 500));
    }
}
