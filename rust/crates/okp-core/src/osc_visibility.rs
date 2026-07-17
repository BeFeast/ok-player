//! Portable projection for the standard playback OSC's surface state.
//!
//! The platform shell supplies three facts: whether media is loaded, whether a
//! mutually exclusive surface such as compact mode suppresses the standard
//! OSC, and whether auto-hide currently reveals the controls. The projection
//! keeps the media-presence boundary distinct from the reveal animation: an
//! auto-hidden OSC remains mapped for motion, while an idle-canvas OSC is
//! absent from layout, focus traversal, and hit testing.

/// Effective surface state for the standard playback OSC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OscVisibility {
    pub visible: bool,
    pub mapped: bool,
    pub focusable: bool,
    pub hit_testable: bool,
}

impl OscVisibility {
    pub const HIDDEN: Self = Self {
        visible: false,
        mapped: false,
        focusable: false,
        hit_testable: false,
    };
}

/// Project media, surface, and reveal state into platform widget semantics.
#[must_use]
pub const fn project(has_media: bool, surface_suppressed: bool, revealed: bool) -> OscVisibility {
    let present = has_media && !surface_suppressed;
    let interactive = present && revealed;
    OscVisibility {
        visible: present,
        mapped: present,
        focusable: interactive,
        hit_testable: interactive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_lifecycle_maps_the_osc_once_and_unmaps_it_once() {
        let visible = OscVisibility {
            visible: true,
            mapped: true,
            focusable: true,
            hit_testable: true,
        };
        let states = [
            ("initial Welcome", false, OscVisibility::HIDDEN),
            ("Continue Watching", false, OscVisibility::HIDDEN),
            ("History", false, OscVisibility::HIDDEN),
            ("media loaded", true, visible),
            ("duplicate media poll", true, visible),
            ("Close Media", false, OscVisibility::HIDDEN),
        ];

        for (state, has_media, expected) in states {
            assert_eq!(project(has_media, false, true), expected, "{state}");
        }

        let mapping_changes = states
            .windows(2)
            .filter(|pair| pair[0].2.mapped != pair[1].2.mapped)
            .count();
        assert_eq!(mapping_changes, 2, "one load and one Close Media edge");
    }

    #[test]
    fn auto_hide_keeps_the_media_osc_mapped_but_noninteractive() {
        assert_eq!(
            project(true, false, false),
            OscVisibility {
                visible: true,
                mapped: true,
                focusable: false,
                hit_testable: false,
            }
        );
    }

    #[test]
    fn compact_mode_suppresses_the_standard_osc_even_with_media() {
        assert_eq!(project(true, true, true), OscVisibility::HIDDEN);
    }
}
