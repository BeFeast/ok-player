//! Pure selection and layout rules for the welcome "Continue watching" shelf.
//!
//! The fit math is a port of `src/OkPlayer.Core/RecentsShelf.cs`; the C# suite in
//! `tests/OkPlayer.Tests/RecentsShelfTests.cs` is the executable spec. Linux also uses this
//! module for reusable history ranking, resumable filtering, private-session suppression,
//! and search matching so the GTK shell only renders the resulting model.

use crate::history::{FileEntry, History};
use crate::history_format::{self, HistoryStateKind};

/// A history row prepared for either the welcome shelf or the full History surface.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryItem {
    pub path: String,
    pub title: String,
    pub location: String,
    pub position: f64,
    pub duration: f64,
    pub progress: f64,
    pub state_kind: HistoryStateKind,
    pub state_label: String,
    pub updated_at_unix: i64,
    pub poster_path: Option<String>,
}

/// The welcome shelf's privacy-aware state. A private session deliberately carries no items,
/// even when the persisted history has resumable records.
#[derive(Clone, Debug, PartialEq)]
pub enum WelcomeShelf {
    Private,
    Empty,
    Items(Vec<HistoryItem>),
}

/// Select the newest resumable history rows for the welcome surface.
pub fn select(history: &History, private_session: bool, limit: usize) -> WelcomeShelf {
    if private_session {
        return WelcomeShelf::Private;
    }

    let items = sorted_items(history)
        .into_iter()
        .filter(|item| history.files.get(&item.path).is_some_and(is_resumable))
        .take(limit)
        .collect::<Vec<_>>();

    if items.is_empty() {
        WelcomeShelf::Empty
    } else {
        WelcomeShelf::Items(items)
    }
}

/// Return all history rows newest-first, filtered by title, location, or full path.
pub fn search(history: &History, query: &str) -> Vec<HistoryItem> {
    let query = query.trim().to_lowercase();
    sorted_items(history)
        .into_iter()
        .filter(|item| {
            query.is_empty()
                || item.title.to_lowercase().contains(&query)
                || item.location.to_lowercase().contains(&query)
                || item.path.to_lowercase().contains(&query)
        })
        .collect()
}

/// Whether a record has a useful resume point: past the first 5%, before the completion
/// window, unfinished, and backed by finite progress values.
pub fn is_resumable(entry: &FileEntry) -> bool {
    !entry.finished
        && entry.duration.is_finite()
        && entry.duration > 0.0
        && entry.position.is_finite()
        && entry.position > entry.duration * 0.05
        && entry.position < completion_start(entry.duration)
}

/// Start of the completion window. Long media uses the final 30 seconds; short media uses
/// the final 5%, matching the Linux resume policy.
pub fn completion_start(duration: f64) -> f64 {
    (duration * 0.95).max(duration - 30.0)
}

/// Compact runtime label for cards and rows.
pub fn runtime_label(duration: f64) -> String {
    let total_minutes = (duration.max(0.0) / 60.0).floor() as i64;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{}m", minutes.max(1))
    }
}

/// Relative last-opened context derived from Unix seconds.
pub fn opened_context(updated_at_unix: i64, now_unix: i64) -> String {
    if updated_at_unix <= 0 {
        return "Opened previously".to_owned();
    }

    let age = now_unix.saturating_sub(updated_at_unix).max(0);
    match age {
        0..=59 => "Opened just now".to_owned(),
        60..=3_599 => plural(age / 60, "minute", "Opened"),
        3_600..=86_399 => plural(age / 3_600, "hour", "Opened"),
        86_400..=172_799 => "Opened yesterday".to_owned(),
        172_800..=604_799 => plural(age / 86_400, "day", "Opened"),
        604_800..=2_678_399 => plural(age / 604_800, "week", "Opened"),
        _ => plural(age / 2_592_000, "month", "Opened"),
    }
}

fn plural(value: i64, unit: &str, prefix: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{prefix} {value} {unit}{suffix} ago")
}

fn sorted_items(history: &History) -> Vec<HistoryItem> {
    let mut items = history
        .files
        .iter()
        .map(|(path, entry)| presentation_item(path, entry))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .updated_at_unix
            .cmp(&left.updated_at_unix)
            .then_with(|| left.path.cmp(&right.path))
    });
    items
}

