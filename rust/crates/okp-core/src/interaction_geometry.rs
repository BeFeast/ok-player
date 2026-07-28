//! Portable projection from window state to the geometry an input harness needs.
//!
//! Wayland gives a client no way to ask where another window is, and the shells refuse
//! window introspection to unprivileged callers, so automation that drives real pointer
//! input has to guess where the player is. The app is the only process that knows, so it
//! reports the answer on the existing `OKP_DEBUG_INTERACTIONS` diagnostic stream.
//!
//! This module owns the projection: window-local plane rectangles plus the surface state
//! become global logical rectangles, a chrome-free drag surface, and one aim point. The
//! GTK shell only gathers the raw values and prints the rendered lines.

use std::fmt::Write as _;

/// Prefix shared by every emitted geometry line.
pub const GEOMETRY_PREFIX: &str = "interaction: geometry";

/// Value used wherever a coordinate exists conceptually but cannot be resolved.
pub const UNKNOWN: &str = "unknown";

/// Canonical plane name of the video surface.
pub const VIDEO_PLANE: &str = "video";

/// A point in logical (device-independent) pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Nearest whole pixel. Pointer injectors take integers, so an aim point that keeps
    /// halves would be rounded by the tool anyway - do it here where it can be tested.
    pub fn rounded(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }
}

/// A rectangle in logical (device-independent) pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn area(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.width * self.height
        }
    }

    pub fn is_empty(&self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn offset(&self, dx: f64, dy: f64) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Convert logical pixels to the surface's device pixels for tools that work there.
    pub fn to_device(&self, scale: f64) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self::new(
            self.x * scale,
            self.y * scale,
            self.width * scale,
            self.height * scale,
        )
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }

    pub fn intersect(&self, other: Rect) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        let candidate = Self::new(x, y, right - x, bottom - y);
        (!candidate.is_empty()).then_some(candidate)
    }

    /// Union of a monitor list or of any other rectangle set.
    pub fn union(&self, other: Rect) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }
}

/// One monitor as the display server describes it, in global logical coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub connector: String,
    pub bounds: Rect,
    pub scale: f64,
}

/// A window-local input plane: the video surface or one piece of chrome.
///
/// Planes are ordered bottom to top, so the last plane containing a point is the one that
/// receives the event.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    pub name: String,
    pub bounds: Rect,
    /// Whether the plane takes pointer input. A visible but click-through overlay is not
    /// an obstacle for a harness aiming at the video underneath.
    pub interactive: bool,
}

impl Plane {
    pub fn new(name: impl Into<String>, bounds: Rect, interactive: bool) -> Self {
        Self {
            name: name.into(),
            bounds,
            interactive,
        }
    }
}

/// How the window's global origin was resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginSource {
    /// The platform reports the toplevel position (X11).
    Reported,
    /// A fullscreen toplevel covers its monitor, so the monitor origin is the window origin.
    FullscreenMonitor,
    /// Wayland: no client may learn its own global position. The harness resolves the
    /// origin from a pointer sample instead (`part=pointer`).
    Unknown,
}

impl OriginSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::FullscreenMonitor => "fullscreen-monitor",
            Self::Unknown => UNKNOWN,
        }
    }
}

/// Why a geometry record was emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryReason {
    Map,
    Resize,
    Move,
    Monitor,
    Fullscreen,
    Compact,
    Layout,
    Pointer,
}

impl GeometryReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Resize => "resize",
            Self::Move => "move",
            Self::Monitor => "monitor",
            Self::Fullscreen => "fullscreen",
            Self::Compact => "compact",
            Self::Layout => "layout",
            Self::Pointer => "pointer",
        }
    }
}

/// Everything the shell knows about its own surfaces at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowGeometry {
    /// Client area in window-local logical pixels, anchored at (0, 0).
    pub client: Rect,
    /// Logical-to-device pixel ratio of the surface.
    pub scale: f64,
    pub fullscreen: bool,
    pub maximized: bool,
    pub compact: bool,
    /// Global logical position where the platform publishes it; `None` on Wayland.
    pub position: Option<Point>,
    /// The monitor the surface currently sits on.
    pub monitor: Option<Monitor>,
    /// Every monitor, so a harness can build the desktop coordinate space it injects into.
    pub monitors: Vec<Monitor>,
    /// Window-local planes, bottom to top: the video surface first, then chrome.
    pub planes: Vec<Plane>,
}

