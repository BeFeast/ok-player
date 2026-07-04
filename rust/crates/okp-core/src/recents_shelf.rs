//! Pure selection, scoring, and layout math for the welcome "Continue watching" shelf.
//!
//! The layout half is a port of `src/OkPlayer.Core/RecentsShelf.cs` (the C# suite in
//! `tests/OkPlayer.Tests/RecentsShelfTests.cs` is the executable spec): how many fixed-width
//! cards fit a given row width, so the shelf shows exactly this many and never needs a
//! horizontal scrollbar (the design's elegance bar); any remaining resumable files stay
//! reachable via History.
//!
//! The selection half ([`select_continue_watching`]) is the shared home for the recents-forward
//! projection that Windows currently keeps inline in `PlayerView.LoadRecents`: which history
//! entries are genuinely resumable, newest-opened first, projected into the per-card display
//! model (title, runtime, time-left, progress fraction, placeholder slot). Kept here — engine-
//! and UI-agnostic — so both shells render the same shelf from one unit-tested rule rather than
//! re-deriving the filter and the labels in each view.

use crate::history::FileEntry;
use crate::media_formats;

/// The C# default for `unmeasured_default` — what the shelf shows before its first measure.
pub const DEFAULT_UNMEASURED: usize = 3;

/// The last stretch of a file that counts as "finished enough" to not offer a resume: the final
/// 5%, but never more than the last 30 seconds (a long film's 5% would swallow the whole ending).
/// A position at or past this point is treated as complete, matching the Windows resume rule and
/// the Linux `HistoryStore` record/resume thresholds.
pub fn completion_start(duration: f64) -> f64 {
    (duration * 0.95).max(duration - 30.0)
}

/// Whether a stored position is genuinely resumable: a real, in-progress spot to jump back to —
/// past the first 5% (so a barely-touched file isn't offered) and before [`completion_start`] (so a
/// finished-or-nearly file isn't). Non-finite or non-positive durations, and non-finite positions,
/// are never resumable. Mirrors the Windows continue-watching filter and the Linux resume check so
/// the shelf, the resume seek, and History all agree on what "resumable" means.
pub fn is_resumable(position: f64, duration: f64) -> bool {
    duration.is_finite()
        && duration > 0.0
        && position.is_finite()
        && position > duration * 0.05
        && position < completion_start(duration)
}

/// One projected "Continue watching" card — everything the shelf renders for a resumable file,
/// derived from its [`FileEntry`]. The Rust peer of the Windows `RecentEntry` view-model; built by
/// [`select_continue_watching`] so the shell only lays these out.
#[derive(Clone, Debug, PartialEq)]
pub struct ContinueWatchingCard {
    /// The history key (a local path, or a URL/stream) — what the shell opens on click.
    pub path: String,
    /// Display title: the cached title, else the file stem.
    pub title: String,
    /// Total runtime, humanised: `"2h 41m"` / `"45m"`.
    pub runtime_label: String,
    /// Time remaining, humanised: `"41m left"` / `"1h 5m left"` (never below `"1m left"`).
    pub time_left_label: String,
    /// Watched fraction in `0..=1`, driving the card's progress fill.
    pub progress: f64,
    /// When the file was last opened (Unix seconds) — the shell maps this to the local "when"
    /// label ([`crate::history_format::when_label`]) for the card's last-opened context line.
    pub updated_at_unix: i64,
    /// Rotating index into the shell's placeholder-gradient palette, so a card without a poster
    /// still looks designed and adjacent cards don't share a tint.
    pub palette_index: usize,
    /// Whether the file is an audio track — drives the "Continue listening" heading and the
    /// audio-styled placeholder.
    pub is_audio: bool,
}

