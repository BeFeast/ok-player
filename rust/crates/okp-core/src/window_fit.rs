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

/// Canonical compact-player shorter edge from the compact-modes handoff.
pub const COMPACT_DEFAULT_SHORT_EDGE: i32 = 270;

/// Smallest compact-player shorter edge that still leaves the transport usable.
pub const COMPACT_MIN_SHORT_EDGE: i32 = 160;

/// Canonical desktop inset for a compact-player corner rest.
pub const COMPACT_SNAP_INSET: i32 = 16;

/// Maximum release distance on each axis that settles into a corner rest.
pub const COMPACT_SNAP_THRESHOLD: i32 = 48;

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

/// Resolve a monitor-local fit area from optional size-only compositor bounds.
///
/// `GdkToplevelSize` deliberately exposes only the largest usable width and
/// height, not which monitor edge owns a panel or dock. When those dimensions
/// are smaller than the monitor, use the intersection of every possible
/// placement of that rectangle inside the monitor. The result is conservative,
/// but it cannot overlap a reserved top, bottom, left, or right edge merely
/// because the shell guessed the missing origin. If no bounds were published,
/// the monitor geometry is the only available fallback.
pub fn monitor_fit_work_area(
    monitor_geometry: WindowRect,
    reported_bounds: Option<WindowSize>,
) -> Option<WindowRect> {
    if monitor_geometry.width <= 0 || monitor_geometry.height <= 0 {
        return None;
    }

    let Some(bounds) = reported_bounds.filter(|bounds| bounds.width > 0 && bounds.height > 0)
    else {
        return Some(monitor_geometry);
    };
    let (x, width) =
        conservative_bounds_axis(monitor_geometry.x, monitor_geometry.width, bounds.width);
    let (y, height) =
        conservative_bounds_axis(monitor_geometry.y, monitor_geometry.height, bounds.height);
    Some(WindowRect {
        x,
        y,
        width,
        height,
    })
}

fn conservative_bounds_axis(origin: i32, monitor_length: i32, bounds_length: i32) -> (i32, i32) {
    let bounds_length = bounds_length.clamp(1, monitor_length);
    let total_reserved = monitor_length - bounds_length;
    let guaranteed_length = bounds_length - total_reserved;

    // A reported bound smaller than half the monitor has no non-empty interval
    // common to every possible origin. Treat that implausible value as
    // unusable instead of collapsing the player to a sliver.
    if guaranteed_length <= 0 {
        return (origin, monitor_length);
    }

    (origin.saturating_add(total_reserved), guaranteed_length)
}

/// Predictable contract for the explicit "Fit window to media" command.
///
/// Automatic source-load fitting never exits a user-selected window state.
/// The explicit command is different: choosing it authorizes the shell to
/// restore a normal window first, then run the same monitor-aware fit
/// transaction. Playback and media loading are intentionally absent from this
/// plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplicitWindowFitAction {
    Disabled,
    FitWindowed,
    RestoreWindowedAndFit,
}

pub const fn explicit_window_fit_action(
    has_media_geometry: bool,
    fullscreen: bool,
    maximized: bool,
) -> ExplicitWindowFitAction {
    if !has_media_geometry {
        ExplicitWindowFitAction::Disabled
    } else if fullscreen || maximized {
        ExplicitWindowFitAction::RestoreWindowedAndFit
    } else {
        ExplicitWindowFitAction::FitWindowed
    }
}

/// Size a compact video window from the real display aspect, keeping the
/// shorter edge fixed and letting the longer edge follow the source.
pub fn compact_size_for_video(
    video_width: i32,
    video_height: i32,
    short_edge: i32,
) -> Option<WindowSize> {
    if video_width <= 0 || video_height <= 0 || short_edge <= 0 {
        return None;
    }

    let scale = if video_width >= video_height {
        f64::from(short_edge) / f64::from(video_height)
    } else {
        f64::from(short_edge) / f64::from(video_width)
    };
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }

    Some(WindowSize {
        width: (f64::from(video_width) * scale).round_ties_even().max(1.0) as i32,
        height: (f64::from(video_height) * scale).round_ties_even().max(1.0) as i32,
    })
}

