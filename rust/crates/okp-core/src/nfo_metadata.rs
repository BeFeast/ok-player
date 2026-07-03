//! Parsed fields from a Kodi/Jellyfin/Emby `.nfo` sidecar — port of
//! `src/OkPlayer.Core/NfoMetadata.cs`; the C# suite in
//! `tests/OkPlayer.Tests/NfoMetadataTests.cs` is the executable spec. The local-library
//! metadata convention: an XML file next to the media, or a `movie.nfo` in the movie's
//! folder. A pure, tolerant parse: reads the common fields from whatever root the file uses
//! (`movie`, `episodedetails`, `musicvideo`, `tvshow`, …) and ignores the rest. Returns
//! `None` for a non-XML `.nfo` (some are just a bare scraper URL) or one with no usable
//! title. Engine- and UI-free.

use roxmltree::{Document, Node};

/// The usable fields of a `.nfo` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NfoMetadata {
    pub title: String,
    pub year: Option<i32>,
    pub plot: Option<String>,
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
}
