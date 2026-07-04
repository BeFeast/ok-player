//! Screenshot capture: output format plus collision-safe filename generation.
//!
//! This is the portable naming/format logic both shells converge on (freeze-boundary: no
//! shell owns filename or format rules). Path *resolution* — the XDG Pictures directory,
//! the configured save folder, the temp dir for a clipboard frame — stays behind the shell
//! seam, because it touches the environment and the filesystem. This module stays pure: it
//! composes a name and resolves collisions against an injected existence probe, so it is
//! unit-testable without a real filesystem.
//!
//! Capture itself is done by libmpv's `screenshot-to-file` against the *decoded* frame
//! (mode `video` / `subtitles`), never a screen grab — so a confirmation toast or any other
//! shell overlay can never appear in the captured image. The `subtitles` mode burns in the
//! rendered captions; the `video` mode omits them, so the clean and with-subtitle actions
//! produce distinct expected files. The format below drives only the filename extension,
//! which is how libmpv selects its encoder.

/// The output image format for a saved screenshot. libmpv picks its encoder from the
/// filename extension, so each variant maps 1:1 to a writable file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenshotFormat {
    /// Lossless PNG — the default, matching the historical Linux behaviour.
    #[default]
    Png,
    /// Lossy JPEG — smaller files, no alpha.
    Jpeg,
    /// WebP — modern, smaller than PNG at comparable quality.
    WebP,
}

impl ScreenshotFormat {
    /// Every format, in the order the settings toggle cycles through them.
    pub const ALL: [ScreenshotFormat; 3] = [Self::Png, Self::Jpeg, Self::WebP];

    /// Parse the canonical settings token, tolerating the common aliases an older document
    /// or a hand-edited file might carry (`jpg`/`jpeg`, any case). Anything unrecognized —
    /// including absent — falls back to the default PNG, so a bad value never breaks capture.
    pub fn from_settings_value(value: Option<&str>) -> Self {
        match value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("jpg" | "jpeg") => Self::Jpeg,
            Some("webp") => Self::WebP,
            _ => Self::Png,
        }
    }

    /// The canonical token persisted in settings (also the filename extension).
    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
        }
    }

    /// The filename extension (without the dot) libmpv keys its encoder on.
    pub fn extension(self) -> &'static str {
        self.settings_value()
    }

    /// A human-readable label for the settings UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::WebP => "WebP",
        }
    }

    /// The next format in the cycle, so a single control can step through all three.
    pub fn next(self) -> Self {
        match self {
            Self::Png => Self::Jpeg,
            Self::Jpeg => Self::WebP,
            Self::WebP => Self::Png,
        }
    }
}

/// Characters kept from a sanitized media stem before the position/timestamp are appended,
/// so a very long title can't push the final name past a filesystem's per-name limit.
const MAX_STEM_CHARS: usize = 80;

/// Base used when the media has no usable stem (a stream URL, or a name that sanitizes away
/// to nothing).
pub const FALLBACK_BASE: &str = "ok-player";

/// Upper bound on collision-breaking suffixes tried before giving up. The composed stem
/// already carries a millisecond timestamp, so a genuine collision is astronomically
/// unlikely; this only bounds the probe loop and guarantees termination.
const MAX_COLLISION_SUFFIX: u32 = 1000;

/// Sanitize a media stem into a filesystem-safe fragment: replace path punctuation and
/// control characters with `-`, trim leading/trailing spaces, dots and dashes, and cap the
/// length. Returns an empty string when nothing survives — the caller decides the fallback
/// (see [`screenshot_stem`]).
pub fn sanitize_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            ch if ch.is_control() => '-',
            ch => ch,
        })
        .collect::<String>();

    sanitized
        .trim_matches(|ch| matches!(ch, ' ' | '.' | '-'))
        .chars()
        .take(MAX_STEM_CHARS)
        .collect()
}

/// Format a media position as a compact time slug (`01h05m06s` / `53m42s`), used to anchor a
/// capture to the frame it came from.
pub fn time_slug(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}h{minutes:02}m{seconds:02}s")
    } else {
        format!("{minutes:02}m{seconds:02}s")
    }
}

/// Compose the base file *stem* (no extension) for a capture: `base[-position]-timestamp`.
/// `media_stem` is the source file's stem (already extracted by the shell); a non-finite or
/// negative position is dropped. The trailing millisecond `timestamp` makes the stem unique
/// in practice, so [`resolve_unique_name`] almost never has to add a numeric suffix.
pub fn screenshot_stem(media_stem: Option<&str>, position: Option<f64>, timestamp: u128) -> String {
    let base = media_stem
        .map(sanitize_filename)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| FALLBACK_BASE.to_owned());
    let position = position
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| format!("-{}", time_slug(value)))
        .unwrap_or_default();
    format!("{base}{position}-{timestamp}")
}

