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

/// Where a chapter-like marker came from. Embedded chapters are authoritative file
/// metadata; interval and generated chapters are synthesized by the player; bookmarks are
/// user-authored. Keeping the source explicit prevents derived markers from being folded into
/// the embedded chapter list or presented with the same semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterSource {
    Embedded,
    Interval,
    Generated,
    Bookmark,
}

impl ChapterSource {
    /// The noun used for one marker of this source.
    pub fn label(self) -> &'static str {
        match self {
            ChapterSource::Embedded => "Chapter",
            ChapterSource::Interval => "Marker",
            ChapterSource::Generated => "Scene",
            ChapterSource::Bookmark => "Bookmark",
        }
    }

    /// Whether the player produced the marker rather than reading it from metadata or a
    /// user-authored bookmark.
    pub fn is_synthesized(self) -> bool {
        matches!(self, ChapterSource::Interval | ChapterSource::Generated)
    }
}

/// A synthesized interval marker.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalChapter {
    pub index: usize,
    pub time: f64,
}

/// The maximum number of interval markers generated for one media item.
pub const MAX_INTERVAL_CHAPTERS: usize = 12;

/// Human-friendly interval steps, from 30 seconds through one hour.
const INTERVAL_STEPS_SECS: &[f64] = &[30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0];

/// Choose the smallest round interval that divides `duration` into no more than
/// [`MAX_INTERVAL_CHAPTERS`] markers. Very long media is divided evenly at the cap.
pub fn suggested_interval(duration: f64) -> Option<f64> {
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }

    for &step in INTERVAL_STEPS_SECS {
        if duration > step && (duration / step).ceil() as usize <= MAX_INTERVAL_CHAPTERS {
            return Some(step);
        }
    }

    let largest_step = INTERVAL_STEPS_SECS[INTERVAL_STEPS_SECS.len() - 1];
    (duration > largest_step).then(|| duration / MAX_INTERVAL_CHAPTERS as f64)
}

/// Generate markers every `interval` seconds, beginning at zero and ending strictly before
/// the media duration. Invalid inputs produce no markers and the output is always capped.
pub fn interval_chapters(duration: f64, interval: f64) -> Vec<IntervalChapter> {
    if !duration.is_finite() || duration <= 0.0 || !interval.is_finite() || interval <= 0.0 {
        return Vec::new();
    }

    let mut chapters = Vec::new();
    for index in 0..MAX_INTERVAL_CHAPTERS {
        let time = index as f64 * interval;
        if time >= duration {
            break;
        }
        chapters.push(IntervalChapter { index, time });
    }
    chapters
}

/// Generate the immediate interval fallback for a known media duration.
pub fn fallback_interval_chapters(duration: f64) -> Vec<IntervalChapter> {
    suggested_interval(duration)
        .map(|interval| interval_chapters(duration, interval))
        .unwrap_or_default()
}

/// Select the authoritative chapter source currently available for a media item.
pub fn active_chapter_source(has_embedded_chapters: bool, duration: f64) -> Option<ChapterSource> {
    if has_embedded_chapters {
        Some(ChapterSource::Embedded)
    } else if suggested_interval(duration).is_some() {
        Some(ChapterSource::Interval)
    } else {
        None
    }
}

/// State for the explicit, non-blocking chapter detection action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChapterDetection {
    #[default]
    Idle,
    Detecting {
        percent: u8,
    },
    Done {
        count: usize,
    },
    Unavailable,
}

impl ChapterDetection {
    /// Start a real progress state only when an engine exists. Otherwise fail immediately and
    /// honestly instead of displaying progress that cannot advance.
    pub fn begin(engine_available: bool) -> Self {
        if engine_available {
            ChapterDetection::Detecting { percent: 0 }
        } else {
            ChapterDetection::Unavailable
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, ChapterDetection::Detecting { .. })
    }
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

