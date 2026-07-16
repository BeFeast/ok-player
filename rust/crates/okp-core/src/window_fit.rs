//! Pure geometry for sizing a player window to loaded video. This ports
//! `src/OkPlayer.Core/WindowFit.cs`; the C# suite in
//! `tests/OkPlayer.Tests/WindowFitTests.cs` is the executable compatibility spec.

/// Keep a small desktop margin around videos that need to be scaled down.
/// This matches the Windows player's existing fit-to-video behavior.
pub const WORK_AREA_FILL: f64 = 0.94;

/// Reserve the canonical custom titlebar band in addition to the desktop edge
/// margin. The titlebar overlays the video, but keeping its full logical height
/// inside the fit budget prevents the caption controls from settling against or
/// beyond a monitor edge.
pub const PLAYER_CHROME_RESERVE: i32 = 42;

/// A logical client size requested from the platform windowing API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

/// A logical point in the desktop coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPoint {
    pub x: i32,
    pub y: i32,
}

/// A monitor work area in logical desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The one-time logical size and preferred desktop position for a loaded video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    pub size: WindowSize,
    pub position: WindowPoint,
}

/// One source generation waiting for its one-time initial window fit.
///
/// Shells feed dimensions from event payloads, then consume the request for one
/// bounded initial-fit transaction. Keeping the one-shot state here prevents
/// duplicate reconfigure events from turning window sizing into a shell-owned
/// state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialFitRequest {
    pub source_generation: u64,
    pub video: WindowSize,
}

/// Portable lifecycle for the one-time fit attached to each media generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InitialFitState {
    source_generation: Option<u64>,
    video: Option<WindowSize>,
    consumed: bool,
}

impl InitialFitState {
    /// Start waiting for dimensions from a newly loaded source.
    pub fn begin_source(&mut self, source_generation: u64) {
        self.source_generation = Some(source_generation);
        self.video = None;
        self.consumed = false;
    }

    /// Record the first valid dimension payload for the current source.
    pub fn observe_dimensions(
        &mut self,
        source_generation: u64,
        video_width: i32,
        video_height: i32,
    ) -> bool {
        if self.source_generation != Some(source_generation)
            || self.consumed
            || self.video.is_some()
            || video_width <= 0
            || video_height <= 0
        {
            return false;
        }
        self.video = Some(WindowSize {
            width: video_width,
            height: video_height,
        });
        true
    }

    /// Whether a valid fit is ready without consuming it.
    pub fn is_ready(&self, source_generation: u64) -> bool {
        self.source_generation == Some(source_generation) && self.video.is_some() && !self.consumed
    }

    /// Consume the fit once for the current source generation.
    pub fn take(&mut self, source_generation: u64) -> Option<InitialFitRequest> {
        if !self.is_ready(source_generation) {
            return None;
        }
        self.consumed = true;
        Some(InitialFitRequest {
            source_generation,
            video: self.video?,
        })
    }
}

/// Fit physical video pixels into a logical monitor work area.
///
/// `monitor_scale` maps logical/application pixels to physical device pixels.
/// The result never exceeds the video's natural physical size, uses one scale
/// for both axes, reserves the Windows-compatible 6% edge budget plus the
/// canonical player chrome band, and is centered inside the work area.
pub fn fit_physical_video_to_work_area(
    video_width: i32,
    video_height: i32,
    monitor_scale: f64,
    work_area: WindowRect,
) -> Option<WindowPlacement> {
    if video_width <= 0
        || video_height <= 0
        || !monitor_scale.is_finite()
        || monitor_scale <= 0.0
        || work_area.width <= 0
        || work_area.height <= 0
    {
        return None;
    }

    let budget = work_area_budget(work_area.width, work_area.height)?;
    let scale = (1.0 / monitor_scale)
        .min(f64::from(budget.width) / f64::from(video_width))
        .min(f64::from(budget.height) / f64::from(video_height));
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }

    let size = WindowSize {
        // Floor rather than round: a half logical pixel becomes a real device
        // pixel at HiDPI scale, so rounding up can cross either the work area
        // or the video's natural physical size.
        width: (f64::from(video_width) * scale).floor().max(1.0) as i32,
        height: (f64::from(video_height) * scale).floor().max(1.0) as i32,
    };
    Some(WindowPlacement {
        size,
        position: centered_position(size, work_area),
    })
}

