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

/// Where a chapter-like marker came from. Embedded chapters are the file's own,
/// authoritative and read-only. Interval markers are synthesized as an immediate fallback
/// when a file carries no embedded chapters. Generated chapters come from on-demand scene
/// detection. Bookmarks are the user's saved positions surfaced as a marker. Keeping the
/// source explicit lets every surface style and label the four kinds distinctly and — the
/// point of the freeze-boundary here — never fold a synthesized or user mark into the
/// file's embedded chapter list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterSource {
    Embedded,
    Interval,
    Generated,
    Bookmark,
}

impl ChapterSource {
    /// The noun a surface uses for one marker of this source (e.g. a row title prefix or a
    /// section heading stem).
    pub fn label(self) -> &'static str {
        match self {
            ChapterSource::Embedded => "Chapter",
            ChapterSource::Interval => "Marker",
            ChapterSource::Generated => "Scene",
            ChapterSource::Bookmark => "Bookmark",
        }
    }

    /// Whether markers of this source are produced by the app rather than read from the
    /// file's own metadata. Synthesized markers (interval, scene detection) are styled as a
    /// softer, clearly-derived layer so they never read as the file's authoritative chapters.
    pub fn is_synthesized(self) -> bool {
        matches!(self, ChapterSource::Interval | ChapterSource::Generated)
    }
}

/// A synthesized interval marker: its 0-based ordinal and start time in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct IntervalChapter {
    pub index: usize,
    pub time: f64,
}

/// The most interval markers to synthesize for one clip, so a long film yields a handful of
/// useful divisions rather than hundreds of ticks. Also a hard guard on
/// [`interval_chapters`] against a pathologically small interval.
pub const MAX_INTERVAL_CHAPTERS: usize = 12;

/// Round interval steps in seconds, smallest first: 30s, 1m, 2m, 5m, 10m, 15m, 30m, 1h.
/// [`suggested_interval`] snaps to one of these so the fallback markers land on
/// human-friendly spacing instead of an odd `duration / n` value.
const INTERVAL_STEPS_SECS: &[f64] = &[30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0];

/// Pick a round interval (seconds) that divides `duration` into at most
/// [`MAX_INTERVAL_CHAPTERS`] markers, preferring the smallest step that stays under the cap
/// so short clips still get a few divisions. Returns `None` when there is no usable duration
/// or the clip is too short for even the smallest step to yield a second marker — that
/// "nothing to divide" case is what the shell reads as "no interval fallback to offer".
pub fn suggested_interval(duration: f64) -> Option<f64> {
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    for &step in INTERVAL_STEPS_SECS {
        // `duration > step` guarantees at least the 0 s and one interior marker; the ceil
        // keeps the count at or under the cap.
        if duration > step && (duration / step).ceil() as usize <= MAX_INTERVAL_CHAPTERS {
            return Some(step);
        }
    }
    // Longer than the largest round step can cover within the cap: split evenly into the cap.
    let largest_step = INTERVAL_STEPS_SECS[INTERVAL_STEPS_SECS.len() - 1];
    (duration > largest_step).then(|| duration / MAX_INTERVAL_CHAPTERS as f64)
}

/// Synthesize interval markers every `interval` seconds across a clip of `duration` seconds,
/// starting at 0.0. Each marker's start sits strictly before `duration` (the clip's end is
/// never itself a marker start). Empty when either input is not a usable positive value. The
/// count is capped at [`MAX_INTERVAL_CHAPTERS`] as a final guard even if a caller passes an
/// unusually small interval.
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

/// The interval markers to offer as a fallback for a clip of `duration` seconds: the
/// [`suggested_interval`] spacing filled in by [`interval_chapters`], or empty when the clip
/// is too short (or its duration unknown) to divide usefully. This is the single entry point
/// the shell calls when a file has no embedded chapters.
pub fn fallback_interval_chapters(duration: f64) -> Vec<IntervalChapter> {
    match suggested_interval(duration) {
        Some(interval) => interval_chapters(duration, interval),
        None => Vec::new(),
    }
}

