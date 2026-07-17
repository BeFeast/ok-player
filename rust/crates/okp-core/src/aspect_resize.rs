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
// Linux (GTK4/Wayland) client-size path — issue #331.
//
// The Windows consumer works in outer `(left, top, right, bottom)` rects with
// non-client insets. GTK4/Wayland has no `WM_SIZING`, no separate frame insets
// (the OSC/titlebar overlays the video, so client == content), and no aspect
// geometry hints (GTK4 removed them). The shell instead drives the
// compositor-native interactive resize and, while Shift locks the ratio,
// re-requests an aspect-consistent CLIENT size on each observed size change.
// The pure geometry and the mid-drag Shift state machine live here so the shell
// stays a thin renderer.
// ---------------------------------------------------------------------------

/// Minimum logical client width that keeps the standard-player OSC transport
/// usable during an aspect-locked resize.
pub const MIN_LOCKED_CLIENT_WIDTH: i32 = 320;

/// Minimum logical client height that keeps the OSC and caption band usable
/// during an aspect-locked resize.
pub const MIN_LOCKED_CLIENT_HEIGHT: i32 = 180;

/// A logical client size — the content area GTK sizes via `set_default_size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientSize {
    pub width: i32,
    pub height: i32,
}

/// Logical-pixel bounds for a locked resize: the smallest usable client (so the
/// OSC stays operable) and the largest the monitor workarea allows. Both axes
/// are clamped independently; [`lock_client_size`] keeps the aspect while
/// staying inside the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeBounds {
    pub min_width: i32,
    pub min_height: i32,
    pub max_width: i32,
    pub max_height: i32,
}

impl ResizeBounds {
    /// Build bounds from a monitor work-area budget (already reduced by the
    /// desktop edge and player-chrome reserve, i.e. the accepted fit-to-video
    /// ceiling) and the minimum usable OSC size. The minimum is capped to the
    /// budget so a tiny workarea can never invert the range.
    pub fn from_work_area_budget(budget: crate::window_fit::WindowSize) -> Self {
        let max_width = budget.width.max(1);
        let max_height = budget.height.max(1);
        Self {
            min_width: MIN_LOCKED_CLIENT_WIDTH.min(max_width),
            min_height: MIN_LOCKED_CLIENT_HEIGHT.min(max_height),
            max_width,
            max_height,
        }
    }
}

/// Whether `edge` puts the user's intent on the width axis (the height is then
/// derived). Vertical edges and corners lead with width; horizontal edges lead
/// with height. Corners match the Windows [`constrain`] rule (width leads
/// height) so both platforms behave identically.
fn width_leads(edge: ResizeEdge) -> bool {
    !matches!(edge, ResizeEdge::Top | ResizeEdge::Bottom)
}

/// Lock a `proposed` logical client size (observed mid-drag) to `aspect`
/// (width/height), with `edge` deciding which axis the user is driving, then
/// clamp the result into `bounds` while preserving the aspect exactly.
///
/// Returns `None` when `aspect` is not a positive finite number or when
/// `bounds` cannot hold the aspect at all (a degenerate workarea); the shell
/// then leaves the compositor's freeform size in place.
pub fn lock_client_size(
    proposed: ClientSize,
    edge: ResizeEdge,
    aspect: f64,
    bounds: ResizeBounds,
) -> Option<ClientSize> {
    if !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    let ResizeBounds {
        min_width,
        min_height,
        max_width,
        max_height,
    } = bounds;
    if min_width <= 0 || min_height <= 0 || max_width < min_width || max_height < min_height {
        return None;
    }

    // The set of aspect-correct widths whose derived height also fits the box:
    //   width ∈ [max(min_w, ⌈min_h·a⌉), min(max_w, ⌊max_h·a⌋)]
    // Clamping the leading dimension into this window keeps BOTH axes in range
    // while the aspect holds — this is the workarea/minimum clamp.
    let lo = min_width.max((f64::from(min_height) * aspect).ceil() as i32);
    let hi = max_width.min((f64::from(max_height) * aspect).floor() as i32);
    if lo > hi {
        return None;
    }

    let target_width = if width_leads(edge) {
        proposed.width
    } else {
        (f64::from(proposed.height) * aspect).round_ties_even() as i32
    };
    let width = target_width.clamp(lo, hi);
    let height = (f64::from(width) / aspect).round_ties_even() as i32;
    Some(ClientSize { width, height })
}

/// Whether `target` differs from `current` enough to justify re-requesting a
/// size during a live resize. `scale` is the logical→physical factor (e.g. 1.5
/// for 150% fractional scaling); a difference smaller than one physical pixel on
/// both axes is treated as settled so the shell does not fight the compositor
/// and trigger a configure/resize feedback loop.
pub fn exceeds_resize_tolerance(current: ClientSize, target: ClientSize, scale: f64) -> bool {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let dw = f64::from((target.width - current.width).abs()) * scale;
    let dh = f64::from((target.height - current.height).abs()) * scale;
    dw >= 1.0 || dh >= 1.0
}

