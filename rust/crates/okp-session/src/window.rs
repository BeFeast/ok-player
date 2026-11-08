//! Window behaviour a shell asks the session about: where a window fitted to the
//! loaded video goes, and what a click on the video means.
//!
//! Both answers come from okp-core (`window_fit`, `video_click`), so every shell
//! sizes its window and reads clicks the same way. Neither reads session state:
//! the shell passes what only it knows (the screen, the click count), and the
//! session answers with core's decision.

use okp_core::{video_click, window_fit};
use serde::{Deserialize, Serialize};

/// A size in whole logical units (points on macOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

/// A rectangle in a screen's logical coordinates. `x`, `y` is its corner with
/// the smallest coordinates: the top left in a y-down desktop, the bottom left on
/// macOS. Fitting and centering read the same in either orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// What a primary-button release on the video means (core
/// `video_click::Intent`). The shell owns the timer: a single click waits for
/// the double-click interval, so a double-click toggles fullscreen without
/// pausing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoClickIntent {
    Ignore,
    /// Toggle pause once the double-click interval has passed without a second
    /// click.
    SchedulePlayPause,
    /// Drop the pending pause toggle and toggle fullscreen.
    CancelPlayPauseAndToggleFullscreen,
}

/// The frame of a window fitted to `video` (its display size in pixels) on a
/// screen with `backing_scale` device pixels per logical unit, whose usable area
/// is `visible_frame`.
///
/// Core's fit shows the video at no more than one video pixel per device pixel,
/// keeps the desktop margin and the title strip's reserve, and centers the
/// window. A fit below `min_size` grows the clamped axis to the minimum and the
/// other one with the video's aspect (core `fill_client`), within the same
/// budget. `None` when there is nothing to fit (no video size, no usable area).
#[must_use]
pub fn fit_window(
    video: WindowSize,
    backing_scale: f64,
    visible_frame: WindowRect,
    min_size: Option<WindowSize>,
) -> Option<WindowRect> {
    let work_area = window_fit::WindowRect {
        x: visible_frame.x,
        y: visible_frame.y,
        width: visible_frame.width,
        height: visible_frame.height,
    };
    let placement = window_fit::fit_physical_video_to_work_area(
        video.width,
        video.height,
        backing_scale,
        work_area,
    )?;
    let mut size = placement.size;
    if let Some(min) = min_size.filter(|min| min.width > 0 && min.height > 0) {
        let clamped = window_fit::WindowSize {
            width: size.width.max(min.width),
            height: size.height.max(min.height),
        };
        if clamped != size {
            let budget = window_fit::work_area_budget(work_area.width, work_area.height)?;
            size = window_fit::fill_client(
                video.width,
                video.height,
                clamped.width,
                clamped.height,
                budget.width.max(min.width),
                budget.height.max(min.height),
            )
            .unwrap_or(clamped);
        }
    }
    let position = window_fit::centered_position(size, work_area);
    Some(WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

/// What a primary-button release with `press_count` clicks on the video means.
#[must_use]
pub fn video_click_intent(press_count: i32) -> VideoClickIntent {
    match video_click::release_intent(press_count) {
        video_click::Intent::Ignore => VideoClickIntent::Ignore,
        video_click::Intent::SchedulePlayPause => VideoClickIntent::SchedulePlayPause,
        video_click::Intent::CancelPlayPauseAndToggleFullscreen => {
            VideoClickIntent::CancelPlayPauseAndToggleFullscreen
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 14-inch MacBook Pro's usable area: 1512 x 982 points less the menu bar.
    const LAPTOP: WindowRect = WindowRect {
        x: 0,
        y: 0,
        width: 1512,
        height: 945,
    };
    const MIN: WindowSize = WindowSize {
        width: 420,
        height: 280,
    };

    fn size(width: i32, height: i32) -> WindowSize {
        WindowSize { width, height }
    }

    #[test]
    fn a_video_larger_than_the_screen_is_scaled_into_the_budget_and_centered() {
        let frame = fit_window(size(3840, 2160), 2.0, LAPTOP, Some(MIN)).expect("a fit");
        let budget = window_fit::work_area_budget(LAPTOP.width, LAPTOP.height).unwrap();
        assert!(frame.width <= budget.width && frame.height <= budget.height);
        assert!(
            frame.width == budget.width || frame.height == budget.height,
            "one axis fills the budget: {frame:?}"
        );
        assert!((f64::from(frame.width) / f64::from(frame.height) - 16.0 / 9.0).abs() < 0.01);
        assert_eq!(frame.x, (LAPTOP.width - frame.width) / 2);
        assert_eq!(frame.y, (LAPTOP.height - frame.height) / 2);
    }

    #[test]
    fn a_retina_screen_shows_one_video_pixel_per_device_pixel() {
        assert_eq!(
            fit_window(size(1280, 720), 2.0, LAPTOP, Some(MIN)),
            Some(WindowRect {
                x: 436,
                y: 292,
                width: 640,
                height: 360,
            })
        );
        assert_eq!(
            fit_window(size(1280, 720), 1.0, LAPTOP, Some(MIN)),
            Some(WindowRect {
                x: 116,
                y: 112,
                width: 1280,
                height: 720,
            })
        );
    }

    #[test]
    fn a_fit_below_the_minimum_size_grows_with_the_video_aspect() {
        // 640x360 at 2x is 320x180 points: the height is raised to the minimum
        // and the width follows the 16:9 aspect instead of stopping at 420.
        assert_eq!(
            fit_window(size(640, 360), 2.0, LAPTOP, Some(MIN)),
            Some(WindowRect {
                x: 507,
                y: 332,
                width: 498,
                height: 280,
            })
        );
        // Portrait: the width is raised and the height follows.
        let portrait = fit_window(size(360, 640), 2.0, LAPTOP, Some(MIN)).expect("a fit");
        assert_eq!((portrait.width, portrait.height), (420, 747));
        // Without a minimum the core fit stands as it is.
        assert_eq!(
            fit_window(size(640, 360), 2.0, LAPTOP, None).map(|frame| (frame.width, frame.height)),
            Some((320, 180))
        );
    }

    #[test]
    fn an_offset_screen_keeps_the_window_on_that_screen() {
        // A second display left of the main one, with the Dock along its bottom.
        let screen = WindowRect {
            x: -1920,
            y: 70,
            width: 1920,
            height: 1010,
        };
        let frame = fit_window(size(1920, 1080), 1.0, screen, Some(MIN)).expect("a fit");
        assert!(frame.x >= screen.x && frame.x + frame.width <= screen.x + screen.width);
        assert!(frame.y >= screen.y && frame.y + frame.height <= screen.y + screen.height);
    }

    #[test]
    fn nothing_is_fitted_without_a_video_size_or_a_usable_area() {
        assert_eq!(fit_window(size(0, 1080), 2.0, LAPTOP, Some(MIN)), None);
        assert_eq!(fit_window(size(1920, -1), 2.0, LAPTOP, Some(MIN)), None);
        assert_eq!(fit_window(size(1920, 1080), 0.0, LAPTOP, Some(MIN)), None);
        assert_eq!(
            fit_window(
                size(1920, 1080),
                2.0,
                WindowRect { width: 0, ..LAPTOP },
                Some(MIN)
            ),
            None
        );
    }

    #[test]
    fn clicks_follow_the_core_release_policy() {
        assert_eq!(video_click_intent(1), VideoClickIntent::SchedulePlayPause);
        assert_eq!(
            video_click_intent(2),
            VideoClickIntent::CancelPlayPauseAndToggleFullscreen
        );
        for count in [0, 3, 4, -1] {
            assert_eq!(
                video_click_intent(count),
                VideoClickIntent::Ignore,
                "{count}"
            );
        }
    }
}
