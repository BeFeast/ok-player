using OkPlayer.Core;

namespace OkPlayer.Tests;

public class ChapterMathTests
{
    [Fact]
    public void Merge_FileAndUser_AreTimeSortedReindexedAndTagged()
    {
        var file = new (double, string)[] { (0, "Intro"), (30, "End") };
        var user = new (double, string)[] { (15, "Mid") };

        var merged = ChapterMath.Merge(file, user);

        Assert.Equal(3, merged.Count);
        Assert.Equal(new[] { 0, 1, 2 }, merged.Select(m => m.Index));
        Assert.Equal(new[] { "Intro", "Mid", "End" }, merged.Select(m => m.Title));
        Assert.Equal(new[] { false, true, false }, merged.Select(m => m.IsUserDefined));
    }

    [Fact]
    public void Merge_OnEqualTime_KeepsFileBeforeUser()
    {
        var file = new (double, string)[] { (10, "FileAt10") };
        var user = new (double, string)[] { (10, "UserAt10") };

        var merged = ChapterMath.Merge(file, user);

        Assert.Equal("FileAt10", merged[0].Title);
        Assert.False(merged[0].IsUserDefined);
        Assert.True(merged[1].IsUserDefined);
    }

    [Fact]
    public void Merge_Empty_ReturnsEmpty()
        => Assert.Empty(ChapterMath.Merge(Array.Empty<(double, string)>(), Array.Empty<(double, string)>()));

    [Theory]
    [InlineData(-5, -1)]   // before the first chapter
    [InlineData(0, 0)]     // exactly on the first start
    [InlineData(12, 0)]    // inside chapter 0
    [InlineData(15, 1)]    // exactly on chapter 1
    [InlineData(999, 2)]   // past the last start -> last chapter
    public void CurrentIndex_PicksLastStartedChapter(double position, int expected)
    {
        var times = new double[] { 0, 15, 30 };
        Assert.Equal(expected, ChapterMath.CurrentIndex(times, position));
    }

    [Fact]
    public void CurrentIndex_WithinEpsilon_CountsAsStarted()
        => Assert.Equal(0, ChapterMath.CurrentIndex(new double[] { 0.2 }, 0.0)); // 0.2 <= 0 + 0.25

    [Fact]
    public void CurrentIndex_NoChapters_IsMinusOne()
        => Assert.Equal(-1, ChapterMath.CurrentIndex(Array.Empty<double>(), 50));

    // The boundary cases Greptile flagged: a prev/next jump at an end must not rewind to the same chapter.
    [Fact]
    public void JumpTarget_NextAtLastChapter_ReturnsNull()
        => Assert.Null(ChapterMath.JumpTarget(current: 2, delta: 1, count: 3));

    [Fact]
    public void JumpTarget_PrevAtFirstChapter_ReturnsNull()
        => Assert.Null(ChapterMath.JumpTarget(current: 0, delta: -1, count: 3));

    [Theory]
    [InlineData(1, 1, 2)]   // next from the middle
    [InlineData(1, -1, 0)]  // prev from the middle
    [InlineData(-1, 1, 0)]  // next while before the first chapter -> chapter 0
    public void JumpTarget_FromInside_ReturnsAdjacent(int current, int delta, int expected)
        => Assert.Equal(expected, ChapterMath.JumpTarget(current, delta, count: 3));

    [Fact]
    public void JumpTarget_NoChapters_ReturnsNull()
        => Assert.Null(ChapterMath.JumpTarget(current: -1, delta: 1, count: 0));

    [Fact]
    public void Fractions_DivideStartsByDuration()
        => Assert.Equal(new[] { 0.0, 0.5, 0.75 }, ChapterMath.Fractions(new double[] { 0, 60, 90 }, 120));

    [Fact]
    public void Fractions_ZeroDuration_IsEmpty()
        => Assert.Empty(ChapterMath.Fractions(new double[] { 0, 60 }, 0));

    [Fact]
    public void Fractions_PastDuration_ClampedToOne()
        => Assert.Equal(new[] { 1.0 }, ChapterMath.Fractions(new double[] { 200 }, 100));
}
