//! Pure geometry for sizing a player window to loaded video. This ports
//! `src/OkPlayer.Core/WindowFit.cs`; the C# suite in
//! `tests/OkPlayer.Tests/WindowFitTests.cs` is the executable compatibility spec.

/// Keep a small desktop margin around videos that need to be scaled down.
/// This matches the Windows player's existing fit-to-video behavior.
pub const WORK_AREA_FILL: f64 = 0.94;

/// A logical client size requested from the platform windowing API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

/// Return the video's native logical size, scaled down only when it would exceed
/// the current monitor's work-area budget. One scale is used for both axes so
/// the display aspect is preserved.
pub fn fit_to_work_area(
    video_width: i32,
    video_height: i32,
    work_width: i32,
    work_height: i32,
) -> Option<WindowSize> {
    if video_width <= 0 || video_height <= 0 || work_width <= 0 || work_height <= 0 {
        return None;
    }

    let scale = 1.0_f64.min(
        (f64::from(work_width) * WORK_AREA_FILL / f64::from(video_width))
            .min(f64::from(work_height) * WORK_AREA_FILL / f64::from(video_height)),
    );

    Some(WindowSize {
        width: (f64::from(video_width) * scale).round_ties_even().max(1.0) as i32,
        height: (f64::from(video_height) * scale).round_ties_even().max(1.0) as i32,
    })
}

/// Apply the compatibility correction using the same work-area budget as the
/// initial native-size request.
pub fn fill_client_to_work_area(
    video_width: i32,
    video_height: i32,
    client_width: i32,
    client_height: i32,
    work_width: i32,
    work_height: i32,
) -> Option<WindowSize> {
    if work_width <= 0 || work_height <= 0 {
        return None;
    }
    let max_client_width = (f64::from(work_width) * WORK_AREA_FILL).floor() as i32;
    let max_client_height = (f64::from(work_height) * WORK_AREA_FILL).floor() as i32;
    fill_client(
        video_width,
        video_height,
        client_width,
        client_height,
        max_client_width,
        max_client_height,
    )
}

/// Correct a client size that the window manager clamped above the requested
/// size. The clamped axis is preserved and the other axis grows to remove black
/// bars, capped to the work area. Returns `None` when the current client already
/// matches within approximately one pixel.
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

    #[test]
    fn small_video_keeps_native_size() {
        assert_eq!(
            fit_to_work_area(320, 180, 1280, 900),
            Some(WindowSize {
                width: 320,
                height: 180
            })
        );
    }

    #[test]
    fn four_k_video_fits_work_area_with_aspect_preserved() {
        assert_eq!(
            fit_to_work_area(3840, 2160, 1280, 900),
            Some(WindowSize {
                width: 1203,
                height: 677
            })
        );
    }

    #[test]
    fn tall_video_is_limited_by_work_area_height() {
        assert_eq!(
            fit_to_work_area(1080, 1920, 1280, 900),
            Some(WindowSize {
                width: 476,
                height: 846
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
    fn work_area_correction_uses_the_same_margin_budget() {
        assert_eq!(
            fill_client_to_work_area(640, 480, 704, 480, 1280, 900),
            Some(WindowSize {
                width: 704,
                height: 528
            })
        );
        assert_eq!(
            fill_client_to_work_area(720, 1280, 704, 846, 1280, 900),
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
    fn non_positive_inputs_are_rejected() {
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
        assert_eq!(fit_to_work_area(0, 180, 1280, 900), None);
        assert_eq!(fit_to_work_area(320, 180, 0, 900), None);
        assert_eq!(fill_client_to_work_area(640, 480, 704, 480, 0, 900), None);
    }
}
