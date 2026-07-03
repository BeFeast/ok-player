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
}