/// Tracks a live interactive resize so Shift can engage or release the aspect
/// lock deterministically mid-drag.
///
/// Deterministic transitions (issue #331 "behavior if Shift is pressed or
/// released during an active resize"):
/// - Shift held at press → the lock is engaged for the whole drag.
/// - Shift pressed mid-drag → the lock engages from the current size onward.
/// - Shift released mid-drag → freeform resumes and the size the drag has
///   already reached is kept (no snap-back).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectResizeSession {
    edge: ResizeEdge,
    aspect: f64,
    locking: bool,
}

impl AspectResizeSession {
    /// Begin a resize on `edge`, holding `aspect` (client width/height, usually
    /// the video's display aspect) while locked. `shift_held` seeds the lock
    /// from the modifier state at press time.
    pub fn begin(edge: ResizeEdge, aspect: f64, shift_held: bool) -> Self {
        Self {
            edge,
            aspect,
            locking: shift_held,
        }
    }

    /// The edge the drag grips.
    pub fn edge(&self) -> ResizeEdge {
        self.edge
    }

    /// Whether the aspect lock is currently engaged.
    pub fn is_locking(&self) -> bool {
        self.locking
    }

    /// Update the Shift state mid-drag. Returns the new locking state.
    pub fn set_shift(&mut self, held: bool) -> bool {
        self.locking = held;
        self.locking
    }

