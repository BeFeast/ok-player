//! Parses/serialises the user mpv.conf escape-hatch file as plain `key=value` options for the
//! Settings → Advanced key-value editor — port of `src/OkPlayer.Core/MpvConfText.cs`; the C#
//! suite in `tests/OkPlayer.Tests/MpvConfTextTests.cs` is the executable spec. Pure (no I/O,
//! no UI). The engine-side loader is the source of truth for the on-disk format and this
//! mirrors it exactly so a round-trip is faithful: one option per line; blank lines and lines
//! beginning with `#` are ignored; the value is everything after the first `=` (so values may
//! themselves contain `=`); a bare key with no `=` means `key=yes`; keys and values are
//! trimmed. A `#` that is not the first character is part of the value (e.g.
//! `sub-color=#FFFFFF`), not a comment. mpv profile-section headers (a line beginning with
//! `[`, e.g. `[fast]`) are ignored too: the key/value editor can't represent a section, and
//! the engine loader applies the file as flat set-option calls (it blocks `config`, so mpv
//! never loads profiles itself) — so a section was never honoured here anyway. Ignoring the
//! header keeps a round-trip from rewriting `[fast]` as the bogus option `[fast]=yes`.

/// One mpv option as a `key=value` pair, for the Settings → Advanced key-value editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvOption {
    pub key: String,
    pub value: String,
}

impl MpvOption {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Parse mpv.conf text into options, in file order. Comments, blank lines, and profile-section
/// headers are dropped (the editor is key/value only); a bare key becomes `yes`. Tolerant of
/// CRLF and LF.
pub fn parse(text: Option<&str>) -> Vec<MpvOption> {
    let mut options = Vec::new();
    let Some(text) = text.filter(|text| !text.is_empty()) else {
        return options;
    };

    for raw_line in text.split('\n') {
        let line = raw_line.trim(); // also strips a trailing '\r' from CRLF endings
        // Skip blanks, comments, and mpv profile-section headers ("[name]"). Treating "[fast]"
        // as a bare option would round-trip it to the bogus "[fast]=yes" and destroy the
        // profile boundary; the key/value editor can't represent a section, so it's dropped
        // like a comment instead of mangled.
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }

        let eq = line.find('=');
        let key = eq.map_or(line, |eq| &line[..eq]).trim();
        if key.is_empty() {
            continue;
        }

        let value = eq.map_or("yes", |eq| line[eq + 1..].trim());
        options.push(MpvOption::new(key, value));
    }

    options
}

/// Serialise options back to mpv.conf text: one `key=value` per line with a trailing newline.
/// Options with a blank key are skipped; keys and values are trimmed. Stable, so re-saving an
/// unchanged document is a no-op diff and `parse(serialize(x))` round-trips.
pub fn serialize<'a>(options: impl IntoIterator<Item = &'a MpvOption>) -> String {
    let mut text = String::new();
    for option in options {
        let key = option.key.trim();
        if key.is_empty() {
            continue;
        }
        let value = option.value.trim();
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(options: &[MpvOption]) -> Vec<&str> {
        options.iter().map(|o| o.key.as_str()).collect()
    }

    #[test]
    fn parse_empty_or_none_returns_empty() {
        assert!(parse(None).is_empty());
        assert!(parse(Some("")).is_empty());
    }

    #[test]
    fn parse_basic_key_value() {
        assert_eq!(parse(Some("hwdec=no")), vec![MpvOption::new("hwdec", "no")]);
    }

    #[test]
    fn parse_trims_key_and_value() {
        assert_eq!(
            parse(Some("  sub-font  =  Arial Bold  ")),
            vec![MpvOption::new("sub-font", "Arial Bold")]
        );
    }

    #[test]
    fn parse_skips_comments_and_blank_lines() {
        let options = parse(Some("# a comment\n\n   \n  # indented comment\ncache=yes"));
        assert_eq!(options, vec![MpvOption::new("cache", "yes")]);
    }

    #[test]
    fn parse_bare_key_becomes_yes() {
        assert_eq!(parse(Some("fs")), vec![MpvOption::new("fs", "yes")]);
    }

    #[test]
    fn parse_skips_profile_section_headers() {
        // A "[fast]" header must not become a bare option (which would serialize to the bogus
        // "[fast]=yes"); it's dropped like a comment, and the options keep parsing.
        let options = parse(Some("[fast]\nhwdec=auto\n[slow]\ncache=yes"));
        assert_eq!(keys(&options), ["hwdec", "cache"]);
        assert!(options.iter().all(|o| !o.key.starts_with('[')));
    }

    #[test]
    fn round_trip_does_not_corrupt_profile_header_into_option() {
        // Regression: the round-trip used to rewrite "[fast]" as "[fast]=yes", corrupting the
        // profile boundary.
        let serialized = serialize(&parse(Some("[fast]\nhwdec=auto")));
        assert!(!serialized.contains("[fast]"));
        assert_eq!(serialized, "hwdec=auto\n");
    }

    #[test]
    fn parse_value_with_equals_splits_on_first_only() {
        // glsl-shaders paths and option=sub-option=value forms keep everything after the first '='.
        assert_eq!(
            parse(Some("glsl-shaders=~~/a.glsl=b")),
            vec![MpvOption::new("glsl-shaders", "~~/a.glsl=b")]
        );
    }

    #[test]
    fn parse_hash_inside_value_is_not_a_comment() {
        assert_eq!(
            parse(Some("sub-color=#FFFFFF")),
            vec![MpvOption::new("sub-color", "#FFFFFF")]
        );
    }

    #[test]
    fn parse_handles_crlf_and_preserves_order() {
        let options = parse(Some("a=1\r\nb=2\r\nc=3"));
        assert_eq!(keys(&options), ["a", "b", "c"]);
        let values: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, ["1", "2", "3"]);
    }

    #[test]
    fn serialize_writes_key_value_lines_with_trailing_newline() {
        let options = [
            MpvOption::new("hwdec", "no"),
            MpvOption::new("cache", "yes"),
        ];
        assert_eq!(serialize(&options), "hwdec=no\ncache=yes\n");
    }

    #[test]
    fn serialize_skips_blank_keys_and_trims() {
        let options = [
            MpvOption::new("  hwdec ", "  no "),
            MpvOption::new("   ", "orphan"),
            MpvOption::new("", "alsoDropped"),
        ];
        assert_eq!(serialize(&options), "hwdec=no\n");
    }

    #[test]
    fn serialize_empty_returns_empty_string() {
        assert_eq!(serialize(&[]), "");
    }

    #[test]
    fn round_trip_parse_serialize_parse_is_stable() {
        let canonical = "hwdec=no\nsub-font=Arial\nglsl-shaders=~~/a.glsl=b\nfs=yes\n";
        let once = parse(Some(canonical));
        let serialized = serialize(&once);
        assert_eq!(serialized, canonical);
        assert_eq!(parse(Some(&serialized)), once);
    }
}
