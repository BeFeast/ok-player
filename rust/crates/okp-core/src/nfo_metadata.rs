//! Parsed fields from a Kodi/Jellyfin/Emby `.nfo` sidecar — port of
//! `src/OkPlayer.Core/NfoMetadata.cs`; the C# suite in
//! `tests/OkPlayer.Tests/NfoMetadataTests.cs` is the executable spec. The local-library
//! metadata convention: an XML file next to the media, or a `movie.nfo` in the movie's
//! folder. A pure, tolerant parse: reads the common fields from whatever root the file uses
//! (`movie`, `episodedetails`, `musicvideo`, `tvshow`, …) and ignores the rest. Returns
//! `None` for a non-XML `.nfo` (some are just a bare scraper URL) or one with no usable
//! title. Engine- and UI-free.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};

/// A real Kodi/Jellyfin NFO is normally only a few KiB. Match the Windows guard so a
/// mislabeled media file or other pathological sidecar cannot allocate without bound.
pub const MAX_SIDECAR_BYTES: u64 = 2 * 1024 * 1024;

/// The usable fields of a `.nfo` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NfoMetadata {
    pub title: String,
    pub year: Option<i32>,
    pub plot: Option<String>,
}

/// Per-source NFO resolution state. `Pending` is distinct from a completed miss so a
/// quick progress save does not erase an existing recent title while the worker is
/// still reading, while `Resolved(None)` can deliberately restore filename fallback
/// after a missing or malformed sidecar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NfoTitleState {
    #[default]
    NotApplicable,
    Pending,
    Resolved(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryTitleUpdate {
    Preserve,
    Clear,
    Set(String),
}

impl NfoTitleState {
    pub fn title(&self) -> Option<&str> {
        match self {
            Self::Resolved(Some(title)) => nonempty_trimmed(title),
            Self::NotApplicable | Self::Pending | Self::Resolved(None) => None,
        }
    }

    pub fn history_update(&self, private_session: bool) -> HistoryTitleUpdate {
        if private_session {
            return HistoryTitleUpdate::Preserve;
        }
        match self {
            Self::Resolved(Some(title)) => nonempty_trimmed(title)
                .map(|title| HistoryTitleUpdate::Set(title.to_owned()))
                .unwrap_or(HistoryTitleUpdate::Clear),
            Self::Resolved(None) => HistoryTitleUpdate::Clear,
            Self::NotApplicable | Self::Pending => HistoryTitleUpdate::Preserve,
        }
    }
}

/// Candidate NFO sidecars for a local media file, in precedence order: the per-item
/// same-basename file first, then Kodi's folder-level `movie.nfo` convention.
///
/// URLs have no local sidecar and return an empty list. The function performs no I/O,
/// so shells can inspect or schedule the candidates without probing a filesystem on
/// their UI thread.
pub fn sidecar_candidates(media_path: &Path) -> Vec<PathBuf> {
    if media_path.as_os_str().is_empty()
        || media_path.to_string_lossy().contains("://")
        || media_path.file_name().is_none()
    {
        return Vec::new();
    }

    let Some(parent) = media_path.parent() else {
        return Vec::new();
    };
    let mut same_basename = media_path.to_path_buf();
    same_basename.set_extension("nfo");
    vec![same_basename, parent.join("movie.nfo")]
}

/// Discover, read, and parse a usable NFO sidecar for a local media file. Missing,
/// empty, oversized, unreadable, undecodable, and malformed files all resolve to
/// `None`; a malformed same-basename candidate falls through to `movie.nfo`.
///
/// This function is deliberately blocking filesystem I/O. GUI shells must call it on
/// a worker thread, as the GTK shell does, because a local-looking mount can still be
/// slow. It never performs HTTP or any other explicit network request.
pub fn read_sidecar(media_path: &Path) -> Option<NfoMetadata> {
    sidecar_candidates(media_path)
        .into_iter()
        .find_map(|candidate| read_candidate(&candidate).and_then(|xml| parse(Some(&xml))))
}

/// Choose the title shown by a shell. Curated NFO metadata wins over the engine title;
/// a blank value at either level falls through to the existing file/URL display name.
pub fn display_title(
    sidecar_title: Option<&str>,
    engine_title: Option<&str>,
    fallback: &str,
) -> String {
    sidecar_title
        .and_then(nonempty_trimmed)
        .or_else(|| engine_title.and_then(nonempty_trimmed))
        .unwrap_or_else(|| fallback.trim())
        .to_owned()
}

fn nonempty_trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn read_candidate(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() == 0 || metadata.len() > MAX_SIDECAR_BYTES {
        return None;
    }

    // Recheck the bound while reading so a file that grows after metadata() cannot race
    // the size guard. One extra byte is enough to distinguish an exact-limit document
    // from an oversized one.
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_SIDECAR_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_SIDECAR_BYTES {
        return None;
    }

    decode_text(bytes)
}

fn decode_text(bytes: Vec<u8>) -> Option<String> {
    if let Some(payload) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(payload.to_vec()).ok();
    }
    if let Some(payload) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(payload, u16::from_le_bytes);
    }
    if let Some(payload) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(payload, u16::from_be_bytes);
    }
    String::from_utf8(bytes).ok()
}