impl WindowGeometry {
    pub fn origin_source(&self) -> OriginSource {
        if self.position.is_some() {
            OriginSource::Reported
        } else if self.fullscreen && self.monitor.is_some() {
            OriginSource::FullscreenMonitor
        } else {
            OriginSource::Unknown
        }
    }

    /// Global logical position of the client area's top-left corner, when it is knowable.
    pub fn origin(&self) -> Option<Point> {
        match self.origin_source() {
            OriginSource::Reported => self.position,
            OriginSource::FullscreenMonitor => self
                .monitor
                .as_ref()
                .map(|monitor| Point::new(monitor.bounds.x, monitor.bounds.y)),
            OriginSource::Unknown => None,
        }
    }

    /// The desktop rectangle every monitor lives in - the space a pointer moves through.
    pub fn desktop(&self) -> Option<Rect> {
        self.monitors
            .iter()
            .map(|monitor| monitor.bounds)
            .filter(|bounds| !bounds.is_empty())
            .reduce(|accumulated, bounds| accumulated.union(bounds))
    }

    pub fn plane(&self, name: &str) -> Option<&Plane> {
        self.planes.iter().find(|plane| plane.name == name)
    }

    pub fn video(&self) -> Option<&Plane> {
        self.plane(VIDEO_PLANE)
    }

    /// Project a window-local rectangle into the global pointer space.
    pub fn to_global(&self, local: Rect) -> Option<Rect> {
        self.origin().map(|origin| local.offset(origin.x, origin.y))
    }

    /// The plane that would receive a press at this window-local point.
    pub fn plane_at(&self, local: Point) -> Option<&Plane> {
        self.planes
            .iter()
            .rev()
            .find(|plane| plane.interactive && plane.bounds.contains(local))
    }

    /// The largest part of the video plane no interactive chrome covers.
    ///
    /// A drag that must start over video and not over the OSC needs this rectangle, not
    /// the video rectangle: the OSC sits on top of the video and swallows the press.
    pub fn drag_surface(&self) -> Option<Rect> {
        let video = self.video()?.bounds;
        let blockers = self
            .planes
            .iter()
            .filter(|plane| plane.interactive && plane.name != VIDEO_PLANE)
            .map(|plane| plane.bounds)
            .collect::<Vec<_>>();
        free_rect(video, &blockers)
    }

    /// Where a harness should aim, in the global logical space, at whole pixels.
    ///
    /// The point stays inside the visible desktop so a partially off-screen window still
    /// yields a reachable target.
    pub fn pointer_target(&self) -> Option<Point> {
        let local = self.drag_surface()?;
        let global = self.to_global(local)?;
        let aimed = match self.desktop() {
            Some(desktop) => global.intersect(desktop).unwrap_or(global),
            None => global,
        };
        Some(aimed.center().rounded())
    }

