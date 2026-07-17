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
//! SubRip, WebVTT, ASS/SSA, an unknown or absent codec — as text. Unknown-as-text
//! is the safe default: a novel or missing codec keeps the existing text
//! behaviour rather than hiding controls for a track that might in fact be text.

/// How mpv presents a subtitle track's cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// Text cues mpv renders itself (SubRip, WebVTT) or via libass (ASS/SSA).
    Text,
    /// Pre-rendered bitmap graphics (PGS, VobSub, DVB, XSUB).
    Image,
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
    !is_image_subtitle(codec)
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
    fn appearance_presets_apply_only_to_text_tracks() {
        // Image tracks: the appearance preset cannot restyle a bitmap.
        assert!(!appearance_presets_apply(Some("hdmv_pgs_subtitle")));
        assert!(!appearance_presets_apply(Some("dvd_subtitle")));
        // Text tracks and unclassified codecs keep the presets available.
        assert!(appearance_presets_apply(Some("subrip")));
        assert!(appearance_presets_apply(Some("ass")));
        assert!(appearance_presets_apply(None));
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
