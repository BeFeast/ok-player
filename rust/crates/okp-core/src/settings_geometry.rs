//! Pure geometry for the content-sized Settings window.
//!
//! The Settings shell is a fixed-width, captionless window: a titlebar strip over a
//! `[nav rail | content]` row. Its height should follow the active page's natural
//! content height, bounded by the current monitor workarea so the caption controls
//! and the bottom of the content never land off-screen (#283). This module holds
//! only the arithmetic — the shell feeds in measured natural heights plus the
//! monitor workarea and applies the result; no widget logic lives here.

/// Total vertical margin kept between the window and the monitor workarea edges so
/// the window never presses against panels or docks (the "sane margin" of #283).
pub const WORKAREA_MARGIN: i32 = 64;

/// Fail-safe floor for the body region when monitor information is degenerate
/// (a zero or negative workarea reported during backend races). Keeps the shell
/// usable rather than collapsing to a sliver.
pub const MIN_BODY_HEIGHT: i32 = 240;

/// Height available to the body row (rail + content) on a monitor whose workarea
/// is `workarea_height` tall, under a `titlebar_height` chrome strip that must
/// always stay visible.
pub fn body_height_cap(workarea_height: i32, titlebar_height: i32) -> i32 {
    if workarea_height <= 0 {
        MIN_BODY_HEIGHT
    } else {
        (workarea_height - WORKAREA_MARGIN - titlebar_height).max(0)
    }
}

/// The resolved Settings window height for one page under one workarea cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsWindowHeight {
    /// Outer window height: titlebar + body.
    pub window: i32,
    /// Height granted to the body row (rail + content viewport).
    pub body: i32,
    /// True when the active page's content exceeds `body` and must scroll while
    /// the shell (titlebar + rail) stays fully visible.
    pub content_scrolls: bool,
    /// True when even the navigation rail exceeds `body` (tiny workareas) and
    /// must scroll.
    pub rail_scrolls: bool,
}

/// Bound the window height to the active page: natural content height when it
/// fits, the workarea cap when it does not. The rail's natural height is the
/// floor for short pages so the shell never collapses below its navigation.
pub fn bounded_window_height(
    content_natural: i32,
    rail_natural: i32,
    body_cap: i32,
    titlebar_height: i32,
) -> SettingsWindowHeight {
    let cap = body_cap.max(0);
    let body = content_natural.max(rail_natural).max(0).min(cap);
    SettingsWindowHeight {
        window: titlebar_height + body,
        body,
        content_scrolls: content_natural > body,
        rail_scrolls: rail_natural > body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITLEBAR: i32 = 42;

    #[test]
    fn short_content_uses_the_rail_floor_not_the_legacy_fixed_height() {
        // A five-row page is shorter than the rail; the rail's natural height is
        // the floor and the window is content-sized, well under the legacy 560.
        let cap = body_height_cap(900, TITLEBAR); // 794
        let bounds = bounded_window_height(300, 413, cap, TITLEBAR);
        assert_eq!(bounds.body, 413);
        assert_eq!(bounds.window, TITLEBAR + 413);
        assert!(!bounds.content_scrolls);
        assert!(!bounds.rail_scrolls);
    }

    #[test]
    fn tall_content_that_fits_gets_its_natural_height() {
        // A tall page still fits this workarea cap: no scroll, window exactly
        // titlebar + content.
        let cap = body_height_cap(900, TITLEBAR);
        assert_eq!(cap, 794);
        let bounds = bounded_window_height(752, 413, cap, TITLEBAR);
        assert_eq!(bounds.body, 752);
        assert_eq!(bounds.window, 794);
        assert!(!bounds.content_scrolls);
    }

    #[test]
    fn content_over_the_cap_scrolls_and_the_window_stops_at_the_workarea() {
        let cap = body_height_cap(900, TITLEBAR);
        let bounds = bounded_window_height(1400, 413, cap, TITLEBAR);
        assert_eq!(bounds.body, cap);
        assert_eq!(bounds.window, 900 - WORKAREA_MARGIN);
        assert!(bounds.content_scrolls);
        assert!(!bounds.rail_scrolls);
    }

    #[test]
    fn small_workarea_caps_everything_and_scrolls_even_the_rail() {
        // A 480px workarea leaves less body than the rail needs: both regions
        // scroll, and the window still fits inside the workarea minus margin.
        let cap = body_height_cap(480, TITLEBAR); // 374
        let bounds = bounded_window_height(752, 413, cap, TITLEBAR);
        assert_eq!(bounds.body, 374);
        assert_eq!(bounds.window, 480 - WORKAREA_MARGIN);
        assert!(bounds.content_scrolls);
        assert!(bounds.rail_scrolls);
    }

    #[test]
    fn very_small_valid_workarea_still_keeps_the_window_inside_the_margin() {
        let cap = body_height_cap(300, TITLEBAR); // 194
        let bounds = bounded_window_height(752, 413, cap, TITLEBAR);
        assert_eq!(bounds.body, 194);
        assert_eq!(bounds.window, 300 - WORKAREA_MARGIN);
        assert!(bounds.content_scrolls);
        assert!(bounds.rail_scrolls);
    }

    #[test]
    fn degenerate_workarea_keeps_the_fail_safe_floor() {
        assert_eq!(body_height_cap(0, TITLEBAR), MIN_BODY_HEIGHT);
        assert_eq!(body_height_cap(-1080, TITLEBAR), MIN_BODY_HEIGHT);
        let bounds = bounded_window_height(752, 413, MIN_BODY_HEIGHT, TITLEBAR);
        assert_eq!(bounds.body, MIN_BODY_HEIGHT);
        assert!(bounds.content_scrolls);
        assert!(bounds.rail_scrolls);
    }

    #[test]
    fn negative_measurements_clamp_to_an_empty_body() {
        let bounds = bounded_window_height(-10, -20, 500, TITLEBAR);
        assert_eq!(bounds.body, 0);
        assert_eq!(bounds.window, TITLEBAR);
        assert!(!bounds.content_scrolls);
        assert!(!bounds.rail_scrolls);
    }
}