    /// The record a harness reads: one window line, one line per plane, one aim line.
    pub fn record(&self, reason: GeometryReason, seq: u64) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.planes.len() + 2);
        lines.push(self.window_line(reason, seq));
        for plane in &self.planes {
            lines.push(self.plane_line(&plane.name, plane, reason, seq));
        }
        if let Some(drag) = self.drag_surface() {
            let plane = Plane::new("drag-target", drag, true);
            lines.push(self.plane_line("drag-target", &plane, reason, seq));
        }
        lines
    }

    /// A pointer sample. Injecting at a known global point and reading the reported
    /// window-local coordinates is how a harness resolves the origin on Wayland.
    pub fn pointer_sample(&self, local: Point, seq: u64) -> String {
        let over = self
            .plane_at(local)
            .map(|plane| plane.name.as_str())
            .unwrap_or(UNKNOWN);
        let mut line = String::new();
        let _ = write!(
            line,
            "{GEOMETRY_PREFIX} part=pointer reason={} seq={seq} local-x={} local-y={} over={over} scale={}",
            GeometryReason::Pointer.label(),
            coordinate(local.x),
            coordinate(local.y),
            factor(self.scale),
        );
        line
    }

    fn window_line(&self, reason: GeometryReason, seq: u64) -> String {
        let mut line = String::new();
        let origin = self.origin();
        let _ = write!(
            line,
            "{GEOMETRY_PREFIX} part=window reason={} seq={seq} origin={} x={} y={} w={} h={} scale={} fullscreen={} maximized={} compact={}",
            reason.label(),
            self.origin_source().label(),
            optional(origin.map(|point| point.x)),
            optional(origin.map(|point| point.y)),
            coordinate(self.client.width),
            coordinate(self.client.height),
            factor(self.scale),
            flag(self.fullscreen),
            flag(self.maximized),
            flag(self.compact),
        );
        match self.monitor.as_ref() {
            Some(monitor) => {
                let _ = write!(
                    line,
                    " monitor={} monitor-x={} monitor-y={} monitor-w={} monitor-h={} monitor-scale={}",
                    monitor.connector,
                    coordinate(monitor.bounds.x),
                    coordinate(monitor.bounds.y),
                    coordinate(monitor.bounds.width),
                    coordinate(monitor.bounds.height),
                    factor(monitor.scale),
                );
            }
            None => {
                let _ = write!(
                    line,
                    " monitor={UNKNOWN} monitor-x={UNKNOWN} monitor-y={UNKNOWN} monitor-w={UNKNOWN} monitor-h={UNKNOWN} monitor-scale={UNKNOWN}"
                );
            }
        }
        let desktop = self.desktop();
        let _ = write!(
            line,
            " desktop-x={} desktop-y={} desktop-w={} desktop-h={} monitors={}",
            optional(desktop.map(|rect| rect.x)),
            optional(desktop.map(|rect| rect.y)),
            optional(desktop.map(|rect| rect.width)),
            optional(desktop.map(|rect| rect.height)),
            self.monitors.len(),
        );
        line
    }

    fn plane_line(&self, name: &str, plane: &Plane, reason: GeometryReason, seq: u64) -> String {
        let global = self.to_global(plane.bounds);
        let center = global.map(|rect| {
            match self.desktop() {
                Some(desktop) => rect.intersect(desktop).unwrap_or(rect),
                None => rect,
            }
            .center()
            .rounded()
        });
        let device = plane.bounds.to_device(self.scale);
        let mut line = String::new();
        let _ = write!(
            line,
            "{GEOMETRY_PREFIX} part={name} reason={} seq={seq} local-x={} local-y={} w={} h={} x={} y={} center-x={} center-y={} device-x={} device-y={} device-w={} device-h={} interactive={}",
            reason.label(),
            coordinate(plane.bounds.x),
            coordinate(plane.bounds.y),
            coordinate(plane.bounds.width),
            coordinate(plane.bounds.height),
            optional(global.map(|rect| rect.x)),
            optional(global.map(|rect| rect.y)),
            optional(center.map(|point| point.x)),
            optional(center.map(|point| point.y)),
            coordinate(device.x),
            coordinate(device.y),
            coordinate(device.width),
            coordinate(device.height),
            flag(plane.interactive),
        );
        line
    }
}

/// Classify what changed between two snapshots, so the emitted record says why it moved.
pub fn reason_between(previous: Option<&WindowGeometry>, next: &WindowGeometry) -> GeometryReason {
    let Some(previous) = previous else {
        return GeometryReason::Map;
    };
    if previous.fullscreen != next.fullscreen {
        GeometryReason::Fullscreen
    } else if previous.compact != next.compact {
        GeometryReason::Compact
    } else if monitor_identity(previous) != monitor_identity(next) {
        GeometryReason::Monitor
    } else if previous.client.width != next.client.width
        || previous.client.height != next.client.height
        || previous.maximized != next.maximized
    {
        GeometryReason::Resize
    } else if previous.origin() != next.origin() {
        GeometryReason::Move
    } else {
        GeometryReason::Layout
    }
}

fn monitor_identity(geometry: &WindowGeometry) -> Option<(&str, i64, i64, i64, i64)> {
    geometry.monitor.as_ref().map(|monitor| {
        (
            monitor.connector.as_str(),
            monitor.bounds.x as i64,
            monitor.bounds.y as i64,
            monitor.bounds.width as i64,
            monitor.bounds.height as i64,
        )
    })
}

