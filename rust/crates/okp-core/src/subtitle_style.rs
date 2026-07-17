//! Subtitle appearance presets — port of `src/OkPlayer.Core/SubtitleStyle.cs`; the C# suite in
//! `tests/OkPlayer.Tests/SubtitleStyleTests.cs` is the executable spec. A preset is one of a
//! small, curated set of looks the user picks from in Settings → Subtitles: a fixed map of mpv
//! subtitle-style options, kept as pure data so the option set is unit-testable and one
//! definition drives both the engine (apply) and the UI label. Size and vertical position are
//! curated alongside the appearance preset, so their normalization also lives here rather than
//! in either desktop shell.
//!
//! These options style mpv's OWN text-subtitle renderer (SRT / plain text). ASS/SSA subtitles
//! carry their own embedded styling. The engine adapter pins `sub-ass-override=scale` (and the
//! secondary equivalent), which deliberately preserves authored fonts, colors, inline layout,
//! and signs while still allowing OK Player's explicit size/position controls. A preset therefore
//! does NOT repaint ASS/SSA — the same asymmetry that forced the OSC lift onto `sub-pos` rather
//! than `sub-margin-y` (see [`crate::subtitle_lift`]). Every preset sets the SAME set of options,
//! so switching presets fully overrides the previous one with no residual state.

/// A named subtitle appearance preset. All presets are `'static`; obtain one from [`ALL`] or
/// [`from_key`].
#[derive(Debug, PartialEq, Eq)]
pub struct SubtitleStyle {
    /// Stable identifier persisted in settings (never localized). The Core model only owns the
    /// key and the option set; display text is the shell's concern.
    pub key: &'static str,
    /// Ordered mpv option → value pairs. Applied via set-property at engine init and on a live
    /// settings change. Opaque colors are `#RRGGBB`; the boxed preset uses mpv's documented
    /// `r/g/b/a` form because it is the portable way to request a semi-transparent background.
    pub options: &'static [(&'static str, &'static str)],
}

pub const DEFAULT_SCALE: f64 = 1.0;
pub const MIN_SCALE: f64 = 0.25;
pub const MAX_SCALE: f64 = 4.0;
pub const DEFAULT_POSITION: i64 = 100;
pub const MIN_POSITION: i64 = 0;
pub const MAX_POSITION: i64 = 100;

// The exact set of options every preset writes. Listing all seven in each preset (rather than only
// the ones that differ from mpv's defaults) is deliberate: switching from any preset to any
// other then restores every field, so e.g. going Classic → Default actually repaints yellow back
// to white and going Contrast → Default removes the background box.
pub static DEFAULT: SubtitleStyle = SubtitleStyle {
    key: "Default",
    options: &[
        ("sub-color", "#FFFFFF"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "3"),
        ("sub-border-style", "outline-and-shadow"),
        ("sub-shadow-offset", "0"),
        ("sub-back-color", "#000000"),
        ("sub-bold", "no"),
    ],
};

pub static BOLD: SubtitleStyle = SubtitleStyle {
    key: "Bold",
    options: &[
        ("sub-color", "#FFFFFF"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "3.2"),
        ("sub-border-style", "outline-and-shadow"),
        ("sub-shadow-offset", "0"),
        ("sub-back-color", "#000000"),
        ("sub-bold", "yes"),
    ],
};

pub static CLASSIC: SubtitleStyle = SubtitleStyle {
    key: "Classic",
    options: &[
        ("sub-color", "#FFFF00"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "3"),
        ("sub-border-style", "outline-and-shadow"),
        ("sub-shadow-offset", "0"),
        ("sub-back-color", "#000000"),
        ("sub-bold", "no"),
    ],
};

pub static CONTRAST: SubtitleStyle = SubtitleStyle {
    key: "Contrast",
    options: &[
        ("sub-color", "#FFFFFF"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "2"),
        ("sub-border-style", "background-box"),
        ("sub-shadow-offset", "4"),
        ("sub-back-color", "0.0/0.0/0.0/0.72"),
        ("sub-bold", "no"),
    ],
};

