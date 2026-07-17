//! Portable per-file video geometry state and menu policy.
//!
//! The GTK shell renders these choices and `okp-mpv` applies them, but the values,
//! normalization, action transitions, and eligibility rules live here so neither shell
//! grows a second geometry state machine. The same model is stored in the shared history
//! schema as app-index playback memory (PRD §§11.2 and 12.2).

use serde::{Deserialize, Serialize};

pub const ZOOM_MIN: f64 = 1.0;
pub const ZOOM_MAX: f64 = 4.0;
pub const ZOOM_STEP: f64 = 0.25;
pub const PAN_MIN: f64 = -1.0;
pub const PAN_MAX: f64 = 1.0;
pub const PAN_STEP: f64 = 0.1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoAspect {
    #[serde(rename = "16:9")]
    Wide,
    #[serde(rename = "4:3")]
    Standard,
    #[serde(rename = "2.35:1")]
    Cinema,
    #[default]
    #[serde(rename = "auto", alias = "no", other)]
    Auto,
}

impl VideoAspect {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Wide, Self::Standard, Self::Cinema];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Wide => "16:9",
            Self::Standard => "4:3",
            Self::Cinema => "2.35:1",
        }
    }

    pub const fn mpv_value(self) -> &'static str {
        match self {
            Self::Auto => "no",
            Self::Wide => "16:9",
            Self::Standard => "4:3",
            Self::Cinema => "2.35:1",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VideoGeometryAction {
    SetAspect(VideoAspect),
    ZoomIn,
    ZoomOut,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    Center,
    RotateClockwise,
    ToggleFillScreen,
    ToggleDeinterlace,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoGeometry {
    pub aspect: VideoAspect,
    /// Linear zoom multiplier: `1.0` is the fitted image, `2.0` is 200%.
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub rotation_degrees: i64,
    pub fill_screen: bool,
    pub deinterlace: bool,
}

impl Default for VideoGeometry {
    fn default() -> Self {
        Self {
            aspect: VideoAspect::Auto,
            zoom: ZOOM_MIN,
            pan_x: 0.0,
            pan_y: 0.0,
            rotation_degrees: 0,
            fill_screen: false,
            deinterlace: false,
        }
    }
}

impl VideoGeometry {
    pub fn normalized(mut self) -> Self {
        self.zoom = finite_or(self.zoom, ZOOM_MIN).clamp(ZOOM_MIN, ZOOM_MAX);
        self.pan_x = rounded_pan(finite_or(self.pan_x, 0.0));
        self.pan_y = rounded_pan(finite_or(self.pan_y, 0.0));
        self.rotation_degrees = self.rotation_degrees.rem_euclid(360) / 90 * 90;
        if self.zoom <= ZOOM_MIN {
            self.zoom = ZOOM_MIN;
            self.pan_x = 0.0;
            self.pan_y = 0.0;
        }
        self
    }

    pub fn is_default(self) -> bool {
        self.normalized() == Self::default()
    }

    pub fn zoom_percent(self) -> i32 {
        (self.normalized().zoom * 100.0).round() as i32
    }

    /// mpv's `video-zoom` property uses a base-2 logarithm instead of a linear scale.
    pub fn mpv_zoom(self) -> f64 {
        self.normalized().zoom.log2()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn action_enabled(self, has_video: bool, action: VideoGeometryAction) -> bool {
        if !has_video {
            return false;
        }
        let state = self.normalized();
        match action {
            VideoGeometryAction::ZoomIn => state.zoom < ZOOM_MAX,
            VideoGeometryAction::ZoomOut => state.zoom > ZOOM_MIN,
            VideoGeometryAction::PanLeft => state.zoom > ZOOM_MIN && state.pan_x > PAN_MIN,
            VideoGeometryAction::PanRight => state.zoom > ZOOM_MIN && state.pan_x < PAN_MAX,
            VideoGeometryAction::PanUp => state.zoom > ZOOM_MIN && state.pan_y > PAN_MIN,
            VideoGeometryAction::PanDown => state.zoom > ZOOM_MIN && state.pan_y < PAN_MAX,
            VideoGeometryAction::Center => {
                state.zoom > ZOOM_MIN && (state.pan_x != 0.0 || state.pan_y != 0.0)
            }
            VideoGeometryAction::Reset => !state.is_default(),
            VideoGeometryAction::SetAspect(_)
            | VideoGeometryAction::RotateClockwise
            | VideoGeometryAction::ToggleFillScreen
            | VideoGeometryAction::ToggleDeinterlace => true,
        }
    }

    /// Apply one curated menu action. Returns whether the normalized state changed.
    pub fn apply(&mut self, action: VideoGeometryAction) -> bool {
        let before = self.normalized();
        let mut next = before;
        match action {
            VideoGeometryAction::SetAspect(aspect) => next.aspect = aspect,
            VideoGeometryAction::ZoomIn => next.zoom += ZOOM_STEP,
            VideoGeometryAction::ZoomOut => next.zoom -= ZOOM_STEP,
            VideoGeometryAction::PanLeft => next.pan_x -= PAN_STEP,
            VideoGeometryAction::PanRight => next.pan_x += PAN_STEP,
            VideoGeometryAction::PanUp => next.pan_y -= PAN_STEP,
            VideoGeometryAction::PanDown => next.pan_y += PAN_STEP,
            VideoGeometryAction::Center => {
                next.pan_x = 0.0;
                next.pan_y = 0.0;
            }
            VideoGeometryAction::RotateClockwise => {
                next.rotation_degrees = (next.rotation_degrees + 90).rem_euclid(360);
            }
            VideoGeometryAction::ToggleFillScreen => next.fill_screen = !next.fill_screen,
            VideoGeometryAction::ToggleDeinterlace => next.deinterlace = !next.deinterlace,
            VideoGeometryAction::Reset => next = Self::default(),
        }
        next = next.normalized();
        *self = next;
        next != before
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

fn rounded_pan(value: f64) -> f64 {
    ((value.clamp(PAN_MIN, PAN_MAX) / PAN_STEP).round() * PAN_STEP).clamp(PAN_MIN, PAN_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_presets_map_to_stable_labels_and_engine_values() {
        let expected = [
            (VideoAspect::Auto, "Auto", "no"),
            (VideoAspect::Wide, "16:9", "16:9"),
            (VideoAspect::Standard, "4:3", "4:3"),
            (VideoAspect::Cinema, "2.35:1", "2.35:1"),
        ];
        for (aspect, label, value) in expected {
            assert_eq!(aspect.label(), label);
            assert_eq!(aspect.mpv_value(), value);
        }
    }

    #[test]
    fn zoom_and_pan_actions_are_bounded_and_reset_pan_at_fit() {
        let mut geometry = VideoGeometry::default();
        assert!(!geometry.action_enabled(true, VideoGeometryAction::PanLeft));
        assert!(geometry.apply(VideoGeometryAction::ZoomIn));
        assert_eq!(geometry.zoom, 1.25);
        assert!(geometry.action_enabled(true, VideoGeometryAction::PanLeft));
        assert!(geometry.apply(VideoGeometryAction::PanLeft));
        assert_eq!(geometry.pan_x, -0.1);
        assert!(geometry.apply(VideoGeometryAction::PanUp));
        assert_eq!(geometry.pan_y, -0.1);

        assert!(geometry.apply(VideoGeometryAction::ZoomOut));
        assert_eq!(geometry.zoom, ZOOM_MIN);
        assert_eq!(geometry.pan_x, 0.0);
        assert_eq!(geometry.pan_y, 0.0);
        assert!(!geometry.action_enabled(true, VideoGeometryAction::ZoomOut));
    }

    #[test]
    fn menu_eligibility_reflects_media_and_current_limits() {
        let geometry = VideoGeometry::default();
        assert!(!geometry.action_enabled(false, VideoGeometryAction::RotateClockwise));
        assert!(geometry.action_enabled(true, VideoGeometryAction::ZoomIn));
        assert!(!geometry.action_enabled(true, VideoGeometryAction::ZoomOut));
        assert!(!geometry.action_enabled(true, VideoGeometryAction::Center));
        assert!(!geometry.action_enabled(true, VideoGeometryAction::Reset));

        let changed = VideoGeometry {
            zoom: ZOOM_MAX,
            pan_x: PAN_MAX,
            ..VideoGeometry::default()
        };
        assert!(!changed.action_enabled(true, VideoGeometryAction::ZoomIn));
        assert!(!changed.action_enabled(true, VideoGeometryAction::PanRight));
        assert!(changed.action_enabled(true, VideoGeometryAction::PanLeft));
        assert!(changed.action_enabled(true, VideoGeometryAction::Center));
        assert!(changed.action_enabled(true, VideoGeometryAction::Reset));
    }

    #[test]
    fn rotation_fill_deinterlace_and_reset_transition_as_one_model() {
        let mut geometry = VideoGeometry::default();
        geometry.apply(VideoGeometryAction::RotateClockwise);
        geometry.apply(VideoGeometryAction::ToggleFillScreen);
        geometry.apply(VideoGeometryAction::ToggleDeinterlace);
        assert_eq!(geometry.rotation_degrees, 90);
        assert!(geometry.fill_screen);
        assert!(geometry.deinterlace);

        assert!(geometry.apply(VideoGeometryAction::Reset));
        assert!(geometry.is_default());
    }

    #[test]
    fn persisted_values_are_normalized_before_use() {
        let geometry = VideoGeometry {
            zoom: f64::NAN,
            pan_x: 4.0,
            pan_y: -4.0,
            rotation_degrees: -91,
            ..VideoGeometry::default()
        }
        .normalized();

        assert_eq!(geometry.zoom, ZOOM_MIN);
        assert_eq!(geometry.pan_x, 0.0);
        assert_eq!(geometry.pan_y, 0.0);
        assert_eq!(geometry.rotation_degrees, 180);
    }

    #[test]
    fn linear_zoom_maps_to_mpv_logarithmic_zoom() {
        let geometry = VideoGeometry {
            zoom: 2.0,
            ..VideoGeometry::default()
        };
        assert_eq!(geometry.mpv_zoom(), 1.0);
    }
}
