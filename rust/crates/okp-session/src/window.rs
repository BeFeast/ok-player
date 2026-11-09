//! Window behaviour a shell asks the session about: where a window fitted to a
//! video goes, which fit an opened source is owed, and what a click on the video
//! means.
//!
//! The answers come from okp-core (`window_fit`, `video_click`), so every shell
//! sizes its window and reads clicks the same way. The shell passes what only it
//! knows (the screen, its own controls, the click count); the session answers
//! with core's decision. The one piece of session state is the initial fit each
//! opened source is owed ([`SourceFit`]), kept with core's `InitialFitState`.

use okp_core::{video_click, window_fit};
use serde::{Deserialize, Serialize};

use crate::protocol::VideoGeometry;

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
/// window. `chrome` is the room the window's own controls take outside the video
/// area (a bar below it, a panel beside it): the video is fitted to the screen
/// less that room, and the frame grows by it, so the video area itself has the
/// video's aspect. A fit below `min_size` (the whole window's minimum) grows the
/// clamped axis to the minimum and the other one with the video's aspect (core
/// `fill_client`), within the same budget. `None` when there is nothing to fit
/// (no video size, no usable area).
#[must_use]
pub fn fit_window(
    video: WindowSize,
    backing_scale: f64,
    visible_frame: WindowRect,
    min_size: Option<WindowSize>,
    chrome: Option<WindowSize>,
) -> Option<WindowRect> {
    let chrome = chrome
        .filter(|chrome| chrome.width >= 0 && chrome.height >= 0)
        .unwrap_or(WindowSize {
            width: 0,
            height: 0,
        });
    let work_area = window_fit::WindowRect {
        x: visible_frame.x,
        y: visible_frame.y,
        width: visible_frame.width,
        height: visible_frame.height,
    };
    let video_area = window_fit::WindowRect {
        width: work_area.width.saturating_sub(chrome.width),
        height: work_area.height.saturating_sub(chrome.height),
        ..work_area
    };
    let placement = window_fit::fit_physical_video_to_work_area(
        video.width,
        video.height,
        backing_scale,
        video_area,
    )?;
    let mut size = placement.size;
    if let Some(min) = min_size.filter(|min| min.width > 0 && min.height > 0) {
        // What the window's minimum leaves for the video once the controls
        // have their room.
        let min = window_fit::WindowSize {
            width: min.width.saturating_sub(chrome.width).max(1),
            height: min.height.saturating_sub(chrome.height).max(1),
        };
        let clamped = window_fit::WindowSize {
            width: size.width.max(min.width),
            height: size.height.max(min.height),
        };
        if clamped != size {
            let budget = window_fit::work_area_budget(video_area.width, video_area.height)?;
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
    let size = window_fit::WindowSize {
        width: size.width.saturating_add(chrome.width),
        height: size.height.saturating_add(chrome.height),
    };
    let position = window_fit::centered_position(size, work_area);
    Some(WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

/// The initial fit each opened source is owed, kept with core's
/// `InitialFitState`: every open (and every close) starts a new source
/// generation, the first video size reported while that source is loaded is
/// recorded for it, and the fit is taken once. Sizes reported for an earlier
/// source, and size changes after the first, never become a fit.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SourceFit {
    generation: u64,
    state: window_fit::InitialFitState,
}

impl SourceFit {
    /// A source was opened, or the current one closed.
    pub(crate) fn begin_source(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.state.begin_source(self.generation);
    }

    /// The current source's video size, as the snapshot reports it. The
    /// snapshot carries one only while the current source is loaded.
    pub(crate) fn observe(&mut self, video: Option<&VideoGeometry>) {
        if let Some(video) = video {
            self.state
                .observe_dimensions(self.generation, video.width, video.height);
        }
    }

    /// Answer the owed fit with `fit` of its video size. The fit is spent only
    /// when `fit` answers, so a screen that cannot take a window yet leaves it
    /// owed.
    pub(crate) fn take_with<T>(&mut self, fit: impl FnOnce(WindowSize) -> Option<T>) -> Option<T> {
        let mut next = self.state;
        let request = next.take(self.generation)?;
        let answer = fit(WindowSize {
            width: request.video.width,
            height: request.video.height,
        })?;
        self.state = next;
        Some(answer)
    }
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
        let frame = fit_window(size(3840, 2160), 2.0, LAPTOP, Some(MIN), None).expect("a fit");
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
            fit_window(size(1280, 720), 2.0, LAPTOP, Some(MIN), None),
            Some(WindowRect {
                x: 436,
                y: 292,
                width: 640,
                height: 360,
            })
        );
        assert_eq!(
            fit_window(size(1280, 720), 1.0, LAPTOP, Some(MIN), None),
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
            fit_window(size(640, 360), 2.0, LAPTOP, Some(MIN), None),
            Some(WindowRect {
                x: 507,
                y: 332,
                width: 498,
                height: 280,
            })
        );
        // Portrait: the width is raised and the height follows.
        let portrait = fit_window(size(360, 640), 2.0, LAPTOP, Some(MIN), None).expect("a fit");
        assert_eq!((portrait.width, portrait.height), (420, 747));
        // Without a minimum the core fit stands as it is.
        assert_eq!(
            fit_window(size(640, 360), 2.0, LAPTOP, None, None)
                .map(|frame| (frame.width, frame.height)),
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
        let frame = fit_window(size(1920, 1080), 1.0, screen, Some(MIN), None).expect("a fit");
        assert!(frame.x >= screen.x && frame.x + frame.width <= screen.x + screen.width);
        assert!(frame.y >= screen.y && frame.y + frame.height <= screen.y + screen.height);
    }

    #[test]
    fn nothing_is_fitted_without_a_video_size_or_a_usable_area() {
        assert_eq!(
            fit_window(size(0, 1080), 2.0, LAPTOP, Some(MIN), None),
            None
        );
        assert_eq!(
            fit_window(size(1920, -1), 2.0, LAPTOP, Some(MIN), None),
            None
        );
        assert_eq!(
            fit_window(size(1920, 1080), 0.0, LAPTOP, Some(MIN), None),
            None
        );
        assert_eq!(
            fit_window(
                size(1920, 1080),
                2.0,
                WindowRect { width: 0, ..LAPTOP },
                Some(MIN),
                None
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

    /// The macOS preview's control bar under the video: 30 points of controls,
    /// a 10 point inset and an 8 point gap.
    const BAR: WindowSize = WindowSize {
        width: 0,
        height: 48,
    };

    #[test]
    fn the_controls_room_is_added_so_the_video_area_has_the_videos_aspect() {
        let frame = fit_window(size(640, 360), 1.0, LAPTOP, Some(MIN), Some(BAR)).expect("a fit");
        assert_eq!(
            frame,
            WindowRect {
                x: 436,
                y: 268,
                width: 640,
                height: 408,
            },
            "the video keeps its 640x360 and the bar gets its 48 points"
        );
        // A 4:3 video is limited by the screen's height: it is fitted to the
        // screen less the bar, so the whole window keeps core's margins.
        let large = fit_window(size(2880, 2160), 2.0, LAPTOP, Some(MIN), Some(BAR)).expect("a fit");
        let area = (large.width, large.height - BAR.height);
        assert!(
            (f64::from(area.0) / f64::from(area.1) - 4.0 / 3.0).abs() < 0.01,
            "{area:?}"
        );
        let without = fit_window(size(2880, 2160), 2.0, LAPTOP, Some(MIN), None).expect("a fit");
        assert!(
            area.1 < without.height,
            "the bar's room comes out of the video's budget: {large:?} vs {without:?}"
        );
        let budget = window_fit::work_area_budget(LAPTOP.width, LAPTOP.height).unwrap();
        assert!(
            large.height <= budget.height + window_fit::PLAYER_CHROME_RESERVE,
            "the window keeps core's desktop margin: {large:?}"
        );
        assert!(large.y >= LAPTOP.y && large.y + large.height <= LAPTOP.y + LAPTOP.height);
    }

    #[test]
    fn the_minimum_is_the_whole_windows_and_the_video_area_keeps_its_aspect_above_it() {
        // 320x180 points would leave the window below 420x280 with the bar.
        let frame = fit_window(size(640, 360), 2.0, LAPTOP, Some(MIN), Some(BAR)).expect("a fit");
        assert!(
            frame.width >= MIN.width && frame.height >= MIN.height,
            "{frame:?}"
        );
        let area = (frame.width, frame.height - BAR.height);
        assert!(
            (f64::from(area.0) - f64::from(area.1) * 16.0 / 9.0).abs() <= 1.0,
            "the video area {area:?} is not 16:9"
        );
        // A negative room is no room.
        assert_eq!(
            fit_window(
                size(1280, 720),
                2.0,
                LAPTOP,
                Some(MIN),
                Some(size(-10, -48))
            ),
            fit_window(size(1280, 720), 2.0, LAPTOP, Some(MIN), None)
        );
    }

    fn geometry(width: i32, height: i32) -> VideoGeometry {
        VideoGeometry {
            width,
            height,
            display_aspect: f64::from(width) / f64::from(height),
        }
    }

    fn take(fit: &mut SourceFit) -> Option<WindowSize> {
        fit.take_with(Some)
    }

    #[test]
    fn each_opened_source_is_owed_one_fit_to_its_first_video_size() {
        let mut fit = SourceFit::default();
        fit.observe(Some(&geometry(1920, 800)));
        assert_eq!(take(&mut fit), None, "nothing was opened");

        fit.begin_source();
        assert_eq!(take(&mut fit), None, "the size is not known yet");
        fit.observe(None);
        fit.observe(Some(&geometry(1920, 800)));
        fit.observe(Some(&geometry(1280, 720)));
        assert_eq!(take(&mut fit), Some(size(1920, 800)), "the first size wins");
        assert_eq!(take(&mut fit), None, "a fit is taken once");
        fit.observe(Some(&geometry(1280, 720)));
        assert_eq!(take(&mut fit), None, "a later size change never refits");

        fit.begin_source();
        fit.observe(Some(&geometry(1080, 1920)));
        assert_eq!(
            take(&mut fit),
            Some(size(1080, 1920)),
            "the next open is fitted"
        );
    }

    #[test]
    fn an_untaken_fit_dies_with_its_source_and_survives_a_screen_that_cannot_answer() {
        let mut fit = SourceFit::default();
        fit.begin_source();
        fit.observe(Some(&geometry(1920, 800)));
        fit.begin_source();
        assert_eq!(
            take(&mut fit),
            None,
            "the film's size is not the next source's fit"
        );

        fit.observe(Some(&geometry(1080, 1920)));
        assert_eq!(fit.take_with(|_| None::<WindowRect>), None);
        assert_eq!(
            take(&mut fit),
            Some(size(1080, 1920)),
            "a fit nothing answered is still owed"
        );
    }
}
