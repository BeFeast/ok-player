//! Pure presentation helpers for the History list — port of
//! `src/OkPlayer.Core/HistoryFormat.cs`; the C# suite in
//! `tests/OkPlayer.Tests/HistoryFormatTests.cs` is the executable spec. Day-bucketing, the
//! relative "when" label, the resume-state label, and a short folder breadcrumb — no engine or
//! UI dependency. Mirrors the Claude Design spec (design/OK-Player-History.dc.html —
//! deriveState, the bucket table and the master "when" strings) exactly, with invariant
//! weekday/month names so the English UI never localizes to "вт"/"июн".

use crate::settings::HistoryRetention;

/// Which day-group a history row falls into. Rows are bucketed by how long ago the file was
/// last opened, then shown under a header ([`bucket_header`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryBucket {
    Today,
    Yesterday,
    EarlierThisWeek,
    Earlier,
}

/// The watch-state a history row renders: a finished chip, a "time left" countdown, or a
/// "barely started" hint for files only a few minutes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStateKind {
    Finished,
    Progress,
    Barely,
}

/// Derived row state: the kind, the watched fraction (0..1, for the thumbnail fill) and the
/// human label shown on the right (e.g. "Finished", "23m left", "2m in · 4%").
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRowState {
    pub kind: HistoryStateKind,
    pub percent: f64,
    pub label: String,
}

/// A civil local date-time down to the minute — all the precision the history buckets and
/// labels read (the C# side passes a `DateTime`; nothing below the minute is ever formatted).
/// Fields must hold a valid civil date — the shell maps them from a real clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl LocalDateTime {
    pub fn new(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
        }
    }

    fn day_number(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    /// 0 = Sunday … 6 = Saturday (the `DayOfWeek` numbering the invariant names index by).
    fn weekday(self) -> usize {
        // Day 0 (1970-01-01) was a Thursday.
        (self.day_number() + 4).rem_euclid(7) as usize
    }
}

/// Invariant abbreviated day names, indexed by [`LocalDateTime::weekday`].
const DAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Invariant abbreviated month names, indexed by `month - 1`.
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days since 1970-01-01 for a civil date (Howard Hinnant's `days_from_civil` algorithm) —
/// the same whole-day difference `DateTime.Date` subtraction yields on the C# side.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (i64::from(month) + 9) % 12; // Mar = 0 … Feb = 11
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Bucket a file by its last-opened day relative to `now_local`: same day → Today, one day
/// back → Yesterday, 2–6 days → EarlierThisWeek (a rolling week, not the calendar week),
/// 7+ → Earlier. A future timestamp (clock skew) folds into Today.
pub fn bucket_for(last_opened_local: LocalDateTime, now_local: LocalDateTime) -> HistoryBucket {
    let days = now_local.day_number() - last_opened_local.day_number();
    if days <= 0 {
        return HistoryBucket::Today;
    }
    if days == 1 {
        return HistoryBucket::Yesterday;
    }
    if days <= 6 {
        return HistoryBucket::EarlierThisWeek;
    }
    HistoryBucket::Earlier
}

/// The upper-case group header for a bucket (matches the design's bucket table).
pub fn bucket_header(bucket: HistoryBucket) -> &'static str {
    match bucket {
        HistoryBucket::Today => "TODAY",
        HistoryBucket::Yesterday => "YESTERDAY",
        HistoryBucket::EarlierThisWeek => "EARLIER THIS WEEK",
        HistoryBucket::Earlier => "EARLIER",
    }
}

/// The right-column timestamp label: "Today 21:14", "Yest. 16:40", a weekday + time
/// ("Tue 21:48") within the week, else a day + month ("12 Jun"). 24-hour clock and invariant
/// weekday/month names.
pub fn when_label(last_opened_local: LocalDateTime, now_local: LocalDateTime) -> String {
    let when = last_opened_local;
    match bucket_for(when, now_local) {
        HistoryBucket::Today => format!("Today {:02}:{:02}", when.hour, when.minute),
        HistoryBucket::Yesterday => format!("Yest. {:02}:{:02}", when.hour, when.minute),
        HistoryBucket::EarlierThisWeek => format!(
            "{} {:02}:{:02}",
            DAY_ABBR[when.weekday()],
            when.hour,
            when.minute
        ),
        HistoryBucket::Earlier => {
            format!("{} {}", when.day, MONTH_ABBR[(when.month - 1) as usize])
        }
    }
}

