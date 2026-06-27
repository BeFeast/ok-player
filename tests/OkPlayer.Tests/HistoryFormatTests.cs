using System.Globalization;
using OkPlayer.App.Services;

namespace OkPlayer.Tests;

public class HistoryFormatTests
{
    private static readonly DateTime Now = new(2026, 6, 26, 12, 0, 0, DateTimeKind.Local);

    // ---- DeriveState ----

    [Fact]
    public void DeriveState_Finished_IsAChipWithNoLabel()
    {
        var s = HistoryFormat.DeriveState(position: 0, duration: 600, finished: true);
        Assert.Equal(HistoryStateKind.Finished, s.Kind);
        Assert.Equal("", s.Label);
    }

    [Fact]
    public void DeriveState_PastFivePercent_ShowsTimeLeft_CeilingMinutes()
    {
        // 6840/7920 watched -> 18 minutes remain (1080s, ceil to 18m).
        var s = HistoryFormat.DeriveState(position: 6840, duration: 7920, finished: false);
        Assert.Equal(HistoryStateKind.Progress, s.Kind);
        Assert.Equal("18m left", s.Label);
    }

    [Fact]
    public void DeriveState_UnderFivePercent_ShowsBarelyStartedHint()
    {
        // 120/3240 = 3.7% -> "2m in · 4%" (matches the design's interview record).
        var s = HistoryFormat.DeriveState(position: 120, duration: 3240, finished: false);
        Assert.Equal(HistoryStateKind.Barely, s.Kind);
        Assert.Equal("2m in · 4%", s.Label);
    }

    [Fact]
    public void DeriveState_ExactlyFivePercent_IsProgress_NotBarely()
    {
        // pct == 0.05 is not < 0.05, so it falls through to the time-left branch.
        var s = HistoryFormat.DeriveState(position: 50, duration: 1000, finished: false);
        Assert.Equal(HistoryStateKind.Progress, s.Kind);
        Assert.Equal("16m left", s.Label); // ceil(950/60) = 16
    }

    [Fact]
    public void DeriveState_NearlyDone_ClampsTimeLeftToOneMinute()
    {
        var s = HistoryFormat.DeriveState(position: 595, duration: 600, finished: false);
        Assert.Equal("1m left", s.Label);
    }

    [Fact]
    public void DeriveState_ZeroDuration_IsBarely_WithClampedMinute()
    {
        var s = HistoryFormat.DeriveState(position: 0, duration: 0, finished: false);
        Assert.Equal(HistoryStateKind.Barely, s.Kind);
        Assert.Equal("1m in · 0%", s.Label);
    }

    // ---- BucketFor ----

    [Theory]
    [InlineData(2026, 6, 26, HistoryBucket.Today)]       // same day
    [InlineData(2026, 6, 25, HistoryBucket.Yesterday)]   // 1 day
    [InlineData(2026, 6, 24, HistoryBucket.EarlierThisWeek)] // 2 days
    [InlineData(2026, 6, 20, HistoryBucket.EarlierThisWeek)] // 6 days
    [InlineData(2026, 6, 19, HistoryBucket.Earlier)]     // 7 days
    [InlineData(2026, 5, 1, HistoryBucket.Earlier)]      // long ago
    public void BucketFor_GroupsByDaysAgo(int y, int m, int d, HistoryBucket expected)
    {
        var when = new DateTime(y, m, d, 8, 0, 0, DateTimeKind.Local);
        Assert.Equal(expected, HistoryFormat.BucketFor(when, Now));
    }

    [Fact]
    public void BucketFor_FutureTimestamp_FoldsIntoToday()
    {
        var future = new DateTime(2026, 6, 27, 8, 0, 0, DateTimeKind.Local); // clock skew
        Assert.Equal(HistoryBucket.Today, HistoryFormat.BucketFor(future, Now));
    }

    [Theory]
    [InlineData(HistoryBucket.Today, "TODAY")]
    [InlineData(HistoryBucket.Yesterday, "YESTERDAY")]
    [InlineData(HistoryBucket.EarlierThisWeek, "EARLIER THIS WEEK")]
    [InlineData(HistoryBucket.Earlier, "EARLIER")]
    public void BucketHeader_MatchesDesignTable(HistoryBucket bucket, string header)
        => Assert.Equal(header, HistoryFormat.BucketHeader(bucket));

    // ---- WhenLabel ----

    [Fact]
    public void WhenLabel_Today_IsTodayPlus24HourClock()
        => Assert.Equal("Today 21:14",
            HistoryFormat.WhenLabel(new DateTime(2026, 6, 26, 21, 14, 0, DateTimeKind.Local), Now));

    [Fact]
    public void WhenLabel_Yesterday_IsAbbreviated()
        => Assert.Equal("Yest. 16:40",
            HistoryFormat.WhenLabel(new DateTime(2026, 6, 25, 16, 40, 0, DateTimeKind.Local), Now));

    [Fact]
    public void WhenLabel_WeekBucket_IsInvariantWeekdayAndTime()
    {
        var when = new DateTime(2026, 6, 23, 21, 48, 0, DateTimeKind.Local); // 3 days back
        string abbr = CultureInfo.InvariantCulture.DateTimeFormat.GetAbbreviatedDayName(when.DayOfWeek);
        Assert.Equal($"{abbr} 21:48", HistoryFormat.WhenLabel(when, Now));
    }

    [Fact]
    public void WhenLabel_Earlier_IsDayAndInvariantMonth()
        => Assert.Equal("12 Jun",
            HistoryFormat.WhenLabel(new DateTime(2026, 6, 12, 9, 0, 0, DateTimeKind.Local), Now));

    // ---- FolderLabel ----

    [Theory]
    [InlineData(@"D:\Movies\2024\Dune Part Two\Dune.2160p.mkv", "2024 › Dune Part Two")]
    [InlineData(@"E:\Footage\June\interview-raw-take3.mov", "Footage › June")]
    [InlineData(@"C:\Movies\film.mkv", "Movies")]
    [InlineData(@"C:\film.mkv", "C:")] // drive-root file: fall back to the drive
    [InlineData("D:/a/b/c/clip.mp4", "b › c")] // forward slashes
    [InlineData("", "")]
    public void FolderLabel_ShowsLastTwoSegments(string path, string expected)
        => Assert.Equal(expected, HistoryFormat.FolderLabel(path));
}