/// Largest axis-aligned rectangle inside `area` that no blocker overlaps.
///
/// Chrome bands are few and mostly full-width or full-height, so enumerating the
/// candidate rectangles cut by every blocker edge is both exact and cheap.
pub fn free_rect(area: Rect, blockers: &[Rect]) -> Option<Rect> {
    if area.is_empty() {
        return None;
    }
    let blockers = blockers
        .iter()
        .filter_map(|blocker| blocker.intersect(area))
        .collect::<Vec<_>>();
    if blockers.is_empty() {
        return Some(area);
    }

    let mut xs = vec![area.x, area.right()];
    let mut ys = vec![area.y, area.bottom()];
    for blocker in &blockers {
        xs.push(blocker.x);
        xs.push(blocker.right());
        ys.push(blocker.y);
        ys.push(blocker.bottom());
    }
    sort_unique(&mut xs);
    sort_unique(&mut ys);

    let mut best: Option<Rect> = None;
    for (left_index, &left) in xs.iter().enumerate() {
        for &right in xs.iter().skip(left_index + 1) {
            for (top_index, &top) in ys.iter().enumerate() {
                for &bottom in ys.iter().skip(top_index + 1) {
                    let candidate = Rect::new(left, top, right - left, bottom - top);
                    if candidate.is_empty() {
                        continue;
                    }
                    if best.is_some_and(|best| best.area() >= candidate.area()) {
                        continue;
                    }
                    if blockers
                        .iter()
                        .any(|blocker| blocker.intersect(candidate).is_some())
                    {
                        continue;
                    }
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

fn sort_unique(values: &mut Vec<f64>) {
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    values.dedup_by(|left, right| (*left - *right).abs() < f64::EPSILON);
}

fn coordinate(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.1}")
    } else {
        UNKNOWN.to_owned()
    }
}

fn factor(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.3}")
    } else {
        UNKNOWN.to_owned()
    }
}

fn optional(value: Option<f64>) -> String {
    value.map(coordinate).unwrap_or_else(|| UNKNOWN.to_owned())
}

fn flag(value: bool) -> u8 {
    u8::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(connector: &str, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Monitor {
        Monitor {
            connector: connector.to_owned(),
            bounds: Rect::new(x, y, width, height),
            scale,
        }
    }

    /// Windowed player on the operator's dual-head layout: 1920x1080 logical at the
    /// origin plus a scaled panel to its right.
    fn windowed() -> WindowGeometry {
        WindowGeometry {
            client: Rect::new(0.0, 0.0, 1120.0, 680.0),
            scale: 2.0,
            fullscreen: false,
            maximized: false,
            compact: false,
            position: None,
            monitor: Some(monitor("DP-5", 0.0, 0.0, 1920.0, 1080.0, 2.0)),
            monitors: vec![
                monitor("DP-5", 0.0, 0.0, 1920.0, 1080.0, 2.0),
                monitor("eDP-1", 1920.0, 425.0, 1164.0, 655.0, 1.65),
            ],
            planes: vec![
                Plane::new(VIDEO_PLANE, Rect::new(0.0, 0.0, 1120.0, 680.0), true),
                Plane::new("titlebar", Rect::new(0.0, 0.0, 1120.0, 42.0), true),
                Plane::new("osc", Rect::new(0.0, 590.0, 1120.0, 90.0), true),
            ],
        }
    }

    fn fields(line: &str) -> std::collections::HashMap<String, String> {
        line.split_whitespace()
            .filter_map(|token| token.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }

    fn record_part(
        geometry: &WindowGeometry,
        reason: GeometryReason,
        part: &str,
    ) -> std::collections::HashMap<String, String> {
        let lines = geometry.record(reason, 7);
        let line = lines
            .iter()
            .find(|line| fields(line).get("part").map(String::as_str) == Some(part))
            .unwrap_or_else(|| panic!("no {part} line in {lines:?}"));
        fields(line)
    }

    #[test]
    fn a_fullscreen_window_resolves_its_origin_from_the_monitor_it_covers() {
        let mut geometry = windowed();
        assert_eq!(geometry.origin_source(), OriginSource::Unknown);
        assert_eq!(geometry.origin(), None);
        assert_eq!(geometry.pointer_target(), None);

        geometry.fullscreen = true;
        geometry.monitor = Some(monitor("eDP-1", 1920.0, 425.0, 1164.0, 655.0, 1.65));
        geometry.client = Rect::new(0.0, 0.0, 1164.0, 655.0);
        geometry.planes[0].bounds = Rect::new(0.0, 0.0, 1164.0, 655.0);
        geometry.planes[1].bounds = Rect::new(0.0, 0.0, 1164.0, 42.0);
        geometry.planes[2].bounds = Rect::new(0.0, 565.0, 1164.0, 90.0);

        assert_eq!(geometry.origin_source(), OriginSource::FullscreenMonitor);
        assert_eq!(geometry.origin(), Some(Point::new(1920.0, 425.0)));
        assert_eq!(
            geometry.to_global(Rect::new(10.0, 20.0, 100.0, 50.0)),
            Some(Rect::new(1930.0, 445.0, 100.0, 50.0))
        );
    }

    #[test]
    fn a_reported_position_wins_over_the_fullscreen_shortcut() {
        let mut geometry = windowed();
        geometry.position = Some(Point::new(300.0, 150.0));
        assert_eq!(geometry.origin_source(), OriginSource::Reported);
        assert_eq!(geometry.origin(), Some(Point::new(300.0, 150.0)));

        geometry.fullscreen = true;
        assert_eq!(geometry.origin(), Some(Point::new(300.0, 150.0)));
    }

    #[test]
    fn the_drag_surface_is_the_video_band_no_chrome_covers() {
        let geometry = windowed();
        let drag = geometry.drag_surface().expect("drag surface");
        assert_eq!(drag, Rect::new(0.0, 42.0, 1120.0, 548.0));

        let titlebar = geometry.plane("titlebar").expect("titlebar").bounds;
        let osc = geometry.plane("osc").expect("osc").bounds;
        assert_eq!(drag.intersect(titlebar), None);
        assert_eq!(drag.intersect(osc), None);
        assert!(geometry.plane_at(drag.center()).is_some());
        assert_eq!(
            geometry
                .plane_at(drag.center())
                .map(|plane| plane.name.as_str()),
            Some(VIDEO_PLANE)
        );
    }

    #[test]
    fn a_side_panel_pushes_the_drag_surface_off_the_covered_edge() {
        let mut geometry = windowed();
        geometry.planes.push(Plane::new(
            "side-panel",
            Rect::new(800.0, 0.0, 320.0, 680.0),
            true,
        ));

        let drag = geometry.drag_surface().expect("drag surface");
        assert_eq!(drag, Rect::new(0.0, 42.0, 800.0, 548.0));
        assert!(drag.right() <= 800.0);
    }

    #[test]
    fn a_click_through_overlay_does_not_shrink_the_drag_surface() {
        let mut geometry = windowed();
        geometry.planes.push(Plane::new(
            "media-state",
            Rect::new(0.0, 0.0, 1120.0, 680.0),
            false,
        ));

        assert_eq!(
            geometry.drag_surface(),
            Some(Rect::new(0.0, 42.0, 1120.0, 548.0))
        );
    }

    #[test]
    fn the_aim_point_is_a_whole_pixel_inside_the_video_plane_in_global_space() {
        let mut geometry = windowed();
        geometry.fullscreen = true;
        geometry.monitor = Some(monitor("eDP-1", 1920.0, 425.0, 1164.0, 655.0, 1.65));
        geometry.client = Rect::new(0.0, 0.0, 1164.0, 655.0);
        geometry.planes[0].bounds = Rect::new(0.0, 0.0, 1164.0, 655.0);
        geometry.planes[1].bounds = Rect::new(0.0, 0.0, 1164.0, 41.0);
        geometry.planes[2].bounds = Rect::new(0.0, 565.0, 1164.0, 90.0);

        let target = geometry.pointer_target().expect("aim point");
        assert_eq!(target, Point::new(2502.0, 728.0));
        assert_eq!(target.x.fract(), 0.0);
        assert_eq!(target.y.fract(), 0.0);

        let local = Point::new(target.x - 1920.0, target.y - 425.0);
        assert_eq!(
            geometry.plane_at(local).map(|plane| plane.name.as_str()),
            Some(VIDEO_PLANE)
        );
    }

    #[test]
    fn an_aim_point_stays_on_the_desktop_when_the_window_hangs_off_it() {
        let mut geometry = windowed();
        geometry.position = Some(Point::new(1400.0, 800.0));
        geometry.monitors = vec![monitor("DP-5", 0.0, 0.0, 1920.0, 1080.0, 2.0)];
        geometry.monitor = Some(monitor("DP-5", 0.0, 0.0, 1920.0, 1080.0, 2.0));

        let desktop = geometry.desktop().expect("desktop");
        let target = geometry.pointer_target().expect("aim point");
        assert!(desktop.contains(target), "{target:?} outside {desktop:?}");
        assert_eq!(target, Point::new(1660.0, 961.0));
    }

    #[test]
    fn the_desktop_rectangle_spans_every_monitor() {
        let geometry = windowed();
        assert_eq!(
            geometry.desktop(),
            Some(Rect::new(0.0, 0.0, 3084.0, 1080.0))
        );
    }

    #[test]
    fn chrome_over_the_video_owns_the_point_under_the_pointer() {
        let geometry = windowed();
        let osc_sample = geometry.pointer_sample(Point::new(560.0, 620.0), 3);
        assert_eq!(
            fields(&osc_sample).get("over").map(String::as_str),
            Some("osc")
        );

        let video_sample = geometry.pointer_sample(Point::new(560.0, 300.0), 4);
        let parsed = fields(&video_sample);
        assert_eq!(parsed.get("over").map(String::as_str), Some(VIDEO_PLANE));
        assert_eq!(parsed.get("local-x").map(String::as_str), Some("560.0"));
        assert_eq!(parsed.get("local-y").map(String::as_str), Some("300.0"));

        let outside = geometry.pointer_sample(Point::new(-5.0, 300.0), 5);
        assert_eq!(
            fields(&outside).get("over").map(String::as_str),
            Some(UNKNOWN)
        );
    }

    #[test]
    fn a_harness_reads_the_video_rectangle_and_the_scale_out_of_the_record() {
        let mut geometry = windowed();
        geometry.fullscreen = true;
        geometry.monitor = Some(monitor("DP-5", 0.0, 0.0, 1920.0, 1080.0, 2.0));
        geometry.client = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        geometry.planes[0].bounds = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        geometry.planes[1].bounds = Rect::new(0.0, 0.0, 1920.0, 42.0);
        geometry.planes[2].bounds = Rect::new(0.0, 990.0, 1920.0, 90.0);

        let window = record_part(&geometry, GeometryReason::Fullscreen, "window");
        assert_eq!(window.get("reason").map(String::as_str), Some("fullscreen"));
        assert_eq!(window.get("seq").map(String::as_str), Some("7"));
        assert_eq!(
            window.get("origin").map(String::as_str),
            Some("fullscreen-monitor")
        );
        assert_eq!(window.get("x").map(String::as_str), Some("0.0"));
        assert_eq!(window.get("w").map(String::as_str), Some("1920.0"));
        assert_eq!(window.get("scale").map(String::as_str), Some("2.000"));
        assert_eq!(window.get("monitor").map(String::as_str), Some("DP-5"));
        assert_eq!(window.get("desktop-w").map(String::as_str), Some("3084.0"));
        assert_eq!(window.get("fullscreen").map(String::as_str), Some("1"));

        let video = record_part(&geometry, GeometryReason::Fullscreen, VIDEO_PLANE);
        assert_eq!(video.get("x").map(String::as_str), Some("0.0"));
        assert_eq!(video.get("h").map(String::as_str), Some("1080.0"));
        assert_eq!(video.get("device-w").map(String::as_str), Some("3840.0"));

        let drag = record_part(&geometry, GeometryReason::Fullscreen, "drag-target");
        let aim = geometry.pointer_target().expect("aim point");
        assert_eq!(
            drag.get("center-x").map(String::as_str),
            Some(format!("{:.1}", aim.x).as_str())
        );
        assert_eq!(
            drag.get("center-y").map(String::as_str),
            Some(format!("{:.1}", aim.y).as_str())
        );
        assert_eq!(drag.get("center-y").map(String::as_str), Some("516.0"));
    }

    #[test]
    fn a_wayland_record_marks_the_global_coordinates_unresolved_and_keeps_the_local_ones() {
        let geometry = windowed();
        let window = record_part(&geometry, GeometryReason::Map, "window");
        assert_eq!(window.get("origin").map(String::as_str), Some(UNKNOWN));
        assert_eq!(window.get("x").map(String::as_str), Some(UNKNOWN));
        assert_eq!(window.get("y").map(String::as_str), Some(UNKNOWN));
        assert_eq!(window.get("w").map(String::as_str), Some("1120.0"));
        assert_eq!(window.get("monitor").map(String::as_str), Some("DP-5"));

        let video = record_part(&geometry, GeometryReason::Map, VIDEO_PLANE);
        assert_eq!(video.get("x").map(String::as_str), Some(UNKNOWN));
        assert_eq!(video.get("center-x").map(String::as_str), Some(UNKNOWN));
        assert_eq!(video.get("local-x").map(String::as_str), Some("0.0"));
        assert_eq!(video.get("w").map(String::as_str), Some("1120.0"));
    }

    #[test]
    fn a_pointer_sample_resolves_the_origin_a_wayland_client_cannot_ask_for() {
        let geometry = windowed();
        // The harness injected at this global point and the app reported where it landed.
        let injected = Point::new(1005.0, 421.0);
        let sample = fields(&geometry.pointer_sample(Point::new(325.0, 156.0), 11));
        let local_x = sample["local-x"].parse::<f64>().expect("local-x");
        let local_y = sample["local-y"].parse::<f64>().expect("local-y");

        let resolved = WindowGeometry {
            position: Some(Point::new(injected.x - local_x, injected.y - local_y)),
            ..geometry
        };
        assert_eq!(resolved.origin(), Some(Point::new(680.0, 265.0)));
        assert_eq!(resolved.pointer_target(), Some(Point::new(1240.0, 581.0)));
    }

    #[test]
    fn every_geometry_change_reports_why_it_moved() {
        let base = windowed();
        assert_eq!(reason_between(None, &base), GeometryReason::Map);
        assert_eq!(reason_between(Some(&base), &base), GeometryReason::Layout);

        let resized = WindowGeometry {
            client: Rect::new(0.0, 0.0, 900.0, 600.0),
            ..base.clone()
        };
        assert_eq!(
            reason_between(Some(&base), &resized),
            GeometryReason::Resize
        );

        let fullscreen = WindowGeometry {
            fullscreen: true,
            ..base.clone()
        };
        assert_eq!(
            reason_between(Some(&base), &fullscreen),
            GeometryReason::Fullscreen
        );

        let compact = WindowGeometry {
            compact: true,
            ..base.clone()
        };
        assert_eq!(
            reason_between(Some(&base), &compact),
            GeometryReason::Compact
        );

        let moved_monitor = WindowGeometry {
            monitor: Some(monitor("eDP-1", 1920.0, 425.0, 1164.0, 655.0, 1.65)),
            ..base.clone()
        };
        assert_eq!(
            reason_between(Some(&base), &moved_monitor),
            GeometryReason::Monitor
        );

        let placed = WindowGeometry {
            position: Some(Point::new(10.0, 10.0)),
            ..base.clone()
        };
        let moved = WindowGeometry {
            position: Some(Point::new(40.0, 10.0)),
            ..base.clone()
        };
        assert_eq!(reason_between(Some(&placed), &moved), GeometryReason::Move);

        let relaid = WindowGeometry {
            planes: vec![Plane::new(
                VIDEO_PLANE,
                Rect::new(0.0, 0.0, 1120.0, 680.0),
                true,
            )],
            ..base.clone()
        };
        assert_eq!(reason_between(Some(&base), &relaid), GeometryReason::Layout);
    }

    #[test]
    fn a_fully_covered_video_plane_has_no_drag_surface() {
        let mut geometry = windowed();
        geometry.planes[1].bounds = Rect::new(0.0, 0.0, 1120.0, 680.0);
        assert_eq!(geometry.drag_surface(), None);
        assert_eq!(geometry.pointer_target(), None);
    }

    #[test]
    fn device_pixels_follow_the_surface_scale() {
        let logical = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(logical.to_device(2.0), Rect::new(20.0, 40.0, 200.0, 100.0));
        assert_eq!(logical.to_device(1.0), logical);
        assert_eq!(logical.to_device(0.0), logical);
        assert_eq!(logical.to_device(f64::NAN), logical);
    }
}