/// Resume state for a row. Finished files show a chip; files under 5% watched show a
/// "barely started" hint ("3m in · 4%"); everything else shows time remaining ("23m left").
/// Minutes are clamped to ≥ 1 so a just-started or nearly-done file never reads "0m".
pub fn derive_state(position: f64, duration: f64, finished: bool) -> HistoryRowState {
    if finished {
        return HistoryRowState {
            kind: HistoryStateKind::Finished,
            percent: 0.0,
            label: String::new(),
        };
    }
    let percent = if duration > 0.0 {
        position / duration
    } else {
        0.0
    };
    if percent < 0.05 {
        // round_ties_even mirrors C# Math.Round (banker's rounding at .5 midpoints).
        let minutes_in = ((position / 60.0).round_ties_even() as i64).max(1);
        let percent_label = (percent * 100.0).round_ties_even() as i64;
        return HistoryRowState {
            kind: HistoryStateKind::Barely,
            percent,
            label: format!("{minutes_in}m in · {percent_label}%"),
        };
    }
    let left_minutes = (((duration - position) / 60.0).ceil() as i64).max(1);
    HistoryRowState {
        kind: HistoryStateKind::Progress,
        percent,
        label: format!("{left_minutes}m left"),
    }
}

/// A short location breadcrumb for the row's secondary line: the last two folder segments of
/// the file's directory joined with " › " (e.g. "Severance › Season 02"). Bare drive roots
/// ("D:") are dropped from the breadcrumb but kept as a fallback when nothing else remains.
/// Splits on both separators explicitly — history rows may carry Windows paths ('\') while the
/// code runs on Linux, where the platform path API wouldn't see '\'.
pub fn folder_label(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = path.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    if parts.len() <= 1 {
        return String::new(); // just a bare filename — no folder to show
    }
    // Drop the file name (last part), then the bare drive letter ("C:") so it doesn't eat a slot.
    let dirs: Vec<&str> = parts[..parts.len() - 1]
        .iter()
        .copied()
        .filter(|s| !is_bare_drive(s))
        .collect();
    if dirs.is_empty() {
        return parts[parts.len() - 2].to_string(); // e.g. a drive-root file "C:\clip.mkv" -> "C:"
    }
    let take = dirs.len().min(2);
    dirs[dirs.len() - take..].join(" › ")
}

/// Read-only retention echo for the History canvas. Management remains in
/// Settings; this copy simply reflects the persisted shared setting.
pub fn retention_summary(retention: HistoryRetention) -> String {
    match retention {
        HistoryRetention::Forever => "Everything you’ve opened · kept forever".to_owned(),
        retention => format!(
            "Everything you’ve opened · keeping last {}",
            retention.label()
        ),
    }
}

pub fn retention_end_cap(retention: HistoryRetention) -> String {
    match retention {
        HistoryRetention::Forever => "End of history · kept forever".to_owned(),
        retention => format!("End of history · keeping last {}", retention.label()),
    }
}

