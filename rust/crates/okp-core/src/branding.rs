//! Canonical OK Player identity geometry and icon-size hierarchy.

pub const MARK_VIEWBOX_WIDTH: f64 = 176.0;
pub const MARK_VIEWBOX_HEIGHT: f64 = 96.0;
pub const MARK_O_CENTER: (f64, f64) = (46.0, 48.0);
pub const MARK_O_RADIUS: f64 = 33.0;
pub const MARK_STEM_Y: f64 = 12.0;
pub const MARK_STEM_HEIGHT: f64 = 72.0;
pub const MARK_STEM_RADIUS: f64 = 4.0;
pub const MARK_TRIANGLE_X: f64 = 111.0;
pub const MARK_TRIANGLE_APEX_Y: f64 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FullMarkGeometry {
    pub o_stroke: f64,
    pub stem_x: f64,
    pub stem_width: f64,
    pub triangle_top: f64,
    pub triangle_bottom: f64,
    pub triangle_apex_x: f64,
    pub triangle_stroke: f64,
}

pub const CANONICAL_FULL_MARK: FullMarkGeometry = FullMarkGeometry {
    o_stroke: 15.0,
    stem_x: 92.0,
    stem_width: 15.0,
    triangle_top: 14.0,
    triangle_bottom: 82.0,
    triangle_apex_x: 161.0,
    triangle_stroke: 6.0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppIconMark {
    Full,
    PlayOnly,
}

pub const fn app_icon_mark(size: u32) -> AppIconMark {
    if size <= 24 {
        AppIconMark::PlayOnly
    } else {
        AppIconMark::Full
    }
}

pub const fn full_mark_for_icon_size(size: u32) -> FullMarkGeometry {
    match size {
        32 => FullMarkGeometry {
            o_stroke: 18.0,
            stem_x: 91.0,
            stem_width: 18.0,
            triangle_top: 11.0,
            triangle_bottom: 85.0,
            triangle_apex_x: 164.0,
            triangle_stroke: 9.0,
        },
        48 => FullMarkGeometry {
            o_stroke: 17.0,
            stem_x: 91.0,
            stem_width: 17.0,
            triangle_top: 12.0,
            triangle_bottom: 84.0,
            triangle_apex_x: 163.0,
            triangle_stroke: 8.0,
        },
        64 => FullMarkGeometry {
            o_stroke: 16.0,
            stem_x: 92.0,
            stem_width: 16.0,
            triangle_top: 13.0,
            triangle_bottom: 83.0,
            triangle_apex_x: 162.0,
            triangle_stroke: 7.0,
        },
        _ => CANONICAL_FULL_MARK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_mark_matches_the_branding_construction() {
        assert_eq!((MARK_VIEWBOX_WIDTH, MARK_VIEWBOX_HEIGHT), (176.0, 96.0));
        assert_eq!(MARK_O_CENTER, (46.0, 48.0));
        assert_eq!(MARK_O_RADIUS, 33.0);
        assert_eq!(MARK_STEM_Y, 12.0);
        assert_eq!(MARK_STEM_HEIGHT, 72.0);
        assert_eq!(MARK_STEM_RADIUS, 4.0);
        assert_eq!(MARK_TRIANGLE_X, 111.0);
        assert_eq!(MARK_TRIANGLE_APEX_Y, 48.0);
        assert_eq!(CANONICAL_FULL_MARK.o_stroke, 15.0);
        assert_eq!(CANONICAL_FULL_MARK.stem_x, 92.0);
        assert_eq!(CANONICAL_FULL_MARK.stem_width, 15.0);
        assert_eq!(CANONICAL_FULL_MARK.triangle_top, 14.0);
        assert_eq!(CANONICAL_FULL_MARK.triangle_bottom, 82.0);
        assert_eq!(CANONICAL_FULL_MARK.triangle_apex_x, 161.0);
        assert_eq!(CANONICAL_FULL_MARK.triangle_stroke, 6.0);
    }

    #[test]
    fn icon_hierarchy_keeps_the_full_mark_above_24_pixels() {
        assert_eq!(app_icon_mark(64), AppIconMark::Full);
        assert_eq!(app_icon_mark(48), AppIconMark::Full);
        assert_eq!(app_icon_mark(32), AppIconMark::Full);
        assert_eq!(app_icon_mark(24), AppIconMark::PlayOnly);
        assert_eq!(app_icon_mark(16), AppIconMark::PlayOnly);
    }

    #[test]
    fn exact_small_full_mark_weights_match_the_branding_artifact() {
        let mark_64 = full_mark_for_icon_size(64);
        assert_eq!((mark_64.o_stroke, mark_64.stem_width), (16.0, 16.0));
        assert_eq!(
            (
                mark_64.triangle_top,
                mark_64.triangle_bottom,
                mark_64.triangle_apex_x,
                mark_64.triangle_stroke,
            ),
            (13.0, 83.0, 162.0, 7.0)
        );

        let mark_48 = full_mark_for_icon_size(48);
        assert_eq!((mark_48.o_stroke, mark_48.stem_x), (17.0, 91.0));
        assert_eq!(
            (
                mark_48.triangle_top,
                mark_48.triangle_bottom,
                mark_48.triangle_apex_x,
                mark_48.triangle_stroke,
            ),
            (12.0, 84.0, 163.0, 8.0)
        );

        let mark_32 = full_mark_for_icon_size(32);
        assert_eq!((mark_32.o_stroke, mark_32.stem_width), (18.0, 18.0));
        assert_eq!(
            (
                mark_32.triangle_top,
                mark_32.triangle_bottom,
                mark_32.triangle_apex_x,
                mark_32.triangle_stroke,
            ),
            (11.0, 85.0, 164.0, 9.0)
        );
    }
}
