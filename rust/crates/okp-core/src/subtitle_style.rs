//! Subtitle appearance presets — port of `src/OkPlayer.Core/SubtitleStyle.cs`; the C# suite in
//! `tests/OkPlayer.Tests/SubtitleStyleTests.cs` is the executable spec. A preset is one of a
//! small, curated set of looks the user picks from in Settings → Subtitles: a fixed map of mpv
//! subtitle-style options, kept as pure data so the option set is unit-testable and one
//! definition drives both the engine (apply) and the UI label.
//!
//! These options style mpv's OWN text-subtitle renderer (SRT / plain text). ASS/SSA subtitles
//! carry their own embedded styling, which mpv respects by design (`sub-ass-override` defaults
//! to `scale`), so a preset deliberately does NOT repaint them — the same asymmetry that forced
//! the OSC lift onto `sub-pos` rather than `sub-margin-y` (see [`crate::subtitle_lift`]). Every
//! preset sets the SAME set of options, so switching presets fully overrides the previous one
//! with no residual state.

/// A named subtitle appearance preset. All presets are `'static`; obtain one from [`ALL`] or
/// [`from_key`].
#[derive(Debug, PartialEq, Eq)]
pub struct SubtitleStyle {
    /// Stable identifier persisted in settings (never localized). The Core model only owns the
    /// key and the option set; display text is the shell's concern.
    pub key: &'static str,
    /// Ordered mpv option → value pairs. Applied via set-property at engine init and on a live
    /// settings change. Colors are `#RRGGBB` (fully opaque) — the universally-accepted mpv color
    /// form, so they parse identically regardless of whether a build expects a leading alpha
    /// byte.
    pub options: &'static [(&'static str, &'static str)],
}

// The exact set of options every preset writes. Listing all six in each preset (rather than only
// the ones that differ from mpv's defaults) is deliberate: switching from any preset to any
// other then restores every field, so e.g. going Classic → Default actually repaints yellow back
// to white.
pub static DEFAULT: SubtitleStyle = SubtitleStyle {
    key: "Default",
    options: &[
        ("sub-color", "#FFFFFF"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "3"),
        ("sub-shadow-offset", "0"),
        ("sub-shadow-color", "#000000"),
        ("sub-bold", "no"),
    ],
};

pub static BOLD: SubtitleStyle = SubtitleStyle {
    key: "Bold",
    options: &[
        ("sub-color", "#FFFFFF"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "3.2"),
        ("sub-shadow-offset", "0"),
        ("sub-shadow-color", "#000000"),
        ("sub-bold", "yes"),
    ],
};

pub static CLASSIC: SubtitleStyle = SubtitleStyle {
    key: "Classic",
    options: &[
        ("sub-color", "#FFFF00"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "3"),
        ("sub-shadow-offset", "0"),
        ("sub-shadow-color", "#000000"),
        ("sub-bold", "no"),
    ],
};

pub static CONTRAST: SubtitleStyle = SubtitleStyle {
    key: "Contrast",
    options: &[
        ("sub-color", "#FFFFFF"),
        ("sub-border-color", "#000000"),
        ("sub-border-size", "4"),
        ("sub-shadow-offset", "1.5"),
        ("sub-shadow-color", "#000000"),
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
        assert_eq!(6, expected.len());
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
    fn all_colors_use_six_digit_rrggbb_the_universally_accepted_mpv_form() {
        // Colour-valued options must be #RRGGBB (no alpha byte), so they parse identically
        // regardless of whether a libmpv build expects a leading alpha. Catches a stray
        // #AARRGGBB / #RRGGBBAA slipping in.
        let color_keys = ["sub-color", "sub-border-color", "sub-shadow-color"];
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
}
