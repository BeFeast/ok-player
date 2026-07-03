//! Pure chapter logic — the merge, current-by-time, prev/next and seek-bar-fraction math
//! behind the player's chapter list. Port of `src/OkPlayer.Core/ChapterMath.cs`; the C# suite
//! in `tests/OkPlayer.Tests/ChapterMathTests.cs` is the executable spec. No UI or engine
//! dependency.

/// A chapter after merging the file's own with the user's: time-sorted and re-indexed.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedChapter {
    pub index: usize,
    pub time: f64,
    pub title: String,
    pub is_user_defined: bool,
}

/// The C# default for [`current_index`]'s `epsilon`.
pub const DEFAULT_EPSILON: f64 = 0.25;

/// Merge the file's chapters (read-only) with the user's into one time-sorted, re-indexed
/// list. A stable sort keeps a file and user chapter at the same timestamp in file-then-user
/// order.
pub fn merge<F: AsRef<str>, U: AsRef<str>>(
    file_chapters: &[(f64, F)],
    user_chapters: &[(f64, U)],
) -> Vec<MergedChapter> {
    let mut tagged: Vec<(f64, &str, bool)> =
        Vec::with_capacity(file_chapters.len() + user_chapters.len());
    for (time, title) in file_chapters {
        tagged.push((*time, title.as_ref(), false));
    }
    for (time, title) in user_chapters {
        tagged.push((*time, title.as_ref(), true));
    }
    // Stable sort: ties keep insertion (file-then-user) order.
    tagged.sort_by(|a, b| a.0.total_cmp(&b.0));

    tagged
        .into_iter()
        .enumerate()
        .map(|(index, (time, title, is_user_defined))| MergedChapter {
            index,
            time,
            title: title.to_string(),
            is_user_defined,
        })
        .collect()
}

/// Index of the chapter containing `position` (the last start <= position, within `epsilon`),
/// or `None` before the first chapter. `times` ascending.
pub fn current_index(times: &[f64], position: f64, epsilon: f64) -> Option<usize> {
    let mut index = None;
    for (i, time) in times.iter().enumerate() {
        if *time <= position + epsilon {
            index = Some(i);
        } else {
            break;
        }
    }
    index
}

/// Target index for a prev/next-chapter jump, or `None` when already at the first/last
/// chapter — so a jump at a boundary does nothing rather than rewinding to the current
/// chapter's own start. `current` is `None` while before the first chapter.
pub fn jump_target(current: Option<usize>, delta: i32, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let current = current.map_or(-1, |i| i as i64);
    let target = current + i64::from(delta);
    if target >= 0 && target < count as i64 {
        Some(target as usize)
    } else {
        None
    }
}

/// Chapter start positions as 0..1 fractions for the seek-bar tick markers (empty if no
/// duration).
pub fn fractions(times: &[f64], duration: f64) -> Vec<f64> {
    if duration <= 0.0 {
        return Vec::new();
    }
    times
        .iter()
        .map(|t| (t / duration).clamp(0.0, 1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_file_and_user_are_time_sorted_reindexed_and_tagged() {
        let file = [(0.0, "Intro"), (30.0, "End")];
        let user = [(15.0, "Mid")];

        let merged = merge(&file, &user);

        assert_eq!(merged.len(), 3);
        let indices: Vec<usize> = merged.iter().map(|m| m.index).collect();
        assert_eq!(indices, [0, 1, 2]);
        let titles: Vec<&str> = merged.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["Intro", "Mid", "End"]);
        let user_flags: Vec<bool> = merged.iter().map(|m| m.is_user_defined).collect();
        assert_eq!(user_flags, [false, true, false]);
    }

    #[test]
    fn merge_on_equal_time_keeps_file_before_user() {
        let file = [(10.0, "FileAt10")];
        let user = [(10.0, "UserAt10")];

        let merged = merge(&file, &user);

        assert_eq!(merged[0].title, "FileAt10");
        assert!(!merged[0].is_user_defined);
        assert!(merged[1].is_user_defined);
    }

    #[test]
    fn merge_empty_returns_empty() {
        assert!(merge::<&str, &str>(&[], &[]).is_empty());
    }

    #[test]
    fn current_index_picks_last_started_chapter() {
        let times = [0.0, 15.0, 30.0];
        let cases = [
            (-5.0, None),     // before the first chapter
            (0.0, Some(0)),   // exactly on the first start
            (12.0, Some(0)),  // inside chapter 0
            (15.0, Some(1)),  // exactly on chapter 1
            (999.0, Some(2)), // past the last start -> last chapter
        ];
        for (position, expected) in cases {
            assert_eq!(
                current_index(&times, position, DEFAULT_EPSILON),
                expected,
                "position {position}"
            );
        }
    }

    #[test]
    fn current_index_within_epsilon_counts_as_started() {
        // 0.2 <= 0 + 0.25
        assert_eq!(current_index(&[0.2], 0.0, DEFAULT_EPSILON), Some(0));
    }

    #[test]
    fn current_index_no_chapters_is_none() {
        assert_eq!(current_index(&[], 50.0, DEFAULT_EPSILON), None);
    }

    // The boundary cases Greptile flagged: a prev/next jump at an end must not rewind to the
    // same chapter.
    #[test]
    fn jump_target_next_at_last_chapter_returns_none() {
        assert_eq!(jump_target(Some(2), 1, 3), None);
    }

    #[test]
    fn jump_target_prev_at_first_chapter_returns_none() {
        assert_eq!(jump_target(Some(0), -1, 3), None);
    }

    #[test]
    fn jump_target_from_inside_returns_adjacent() {
        let cases = [
            (Some(1), 1, Some(2)),  // next from the middle
            (Some(1), -1, Some(0)), // prev from the middle
            (None, 1, Some(0)),     // next while before the first chapter -> chapter 0
        ];
        for (current, delta, expected) in cases {
            assert_eq!(jump_target(current, delta, 3), expected);
        }
    }

    #[test]
    fn jump_target_no_chapters_returns_none() {
        assert_eq!(jump_target(None, 1, 0), None);
    }

    #[test]
    fn fractions_divide_starts_by_duration() {
        assert_eq!(fractions(&[0.0, 60.0, 90.0], 120.0), [0.0, 0.5, 0.75]);
    }

    #[test]
    fn fractions_zero_duration_is_empty() {
        assert!(fractions(&[0.0, 60.0], 0.0).is_empty());
    }

    #[test]
    fn fractions_past_duration_clamped_to_one() {
        assert_eq!(fractions(&[200.0], 100.0), [1.0]);
    }
}
