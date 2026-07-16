//! Compose the one-line descriptor shown for a track row in the audio/subtitle
//! quick-switchers. Pure and UI-free so the label logic is unit-tested here in
//! the core and the shells only wire the finished string into a widget
//! (freeze-boundary). There is no Windows counterpart yet — the WinUI shell
//! builds its flyout rows in XAML — so this is Linux-lane logic that a future
//! port can share verbatim.

/// The primary name shown for a track: its title, else its language code, else
/// a `Track {id}` placeholder. Blank or whitespace-only values count as absent.
pub fn primary_track_name(id: i64, title: Option<&str>, lang: Option<&str>) -> String {
    clean(title)
        .or_else(|| clean(lang))
        .unwrap_or_else(|| format!("Track {id}"))
}

/// The audio quick-switcher descriptor: the primary name followed by the
/// language, channel-layout, and codec tags that add information, joined with
/// " · ". The language tag is appended only when a real title is present and
/// does not already spell the language out, so a titled commentary reads
/// `Commentary · ENG · 5.1 · AC3` while a bare `English` track (whose name
/// already is the language) stays `English · 5.1 · EAC3` and a code-only track
/// stays `jpn · 2.0 · AAC` without repeating itself.
pub fn audio_track_label(
    id: i64,
    title: Option<&str>,
    lang: Option<&str>,
    channels: Option<&str>,
    codec: Option<&str>,
) -> String {
    let (name, detail) = audio_track_parts(id, title, lang, channels, codec);
    if detail.is_empty() {
        name
    } else {
        format!("{name} · {detail}")
    }
}

/// The compact audio-switcher hierarchy: a stable primary name followed by an
/// optional language/channel/codec detail line. Shells can render the two parts
/// natively without duplicating normalization or language deduplication.
pub fn audio_track_parts(
    id: i64,
    title: Option<&str>,
    lang: Option<&str>,
    channels: Option<&str>,
    codec: Option<&str>,
) -> (String, String) {
    let primary = primary_track_name(id, title, lang);
    let mut detail = Vec::new();

    if clean(title).is_some()
        && let Some(lang) = clean(lang)
        && !primary
            .to_ascii_lowercase()
            .contains(&lang.to_ascii_lowercase())
    {
        detail.push(lang.to_ascii_uppercase());
    }
    if let Some(channels) = clean(channels) {
        detail.push(channels);
    }
    if let Some(codec) = clean(codec) {
        detail.push(codec.to_ascii_uppercase());
    }

    (primary, detail.join(" · "))
}

/// The subtitle quick-switcher descriptor: the primary name followed by a
/// normalized format tag and the source marker. WebVTT and SubRip are named
/// explicitly so an external `.vtt` never reads like an SRT or an untyped
/// embedded track. mpv remains responsible for parsing/rendering subtitle cue
/// payloads; the core only classifies the track metadata it reports.
pub fn subtitle_track_label(
    id: i64,
    title: Option<&str>,
    lang: Option<&str>,
    codec: Option<&str>,
    external: bool,
    default: bool,
) -> String {
    let mut parts = vec![primary_track_name(id, title, lang)];

    if let Some(codec) = clean(codec) {
        parts.push(subtitle_codec_label(&codec));
    }
    if external {
        parts.push("EXT".to_owned());
    } else if default {
        parts.push("Default".to_owned());
    }

    parts.join(" · ")
}

fn subtitle_codec_label(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "subrip" | "srt" => "SRT".to_owned(),
        "webvtt" | "vtt" => "WebVTT".to_owned(),
        _ => codec.to_ascii_uppercase(),
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_name_prefers_title_then_lang_then_placeholder() {
        assert_eq!(
            primary_track_name(2, Some("Director's Cut"), Some("eng")),
            "Director's Cut"
        );
        assert_eq!(primary_track_name(2, None, Some("jpn")), "jpn");
        assert_eq!(primary_track_name(5, None, None), "Track 5");
        // Blank strings are treated as absent so a stray empty tag never wins.
        assert_eq!(primary_track_name(5, Some("  "), Some("")), "Track 5");
    }

    #[test]
    fn audio_label_keeps_a_plain_english_track_without_a_redundant_lang_tag() {
        assert_eq!(
            audio_track_label(1, Some("English"), Some("eng"), Some("5.1"), Some("eac3")),
            "English · 5.1 · EAC3"
        );
    }

    #[test]
    fn audio_label_surfaces_language_for_a_named_commentary_track() {
        assert_eq!(
            audio_track_label(
                3,
                Some("Director's Commentary"),
                Some("eng"),
                Some("2.0"),
                Some("ac3"),
            ),
            "Director's Commentary · ENG · 2.0 · AC3"
        );
    }

    #[test]
    fn audio_parts_preserve_the_compact_two_line_hierarchy() {
        assert_eq!(
            audio_track_parts(
                3,
                Some("Director's Commentary"),
                Some("eng"),
                Some("2.0"),
                Some("ac3"),
            ),
            (
                "Director's Commentary".to_owned(),
                "ENG · 2.0 · AC3".to_owned()
            )
        );
        assert_eq!(
            audio_track_parts(7, None, None, None, None),
            ("Track 7".to_owned(), String::new())
        );
    }

    #[test]
    fn audio_parts_do_not_repeat_a_language_used_as_the_primary_name() {
        assert_eq!(
            audio_track_parts(4, None, Some("jpn"), Some("2.0"), Some("aac")),
            ("jpn".to_owned(), "2.0 · AAC".to_owned())
        );
        assert_eq!(
            audio_track_parts(1, Some("English"), Some("eng"), Some("5.1"), Some("eac3")),
            ("English".to_owned(), "5.1 · EAC3".to_owned())
        );
    }

    #[test]
    fn audio_label_falls_back_to_the_language_code_when_untitled() {
        // No title: the code becomes the primary name and is not repeated as a tag.
        assert_eq!(
            audio_track_label(4, None, Some("jpn"), Some("2.0"), Some("aac")),
            "jpn · 2.0 · AAC"
        );
    }

    #[test]
    fn audio_label_uses_a_placeholder_when_nothing_identifies_the_track() {
        assert_eq!(
            audio_track_label(7, None, None, None, None),
            "Track 7".to_owned()
        );
    }

    #[test]
    fn audio_label_omits_blank_or_whitespace_tags() {
        assert_eq!(
            audio_track_label(1, Some("English"), Some("  "), Some(""), Some("aac")),
            "English · AAC"
        );
    }

    #[test]
    fn subtitle_label_distinguishes_webvtt_from_external_srt() {
        assert_eq!(
            subtitle_track_label(2, Some("English"), None, Some("webvtt"), true, false),
            "English · WebVTT · EXT"
        );
        assert_eq!(
            subtitle_track_label(3, Some("English"), None, Some("subrip"), true, false),
            "English · SRT · EXT"
        );
    }

    #[test]
    fn subtitle_label_distinguishes_embedded_and_default_tracks() {
        assert_eq!(
            subtitle_track_label(4, None, Some("eng"), Some("ass"), false, true),
            "eng · ASS · Default"
        );
        assert_eq!(
            subtitle_track_label(5, None, None, None, false, false),
            "Track 5"
        );
    }
}