    #[test]
    fn chapter_sources_keep_embedded_interval_generated_and_bookmark_distinct() {
        assert_eq!(ChapterSource::Embedded.label(), "Chapter");
        assert_eq!(ChapterSource::Interval.label(), "Marker");
        assert_eq!(ChapterSource::Generated.label(), "Scene");
        assert_eq!(ChapterSource::Bookmark.label(), "Bookmark");

        assert!(!ChapterSource::Embedded.is_synthesized());
        assert!(ChapterSource::Interval.is_synthesized());
        assert!(ChapterSource::Generated.is_synthesized());
        assert!(!ChapterSource::Bookmark.is_synthesized());
    }

    #[test]
    fn suggested_interval_uses_round_steps_under_the_cap() {
        assert_eq!(suggested_interval(45.0), Some(30.0));
        assert_eq!(suggested_interval(600.0), Some(60.0));
        assert_eq!(suggested_interval(3600.0), Some(300.0));
        assert_eq!(suggested_interval(7200.0), Some(600.0));
        assert_eq!(suggested_interval(10800.0), Some(900.0));
    }

    #[test]
    fn suggested_interval_rejects_short_unknown_and_invalid_durations() {
        assert_eq!(suggested_interval(30.0), None);
        assert_eq!(suggested_interval(12.0), None);
        assert_eq!(suggested_interval(0.0), None);
        assert_eq!(suggested_interval(-5.0), None);
        assert_eq!(suggested_interval(f64::NAN), None);
        assert_eq!(suggested_interval(f64::INFINITY), None);
    }

    #[test]
    fn suggested_interval_evenly_splits_media_beyond_the_largest_round_step() {
        assert_eq!(
            suggested_interval(100_000.0),
            Some(100_000.0 / MAX_INTERVAL_CHAPTERS as f64)
        );
    }

    #[test]
    fn interval_chapters_are_evenly_spaced_and_stop_before_the_end() {
        let markers = interval_chapters(600.0, 60.0);
        assert_eq!(
            markers.iter().map(|marker| marker.time).collect::<Vec<_>>(),
            [
                0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0, 420.0, 480.0, 540.0
            ]
        );
        assert_eq!(
            markers
                .iter()
                .map(|marker| marker.index)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
    }

    #[test]
    fn interval_chapters_reject_invalid_inputs_and_cap_output() {
        assert!(interval_chapters(0.0, 60.0).is_empty());
        assert!(interval_chapters(600.0, 0.0).is_empty());
        assert!(interval_chapters(600.0, f64::NAN).is_empty());
        assert!(interval_chapters(f64::INFINITY, 60.0).is_empty());
        assert_eq!(
            interval_chapters(100_000.0, 1.0).len(),
            MAX_INTERVAL_CHAPTERS
        );
    }

    #[test]
    fn fallback_interval_chapters_fill_the_suggested_spacing() {
        let markers = fallback_interval_chapters(3600.0);
        assert_eq!(markers.len(), 12);
        assert_eq!(markers.first().map(|marker| marker.time), Some(0.0));
        assert_eq!(markers.last().map(|marker| marker.time), Some(3300.0));
        assert!(fallback_interval_chapters(20.0).is_empty());
    }

    #[test]
    fn active_source_prefers_embedded_then_interval_then_nothing() {
        assert_eq!(
            active_chapter_source(true, 3600.0),
            Some(ChapterSource::Embedded)
        );
        assert_eq!(
            active_chapter_source(true, 0.0),
            Some(ChapterSource::Embedded)
        );
        assert_eq!(
            active_chapter_source(false, 3600.0),
            Some(ChapterSource::Interval)
        );
        assert_eq!(active_chapter_source(false, 10.0), None);
        assert_eq!(active_chapter_source(false, 0.0), None);
    }

    #[test]
    fn chapter_detection_only_reports_progress_when_an_engine_exists() {
        assert_eq!(
            ChapterDetection::begin(false),
            ChapterDetection::Unavailable
        );
        assert_eq!(
            ChapterDetection::begin(true),
            ChapterDetection::Detecting { percent: 0 }
        );
        assert!(!ChapterDetection::Idle.is_running());
        assert!(ChapterDetection::Detecting { percent: 40 }.is_running());
        assert!(!ChapterDetection::Done { count: 3 }.is_running());
        assert!(!ChapterDetection::Unavailable.is_running());
    }
}