/// Refit only when the compositor-selected work area requires a smaller
/// client than the one already requested.
///
/// Initial placement is compositor-owned on Wayland. A compositor may move a
/// newly resized toplevel to another monitor; in that case retaining a target
/// calculated from the previous monitor can crop the window. This correction
/// is deliberately monotonic: it may shrink once to remain visible, but never
/// grows and therefore cannot fight a user resize or bounce between monitors.
pub fn smaller_fit_for_work_area(
    video_width: i32,
    video_height: i32,
    monitor_scale: f64,
    work_area: WindowRect,
    requested: WindowPlacement,
) -> Option<WindowPlacement> {
    let current =
        fit_physical_video_to_work_area(video_width, video_height, monitor_scale, work_area)?;
    (current.size.width < requested.size.width || current.size.height < requested.size.height)
        .then_some(current)
}

/// The largest whole-window size available after the standard edge and chrome
/// reservations have been applied.
pub fn work_area_budget(work_width: i32, work_height: i32) -> Option<WindowSize> {
    if work_width <= 0 || work_height <= 0 {
        return None;
    }
    let width = (f64::from(work_width) * WORK_AREA_FILL).floor() as i32;
    let height = (f64::from(work_height) * WORK_AREA_FILL).floor() as i32 - PLAYER_CHROME_RESERVE;
    (width > 0 && height > 0).then_some(WindowSize { width, height })
}

/// Center a window in the work area, then clamp the result for defensive use
/// with off-origin monitors and compositor-adjusted sizes.
pub fn centered_position(size: WindowSize, work_area: WindowRect) -> WindowPoint {
    let x =
        i64::from(work_area.x) + (i64::from(work_area.width) - i64::from(size.width)).max(0) / 2;
    let y =
        i64::from(work_area.y) + (i64::from(work_area.height) - i64::from(size.height)).max(0) / 2;
    clamp_position(
        WindowPoint {
            x: x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        },
        size,
        work_area,
    )
}