/// Resolve a collision-free file name for `stem`.`extension`, probing `exists` for each
/// candidate: `stem.ext`, then `stem-1.ext`, `stem-2.ext`, … up to [`MAX_COLLISION_SUFFIX`].
/// Returns the first free name, or `None` if every candidate is taken. The caller must treat
/// `None` as a failure and surface it — a capture never overwrites an existing file.
pub fn resolve_unique_name(
    stem: &str,
    extension: &str,
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    for suffix in 0..=MAX_COLLISION_SUFFIX {
        let name = if suffix == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{suffix}.{extension}")
        };
        if !exists(&name) {
            return Some(name);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn format_parses_tokens_and_aliases_case_insensitively() {
        assert_eq!(
            ScreenshotFormat::from_settings_value(None),
            ScreenshotFormat::Png
        );
        assert_eq!(
            ScreenshotFormat::from_settings_value(Some("png")),
            ScreenshotFormat::Png
        );
        assert_eq!(
            ScreenshotFormat::from_settings_value(Some(" JPG ")),
            ScreenshotFormat::Jpeg
        );
        assert_eq!(
            ScreenshotFormat::from_settings_value(Some("jpeg")),
            ScreenshotFormat::Jpeg
        );
        assert_eq!(
            ScreenshotFormat::from_settings_value(Some("WebP")),
            ScreenshotFormat::WebP
        );
        // Unknown falls back to the default rather than breaking capture.
        assert_eq!(
            ScreenshotFormat::from_settings_value(Some("gif")),
            ScreenshotFormat::Png
        );
    }

    #[test]
    fn format_round_trips_through_its_settings_token() {
        for format in ScreenshotFormat::ALL {
            assert_eq!(
                ScreenshotFormat::from_settings_value(Some(format.settings_value())),
                format
            );
            assert_eq!(format.extension(), format.settings_value());
        }
    }

    #[test]
    fn format_cycle_visits_every_variant_and_returns() {
        assert_eq!(ScreenshotFormat::Png.next(), ScreenshotFormat::Jpeg);
        assert_eq!(ScreenshotFormat::Jpeg.next(), ScreenshotFormat::WebP);
        assert_eq!(ScreenshotFormat::WebP.next(), ScreenshotFormat::Png);
    }

    #[test]
    fn sanitize_filename_replaces_path_punctuation_and_trims() {
        assert_eq!(
            sanitize_filename("  Movie: Cut/Scene?.mkv  "),
            "Movie- Cut-Scene-.mkv"
        );
        // Nothing survives -> empty; the fallback is applied by screenshot_stem.
        assert_eq!(sanitize_filename("...---"), "");
        assert_eq!(sanitize_filename(""), "");
    }

    #[test]
    fn sanitize_filename_caps_length() {
        let long = "a".repeat(200);
        assert_eq!(sanitize_filename(&long).chars().count(), MAX_STEM_CHARS);
    }

    #[test]
    fn time_slug_formats_media_positions() {
        assert_eq!(time_slug(53.2), "00m53s");
        assert_eq!(time_slug(3222.0), "53m42s");
        assert_eq!(time_slug(3906.0), "01h05m06s");
    }

    #[test]
    fn screenshot_stem_composes_base_position_and_timestamp() {
        assert_eq!(
            screenshot_stem(Some("Movie"), Some(83.0), 1_700_000_000_000),
            "Movie-01m23s-1700000000000"
        );
    }

    #[test]
    fn screenshot_stem_falls_back_when_the_stem_is_unusable() {
        // No stem (a stream), and a stem that sanitizes away, both use the fallback base.
        assert_eq!(screenshot_stem(None, None, 42), "ok-player-42");
        assert_eq!(screenshot_stem(Some("...---"), None, 42), "ok-player-42");
    }

    #[test]
    fn screenshot_stem_drops_a_non_finite_or_negative_position() {
        assert_eq!(screenshot_stem(Some("Clip"), Some(-1.0), 42), "Clip-42");
        assert_eq!(screenshot_stem(Some("Clip"), Some(f64::NAN), 42), "Clip-42");
    }

    #[test]
    fn resolve_unique_name_uses_the_bare_name_when_free() {
        assert_eq!(
            resolve_unique_name("shot", "png", |_| false),
            Some("shot.png".to_owned())
        );
    }

    #[test]
    fn resolve_unique_name_adds_the_first_free_numeric_suffix() {
        let taken: HashSet<&str> = ["shot.png", "shot-1.png", "shot-2.png"]
            .into_iter()
            .collect();
        assert_eq!(
            resolve_unique_name("shot", "png", |name| taken.contains(name)),
            Some("shot-3.png".to_owned())
        );
    }

    #[test]
    fn resolve_unique_name_returns_none_rather_than_overwriting() {
        // Every candidate is taken: the resolver refuses to overwrite and reports failure.
        assert_eq!(resolve_unique_name("shot", "jpg", |_| true), None);
    }

    #[test]
    fn resolve_unique_name_honours_the_requested_extension() {
        assert_eq!(
            resolve_unique_name("frame-1700000000000", "webp", |_| false),
            Some("frame-1700000000000.webp".to_owned())
        );
    }
}
