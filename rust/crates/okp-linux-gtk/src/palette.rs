//! The BeFeast brand palette shared by every OK Player shell (#791).
//!
//! `brand/palette.json` at the repository root is the one source of the
//! application's colours. It is embedded at build time and turned into GTK
//! `@define-color` entries for the stylesheet, plus Cairo colours for the few
//! surfaces drawn by hand. The shell keeps no hex copies of these values.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

use serde::Deserialize;

const PALETTE_JSON: &str = include_str!("../../../../brand/palette.json");

/// Semantic roles every scheme of the palette defines, in stylesheet order.
pub(crate) const ROLES: [&str; 16] = [
    "canvas",
    "surface",
    "surfaceRaised",
    "text",
    "textMuted",
    "border",
    "borderSubtle",
    "accent",
    "onAccent",
    "selected",
    "focus",
    "link",
    "success",
    "warning",
    "danger",
    "info",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub(crate) const ALL: [Scheme; 2] = [Scheme::Light, Scheme::Dark];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }
}

#[derive(Deserialize)]
struct PaletteFile {
    light: BTreeMap<String, String>,
    dark: BTreeMap<String, String>,
}

/// One colour, as the 8-bit channels `#RRGGBB` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rgb {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
}

impl Rgb {
    fn parse(hex: &str) -> Option<Self> {
        let digits = hex.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        Some(Self {
            red: channel(0)?,
            green: channel(2)?,
            blue: channel(4)?,
        })
    }

    /// Channels scaled to `0.0..=1.0` for Cairo.
    pub(crate) fn unit(self) -> (f64, f64, f64) {
        (
            f64::from(self.red) / 255.0,
            f64::from(self.green) / 255.0,
            f64::from(self.blue) / 255.0,
        )
    }
}

struct Palette {
    light: BTreeMap<&'static str, Rgb>,
    dark: BTreeMap<&'static str, Rgb>,
}

static PALETTE: LazyLock<Palette> = LazyLock::new(|| parse(PALETTE_JSON));

fn parse(json: &str) -> Palette {
    let file: PaletteFile =
        serde_json::from_str(json).expect("brand/palette.json must be valid palette JSON");
    let scheme = |name: &str, values: &BTreeMap<String, String>| {
        ROLES
            .iter()
            .map(|&role| {
                let hex = values
                    .get(role)
                    .unwrap_or_else(|| panic!("brand/palette.json {name} lacks `{role}`"));
                let rgb = Rgb::parse(hex).unwrap_or_else(|| {
                    panic!("brand/palette.json {name}.{role} is not #RRGGBB: {hex}")
                });
                (role, rgb)
            })
            .collect::<BTreeMap<_, _>>()
    };
    Palette {
        light: scheme("light", &file.light),
        dark: scheme("dark", &file.dark),
    }
}

/// The colour a scheme assigns to a semantic role from [`ROLES`].
pub(crate) fn color(scheme: Scheme, role: &str) -> Rgb {
    let values = match scheme {
        Scheme::Light => &PALETTE.light,
        Scheme::Dark => &PALETTE.dark,
    };
    *values
        .get(role)
        .unwrap_or_else(|| panic!("unknown palette role `{role}`"))
}

/// The GTK colour name the stylesheet uses for a role, e.g. `surfaceRaised`
/// in the dark scheme becomes `okp_dark_surface_raised`.
pub(crate) fn css_color_name(scheme: Scheme, role: &str) -> String {
    let mut name = format!("okp_{}_", scheme.key());
    for character in role.chars() {
        if character.is_ascii_uppercase() {
            name.push('_');
            name.push(character.to_ascii_lowercase());
        } else {
            name.push(character);
        }
    }
    name
}

/// `@define-color` entries for every role of both schemes.
pub(crate) fn css_definitions() -> String {
    let mut css = String::from("\n        /* Generated from brand/palette.json (palette.rs). */\n");
    for scheme in Scheme::ALL {
        for role in ROLES {
            let rgb = color(scheme, role);
            let _ = writeln!(
                css,
                "        @define-color {} #{:02X}{:02X}{:02X};",
                css_color_name(scheme, role),
                rgb.red,
                rgb.green,
                rgb.blue
            );
        }
    }
    css
}