/// Which chapter source a file's chapter surfaces should present right now: its own embedded
/// chapters when it has any, otherwise synthesized interval markers when the duration is long
/// enough to divide, otherwise nothing to show. The shell reads this to choose between the
/// read-only embedded spine, the interval fallback, and an honest empty state — the branch
/// that keeps a metadata-less file from showing a dead panel.
pub fn active_chapter_source(has_embedded_chapters: bool, duration: f64) -> Option<ChapterSource> {
    if has_embedded_chapters {
        Some(ChapterSource::Embedded)
    } else if suggested_interval(duration).is_some() {
        Some(ChapterSource::Interval)
    } else {
        None
    }
}

/// The state of the on-demand "Detect chapters" action. Scene detection is expensive and its
/// engine is not wired on every platform yet, so the entry point stays explicit and its
/// states stay honest: nothing pretends to run when no engine exists. The variants are ready
/// for a real engine (progress, a result count) so wiring one in later needs no new states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChapterDetection {
    /// Not started — the "Detect chapters" affordance is offered.
    #[default]
    Idle,
    /// Detection is running; carries a 0..=100 progress percent for the UI.
    Detecting { percent: u8 },
    /// Detection finished and produced this many chapters.
    Done { count: usize },
    /// Detection could not run (no engine wired) or failed. The surface pairs this with an
    /// honest, user-facing reason string it owns.
    Unavailable,
}

impl ChapterDetection {
    /// Begin detection from [`ChapterDetection::Idle`]. With an engine wired this returns
    /// [`ChapterDetection::Detecting`] at 0%; without one it resolves straight to
    /// [`ChapterDetection::Unavailable`] rather than showing a progress bar that would never
    /// advance — the honest failure the PRD asks for over a faked "detecting" spinner.
    pub fn begin(engine_available: bool) -> Self {
        if engine_available {
            ChapterDetection::Detecting { percent: 0 }
        } else {
            ChapterDetection::Unavailable
        }
    }

    /// Whether detection is actively running (so the entry point shows progress rather than a
    /// re-triggerable action).
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
    fn chapter_source_labels_and_synthesized_flag_separate_the_four_kinds() {
        // Every source has its own noun so a surface never mislabels a synthesized or user
        // mark as a file chapter.
        assert_eq!(ChapterSource::Embedded.label(), "Chapter");
        assert_eq!(ChapterSource::Interval.label(), "Marker");
        assert_eq!(ChapterSource::Generated.label(), "Scene");
        assert_eq!(ChapterSource::Bookmark.label(), "Bookmark");

        // Only the app-produced kinds are "synthesized" — the file's own chapters and the
        // user's bookmarks are not, so they keep authoritative (non-derived) styling.
        assert!(!ChapterSource::Embedded.is_synthesized());
        assert!(ChapterSource::Interval.is_synthesized());
        assert!(ChapterSource::Generated.is_synthesized());
        assert!(!ChapterSource::Bookmark.is_synthesized());
    }

    #[test]
    fn suggested_interval_snaps_to_round_steps_under_the_cap() {
        // Each duration snaps to the smallest round step whose marker count stays within
        // MAX_INTERVAL_CHAPTERS.
        assert_eq!(suggested_interval(45.0), Some(30.0)); // 0, 30
        assert_eq!(suggested_interval(600.0), Some(60.0)); // 10 min -> 1 min steps
        assert_eq!(suggested_interval(3600.0), Some(300.0)); // 1 h -> 5 min steps
        assert_eq!(suggested_interval(7200.0), Some(600.0)); // 2 h -> 10 min steps
        assert_eq!(suggested_interval(10800.0), Some(900.0)); // 3 h -> 15 min steps
    }

