//! Classify a subtitle track's codec as text- or image-based, and decide whether
//! the curated text appearance presets apply to it. mpv renders image (bitmap)
//! subtitles — PGS (Blu-ray), VobSub (DVD), DVB, XSUB — from pre-rendered
//! graphics, so the text-styling options a preset writes (colour, border,
//! background box) have nothing to act on, exactly like the authored styling of
//! an ASS/SSA track. PRD P2-S19 lists these formats as *Later*, but even without
//! full styling the shells must not present them as broken text tracks or imply a
//! preset will restyle them.
//!
//! This classification is pure and UI-free so it lives here (freeze-boundary):
//! the Linux GTK shell's subtitle picker, the media-info surface, and the mpv
//! wrapper that builds the media-info detail all share one rule instead of each
//! re-deriving the codec list. There is no `OkPlayer.Core` counterpart yet — this
//! is Linux-lane logic a future Windows port can share verbatim.
//!
//! mpv exposes the codec as the ffmpeg short name (e.g. `hdmv_pgs_subtitle`); we
//! match the known bitmap families case-insensitively and treat everything else —
//! SubRip, WebVTT, ASS/SSA, an unknown or absent codec — as text for the coarse
//! rendering classification. Appearance-preset applicability is stricter: it
//! distinguishes ASS/SSA native styling and leaves unknown metadata unsupported
//! rather than promising that curated text styling will work.

/// How mpv presents a subtitle track's cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// Text cues mpv renders itself (SubRip, WebVTT) or via libass (ASS/SSA).
    Text,
    /// Pre-rendered bitmap graphics (PGS, VobSub, DVB, XSUB).
    Image,
}

/// Format families that affect OK Player's curated appearance preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitlePresetFormat {
    /// SubRip/plain SRT text rendered with the curated preset.
    SubRip,
    /// WebVTT text rendered with the curated preset.
    WebVtt,
    /// Another known plain-text subtitle codec rendered by mpv's text renderer.
    OtherText,
    /// Advanced SubStation Alpha with authored native styling.
    Ass,
    /// SubStation Alpha with authored native styling.
    Ssa,
    /// Pre-rendered bitmap graphics (PGS, VobSub, DVB, XSUB).
    Image,
    /// Missing or unrecognized metadata. Preset support cannot be promised.
    Unknown,
}

impl SubtitlePresetFormat {
    /// Short user-facing format name when the family has a stable label.
    #[must_use]
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::SubRip => Some("SRT"),
            Self::WebVtt => Some("WebVTT"),
            Self::Ass => Some("ASS"),
            Self::Ssa => Some("SSA"),
            Self::OtherText | Self::Image | Self::Unknown => None,
        }
    }
}

/// Whether a selected subtitle can use OK Player's appearance preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitlePresetApplicability {
    /// A known text track rendered with the curated preset.
    Applies(SubtitlePresetFormat),
    /// ASS/SSA owns its authored styling; OK Player preserves it.
    NativeStyle(SubtitlePresetFormat),
    /// The format is image-based or unknown, so preset support cannot be claimed.
    Unsupported(SubtitlePresetFormat),
    /// No primary subtitle is selected.
    NoActiveTrack,
}

impl SubtitleFormat {
    /// Whether this is an image (bitmap) format.
    pub fn is_image(self) -> bool {
        matches!(self, SubtitleFormat::Image)
    }
}

/// Classify a subtitle codec. A codec matching a known bitmap family is
/// [`SubtitleFormat::Image`]; anything else — including `None` — is
/// [`SubtitleFormat::Text`].
pub fn subtitle_format(codec: Option<&str>) -> SubtitleFormat {
    if image_format_name(codec).is_some() {
        SubtitleFormat::Image
    } else {
        SubtitleFormat::Text
    }
}

/// Whether a subtitle codec is image (bitmap) based.
pub fn is_image_subtitle(codec: Option<&str>) -> bool {
    subtitle_format(codec).is_image()
}

/// Whether the curated text *appearance* presets (colour, border, background
/// box) can restyle a track. `false` for image subtitles, whose bitmaps mpv
/// draws verbatim. Size and vertical position are a separate concern and still
/// apply to image tracks, so this gate is only about the appearance preset.
pub fn appearance_presets_apply(codec: Option<&str>) -> bool {
    matches!(
        preset_applicability(codec, None),
        SubtitlePresetApplicability::Applies(_)
    )
}