/// All presets in display order (the order the Settings buttons render).
pub static ALL: [&SubtitleStyle; 4] = [&DEFAULT, &BOLD, &CLASSIC, &CONTRAST];

/// The preset for a stored key, falling back to [`struct@DEFAULT`] for an unknown, empty, or
/// absent key so settings written by another version (or a hand-edited file) degrade gracefully.
pub fn from_key(key: Option<&str>) -> &'static SubtitleStyle {
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        for style in ALL {
            if style.key.eq_ignore_ascii_case(key) {
                return style;
            }
        }
    }
    &DEFAULT
}

/// The next preset in display order, wrapping from the last preset back to the default. Used by
/// the compact subtitle switcher's one-click Style shortcut.
pub fn next(style: &SubtitleStyle) -> &'static SubtitleStyle {
    let index = ALL
        .iter()
        .position(|candidate| std::ptr::eq(*candidate, style))
        .unwrap_or(0);
    ALL[(index + 1) % ALL.len()]
}

/// Resolve a persisted subtitle scale to a finite value accepted by mpv.
pub fn normalized_scale(scale: Option<f64>) -> f64 {
    scale
        .filter(|scale| scale.is_finite())
        .unwrap_or(DEFAULT_SCALE)
        .clamp(MIN_SCALE, MAX_SCALE)
}

/// Resolve a persisted `sub-pos` value to mpv's valid percentage range.
pub fn normalized_position(position: Option<i64>) -> i64 {
    position
        .unwrap_or(DEFAULT_POSITION)
        .clamp(MIN_POSITION, MAX_POSITION)
}