    /// Resolve the aspect-locked client size for a `proposed` size seen during
    /// the drag, or `None` when the lock is disengaged or the geometry is not
    /// resolvable (the shell then leaves the freeform size in place).
    pub fn resolve(&self, proposed: ClientSize, bounds: ResizeBounds) -> Option<ClientSize> {
        if !self.locking {
            return None;
        }
        lock_client_size(proposed, self.edge, self.aspect, bounds)
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

    // --- Linux client-size path (issue #331) -------------------------------

    const PORTRAIT: f64 = 9.0 / 16.0;
    const SQUARE: f64 = 1.0;

    /// Bounds wide enough that the aspect clamp, not the box, decides the size.
    fn open_bounds() -> ResizeBounds {
        ResizeBounds {
            min_width: 1,
            min_height: 1,
            max_width: 100_000,
            max_height: 100_000,
        }
    }

    fn size(width: i32, height: i32) -> ClientSize {
        ClientSize { width, height }
    }

    #[test]
    fn vertical_edges_and_corners_lead_with_width() {
        for edge in [
            ResizeEdge::Left,
            ResizeEdge::Right,
            ResizeEdge::TopLeft,
            ResizeEdge::TopRight,
            ResizeEdge::BottomLeft,
            ResizeEdge::BottomRight,
        ] {
            // Width 1600 leads; a stale 500 height snaps to 900 for 16:9.
            let locked = lock_client_size(size(1600, 500), edge, WIDE, open_bounds()).unwrap();
            assert_eq!(locked, size(1600, 900), "edge {edge:?}");
        }
    }

    #[test]
    fn horizontal_edges_lead_with_height() {
        for edge in [ResizeEdge::Top, ResizeEdge::Bottom] {
            // Height 900 leads; a stale 500 width snaps to 1600 for 16:9.
            let locked = lock_client_size(size(500, 900), edge, WIDE, open_bounds()).unwrap();
            assert_eq!(locked, size(1600, 900), "edge {edge:?}");
        }
    }

    #[test]
    fn portrait_and_square_media_lock_from_width() {
        assert_eq!(
            lock_client_size(size(540, 200), ResizeEdge::Right, PORTRAIT, open_bounds()),
            Some(size(540, 960)),
        );
        assert_eq!(
            lock_client_size(size(480, 100), ResizeEdge::Right, SQUARE, open_bounds()),
            Some(size(480, 480)),
        );
    }

    #[test]
    fn workarea_ceiling_clamps_while_preserving_aspect() {
        // Height ceiling forces a width smaller than the drag asked for.
        let bounds = ResizeBounds {
            min_width: 320,
            min_height: 180,
            max_width: 4000,
            max_height: 900,
        };
        let locked = lock_client_size(size(3000, 900), ResizeEdge::Right, WIDE, bounds).unwrap();
        assert_eq!(locked, size(1600, 900));
        assert!(locked.width <= bounds.max_width && locked.height <= bounds.max_height);
    }

    #[test]
    fn width_ceiling_clamps_while_preserving_aspect() {
        let bounds = ResizeBounds {
            min_width: 320,
            min_height: 180,
            max_width: 1280,
            max_height: 2000,
        };
        let locked = lock_client_size(size(1920, 400), ResizeEdge::Right, WIDE, bounds).unwrap();
        assert_eq!(locked, size(1280, 720));
    }

    #[test]
    fn minimum_osc_size_clamps_small_drags() {
        let bounds = ResizeBounds::from_work_area_budget(crate::window_fit::WindowSize {
            width: 3000,
            height: 2000,
        });
        // Dragging tiny still yields at least the minimum usable OSC client,
        // and the aspect holds.
        let locked = lock_client_size(size(50, 20), ResizeEdge::BottomRight, WIDE, bounds).unwrap();
        assert!(locked.width >= MIN_LOCKED_CLIENT_WIDTH);
        assert!(locked.height >= MIN_LOCKED_CLIENT_HEIGHT);
        assert_eq!(
            locked.height,
            (f64::from(locked.width) / WIDE).round() as i32
        );
    }

    #[test]
    fn degenerate_bounds_that_cannot_hold_aspect_return_none() {
        // A very wide minimum against a very short maximum height cannot hold
        // 16:9 — the shell keeps the freeform size instead.
        let bounds = ResizeBounds {
            min_width: 1900,
            min_height: 180,
            max_width: 2000,
            max_height: 200,
        };
        assert_eq!(
            lock_client_size(size(1950, 190), ResizeEdge::Right, WIDE, bounds),
            None,
        );
    }

    #[test]
    fn non_positive_or_nan_aspect_returns_none() {
        for aspect in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                lock_client_size(size(800, 450), ResizeEdge::Right, aspect, open_bounds()),
                None,
                "aspect {aspect}",
            );
        }
    }

    #[test]
    fn fractional_scale_tolerance_ignores_sub_physical_pixel_drift() {
        // At 150% scale a 1-logical-pixel difference is 1.5 physical pixels →
        // worth re-requesting; identical sizes never re-request.
        assert!(exceeds_resize_tolerance(
            size(800, 450),
            size(801, 450),
            1.5
        ));
        assert!(!exceeds_resize_tolerance(
            size(800, 450),
            size(800, 450),
            1.5
        ));
        // At 50% downscale a 1-logical-pixel difference is only 0.5 physical
        // pixels → below tolerance, so no feedback-loop re-request.
        assert!(!exceeds_resize_tolerance(
            size(800, 450),
            size(801, 450),
            0.5
        ));
        assert!(exceeds_resize_tolerance(
            size(800, 450),
            size(802, 450),
            0.5
        ));
        // Nonsense scale falls back to one logical pixel.
        assert!(exceeds_resize_tolerance(
            size(800, 450),
            size(801, 450),
            0.0
        ));
    }

    #[test]
    fn lock_output_is_idempotent_so_corrections_converge() {
        // Re-locking an already-locked size yields the same size (no drift),
        // which is what stops the compositor configure/resize loop.
        let once =
            lock_client_size(size(1600, 500), ResizeEdge::Right, WIDE, open_bounds()).unwrap();
        let twice = lock_client_size(once, ResizeEdge::Right, WIDE, open_bounds()).unwrap();
        assert_eq!(once, twice);
        assert!(!exceeds_resize_tolerance(once, twice, 1.0));
    }

    #[test]
    fn session_seeds_lock_from_shift_at_press() {
        let locked = AspectResizeSession::begin(ResizeEdge::Right, WIDE, true);
        assert!(locked.is_locking());
        assert_eq!(locked.edge(), ResizeEdge::Right);
        assert_eq!(
            locked.resolve(size(1600, 500), open_bounds()),
            Some(size(1600, 900)),
        );

        let freeform = AspectResizeSession::begin(ResizeEdge::Right, WIDE, false);
        assert!(!freeform.is_locking());
        assert_eq!(freeform.resolve(size(1600, 500), open_bounds()), None);
    }

    #[test]
    fn session_shift_pressed_then_released_mid_drag_is_deterministic() {
        let mut session = AspectResizeSession::begin(ResizeEdge::Bottom, WIDE, false);
        // Freeform while Shift is up.
        assert_eq!(session.resolve(size(500, 900), open_bounds()), None);
        // Shift pressed → lock engages from the current size (height leads here).
        assert!(session.set_shift(true));
        assert_eq!(
            session.resolve(size(500, 900), open_bounds()),
            Some(size(1600, 900)),
        );
        // Shift released → freeform resumes and the reached size is kept.
        assert!(!session.set_shift(false));
        assert_eq!(session.resolve(size(1600, 900), open_bounds()), None);
    }

    #[test]
    fn bounds_from_budget_caps_minimum_to_a_tiny_workarea() {
        let bounds = ResizeBounds::from_work_area_budget(crate::window_fit::WindowSize {
            width: 200,
            height: 120,
        });
        assert_eq!(bounds.max_width, 200);
        assert_eq!(bounds.max_height, 120);
        assert!(bounds.min_width <= bounds.max_width);
        assert!(bounds.min_height <= bounds.max_height);
    }
}