/// Settle a compact window into the nearest work-area corner when the release
/// position is within the configured threshold on both axes.
pub fn compact_corner_snap(
    position: WindowPoint,
    size: WindowSize,
    work_area: WindowRect,
    inset: i32,
    threshold: i32,
) -> Option<WindowPoint> {
    if size.width <= 0
        || size.height <= 0
        || work_area.width <= 0
        || work_area.height <= 0
        || inset < 0
        || threshold < 0
    {
        return None;
    }
    let left = work_area.x.saturating_add(inset);
    let top = work_area.y.saturating_add(inset);
    let right = i64::from(work_area.x)
        .saturating_add(i64::from(work_area.width))
        .saturating_sub(i64::from(size.width))
        .saturating_sub(i64::from(inset))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let bottom = i64::from(work_area.y)
        .saturating_add(i64::from(work_area.height))
        .saturating_sub(i64::from(size.height))
        .saturating_sub(i64::from(inset))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let corners = [
        WindowPoint { x: left, y: top },
        WindowPoint { x: right, y: top },
        WindowPoint { x: left, y: bottom },
        WindowPoint {
            x: right,
            y: bottom,
        },
    ];
    corners
        .into_iter()
        .filter(|corner| {
            i64::from(position.x).abs_diff(i64::from(corner.x)) <= threshold as u64
                && i64::from(position.y).abs_diff(i64::from(corner.y)) <= threshold as u64
        })
        .min_by_key(|corner| {
            i64::from(position.x).abs_diff(i64::from(corner.x)).pow(2)
                + i64::from(position.y).abs_diff(i64::from(corner.y)).pow(2)
        })
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
    fn compact_sizes_follow_real_video_aspect() {
        assert_eq!(
            compact_size_for_video(1920, 1080, COMPACT_DEFAULT_SHORT_EDGE),
            Some(WindowSize {
                width: 480,
                height: 270
            })
        );
        assert_eq!(
            compact_size_for_video(1080, 1920, COMPACT_DEFAULT_SHORT_EDGE),
            Some(WindowSize {
                width: 270,
                height: 480
            })
        );
        assert_eq!(
            compact_size_for_video(2390, 1000, COMPACT_DEFAULT_SHORT_EDGE),
            Some(WindowSize {
                width: 645,
                height: 270
            })
        );
    }

    #[test]
    fn explicit_fit_only_restores_window_state_after_deliberate_activation() {
        assert_eq!(
            explicit_window_fit_action(false, true, true),
            ExplicitWindowFitAction::Disabled
        );
        assert_eq!(
            explicit_window_fit_action(true, false, false),
            ExplicitWindowFitAction::FitWindowed
        );
        assert_eq!(
            explicit_window_fit_action(true, true, false),
            ExplicitWindowFitAction::RestoreWindowedAndFit
        );
        assert_eq!(
            explicit_window_fit_action(true, false, true),
            ExplicitWindowFitAction::RestoreWindowedAndFit
        );
    }

    #[test]
    fn compact_floor_uses_the_limiting_dimension() {
        assert_eq!(
            compact_size_for_video(1920, 1080, COMPACT_MIN_SHORT_EDGE),
            Some(WindowSize {
                width: 284,
                height: 160
            })
        );
        assert_eq!(
            compact_size_for_video(1080, 1920, COMPACT_MIN_SHORT_EDGE),
            Some(WindowSize {
                width: 160,
                height: 284
            })
        );
    }

    #[test]
    fn compact_size_rejects_non_positive_inputs() {
        assert_eq!(compact_size_for_video(0, 1080, 270), None);
        assert_eq!(compact_size_for_video(1920, 0, 270), None);
        assert_eq!(compact_size_for_video(1920, 1080, 0), None);
    }

    #[test]
    fn compact_window_snaps_to_each_inset_corner() {
        let work_area = WindowRect {
            x: 100,
            y: 50,
            width: 1280,
            height: 900,
        };
        let size = WindowSize {
            width: 480,
            height: 270,
        };
        for (release, expected) in [
            (WindowPoint { x: 120, y: 70 }, WindowPoint { x: 116, y: 66 }),
            (WindowPoint { x: 870, y: 75 }, WindowPoint { x: 884, y: 66 }),
            (
                WindowPoint { x: 110, y: 650 },
                WindowPoint { x: 116, y: 664 },
            ),
            (
                WindowPoint { x: 900, y: 680 },
                WindowPoint { x: 884, y: 664 },
            ),
        ] {
            assert_eq!(
                compact_corner_snap(
                    release,
                    size,
                    work_area,
                    COMPACT_SNAP_INSET,
                    COMPACT_SNAP_THRESHOLD,
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn compact_window_does_not_snap_from_the_middle() {
        assert_eq!(
            compact_corner_snap(
                WindowPoint { x: 400, y: 300 },
                WindowSize {
                    width: 480,
                    height: 270,
                },
                WindowRect {
                    x: 0,
                    y: 0,
                    width: 1280,
                    height: 900,
                },
                COMPACT_SNAP_INSET,
                COMPACT_SNAP_THRESHOLD,
            ),
            None
        );
    }

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
    fn missing_toplevel_bounds_fall_back_to_the_current_monitor() {
        let monitor = WindowRect {
            x: 1920,
            y: -120,
            width: 2560,
            height: 1440,
        };
        assert_eq!(monitor_fit_work_area(monitor, None), Some(monitor));
    }

    #[test]
    fn size_only_bounds_cannot_overlap_an_unknown_reserved_edge() {
        let monitor = WindowRect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let resolved = monitor_fit_work_area(
            monitor,
            Some(WindowSize {
                width: 1880,
                height: 1040,
            }),
        )
        .expect("valid monitor-local workarea");

        assert_eq!(
            resolved,
            WindowRect {
                x: 1960,
                y: 40,
                width: 1840,
                height: 1000,
            }
        );
        for possible_work_area in [
            WindowRect {
                x: 1960,
                y: 40,
                width: 1880,
                height: 1040,
            },
            WindowRect {
                x: 1920,
                y: 0,
                width: 1880,
                height: 1040,
            },
            WindowRect {
                x: 1940,
                y: 20,
                width: 1880,
                height: 1040,
            },
        ] {
            assert!(rect_contains(possible_work_area, resolved));
        }
    }

    #[test]
    fn desktop_union_bounds_are_clamped_to_one_monitor() {
        let monitor = WindowRect {
            x: -1280,
            y: 24,
            width: 1280,
            height: 1024,
        };
        assert_eq!(
            monitor_fit_work_area(
                monitor,
                Some(WindowSize {
                    width: 3200,
                    height: 1040,
                })
            ),
            Some(monitor)
        );
    }

    #[test]
    fn invalid_or_implausibly_small_bounds_keep_a_usable_monitor_fallback() {
        let monitor = WindowRect {
            x: 0,
            y: 0,
            width: 1280,
            height: 900,
        };
        assert_eq!(
            monitor_fit_work_area(
                monitor,
                Some(WindowSize {
                    width: 0,
                    height: 800,
                })
            ),
            Some(monitor)
        );
        assert_eq!(
            monitor_fit_work_area(
                monitor,
                Some(WindowSize {
                    width: 500,
                    height: 400,
                })
            ),
            Some(monitor)
        );
        assert_eq!(
            monitor_fit_work_area(
                WindowRect {
                    width: 0,
                    ..monitor
                },
                None
            ),
            None
        );
    }

    fn rect_contains(outer: WindowRect, inner: WindowRect) -> bool {
        i64::from(inner.x) >= i64::from(outer.x)
            && i64::from(inner.y) >= i64::from(outer.y)
            && i64::from(inner.x) + i64::from(inner.width)
                <= i64::from(outer.x) + i64::from(outer.width)
            && i64::from(inner.y) + i64::from(inner.height)
                <= i64::from(outer.y) + i64::from(outer.height)
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
