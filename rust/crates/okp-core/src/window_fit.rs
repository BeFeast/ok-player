//! Pure geometry for sizing a player window to loaded video. The video dimensions
//! come from mpv in physical pixels, while GTK monitor geometry and window sizes are
//! expressed in logical application pixels. Keeping that conversion and the complete
//! fit policy here prevents platform shells from growing their own geometry rules.
//!
//! [`fill_client`] remains the port of `src/OkPlayer.Core/WindowFit.cs`; the C# suite
//! in `tests/OkPlayer.Tests/WindowFitTests.cs` is its executable compatibility spec.

/// Keep every fitted edge visibly inside the usable workarea.
pub const WORK_AREA_EDGE_MARGIN: i32 = 24;

/// Additional vertical budget for the captionless player's 42 logical-pixel titlebar.
/// The titlebar overlays the video, but reserving its height keeps the complete player
/// comfortably inside short workareas instead of fitting right up to the panel edge.
pub const PLAYER_CHROME_RESERVE: i32 = 42;

/// A logical size requested from the platform windowing API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

/// A rectangle in the desktop's logical coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The complete one-shot fit result. `video` is the video's natural logical size;
/// `window` is the aspect-fitted, centered/clamped logical toplevel rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowFit {
    pub video: WindowSize,
    pub window: WindowRect,
}

/// Convert mpv's physical display dimensions into GTK logical application pixels.
/// Flooring fractional results guarantees that requesting the returned logical size
/// never asks the compositor to upscale beyond the video's physical dimensions.
pub fn physical_to_logical(
    physical_width: i32,
    physical_height: i32,
    scale_factor: f64,
) -> Option<WindowSize> {
    if physical_width <= 0
        || physical_height <= 0
        || !scale_factor.is_finite()
        || scale_factor <= 0.0
    {
        return None;
    }

    Some(WindowSize {
        width: (f64::from(physical_width) / scale_factor)
            .floor()
            .clamp(1.0, f64::from(i32::MAX)) as i32,
        height: (f64::from(physical_height) / scale_factor)
            .floor()
            .clamp(1.0, f64::from(i32::MAX)) as i32,
    })
}

/// The maximum logical player size after the fixed edge and chrome reservations.
pub fn work_area_budget(work_area: WindowRect) -> Option<WindowSize> {
    if work_area.width <= 0 || work_area.height <= 0 {
        return None;
    }

    let width = work_area
        .width
        .checked_sub(WORK_AREA_EDGE_MARGIN.checked_mul(2)?)?;
    let height = work_area
        .height
        .checked_sub(WORK_AREA_EDGE_MARGIN.checked_mul(2)?)?
        .checked_sub(PLAYER_CHROME_RESERVE)?;
    (width > 0 && height > 0).then_some(WindowSize { width, height })
}

/// Convert the physical video size to logical coordinates, aspect-fit it inside the
/// current logical workarea, and center the result. The scale is capped at one so a
/// small video keeps its natural physical size on every monitor scale factor.
pub fn fit_window_to_work_area(
    video_width: i32,
    video_height: i32,
    work_area: WindowRect,
    scale_factor: f64,
) -> Option<WindowFit> {
    let video = physical_to_logical(video_width, video_height, scale_factor)?;
    let budget = work_area_budget(work_area)?;
    let scale = 1.0_f64.min(
        (f64::from(budget.width) / f64::from(video.width))
            .min(f64::from(budget.height) / f64::from(video.height)),
    );
    let width = ((f64::from(video.width) * scale).round_ties_even() as i32)
        .clamp(1, budget.width)
        .min(video.width);
    let height = ((f64::from(video.height) * scale).round_ties_even() as i32)
        .clamp(1, budget.height)
        .min(video.height);
    let centered = WindowRect {
        x: centered_axis(work_area.x, work_area.width, width)?,
        y: centered_axis(work_area.y, work_area.height, height)?,
        width,
        height,
    };

    Some(WindowFit {
        video,
        window: clamp_window_to_work_area(centered, work_area)?,
    })
}

/// Clamp an already-sized window so all four edges stay inside the reserved workarea.
/// This is also useful after a compositor adjusts a requested size or placement.
pub fn clamp_window_to_work_area(window: WindowRect, work_area: WindowRect) -> Option<WindowRect> {
    let budget = work_area_budget(work_area)?;
    if window.width <= 0
        || window.height <= 0
        || window.width > budget.width
        || window.height > budget.height
    {
        return None;
    }

    let min_x = work_area.x.checked_add(WORK_AREA_EDGE_MARGIN)?;
    let min_y = work_area.y.checked_add(WORK_AREA_EDGE_MARGIN)?;
    let max_x = work_area
        .x
        .checked_add(work_area.width)?
        .checked_sub(WORK_AREA_EDGE_MARGIN)?
        .checked_sub(window.width)?;
    let max_y = work_area
        .y
        .checked_add(work_area.height)?
        .checked_sub(WORK_AREA_EDGE_MARGIN)?
        .checked_sub(window.height)?;

    Some(WindowRect {
        x: window.x.clamp(min_x, max_x),
        y: window.y.clamp(min_y, max_y),
        ..window
    })
}