/// Clamp a requested logical position so all window edges remain within the
/// work area. If the compositor made the window larger than the work area,
/// anchor it to that work area's origin instead of producing an inverted range.
pub fn clamp_position(
    requested: WindowPoint,
    size: WindowSize,
    work_area: WindowRect,
) -> WindowPoint {
    let max_x =
        i64::from(work_area.x) + (i64::from(work_area.width) - i64::from(size.width)).max(0);
    let max_y =
        i64::from(work_area.y) + (i64::from(work_area.height) - i64::from(size.height)).max(0);
    WindowPoint {
        x: i64::from(requested.x)
            .clamp(i64::from(work_area.x), max_x)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y: i64::from(requested.y)
            .clamp(i64::from(work_area.y), max_y)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    }
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
    fn physical_video_is_converted_to_logical_size_without_upscale() {
        assert_eq!(
            fit_physical_video_to_work_area(
                320,
                180,
                2.0,
                WindowRect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 900,
                }
            ),
            Some(WindowPlacement {
                size: WindowSize {
                    width: 160,
                    height: 90,
                },
                position: WindowPoint { x: 560, y: 405 },
            })
        );
    }

    #[test]
    fn initial_fit_state_consumes_each_source_once() {
        let mut state = InitialFitState::default();
        state.begin_source(7);
        assert!(!state.observe_dimensions(6, 3840, 2160));
        assert!(!state.observe_dimensions(7, 0, 2160));
        assert!(state.observe_dimensions(7, 3840, 2160));
        assert!(!state.observe_dimensions(7, 1920, 1080));
        assert_eq!(
            state.take(7),
            Some(InitialFitRequest {
                source_generation: 7,
                video: WindowSize {
                    width: 3840,
                    height: 2160,
                },
            })
        );
        assert_eq!(state.take(7), None);

        state.begin_source(8);
        assert!(state.observe_dimensions(8, 1920, 1080));
        assert_eq!(state.take(7), None);
        assert_eq!(state.take(8).map(|request| request.video.width), Some(1920));
    }

    #[test]
    fn fractional_monitor_scale_keeps_the_physical_natural_size_ceiling() {
        let placement = fit_physical_video_to_work_area(
            300,
            150,
            1.5,
            WindowRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 900,
            },
        )
        .expect("valid fractional-scale fit");
        assert_eq!(
            placement.size,
            WindowSize {
                width: 200,
                height: 100,
            }
        );
    }

    #[test]
    fn scaled_four_k_video_reserves_edges_and_chrome() {
        let placement = fit_physical_video_to_work_area(
            3840,
            2160,
            2.0,
            WindowRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
        )
        .expect("valid scaled fit");

        assert_eq!(
            work_area_budget(1920, 1040),
            Some(WindowSize {
                width: 1804,
                height: 935,
            })
        );
        assert_eq!(
            placement,
            WindowPlacement {
                size: WindowSize {
                    width: 1662,
                    height: 935,
                },
                position: WindowPoint { x: 129, y: 52 },
            }
        );
        assert!(placement.size.width * 2 <= 3840);
        assert!(placement.size.height * 2 <= 2160);
    }

    #[test]
    fn oversized_video_is_aspect_fit_and_centered_on_offset_monitor() {
        assert_eq!(
            fit_physical_video_to_work_area(
                3840,
                2160,
                1.0,
                WindowRect {
                    x: 1920,
                    y: -120,
                    width: 1024,
                    height: 768,
                }
            ),
            Some(WindowPlacement {
                size: WindowSize {
                    width: 962,
                    height: 541,
                },
                position: WindowPoint { x: 1951, y: -7 },
            })
        );
    }

    #[test]
    fn compositor_move_to_smaller_monitor_retargets_without_stale_size() {
        let large = fit_physical_video_to_work_area(
            3840,
            2160,
            2.0,
            WindowRect {
                x: 0,
                y: 14,
                width: 1920,
                height: 1051,
            },
        )
        .expect("large-monitor fit");
        assert_eq!(
            large.size,
            WindowSize {
                width: 1680,
                height: 945
            }
        );

        let corrected = smaller_fit_for_work_area(
            3840,
            2160,
            2.0,
            WindowRect {
                x: 1920,
                y: 432,
                width: 1152,
                height: 648,
            },
            large,
        )
        .expect("smaller monitor must replace the stale target");
        assert_eq!(
            corrected.size,
            WindowSize {
                width: 1008,
                height: 567
            }
        );
        assert_eq!(corrected.position, WindowPoint { x: 1992, y: 472 });
    }

    #[test]
    fn compositor_move_to_larger_monitor_never_grows_the_initial_target() {
        let small = fit_physical_video_to_work_area(
            3840,
            2160,
            2.0,
            WindowRect {
                x: 1920,
                y: 432,
                width: 1152,
                height: 648,
            },
        )
        .expect("small-monitor fit");
        assert_eq!(
            smaller_fit_for_work_area(
                3840,
                2160,
                2.0,
                WindowRect {
                    x: 0,
                    y: 14,
                    width: 1920,
                    height: 1051,
                },
                small,
            ),
            None
        );
    }

    #[test]
    fn requested_position_is_clamped_to_every_workarea_edge() {
        let work_area = WindowRect {
            x: -1920,
            y: 24,
            width: 1920,
            height: 1056,
        };
        let size = WindowSize {
            width: 1200,
            height: 800,
        };
        assert_eq!(
            clamp_position(WindowPoint { x: -2500, y: -100 }, size, work_area),
            WindowPoint { x: -1920, y: 24 }
        );
        assert_eq!(
            clamp_position(WindowPoint { x: -100, y: 900 }, size, work_area),
            WindowPoint { x: -1200, y: 280 }
        );
    }

    #[test]
    fn window_larger_than_workarea_clamps_to_origin() {
        assert_eq!(
            clamp_position(
                WindowPoint { x: 900, y: 900 },
                WindowSize {
                    width: 1200,
                    height: 900,
                },
                WindowRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 600,
                }
            ),
            WindowPoint { x: 100, y: 50 }
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