fn presentation_item(path: &str, entry: &FileEntry) -> HistoryItem {
    let state = history_format::derive_state(entry.position, entry.duration, entry.finished);
    let title = entry
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path_title(path));
    let folder = history_format::folder_label(path);
    HistoryItem {
        path: path.to_owned(),
        title,
        location: if folder.is_empty() {
            path.to_owned()
        } else {
            folder
        },
        position: entry.position,
        duration: entry.duration,
        progress: state.percent.clamp(0.0, 1.0),
        state_kind: state.kind,
        state_label: if state.kind == HistoryStateKind::Finished {
            "Finished".to_owned()
        } else {
            state.label
        },
        updated_at_unix: entry.updated_at_unix,
        poster_path: entry.poster_path.clone(),
    }
}

fn path_title(path: &str) -> String {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file_name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or(file_name, |(stem, _)| stem);
    if stem.is_empty() {
        path.to_owned()
    } else {
        stem.to_owned()
    }
}

/// The C# default for `unmeasured_default` — what the shelf shows before its first measure.
pub const DEFAULT_UNMEASURED: usize = 3;

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
    use crate::history::FileEntry;
    use std::collections::BTreeMap;

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

    fn history(entries: [(&str, FileEntry); 4]) -> History {
        History {
            files: entries
                .into_iter()
                .map(|(path, entry)| (path.to_owned(), entry))
                .collect::<BTreeMap<_, _>>(),
            ..History::default()
        }
    }

    fn entry(position: f64, duration: f64, finished: bool, updated: i64) -> FileEntry {
        FileEntry {
            position,
            duration,
            finished,
            updated_at_unix: updated,
            ..FileEntry::default()
        }
    }

    #[test]
    fn select_returns_newest_resumable_items_only() {
        let history = history([
            ("/media/older.mkv", entry(120.0, 600.0, false, 10)),
            ("/media/finished.mkv", entry(0.0, 600.0, true, 40)),
            ("/media/barely.mkv", entry(20.0, 600.0, false, 30)),
            ("/media/newer.mkv", entry(240.0, 600.0, false, 20)),
        ]);

        let WelcomeShelf::Items(items) = select(&history, false, 10) else {
            panic!("expected shelf items");
        };
        assert_eq!(
            items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["newer", "older"]
        );
    }

    #[test]
    fn select_hides_all_persisted_items_during_private_session() {
        let history = history([
            ("/media/a.mkv", entry(120.0, 600.0, false, 40)),
            ("/media/b.mkv", entry(120.0, 600.0, false, 30)),
            ("/media/c.mkv", entry(120.0, 600.0, false, 20)),
            ("/media/d.mkv", entry(120.0, 600.0, false, 10)),
        ]);

        assert_eq!(select(&history, true, 10), WelcomeShelf::Private);
    }

    #[test]
    fn search_matches_title_location_and_full_path_case_insensitively() {
        let mut titled = entry(120.0, 600.0, false, 40);
        titled.title = Some("The Interview".to_owned());
        let history = history([
            ("/shows/Season 02/interview.mkv", titled),
            ("/music/Live/song.flac", entry(20.0, 300.0, false, 30)),
            ("/films/Arrival.mkv", entry(0.0, 600.0, true, 20)),
            ("/clips/demo.mp4", entry(10.0, 60.0, false, 10)),
        ]);

        assert_eq!(search(&history, "INTERVIEW").len(), 1);
        assert_eq!(search(&history, "season 02").len(), 1);
        assert_eq!(search(&history, "/films/").len(), 1);
        assert_eq!(search(&history, "missing").len(), 0);
        assert_eq!(search(&history, "").len(), 4);
    }

    #[test]
    fn opened_context_formats_recent_and_unknown_timestamps() {
        let now = 2_000_000;
        assert_eq!(opened_context(0, now), "Opened previously");
        assert_eq!(opened_context(now - 45, now), "Opened just now");
        assert_eq!(opened_context(now - 120, now), "Opened 2 minutes ago");
        assert_eq!(opened_context(now - 86_400, now), "Opened yesterday");
        assert_eq!(opened_context(now - 3 * 86_400, now), "Opened 3 days ago");
    }
}
