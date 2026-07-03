//! Best-effort resolution of an audio track's (artist, title) for a lyrics lookup — port of
//! `src/OkPlayer.Core/TrackTags.cs`; the C# suite in `tests/OkPlayer.Tests/TrackTagsTests.cs`
//! is the executable spec. Real tags win; when an artist tag is missing it mines a
//! `"Artist - Title"` display string (the mpv media-title, else the file name) — the common
//! shape for ripped/downloaded files that carry a title but no separate artist tag. Either
//! field may come back `None` when nothing usable is present. Pure / UI-free for headless
//! tests.

/// Resolve (artist, track) from the available signals. `tag_artist`/`tag_title` are the
/// file's metadata tags; `display` is the mpv media-title; `file_stem` is the file name
/// without extension. Splits on the first `" - "` only to fill a field a tag didn't provide.
pub fn resolve(
    tag_artist: Option<&str>,
    tag_title: Option<&str>,
    display: Option<&str>,
    file_stem: Option<&str>,
) -> (Option<String>, Option<String>) {
    let mut artist = clean(tag_artist);
    let mut track = clean(tag_title);
    let source = clean(display).or_else(|| clean(file_stem)); // the string to mine when a tag is missing

    if (artist.is_none() || track.is_none())
        && let Some(source) = &source
        && let Some(dash) = source.find(" - ")
        && dash > 0
        && dash + 3 < source.len()
    {
        if artist.is_none() {
            artist = clean(Some(&source[..dash]));
        }
        if track.is_none() {
            track = clean(Some(&source[dash + 3..]));
        }
    }
    // Last resort: treat the whole display/filename as the track name.
    (artist, track.or(source))
}

fn clean(s: Option<&str>) -> Option<String> {
    let trimmed = s?.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_tags_are_used_verbatim_and_display_is_not_mined() {
        let (artist, track) = resolve(
            Some("Daft Punk"),
            Some("Aerodynamic"),
            Some("Something - Else"),
            Some("99 - whatever"),
        );
        assert_eq!(artist.as_deref(), Some("Daft Punk"));
        assert_eq!(track.as_deref(), Some("Aerodynamic"));
    }

    #[test]
    fn no_tags_mines_artist_and_title_from_display() {
        let (artist, track) = resolve(
            None,
            None,
            Some("Daft Punk - Aerodynamic"),
            Some("01 - track"),
        );
        assert_eq!(artist.as_deref(), Some("Daft Punk"));
        assert_eq!(track.as_deref(), Some("Aerodynamic"));
    }

    #[test]
    fn missing_artist_tag_is_filled_from_display_title_tag_kept() {
        let (artist, track) = resolve(
            None,
            Some("Aerodynamic"),
            Some("Daft Punk - Aerodynamic"),
            None,
        );
        assert_eq!(artist.as_deref(), Some("Daft Punk"));
        // The real title tag is kept, not the split half.
        assert_eq!(track.as_deref(), Some("Aerodynamic"));
    }

    #[test]
    fn no_dash_whole_string_becomes_track_artist_none() {
        let (artist, track) = resolve(None, None, Some("Some Untitled Jam"), None);
        assert_eq!(artist, None);
        assert_eq!(track.as_deref(), Some("Some Untitled Jam"));
    }

    #[test]
    fn falls_back_to_file_stem_when_no_display() {
        let (artist, track) = resolve(None, None, None, Some("Radiohead - Idioteque"));
        assert_eq!(artist.as_deref(), Some("Radiohead"));
        assert_eq!(track.as_deref(), Some("Idioteque"));
    }

    #[test]
    fn whitespace_is_treated_as_absent_and_trimmed() {
        let (artist, track) = resolve(Some("   "), Some("  Hey  "), Some("  "), None);
        assert_eq!(artist, None);
        assert_eq!(track.as_deref(), Some("Hey"));
    }

    #[test]
    fn empty_everything_is_all_none() {
        let (artist, track) = resolve(None, None, None, None);
        assert_eq!(artist, None);
        assert_eq!(track, None);
    }
}