/// A two-character segment ending in ':' — a bare drive root like "C:".
fn is_bare_drive(segment: &str) -> bool {
    let mut chars = segment.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(_), Some(':'), None)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> LocalDateTime {
        LocalDateTime::new(2026, 6, 26, 12, 0)
    }

    // ---- derive_state ----

    #[test]
    fn derive_state_finished_is_a_chip_with_no_label() {
        let s = derive_state(0.0, 600.0, true);
        assert_eq!(s.kind, HistoryStateKind::Finished);
        assert_eq!(s.label, "");
    }

    #[test]
    fn derive_state_past_five_percent_shows_time_left_ceiling_minutes() {
        // 6840/7920 watched -> 18 minutes remain (1080s, ceil to 18m).
        let s = derive_state(6840.0, 7920.0, false);
        assert_eq!(s.kind, HistoryStateKind::Progress);
        assert_eq!(s.label, "18m left");
    }

    #[test]
    fn derive_state_under_five_percent_shows_barely_started_hint() {
        // 120/3240 = 3.7% -> "2m in · 4%" (matches the design's interview record).
        let s = derive_state(120.0, 3240.0, false);
        assert_eq!(s.kind, HistoryStateKind::Barely);
        assert_eq!(s.label, "2m in · 4%");
    }

    #[test]
    fn derive_state_exactly_five_percent_is_progress_not_barely() {
        // pct == 0.05 is not < 0.05, so it falls through to the time-left branch.
        let s = derive_state(50.0, 1000.0, false);
        assert_eq!(s.kind, HistoryStateKind::Progress);
        assert_eq!(s.label, "16m left"); // ceil(950/60) = 16
    }

    #[test]
    fn derive_state_nearly_done_clamps_time_left_to_one_minute() {
        let s = derive_state(595.0, 600.0, false);
        assert_eq!(s.label, "1m left");
    }

    #[test]
    fn derive_state_zero_duration_is_barely_with_clamped_minute() {
        let s = derive_state(0.0, 0.0, false);
        assert_eq!(s.kind, HistoryStateKind::Barely);
        assert_eq!(s.label, "1m in · 0%");
    }

    // ---- bucket_for ----

    #[test]
    fn bucket_for_groups_by_days_ago() {
        let cases = [
            (2026, 6, 26, HistoryBucket::Today),           // same day
            (2026, 6, 25, HistoryBucket::Yesterday),       // 1 day
            (2026, 6, 24, HistoryBucket::EarlierThisWeek), // 2 days
            (2026, 6, 20, HistoryBucket::EarlierThisWeek), // 6 days
            (2026, 6, 19, HistoryBucket::Earlier),         // 7 days
            (2026, 5, 1, HistoryBucket::Earlier),          // long ago
        ];
        for (year, month, day, expected) in cases {
            let when = LocalDateTime::new(year, month, day, 8, 0);
            assert_eq!(bucket_for(when, now()), expected, "{year}-{month}-{day}");
        }
    }

    #[test]
    fn bucket_for_future_timestamp_folds_into_today() {
        let future = LocalDateTime::new(2026, 6, 27, 8, 0); // clock skew
        assert_eq!(bucket_for(future, now()), HistoryBucket::Today);
    }

    #[test]
    fn bucket_header_matches_design_table() {
        let cases = [
            (HistoryBucket::Today, "TODAY"),
            (HistoryBucket::Yesterday, "YESTERDAY"),
            (HistoryBucket::EarlierThisWeek, "EARLIER THIS WEEK"),
            (HistoryBucket::Earlier, "EARLIER"),
        ];
        for (bucket, header) in cases {
            assert_eq!(bucket_header(bucket), header);
        }
    }

    // ---- when_label ----

    #[test]
    fn when_label_today_is_today_plus_24_hour_clock() {
        assert_eq!(
            when_label(LocalDateTime::new(2026, 6, 26, 21, 14), now()),
            "Today 21:14"
        );
    }

    #[test]
    fn when_label_yesterday_is_abbreviated() {
        assert_eq!(
            when_label(LocalDateTime::new(2026, 6, 25, 16, 40), now()),
            "Yest. 16:40"
        );
    }

    #[test]
    fn when_label_week_bucket_is_invariant_weekday_and_time() {
        // 2026-06-23, 3 days back, was a Tuesday.
        let when = LocalDateTime::new(2026, 6, 23, 21, 48);
        assert_eq!(when_label(when, now()), "Tue 21:48");
    }

    #[test]
    fn when_label_earlier_is_day_and_invariant_month() {
        assert_eq!(
            when_label(LocalDateTime::new(2026, 6, 12, 9, 0), now()),
            "12 Jun"
        );
    }

    // ---- folder_label ----

    #[test]
    fn folder_label_shows_last_two_segments() {
        let cases = [
            (
                r"D:\Movies\2024\Dune Part Two\Dune.2160p.mkv",
                "2024 › Dune Part Two",
            ),
            (r"E:\Footage\June\interview-raw-take3.mov", "Footage › June"),
            (r"C:\Movies\film.mkv", "Movies"),
            (r"C:\film.mkv", "C:"), // drive-root file: fall back to the drive
            ("D:/a/b/c/clip.mp4", "b › c"), // forward slashes
            ("", ""),
        ];
        for (path, expected) in cases {
            assert_eq!(folder_label(path), expected, "{path}");
        }
    }

    #[test]
    fn retention_summary_matches_the_history_design_copy() {
        assert_eq!(
            retention_summary(HistoryRetention::Forever),
            "Everything you’ve opened · kept forever"
        );
        assert_eq!(
            retention_summary(HistoryRetention::Days90),
            "Everything you’ve opened · keeping last 90 days"
        );
        assert_eq!(
            retention_end_cap(HistoryRetention::Forever),
            "End of history · kept forever"
        );
        assert_eq!(
            retention_end_cap(HistoryRetention::Days365),
            "End of history · keeping last 365 days"
        );
    }
}