/// Whether an mpv option is owned by the curated subtitle presentation surface. Raw `mpv.conf`
/// remains available for advanced options, but it must not silently override a Settings choice.
pub fn is_managed_option(name: &str) -> bool {
    name.eq_ignore_ascii_case("sub-ass-override")
        || name.eq_ignore_ascii_case("secondary-sub-ass-override")
        || name.eq_ignore_ascii_case("sub-scale")
        || name.eq_ignore_ascii_case("sub-pos")
        || ["sub-outline-color", "sub-outline-size", "sub-shadow-color"]
            .iter()
            .any(|alias| name.eq_ignore_ascii_case(alias))
        || DEFAULT
            .options
            .iter()
            .any(|(option, _)| name.eq_ignore_ascii_case(option))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option_value(style: &SubtitleStyle, name: &str) -> Option<&'static str> {
        style
            .options
            .iter()
            .find(|(option, _)| *option == name)
            .map(|(_, value)| *value)
    }

    #[test]
    fn all_lists_the_four_presets_with_unique_keys() {
        let keys = ALL.map(|style| style.key);
        assert_eq!(["Default", "Bold", "Classic", "Contrast"], keys);
        for (i, key) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(key), "duplicate key {key}");
        }
    }

    #[test]
    fn every_preset_writes_the_same_option_names_so_switching_fully_overrides() {
        // The whole design rests on this: because every preset sets the identical set of options,
        // switching from any preset to any other repaints every field and leaves no residual
        // state (e.g. Classic's yellow can't linger after picking Default). If a preset adds or
        // drops an option, that breaks.
        let mut expected: Vec<&str> = DEFAULT.options.iter().map(|(name, _)| *name).collect();
        expected.sort_unstable();
        assert_eq!(7, expected.len());
        for style in ALL {
            let mut names: Vec<&str> = style.options.iter().map(|(name, _)| *name).collect();
            names.sort_unstable();
            assert_eq!(expected, names, "preset {}", style.key);
            // No option set twice within a preset (a later value would silently win).
            for (i, name) in names.iter().enumerate() {
                assert!(
                    !names[i + 1..].contains(name),
                    "option {name} set twice in {}",
                    style.key
                );
            }
        }
    }

    #[test]
    fn from_key_resolves_known_keys() {
        for (key, expected) in [
            ("Default", "Default"),
            ("Bold", "Bold"),
            ("Classic", "Classic"),
            ("Contrast", "Contrast"),
            ("classic", "Classic"), // case-insensitive
            ("CONTRAST", "Contrast"),
        ] {
            assert_eq!(expected, from_key(Some(key)).key);
        }
    }

    #[test]
    fn from_key_falls_back_to_default_for_unknown_or_empty() {
        for key in [None, Some(""), Some("nonsense")] {
            assert!(std::ptr::eq(from_key(key), &DEFAULT), "key {key:?}");
        }
    }

    #[test]
    fn default_preset_is_the_white_unbolded_baseline() {
        assert_eq!(Some("#FFFFFF"), option_value(&DEFAULT, "sub-color"));
        assert_eq!(Some("no"), option_value(&DEFAULT, "sub-bold"));
    }

    #[test]
    fn classic_preset_is_yellow() {
        // Mirrors the C# integration test's pixel assertion at the data level: Classic must
        // request yellow text.
        assert_eq!(Some("#FFFF00"), option_value(&CLASSIC, "sub-color"));
    }

    #[test]
    fn contrast_preset_uses_a_semi_transparent_background_box() {
        assert_eq!(
            Some("background-box"),
            option_value(&CONTRAST, "sub-border-style")
        );
        assert_eq!(
            Some("0.0/0.0/0.0/0.72"),
            option_value(&CONTRAST, "sub-back-color")
        );
    }

    #[test]
    fn non_boxed_presets_restore_outline_and_shadow() {
        for style in [&DEFAULT, &BOLD, &CLASSIC] {
            assert_eq!(
                Some("outline-and-shadow"),
                option_value(style, "sub-border-style")
            );
            assert_eq!(Some("#000000"), option_value(style, "sub-back-color"));
        }
    }

    #[test]
    fn opaque_colors_use_six_digit_rrggbb() {
        let color_keys = ["sub-color", "sub-border-color"];
        for style in ALL {
            for (name, value) in style.options {
                if color_keys.contains(name) {
                    let hex = value.strip_prefix('#').unwrap_or_else(|| {
                        panic!("{}.{name} = {value} must start with '#'", style.key)
                    });
                    assert!(
                        hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()),
                        "{}.{name} = {value} must be #RRGGBB",
                        style.key
                    );
                }
            }
        }
    }

    #[test]
    fn next_cycles_in_display_order_and_wraps() {
        assert!(std::ptr::eq(next(&DEFAULT), &BOLD));
        assert!(std::ptr::eq(next(&BOLD), &CLASSIC));
        assert!(std::ptr::eq(next(&CLASSIC), &CONTRAST));
        assert!(std::ptr::eq(next(&CONTRAST), &DEFAULT));
    }

    #[test]
    fn presentation_values_default_and_clamp_safely() {
        assert_eq!(DEFAULT_SCALE, normalized_scale(None));
        assert_eq!(DEFAULT_SCALE, normalized_scale(Some(f64::NAN)));
        assert_eq!(MIN_SCALE, normalized_scale(Some(-1.0)));
        assert_eq!(MAX_SCALE, normalized_scale(Some(9.0)));
        assert_eq!(1.4, normalized_scale(Some(1.4)));

        assert_eq!(DEFAULT_POSITION, normalized_position(None));
        assert_eq!(MIN_POSITION, normalized_position(Some(-20)));
        assert_eq!(MAX_POSITION, normalized_position(Some(120)));
        assert_eq!(90, normalized_position(Some(90)));
    }

    #[test]
    fn managed_options_cover_size_position_and_every_style_field() {
        assert!(is_managed_option("sub-scale"));
        assert!(is_managed_option("SUB-POS"));
        assert!(is_managed_option("sub-ass-override"));
        assert!(is_managed_option("SECONDARY-SUB-ASS-OVERRIDE"));
        assert!(is_managed_option("sub-outline-size"));
        assert!(is_managed_option("sub-shadow-color"));
        for (name, _) in DEFAULT.options {
            assert!(is_managed_option(name), "{name} should be protected");
        }
        assert!(!is_managed_option("sub-font"));
        assert!(!is_managed_option("profile"));
    }
}