/// Project the resumable history entries into the recents-forward shelf model, newest-opened first.
///
/// `entries` is any iterator of `(path, record)` pairs (the shell passes its history map, already
/// filtered to still-listable paths). A [`private`](crate::history) session returns an empty shelf —
/// the invariant that a private session never leaks recents, enforced here rather than trusted to
/// each shell. Otherwise entries that are finished or not [`is_resumable`] are dropped, the rest are
/// ordered by `updated_at_unix` descending (ties broken by path so the order is deterministic), and
/// the leading `max_cards` are projected into [`ContinueWatchingCard`]s with rotating palette slots.
pub fn select_continue_watching<'a, I>(
    entries: I,
    private: bool,
    max_cards: usize,
) -> Vec<ContinueWatchingCard>
where
    I: IntoIterator<Item = (&'a str, &'a FileEntry)>,
{
    if private || max_cards == 0 {
        return Vec::new();
    }

    let mut resumable: Vec<(&str, &FileEntry)> = entries
        .into_iter()
        .filter(|(_, entry)| !entry.finished && is_resumable(entry.position, entry.duration))
        .collect();
    // Newest opened first; a stable path tie-break keeps equal-timestamp order deterministic.
    resumable.sort_by(|(a_path, a), (b_path, b)| {
        b.updated_at_unix
            .cmp(&a.updated_at_unix)
            .then_with(|| a_path.cmp(b_path))
    });

    resumable
        .into_iter()
        .take(max_cards)
        .enumerate()
        .map(|(index, (path, entry))| ContinueWatchingCard {
            path: path.to_owned(),
            title: card_title(path, entry.title.as_deref()),
            runtime_label: runtime_label(entry.duration),
            time_left_label: time_left_label(entry.duration - entry.position),
            progress: (entry.position / entry.duration).clamp(0.0, 1.0),
            updated_at_unix: entry.updated_at_unix,
            palette_index: index,
            is_audio: media_formats::is_audio(path),
        })
        .collect()
}

/// The shelf heading: "Continue watching", or "Continue listening" when every resumable item is an
/// audio track — you don't "watch" audio, but a mix or any video keeps "watching". An empty shelf
/// defaults to "Continue watching" (the heading is unused then).
pub fn shelf_header(cards: &[ContinueWatchingCard]) -> &'static str {
    if !cards.is_empty() && cards.iter().all(|card| card.is_audio) {
        "Continue listening"
    } else {
        "Continue watching"
    }
}

/// A card's display title: the cached `title` when present and non-empty, else the file stem —
/// the last path segment (split on both separators, since a Windows history path may reach the
/// Linux shell) with its extension dropped.
fn card_title(path: &str, title: Option<&str>) -> String {
    match title {
        Some(title) if !title.is_empty() => title.to_owned(),
        _ => file_stem(path),
    }
}

/// The file name without its extension, like `Path.GetFileNameWithoutExtension`: last segment after
/// either separator, trimmed at the final dot (a leading-dot dotfile keeps its whole name).
fn file_stem(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_owned(),
        _ => name.to_owned(),
    }
}