fn decode_utf16(payload: &[u8], decode: fn([u8; 2]) -> u16) -> Option<String> {
    let mut chunks = payload.chunks_exact(2);
    let units = chunks
        .by_ref()
        .map(|chunk| decode([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    chunks.remainder().is_empty().then_some(())?;
    String::from_utf16(&units).ok()
}

/// Parse a `.nfo` document. `None` when the text isn't XML or carries no title.
pub fn parse(xml: Option<&str>) -> Option<NfoMetadata> {
    let xml = xml?;
    if xml.trim().is_empty() {
        return None;
    }
    // Not XML (e.g. a legacy .nfo that's just an IMDb URL) — nothing to read.
    let doc = Document::parse(xml).ok()?;
    let root = doc.root_element();

    // Title is required to be useful; <title> first, then <originaltitle>.
    // child() only returns non-whitespace values, so the title is usable as-is.
    let title = child(root, "title").or_else(|| child(root, "originaltitle"))?;

    let year = child(root, "year")
        .as_deref()
        .and_then(parse_year)
        // <premiered>2020-05-01</premiered> — the year is the first four characters.
        .or_else(|| {
            child(root, "premiered")
                .as_deref()
                .and_then(parse_year_prefix)
        })
        .or_else(|| child(root, "aired").as_deref().and_then(parse_year_prefix));

    let plot = child(root, "plot")
        .or_else(|| child(root, "outline"))
        .map(|p| p.trim().to_string());
    Some(NfoMetadata {
        title: title.trim().to_string(),
        year,
        plot,
    })
}

/// A positive integer year, tolerating surrounding whitespace (like C# `int.TryParse`).
fn parse_year(value: &str) -> Option<i32> {
    let year: i32 = value.trim().parse().ok()?;
    (year > 0).then_some(year)
}

/// The year in a date's first four characters, when the value is at least that long.
fn parse_year_prefix(value: &str) -> Option<i32> {
    let prefix: String = value.chars().take(4).collect();
    if prefix.chars().count() < 4 {
        return None;
    }
    parse_year(&prefix)
}

/// First DIRECT child element with the given local name (namespace-agnostic), non-empty
/// trimmed value or `None`. Direct children only, so a nested `<title>` (inside `<set>`,
/// `<actor>`, …) can't be mistaken for the item title.
fn child(root: Node, name: &str) -> Option<String> {
    for element in root.children().filter(Node::is_element) {
        if element.tag_name().name().eq_ignore_ascii_case(name) {
            let value = element_value(element);
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// The concatenated text of all descendant text/CDATA nodes — what C# `XElement.Value` reads.
fn element_value(element: Node) -> String {
    element
        .descendants()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;
    use std::fs;

    #[test]
    fn kodi_movie_reads_title_year_plot() {
        let xml = "<movie>\n  <title>Blade Runner 2049</title>\n  <year>2017</year>\n  \
                   <plot>A young blade runner uncovers a long-buried secret.</plot>\n</movie>";
        let nfo = parse(Some(xml)).expect("usable nfo");
        assert_eq!(nfo.title, "Blade Runner 2049");
        assert_eq!(nfo.year, Some(2017));
        assert_eq!(
            nfo.plot.as_deref(),
            Some("A young blade runner uncovers a long-buried secret.")
        );
    }

    #[test]
    fn episode_reads_aired_year_when_no_year_element() {
        let xml = "<episodedetails>\n  <title>The Constant</title>\n  <aired>2008-02-28</aired>\n  \
                   <outline>Desmond experiences unusual side effects.</outline>\n</episodedetails>";
        let nfo = parse(Some(xml)).expect("usable nfo");
        assert_eq!(nfo.title, "The Constant");
        assert_eq!(nfo.year, Some(2008)); // from <aired>
        // <outline> fallback.
        assert_eq!(
            nfo.plot.as_deref(),
            Some("Desmond experiences unusual side effects.")
        );
    }

    #[test]
    fn premiered_supplies_year_over_missing_year() {
        let nfo = parse(Some(
            "<movie><title>Dune</title><premiered>2021-10-22</premiered></movie>",
        ))
        .expect("usable nfo");
        assert_eq!(nfo.year, Some(2021));
    }

    #[test]
    fn original_title_falls_back_when_no_title() {
        let nfo = parse(Some(
            "<movie><originaltitle>Spirited Away</originaltitle></movie>",
        ))
        .expect("usable nfo");
        assert_eq!(nfo.title, "Spirited Away");
    }

    #[test]
    fn nested_title_does_not_masquerade_as_item_title() {
        // A <title> nested inside <set> must not be picked as the movie title — only direct
        // children count.
        let xml = "<movie>\n  <set><name>Trilogy</name><title>Set Title</title></set>\n  \
                   <title>The Real Movie</title>\n</movie>";
        assert_eq!(
            parse(Some(xml)).expect("usable nfo").title,
            "The Real Movie"
        );
    }

    #[test]
    fn title_trimmed_and_namespace_agnostic() {
        let nfo = parse(Some("<movie><title>  Arrival  </title></movie>")).expect("usable nfo");
        assert_eq!(nfo.title, "Arrival");
        assert_eq!(nfo.year, None);
        assert_eq!(nfo.plot, None);
    }

    #[test]
    fn unusable_returns_none() {
        let cases = [
            "",
            "   ",
            "https://www.imdb.com/title/tt1856101/", // legacy URL-only .nfo — not XML
            "<movie><year>2020</year></movie>",      // no title -> not useful
            "<movie></movie>",
            "not xml at all <<<",
        ];
        for input in cases {
            assert_eq!(parse(Some(input)), None, "{input:?}");
        }
        assert_eq!(parse(None), None);
    }

    #[test]
    fn garbage_year_ignored() {
        let nfo =
            parse(Some("<movie><title>X</title><year>n/a</year></movie>")).expect("usable nfo");
        assert_eq!(nfo.year, None);
    }

    #[test]
    fn sidecar_candidates_prefer_same_basename_then_folder_movie() {
        assert_eq!(
            sidecar_candidates(Path::new("/media/films/Movie.mkv")),
            [
                PathBuf::from("/media/films/Movie.nfo"),
                PathBuf::from("/media/films/movie.nfo"),
            ]
        );
        assert!(sidecar_candidates(Path::new("https://example.com/Movie.mkv")).is_empty());
    }

    #[test]
    fn read_sidecar_uses_same_basename_title() {
        let dir = unique_temp_dir("okp-nfo-same-basename");
        fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("Movie.mkv");
        fs::write(&media, b"media").expect("media fixture");
        fs::write(
            dir.join("Movie.nfo"),
            b"<movie><title>Curated Movie Title</title><year>2024</year></movie>",
        )
        .expect("nfo fixture");
        fs::write(
            dir.join("movie.nfo"),
            b"<movie><title>Folder Fallback</title></movie>",
        )
        .expect("folder nfo fixture");

        let metadata = read_sidecar(&media).expect("usable sidecar");
        assert_eq!(metadata.title, "Curated Movie Title");
        assert_eq!(metadata.year, Some(2024));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_sidecar_falls_through_malformed_same_basename() {
        let dir = unique_temp_dir("okp-nfo-folder-fallback");
        fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("Movie.mkv");
        fs::write(&media, b"media").expect("media fixture");
        fs::write(dir.join("Movie.nfo"), b"not xml <<<").expect("malformed nfo");
        fs::write(
            dir.join("movie.nfo"),
            b"<movie><title>Folder Fallback</title></movie>",
        )
        .expect("folder nfo fixture");

        assert_eq!(
            read_sidecar(&media).map(|metadata| metadata.title),
            Some("Folder Fallback".to_owned())
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_sidecar_accepts_utf16_bom() {
        let dir = unique_temp_dir("okp-nfo-utf16");
        fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("Movie.mkv");
        fs::write(&media, b"media").expect("media fixture");
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "<movie><title>Wide Title</title></movie>".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(dir.join("Movie.nfo"), bytes).expect("utf16 nfo fixture");

        assert_eq!(
            read_sidecar(&media).map(|metadata| metadata.title),
            Some("Wide Title".to_owned())
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_sidecar_fails_quietly_when_missing_malformed_or_oversized() {
        let dir = unique_temp_dir("okp-nfo-failures");
        fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("Movie.mkv");
        fs::write(&media, b"media").expect("media fixture");

        assert_eq!(read_sidecar(&media), None);

        fs::write(dir.join("Movie.nfo"), b"not xml <<<").expect("malformed nfo");
        assert_eq!(read_sidecar(&media), None);

        fs::write(
            dir.join("Movie.nfo"),
            vec![b'x'; MAX_SIDECAR_BYTES as usize + 1],
        )
        .expect("oversized nfo");
        assert_eq!(read_sidecar(&media), None);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_sidecar_never_resolves_a_url() {
        assert_eq!(
            read_sidecar(Path::new("https://example.com/Movie.mkv")),
            None
        );
    }

    #[test]
    fn display_title_prefers_sidecar_then_engine_then_existing_fallback() {
        assert_eq!(
            display_title(Some("  Curated Title  "), Some("Engine Title"), "Movie.mkv"),
            "Curated Title"
        );
        assert_eq!(
            display_title(Some("  "), Some(" Engine Title "), "Movie.mkv"),
            "Engine Title"
        );
        assert_eq!(display_title(None, Some(""), " Movie.mkv "), "Movie.mkv");
    }

    #[test]
    fn title_state_distinguishes_pending_missing_and_private_persistence() {
        assert_eq!(NfoTitleState::Pending.title(), None);
        assert_eq!(
            NfoTitleState::Pending.history_update(false),
            HistoryTitleUpdate::Preserve
        );
        assert_eq!(
            NfoTitleState::Resolved(None).history_update(false),
            HistoryTitleUpdate::Clear
        );
        let resolved = NfoTitleState::Resolved(Some("  Curated Title  ".to_owned()));
        assert_eq!(resolved.title(), Some("Curated Title"));
        assert_eq!(
            resolved.history_update(false),
            HistoryTitleUpdate::Set("Curated Title".to_owned())
        );
        assert_eq!(resolved.history_update(true), HistoryTitleUpdate::Preserve);
    }
}