fn centered_axis(origin: i32, extent: i32, size: i32) -> Option<i32> {
    let offset = extent.checked_sub(size)?.checked_div(2)?;
    origin.checked_add(offset)
}

/// Apply the compatibility correction using the same logical workarea budget as the
/// initial native-size request.
pub fn fill_client_to_work_area(
    video_width: i32,
    video_height: i32,
    client_width: i32,
    client_height: i32,
    work_area: WindowRect,
) -> Option<WindowSize> {
    let budget = work_area_budget(work_area)?;
    fill_client(
        video_width,
        video_height,
        client_width,
        client_height,
        budget.width,
        budget.height,
    )
}

/// Correct a client size that the window manager clamped above the requested size.
/// The clamped axis is preserved and the other axis grows to remove black bars, capped
/// to the workarea budget. Returns `None` when the current client already matches
/// within approximately one pixel.
pub fn fill_client(
    video_width: i32,
    video_height: i32,
    client_width: i32,
    client_height: i32,
    max_client_width: i32,
    max_client_height: i32,
) -> Option<WindowSize> {
    if video_width <= 0 || video_height <= 0 || client_width <= 0 || client_height <= 0 {
        return None;
    }

    let video_aspect = f64::from(video_width) / f64::from(video_height);
    let side_bars = f64::from(client_width) - f64::from(client_height) * video_aspect;
    let vertical_bars = f64::from(client_height) - f64::from(client_width) / video_aspect;

    if side_bars >= 1.0 {
        let mut target_height = (f64::from(client_width) / video_aspect).round_ties_even() as i32;
        if max_client_height > 0 {
            target_height = target_height.min(max_client_height);
        }
        return (target_height > 0 && target_height != client_height).then_some(WindowSize {
            width: client_width,
            height: target_height,
        });
    }

    if vertical_bars >= 1.0 {
        let mut target_width = (f64::from(client_height) * video_aspect).round_ties_even() as i32;
        if max_client_width > 0 {
            target_width = target_width.min(max_client_width);
        }
        return (target_width > 0 && target_width != client_width).then_some(WindowSize {
            width: target_width,
            height: client_height,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_CAP_WIDTH: i32 = 4096;
    const NO_CAP_HEIGHT: i32 = 4096;
    const WORK_AREA: WindowRect = WindowRect {
        x: 0,
        y: 0,
        width: 1280,
        height: 900,
    };

    #[test]
    fn physical_video_pixels_convert_to_logical_without_upscaling() {
        assert_eq!(
            physical_to_logical(3840, 2160, 2.0),
            Some(WindowSize {
                width: 1920,
                height: 1080
            })
        );
        assert_eq!(
            physical_to_logical(1921, 1081, 2.0),
            Some(WindowSize {
                width: 960,
                height: 540
            })
        );
        assert_eq!(
            physical_to_logical(3840, 2160, 1.25),
            Some(WindowSize {
                width: 3072,
                height: 1728
            })
        );
    }

    #[test]
    fn small_video_keeps_native_logical_size_and_is_centered() {
        assert_eq!(
            fit_window_to_work_area(320, 180, WORK_AREA, 1.0),
            Some(WindowFit {
                video: WindowSize {
                    width: 320,
                    height: 180
                },
                window: WindowRect {
                    x: 480,
                    y: 360,
                    width: 320,
                    height: 180
                }
            })
        );
    }

    #[test]
    fn workarea_reserves_edges_and_player_chrome_before_aspect_fit() {
        assert_eq!(
            work_area_budget(WORK_AREA),
            Some(WindowSize {
                width: 1232,
                height: 810
            })
        );
        assert_eq!(
            fit_window_to_work_area(3840, 2160, WORK_AREA, 1.0),
            Some(WindowFit {
                video: WindowSize {
                    width: 3840,
                    height: 2160
                },
                window: WindowRect {
                    x: 24,
                    y: 103,
                    width: 1232,
                    height: 693
                }
            })
        );
    }

    #[test]
    fn scaled_four_k_video_fits_the_logical_four_k_workarea() {
        let fit = fit_window_to_work_area(
            3840,
            2160,
            WindowRect {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            2.0,
        )
        .expect("scaled workarea should fit");
        assert_eq!(fit.video.width, 1920);
        assert_eq!(fit.video.height, 1080);
        assert_eq!(
            fit.window,
            WindowRect {
                x: 2000,
                y: 45,
                width: 1760,
                height: 990
            }
        );
    }

    #[test]
    fn fitted_window_never_exceeds_the_video_natural_size() {
        let fit = fit_window_to_work_area(640, 360, WORK_AREA, 2.0).expect("fit should exist");
        assert_eq!(fit.video.width, 320);
        assert_eq!(fit.video.height, 180);
        assert_eq!(fit.window.width, 320);
        assert_eq!(fit.window.height, 180);
    }

    #[test]
    fn portrait_video_is_limited_by_reserved_workarea_height() {
        assert_eq!(
            fit_window_to_work_area(1080, 1920, WORK_AREA, 1.0)
                .expect("portrait fit should exist")
                .window,
            WindowRect {
                x: 412,
                y: 45,
                width: 456,
                height: 810
            }
        );
    }

    #[test]
    fn offscreen_position_is_clamped_inside_every_reserved_edge() {
        assert_eq!(
            clamp_window_to_work_area(
                WindowRect {
                    x: 5000,
                    y: -200,
                    width: 900,
                    height: 600,
                },
                WindowRect {
                    x: 1920,
                    y: 40,
                    width: 1280,
                    height: 900,
                },
            ),
            Some(WindowRect {
                x: 2276,
                y: 64,
                width: 900,
                height: 600
            })
        );
    }

    #[test]
    fn exact_aspect_needs_no_correction() {
        assert_eq!(
            fill_client(640, 480, 640, 480, NO_CAP_WIDTH, NO_CAP_HEIGHT),
            None
        );
    }

    #[test]
    fn sub_pixel_mismatch_needs_no_correction() {
        assert_eq!(
            fill_client(3840, 1606, 1805, 755, NO_CAP_WIDTH, NO_CAP_HEIGHT),
            None
        );
    }

    #[test]
    fn width_clamped_up_grows_height_to_fill() {
        assert_eq!(
            fill_client(640, 480, 704, 480, NO_CAP_WIDTH, NO_CAP_HEIGHT),
            Some(WindowSize {
                width: 704,
                height: 528
            })
        );
    }

    #[test]
    fn height_clamped_up_grows_width_to_fill() {
        assert_eq!(
            fill_client(480, 640, 480, 704, NO_CAP_WIDTH, NO_CAP_HEIGHT),
            Some(WindowSize {
                width: 528,
                height: 704
            })
        );
    }

    #[test]
    fn growth_is_capped_to_work_area() {
        assert_eq!(fill_client(720, 1280, 704, 647, 1203, 647), None);
        assert_eq!(
            fill_client(720, 1280, 704, 647, 1203, 900),
            Some(WindowSize {
                width: 704,
                height: 900
            })
        );
    }

    #[test]
    fn work_area_correction_uses_the_same_reservation_budget() {
        assert_eq!(
            fill_client_to_work_area(640, 480, 704, 480, WORK_AREA),
            Some(WindowSize {
                width: 704,
                height: 528
            })
        );
        assert_eq!(
            fill_client_to_work_area(720, 1280, 704, 810, WORK_AREA),
            None
        );
    }

    #[test]
    fn filled_result_is_stable() {
        let filled = fill_client(640, 480, 704, 480, NO_CAP_WIDTH, NO_CAP_HEIGHT)
            .expect("minimum-width clamp should need correction");
        assert_eq!(
            fill_client(
                640,
                480,
                filled.width,
                filled.height,
                NO_CAP_WIDTH,
                NO_CAP_HEIGHT
            ),
            None
        );
    }

    #[test]
    fn invalid_or_impossibly_small_inputs_are_rejected() {
        for input in [
            (0, 480, 704, 480),
            (640, 0, 704, 480),
            (640, 480, 0, 480),
            (640, 480, 704, 0),
        ] {
            assert_eq!(
                fill_client(
                    input.0,
                    input.1,
                    input.2,
                    input.3,
                    NO_CAP_WIDTH,
                    NO_CAP_HEIGHT
                ),
                None
            );
        }
        assert_eq!(physical_to_logical(0, 180, 1.0), None);
        assert_eq!(physical_to_logical(320, 180, 0.0), None);
        assert_eq!(physical_to_logical(320, 180, f64::NAN), None);
        assert_eq!(
            work_area_budget(WindowRect {
                x: 0,
                y: 0,
                width: 40,
                height: 80
            }),
            None
        );
    }
}
