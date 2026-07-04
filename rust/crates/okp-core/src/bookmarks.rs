//! Pure add/remove/sort/dedup logic for the user's per-file position bookmarks — the
//! marks a viewer drops at the current playhead to jump back to later. Port of the
//! `AddBookmark`/`RemoveBookmark` mutators in `src/OkPlayer.Core/HistoryService.cs`; the
//! `Bookmarks_AddDedupeRemove` case in `tests/OkPlayer.Tests/HistoryServiceTests.cs` is
//! the executable spec. This is list math only over the seconds stored in
//! [`crate::history::FileEntry::bookmarks`]; where the history document lives and how it
//! is read/written stays behind the shell seam, exactly like [`crate::history`] itself.
//!
//! A bookmark is a bare timestamp (Windows `BookmarkEntry` carries only `Time`), so this
//! module deliberately has no title/rename surface — that is the separate
//! `UserChapters` story ([`crate::history::ChapterMark`]), not yet surfaced on Linux.

/// A new bookmark within this many seconds of an existing one is treated as the same
/// spot and refused, so a double-tap at one position does not stack two marks. Mirrors
/// C# `HistoryService.AddBookmark`'s `Math.Abs(existing - time) < 0.5` guard.
pub const ADD_DEDUPE_EPSILON: f64 = 0.5;

/// Tolerance for locating the bookmark to remove — the stored value plus a hair of
/// float slack. Mirrors C# `HistoryService.RemoveBookmark`'s `Math.Abs(existing - time)
/// < 0.01`.
pub const REMOVE_MATCH_EPSILON: f64 = 0.01;

/// Insert `time` as a bookmark, keeping the list sorted and free of near-duplicates.
/// Returns `true` when it was added, `false` when a bookmark already sits within
/// [`ADD_DEDUPE_EPSILON`] or `time` is not a usable position. Non-finite or negative
/// times are rejected: the shell only ever bookmarks a real playhead, but guarding here
/// keeps the "sorted, finite, no near-duplicates" invariant local to this module.
pub fn add(bookmarks: &mut Vec<f64>, time: f64) -> bool {
    if !time.is_finite() || time < 0.0 {
        return false;
    }
    if bookmarks
        .iter()
        .any(|existing| (existing - time).abs() < ADD_DEDUPE_EPSILON)
    {
        return false;
    }

    bookmarks.push(time);
    bookmarks.sort_by(f64::total_cmp);
    true
}

/// Remove every bookmark within [`REMOVE_MATCH_EPSILON`] of `time`. Returns `true` when
/// at least one mark was dropped. Matching a small window (rather than exact equality)
/// lets a row built from a formatted/rounded timestamp still find its stored mark.
pub fn remove(bookmarks: &mut Vec<f64>, time: f64) -> bool {
    let before = bookmarks.len();
    bookmarks.retain(|existing| (existing - time).abs() >= REMOVE_MATCH_EPSILON);
    bookmarks.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_inserts_and_keeps_the_list_sorted() {
        let mut bookmarks = Vec::new();
        assert!(add(&mut bookmarks, 50.0));
        assert!(add(&mut bookmarks, 10.0));
        assert!(add(&mut bookmarks, 30.0));
        assert_eq!(bookmarks, [10.0, 30.0, 50.0]);
    }

    #[test]
    fn add_refuses_a_near_duplicate_within_half_a_second() {
        let mut bookmarks = vec![100.0];
        // 100.2 is inside the 0.5 s window -> refused, list unchanged.
        assert!(!add(&mut bookmarks, 100.2));
        assert!(!add(&mut bookmarks, 99.6));
        assert_eq!(bookmarks, [100.0]);
    }

    #[test]
    fn add_accepts_a_mark_exactly_half_a_second_away() {
        // The guard is strict (`< 0.5`), so a mark 0.5 s away is a distinct spot.
        let mut bookmarks = vec![100.0];
        assert!(add(&mut bookmarks, 100.5));
        assert_eq!(bookmarks, [100.0, 100.5]);
    }

    #[test]
    fn add_rejects_non_finite_and_negative_times() {
        let mut bookmarks = Vec::new();
        assert!(!add(&mut bookmarks, f64::NAN));
        assert!(!add(&mut bookmarks, f64::INFINITY));
        assert!(!add(&mut bookmarks, -1.0));
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn remove_drops_the_matching_mark_and_reports_it() {
        let mut bookmarks = vec![10.0, 100.0];
        // Within 0.01 s of the stored 100.0.
        assert!(remove(&mut bookmarks, 100.005));
        assert_eq!(bookmarks, [10.0]);
    }

    #[test]
    fn remove_returns_false_when_nothing_matches() {
        let mut bookmarks = vec![10.0, 100.0];
        assert!(!remove(&mut bookmarks, 55.0));
        assert_eq!(bookmarks, [10.0, 100.0]);
    }

    // The Windows `Bookmarks_AddDedupeRemove` spec, end to end: add 100, a 100.2 add is
    // deduped to a single mark, then remove empties the list.
    #[test]
    fn add_dedupe_remove_round_trip() {
        let mut bookmarks = Vec::new();
        assert!(add(&mut bookmarks, 100.0));
        assert!(!add(&mut bookmarks, 100.2));
        assert_eq!(bookmarks, [100.0]);
        assert!(remove(&mut bookmarks, 100.0));
        assert!(bookmarks.is_empty());
    }
}