    #[test]
    fn suggested_interval_none_for_short_unknown_or_invalid_durations() {
        // Too short for even a 30 s step to yield a second marker.
        assert_eq!(suggested_interval(30.0), None);
        assert_eq!(suggested_interval(12.0), None);
        // No usable duration.
        assert_eq!(suggested_interval(0.0), None);
        assert_eq!(suggested_interval(-5.0), None);
        assert_eq!(suggested_interval(f64::NAN), None);
        assert_eq!(suggested_interval(f64::INFINITY), None);
    }

    #[test]
    fn suggested_interval_splits_evenly_past_the_largest_round_step() {
        // Longer than 1 h * cap: no round step fits, so split into exactly the cap.
        let interval = suggested_interval(100_000.0).expect("very long clip still divides");
        assert_eq!(interval, 100_000.0 / MAX_INTERVAL_CHAPTERS as f64);
    }

    #[test]
    fn interval_chapters_are_evenly_spaced_and_end_before_duration() {
        let markers = interval_chapters(600.0, 60.0);
        let times: Vec<f64> = markers.iter().map(|m| m.time).collect();
        // 0, 60, ... 540 — ten markers, and 600 (the end) is not itself a marker.
        assert_eq!(
            times,
            [
                0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0, 420.0, 480.0, 540.0
            ]
        );
        let indices: Vec<usize> = markers.iter().map(|m| m.index).collect();
        assert_eq!(indices, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn interval_chapters_reject_unusable_inputs_and_cap_the_count() {
        assert!(interval_chapters(0.0, 60.0).is_empty());
        assert!(interval_chapters(600.0, 0.0).is_empty());
        assert!(interval_chapters(600.0, f64::NAN).is_empty());
        assert!(interval_chapters(f64::INFINITY, 60.0).is_empty());
        // A tiny interval over a long clip is truncated to the hard cap rather than emitting
        // thousands of markers.
        assert_eq!(
            interval_chapters(100_000.0, 1.0).len(),
            MAX_INTERVAL_CHAPTERS
        );
    }

    #[test]
    fn fallback_interval_chapters_fill_the_suggested_spacing() {
        // A one-hour clip snaps to 5-minute steps and fills 12 markers (0..55 min).
        let markers = fallback_interval_chapters(3600.0);
        assert_eq!(markers.len(), 12);
        assert_eq!(markers.first().unwrap().time, 0.0);
        assert_eq!(markers.last().unwrap().time, 3300.0);
        // A clip too short to divide offers no fallback.
        assert!(fallback_interval_chapters(20.0).is_empty());
    }

    #[test]
    fn active_chapter_source_prefers_embedded_then_interval_then_nothing() {
        // Embedded wins whenever the file has its own chapters, whatever the duration.
        assert_eq!(
            active_chapter_source(true, 3600.0),
            Some(ChapterSource::Embedded)
        );
        assert_eq!(
            active_chapter_source(true, 0.0),
            Some(ChapterSource::Embedded)
        );
        // No embedded chapters but a divisible duration -> interval fallback.
        assert_eq!(
            active_chapter_source(false, 3600.0),
            Some(ChapterSource::Interval)
        );
        // No embedded chapters and nothing to divide -> no source (honest empty state).
        assert_eq!(active_chapter_source(false, 10.0), None);
        assert_eq!(active_chapter_source(false, 0.0), None);
    }

    #[test]
    fn chapter_detection_begin_is_honest_without_an_engine() {
        // No engine wired: the action fails honestly instead of faking progress.
        assert_eq!(
            ChapterDetection::begin(false),
            ChapterDetection::Unavailable
        );
        // With an engine it starts a real progress run.
        assert_eq!(
            ChapterDetection::begin(true),
            ChapterDetection::Detecting { percent: 0 }
        );
    }

    #[test]
    fn chapter_detection_is_running_only_while_detecting() {
        assert!(!ChapterDetection::default().is_running());
        assert!(!ChapterDetection::Idle.is_running());
        assert!(ChapterDetection::Detecting { percent: 40 }.is_running());
        assert!(!ChapterDetection::Done { count: 3 }.is_running());
        assert!(!ChapterDetection::Unavailable.is_running());
    }
}