/// Classify the subtitle format for appearance-preset behavior. The external
/// filename extension wins for `.ass`/`.ssa` because FFmpeg commonly reports
/// both as codec `ass`; this preserves the actual user-visible format.
#[must_use]
pub fn preset_format(codec: Option<&str>, external_filename: Option<&str>) -> SubtitlePresetFormat {
    match external_extension(external_filename).as_deref() {
        Some("ass") => return SubtitlePresetFormat::Ass,
        Some("ssa") => return SubtitlePresetFormat::Ssa,
        _ => {}
    }

    if is_image_subtitle(codec) {
        return SubtitlePresetFormat::Image;
    }

    let codec = clean(codec).map(|value| value.to_ascii_lowercase());
    match codec.as_deref() {
        Some("ass") => SubtitlePresetFormat::Ass,
        Some("ssa") => SubtitlePresetFormat::Ssa,
        Some("subrip" | "srt") => SubtitlePresetFormat::SubRip,
        Some("webvtt" | "vtt") => SubtitlePresetFormat::WebVtt,
        Some(
            "text" | "mov_text" | "microdvd" | "subviewer" | "subviewer1" | "jacosub" | "sami"
            | "realtext" | "stl" | "eia_608" | "eia_708",
        ) => SubtitlePresetFormat::OtherText,
        _ => match external_extension(external_filename).as_deref() {
            Some("srt") => SubtitlePresetFormat::SubRip,
            Some("vtt" | "webvtt") => SubtitlePresetFormat::WebVtt,
            _ => SubtitlePresetFormat::Unknown,
        },
    }
}

/// Resolve the honest preset state from raw mpv track metadata.
#[must_use]
pub fn preset_applicability(
    codec: Option<&str>,
    external_filename: Option<&str>,
) -> SubtitlePresetApplicability {
    let format = preset_format(codec, external_filename);
    match format {
        SubtitlePresetFormat::SubRip
        | SubtitlePresetFormat::WebVtt
        | SubtitlePresetFormat::OtherText => SubtitlePresetApplicability::Applies(format),
        SubtitlePresetFormat::Ass | SubtitlePresetFormat::Ssa => {
            SubtitlePresetApplicability::NativeStyle(format)
        }
        SubtitlePresetFormat::Image | SubtitlePresetFormat::Unknown => {
            SubtitlePresetApplicability::Unsupported(format)
        }
    }
}

/// A short, human-readable name for a known image subtitle codec — `PGS`,
/// `VobSub`, `DVB`, or `XSUB` — so the picker and media info name the format
/// cleanly instead of echoing the raw ffmpeg id (`hdmv_pgs_subtitle`). Returns
/// `None` for text or unknown codecs, which the label layer formats itself.
pub fn image_format_name(codec: Option<&str>) -> Option<&'static str> {
    let normalized = codec?.trim().to_ascii_lowercase();
    Some(match normalized.as_str() {
        "hdmv_pgs_subtitle" | "pgssub" | "pgs" => "PGS",
        "dvd_subtitle" | "dvdsub" | "vobsub" => "VobSub",
        "dvb_subtitle" | "dvbsub" => "DVB",
        "xsub" => "XSUB",
        _ => return None,
    })
}

fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn external_extension(filename: Option<&str>) -> Option<String> {
    let filename = clean(filename)?;
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let extension = basename.rsplit_once('.')?.1.trim();
    (!extension.is_empty()).then(|| extension.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bitmap_families_classify_as_image() {
        for codec in [
            "hdmv_pgs_subtitle",
            "pgssub",
            "dvd_subtitle",
            "dvdsub",
            "vobsub",
            "dvb_subtitle",
            "dvbsub",
            "xsub",
        ] {
            assert_eq!(
                subtitle_format(Some(codec)),
                SubtitleFormat::Image,
                "{codec} should be image-based"
            );
            assert!(is_image_subtitle(Some(codec)), "{codec}");
        }
    }

    #[test]
    fn text_and_unknown_codecs_classify_as_text() {
        // Known text codecs, an unknown codec, and no codec at all all stay text
        // so existing tracks keep their controls.
        for codec in [
            Some("subrip"),
            Some("webvtt"),
            Some("ass"),
            Some("ssa"),
            None,
        ] {
            assert_eq!(subtitle_format(codec), SubtitleFormat::Text, "{codec:?}");
            assert!(!is_image_subtitle(codec), "{codec:?}");
        }
        // A codec the classifier has never seen is treated as text, not hidden.
        assert_eq!(subtitle_format(Some("mov_text")), SubtitleFormat::Text);
    }

    #[test]
    fn classification_is_case_and_whitespace_insensitive() {
        assert!(is_image_subtitle(Some("HDMV_PGS_SUBTITLE")));
        assert!(is_image_subtitle(Some("  Dvd_Subtitle  ")));
        assert_eq!(image_format_name(Some("PGSSUB")), Some("PGS"));
    }

    #[test]
    fn appearance_presets_apply_only_to_supported_plain_text_tracks() {
        // Image tracks: the appearance preset cannot restyle a bitmap.
        assert!(!appearance_presets_apply(Some("hdmv_pgs_subtitle")));
        assert!(!appearance_presets_apply(Some("dvd_subtitle")));
        // ASS/SSA owns authored styling, while unknown metadata cannot safely
        // promise preset support.
        assert!(!appearance_presets_apply(Some("ass")));
        assert!(!appearance_presets_apply(Some("ssa")));
        assert!(!appearance_presets_apply(None));
        // Known plain-text tracks keep the presets available.
        assert!(appearance_presets_apply(Some("subrip")));
        assert!(appearance_presets_apply(Some("webvtt")));
        assert!(appearance_presets_apply(Some("mov_text")));
    }

    #[test]
    fn preset_format_distinguishes_ass_and_ssa_with_filename_fallback() {
        assert_eq!(preset_format(Some("ass"), None), SubtitlePresetFormat::Ass);
        assert_eq!(preset_format(Some("ssa"), None), SubtitlePresetFormat::Ssa);
        assert_eq!(
            preset_format(Some("ass"), Some("/media/Feature.EN.SSA")),
            SubtitlePresetFormat::Ssa
        );
        assert_eq!(
            preset_format(None, Some(r"C:\media\Feature.ass")),
            SubtitlePresetFormat::Ass
        );
    }

    #[test]
    fn preset_format_classifies_supported_image_and_unknown_states() {
        assert_eq!(
            preset_format(Some("subrip"), None),
            SubtitlePresetFormat::SubRip
        );
        assert_eq!(
            preset_format(None, Some("Feature.vtt")),
            SubtitlePresetFormat::WebVtt
        );
        assert_eq!(
            preset_format(Some("mov_text"), None),
            SubtitlePresetFormat::OtherText
        );
        assert_eq!(
            preset_format(Some("hdmv_pgs_subtitle"), None),
            SubtitlePresetFormat::Image
        );
        assert_eq!(preset_format(None, None), SubtitlePresetFormat::Unknown);
    }

    #[test]
    fn preset_applicability_preserves_native_styles_and_safe_fallbacks() {
        assert_eq!(
            preset_applicability(Some("subrip"), None),
            SubtitlePresetApplicability::Applies(SubtitlePresetFormat::SubRip)
        );
        assert_eq!(
            preset_applicability(Some("ass"), None),
            SubtitlePresetApplicability::NativeStyle(SubtitlePresetFormat::Ass)
        );
        assert_eq!(
            preset_applicability(Some("ass"), Some("Feature.ssa")),
            SubtitlePresetApplicability::NativeStyle(SubtitlePresetFormat::Ssa)
        );
        assert_eq!(
            preset_applicability(Some("pgs"), None),
            SubtitlePresetApplicability::Unsupported(SubtitlePresetFormat::Image)
        );
        assert_eq!(
            preset_applicability(None, None),
            SubtitlePresetApplicability::Unsupported(SubtitlePresetFormat::Unknown)
        );
    }

    #[test]
    fn image_format_name_maps_families_and_leaves_text_alone() {
        assert_eq!(image_format_name(Some("hdmv_pgs_subtitle")), Some("PGS"));
        assert_eq!(image_format_name(Some("vobsub")), Some("VobSub"));
        assert_eq!(image_format_name(Some("dvb_subtitle")), Some("DVB"));
        assert_eq!(image_format_name(Some("xsub")), Some("XSUB"));
        assert_eq!(image_format_name(Some("subrip")), None);
        assert_eq!(image_format_name(None), None);
    }
}