/// Humanised runtime, `"2h 41m"` past the hour else `"45m"` (port of the Windows `FormatRuntime`);
/// seconds truncate and negatives clamp to zero.
fn runtime_label(seconds: f64) -> String {
    let total = seconds.max(0.0) as i64;
    let (hours, minutes) = (total / 3600, total % 3600 / 60);
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Humanised time remaining, `"1h 5m left"` past the hour else `"41m left"` (port of the Windows
/// `FormatTimeLeft`); the sub-hour minutes clamp to at least 1 so a nearly-done file never reads
/// "0m left", and negatives clamp to zero.
fn time_left_label(seconds: f64) -> String {
    let total = seconds.max(0.0) as i64;
    let (hours, minutes) = (total / 3600, total % 3600 / 60);
    if hours > 0 {
        format!("{hours}h {minutes}m left")
    } else {
        format!("{}m left", minutes.max(1))
    }
}

/// How many cards to show: as many whole cards as fit `row_width`, capped by how many are
/// actually `available`.
///
/// n cards laid out with (n-1) gaps need n*card + (n-1)*spacing ≤ width, i.e.
/// n ≤ (width + spacing) / (card + spacing); we take the floor. This is 0 when not even one
/// card fits — important because the row no longer scrolls, so on a side-snapped or very
/// narrow window we must show nothing (and route the items to the overflow control) rather
/// than clip a full-width card. Before the row is measured (`row_width` ≤ 0) we fall back to
/// `unmeasured_default` so the first paint is sensible; a size-changed then corrects it. The
/// result is always clamped to [0, available].
pub fn visible_count(
    row_width: f64,
    available: usize,
    card_width: f64,
    spacing: f64,
    unmeasured_default: usize,
) -> usize {
    if available == 0 {
        return 0;
    }
    let fit = if row_width <= 0.0 {
        unmeasured_default
    } else {
        // 0 when one card doesn't fit -> no clipping
        ((row_width + spacing) / (card_width + spacing)) as usize
    };
    fit.min(available)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped card geometry: 194px card, 14px gap.
    const CARD: f64 = 194.0;
    const GAP: f64 = 14.0;

    #[test]
    fn visible_count_fits_as_many_whole_cards_as_there_is_room_for() {
        // n cards need n*194 + (n-1)*14 px. 772 is the welcome column's content width
        // (860 MaxWidth - 88 padding): 3 cards = 610, 4 cards = 818 > 772, so exactly 3 fit.
        let cases = [
            (772.0, 3),
            (610.0, 3), // exactly three cards wide -> three
            (609.0, 2), // one px short of three -> two
            (832.0, 4), // a wider column fits four
            (208.0, 1), // one card + one gap
            (194.0, 1), // exactly one card wide -> one
            (193.0, 0), // one px short of a card -> none (the row no longer scrolls, so don't clip)
            (120.0, 0), // far narrower than a card -> none; the overflow control holds them instead
        ];
        for (width, expected) in cases {
            assert_eq!(
                visible_count(width, 20, CARD, GAP, DEFAULT_UNMEASURED),
                expected,
                "width {width}"
            );
        }
    }

    #[test]
    fn visible_count_is_capped_by_what_is_available() {
        assert_eq!(visible_count(2000.0, 2, CARD, GAP, DEFAULT_UNMEASURED), 2);
    }

    #[test]
    fn visible_count_is_zero_when_nothing_is_available() {
        assert_eq!(visible_count(772.0, 0, CARD, GAP, DEFAULT_UNMEASURED), 0);
    }

    #[test]
    fn visible_count_falls_back_to_the_default_before_the_row_is_measured() {
        let cases = [(10, 3), (2, 2)]; // unmeasured -> the default, capped by availability
        for (available, expected) in cases {
            assert_eq!(visible_count(0.0, available, CARD, GAP, 3), expected);
        }
    }

    // ---- selection / scoring ----

    fn entry(position: f64, duration: f64, updated_at_unix: i64) -> FileEntry {
        FileEntry {
            position,
            duration,
            updated_at_unix,
            ..FileEntry::default()
        }
    }

    fn select<'a>(
        entries: &'a [(&'a str, FileEntry)],
        private: bool,
        max_cards: usize,
    ) -> Vec<ContinueWatchingCard> {
        select_continue_watching(
            entries.iter().map(|(path, record)| (*path, record)),
            private,
            max_cards,
        )
    }

    #[test]
    fn is_resumable_matches_the_five_percent_and_completion_window() {
        // 600s file: resumable is (30, 570) exclusive — past 5% (30s) and before the last 30s.
        assert!(is_resumable(120.0, 600.0));
        assert!(!is_resumable(30.0, 600.0)); // exactly 5% -> not yet
        assert!(!is_resumable(29.0, 600.0)); // under 5%
        assert!(!is_resumable(570.0, 600.0)); // exactly the completion window
        assert!(!is_resumable(590.0, 600.0)); // inside the last 30s
        // Degenerate durations are never resumable.
        assert!(!is_resumable(10.0, 0.0));
        assert!(!is_resumable(10.0, f64::NAN));
        assert!(!is_resumable(f64::INFINITY, 600.0));
    }

    #[test]
    fn select_keeps_only_resumable_entries_newest_first() {
        let entries = [
            ("/media/a.mkv", entry(120.0, 600.0, 100)), // resumable, oldest
            ("/media/b.mkv", entry(300.0, 600.0, 300)), // resumable, newest
            ("/media/c.mkv", entry(10.0, 600.0, 200)),  // under 5% -> dropped
            ("/media/d.mkv", entry(0.0, 600.0, 250)),   // finished-style position 0 -> dropped
        ];
        let cards = select(&entries, false, 10);
        let paths: Vec<&str> = cards.iter().map(|card| card.path.as_str()).collect();
        assert_eq!(paths, ["/media/b.mkv", "/media/a.mkv"]);
    }

    #[test]
    fn select_drops_finished_files_even_with_a_resumable_position() {
        let mut finished = entry(120.0, 600.0, 100);
        finished.finished = true;
        let entries = [("/media/done.mkv", finished)];
        assert!(select(&entries, false, 10).is_empty());
    }

    #[test]
    fn select_returns_nothing_in_a_private_session() {
        let entries = [("/media/a.mkv", entry(120.0, 600.0, 100))];
        assert!(
            select(&entries, true, 10).is_empty(),
            "a private session must not leak recents"
        );
    }

    #[test]
    fn select_caps_to_max_cards_and_assigns_rotating_palette_slots() {
        let entries = [
            ("/media/a.mkv", entry(120.0, 600.0, 300)),
            ("/media/b.mkv", entry(120.0, 600.0, 200)),
            ("/media/c.mkv", entry(120.0, 600.0, 100)),
        ];
        let cards = select(&entries, false, 2);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].palette_index, 0);
        assert_eq!(cards[1].palette_index, 1);
        assert!(select(&entries, false, 0).is_empty());
    }

    #[test]
    fn select_breaks_equal_timestamps_by_path_for_determinism() {
        let entries = [
            ("/media/z.mkv", entry(120.0, 600.0, 500)),
            ("/media/a.mkv", entry(120.0, 600.0, 500)),
        ];
        let paths: Vec<String> = select(&entries, false, 10)
            .into_iter()
            .map(|card| card.path)
            .collect();
        assert_eq!(paths, ["/media/a.mkv", "/media/z.mkv"]);
    }

    #[test]
    fn select_projects_labels_progress_and_title() {
        // 4200/9660 watched: 70% -> ~90m left, 2h41m runtime.
        let entries = [("/media/Movies/Dune.mkv", entry(4200.0, 9660.0, 100))];
        let card = &select(&entries, false, 10)[0];
        assert_eq!(card.title, "Dune");
        assert_eq!(card.runtime_label, "2h 41m");
        assert_eq!(card.time_left_label, "1h 31m left");
        assert!((card.progress - 4200.0 / 9660.0).abs() < 1e-9);
        assert_eq!(card.updated_at_unix, 100);
        assert!(!card.is_audio);
    }

    #[test]
    fn select_prefers_the_cached_title_over_the_file_stem() {
        let mut record = entry(120.0, 600.0, 100);
        record.title = Some("The Cached Title".to_owned());
        let entries = [("/media/raw-filename.mkv", record)];
        assert_eq!(select(&entries, false, 10)[0].title, "The Cached Title");
    }

    #[test]
    fn card_title_falls_back_to_the_stem_across_separators() {
        assert_eq!(card_title(r"C:\Movies\Dune.2160p.mkv", None), "Dune.2160p");
        assert_eq!(card_title("/media/clip.mp4", Some("")), "clip");
        assert_eq!(card_title("bare", None), "bare");
        assert_eq!(card_title("/media/.hidden", None), ".hidden");
    }

    #[test]
    fn runtime_and_time_left_labels_humanise_hours_and_clamp() {
        assert_eq!(runtime_label(9660.0), "2h 41m");
        assert_eq!(runtime_label(2700.0), "45m");
        assert_eq!(runtime_label(-5.0), "0m");
        assert_eq!(time_left_label(3900.0), "1h 5m left");
        assert_eq!(time_left_label(2460.0), "41m left");
        assert_eq!(time_left_label(20.0), "1m left"); // clamps to a minute
        assert_eq!(time_left_label(-5.0), "1m left");
    }

    #[test]
    fn shelf_header_is_listening_only_when_every_card_is_audio() {
        let audio = select(&[("/media/song.flac", entry(120.0, 600.0, 100))], false, 10);
        assert_eq!(shelf_header(&audio), "Continue listening");

        let video = select(&[("/media/film.mkv", entry(120.0, 600.0, 100))], false, 10);
        assert_eq!(shelf_header(&video), "Continue watching");

        let mixed = select(
            &[
                ("/media/song.flac", entry(120.0, 600.0, 200)),
                ("/media/film.mkv", entry(120.0, 600.0, 100)),
            ],
            false,
            10,
        );
        assert_eq!(shelf_header(&mixed), "Continue watching");
        assert_eq!(shelf_header(&[]), "Continue watching");
    }
}
