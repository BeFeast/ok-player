using OkPlayer.Core;

namespace OkPlayer.Tests;

/// <summary>Unit tests for the LRC lyric parser + the position→active-line sync. LRC sheets are crowd-sourced
/// and ragged, so the parser must be tolerant (skip junk, never throw), handle the real-world variants
/// (multiple stamps per line, 2/3-digit fractions, ID tags, offset, enhanced word tags), and fall back to
/// plain lyrics when nothing is timed. The sync must pin the active line by a clean ≤ rule.</summary>
public class LrcTests
{
    [Fact]
    public void EmptyOrBlank_IsEmptyDocument()
    {
        Assert.True(Lrc.Parse(null).IsEmpty);
        Assert.True(Lrc.Parse("").IsEmpty);
        Assert.True(Lrc.Parse("   \n\t ").IsEmpty);
        Assert.False(Lrc.Parse(null).HasTimings);
    }

    [Fact]
    public void SyncedLines_AreParsedSortedAndCarryMetadata()
    {
        var doc = Lrc.Parse("[ar:A][ti:T][al:Al]\n[00:01.00]one\n[00:03.50]three\n[00:02.00]two\n");
        Assert.True(doc.HasTimings);
        Assert.Equal(new[] { "one", "two", "three" }, doc.Lines.Select(l => l.Text));
        Assert.Equal(1.0, doc.Lines[0].Time.TotalSeconds, 6);
        Assert.Equal(2.0, doc.Lines[1].Time.TotalSeconds, 6);
        Assert.Equal(3.5, doc.Lines[2].Time.TotalSeconds, 6);
        Assert.Equal("A", doc.Artist);
        Assert.Equal("T", doc.Title);
        Assert.Equal("Al", doc.Album);
    }

    [Fact]
    public void MultipleStampsOnOneLine_FanOutToOneLineEach()
    {
        var doc = Lrc.Parse("[00:12.00][00:47.10]La la la");
        Assert.Equal(2, doc.Lines.Count);
        Assert.All(doc.Lines, l => Assert.Equal("La la la", l.Text));
        Assert.Equal(12.0, doc.Lines[0].Time.TotalSeconds, 6);
        Assert.Equal(47.1, doc.Lines[1].Time.TotalSeconds, 6);
    }

    [Theory]
    [InlineData("[01:02]x", 62.0)]      // no fraction
    [InlineData("[01:02.5]x", 62.5)]    // 1-digit = tenths
    [InlineData("[01:02.50]x", 62.5)]   // 2-digit = centiseconds
    [InlineData("[01:02.500]x", 62.5)]  // 3-digit = milliseconds
    [InlineData("[01:02.05]x", 62.05)]  // leading-zero centiseconds
    [InlineData("[100:00.00]x", 6000.0)] // minutes may exceed 59
    public void FractionAndMinuteForms_ParseToSeconds(string line, double expectedSeconds)
    {
        var doc = Lrc.Parse(line);
        Assert.Single(doc.Lines);
        Assert.Equal(expectedSeconds, doc.Lines[0].Time.TotalSeconds, 6);
    }

    [Fact]
    public void Offset_PositiveShiftsEarlier_NegativeShiftsLater_ClampedAtZero()
    {
        Assert.Equal(9.5, Lrc.Parse("[offset:500]\n[00:10.00]a").Lines[0].Time.TotalSeconds, 6);
        Assert.Equal(10.5, Lrc.Parse("[offset:-500]\n[00:10.00]a").Lines[0].Time.TotalSeconds, 6);
        Assert.Equal(0.0, Lrc.Parse("[offset:5000]\n[00:01.00]a").Lines[0].Time.TotalSeconds, 6);
    }

    [Theory]
    [InlineData("[01:02:03]x", 62.03)]   // 2-digit = centiseconds (colon separator)
    [InlineData("[01:02:50]x", 62.5)]
    [InlineData("[01:02:500]x", 62.5)]   // 3-digit colon = milliseconds — must be +0.5s, NOT +5s (=67s)
    [InlineData("[01:02:5]x", 62.5)]     // 1-digit = tenths
    [InlineData("[01:02.500]x", 62.5)]   // the '.' separator parses identically (length-based)
    public void Fractions_AreLengthBased_RegardlessOfSeparator(string line, double expectedSeconds)
    {
        var doc = Lrc.Parse(line);
        Assert.Single(doc.Lines);
        Assert.Equal(expectedSeconds, doc.Lines[0].Time.TotalSeconds, 6);
    }

    [Fact]
    public void BracketedSectionHeaders_ArePreservedInPlainLyrics()
    {
        // A plain (untimed) sheet with [Chorus]/[Verse] markers: those header-only lines must survive, not be
        // swallowed as unknown tags and dropped.
        var doc = Lrc.Parse("[Chorus]\nWe're no strangers to love\n[Verse 1]\nYou know the rules");
        Assert.False(doc.HasTimings);
        Assert.Equal(new[] { "[Chorus]", "We're no strangers to love", "[Verse 1]", "You know the rules" },
                     doc.Lines.Select(l => l.Text));
    }

    [Fact]
    public void EnhancedWordTags_AreStrippedToCleanLineText()
    {
        var doc = Lrc.Parse("[00:05.00]<00:05.00>Hello <00:05.50>world");
        Assert.Single(doc.Lines);
        Assert.Equal("Hello world", doc.Lines[0].Text);
    }

    [Fact]
    public void GapLine_WithNoText_IsPreserved()
    {
        var doc = Lrc.Parse("[00:01.00]a\n[00:02.00]\n[00:03.00]b");
        Assert.Equal(3, doc.Lines.Count);
        Assert.Equal("", doc.Lines[1].Text);
    }

    [Fact]
    public void MalformedLines_AreSkipped_NotThrown_InASyncedSheet()
    {
        var doc = Lrc.Parse("[bad:tag]junk\n[00:01.00]good\n[not-a-time]more junk");
        Assert.True(doc.HasTimings);
        Assert.Single(doc.Lines);
        Assert.Equal("good", doc.Lines[0].Text);
    }

    [Fact]
    public void NoTimestamps_FallsBackToPlainLyricsInOrder()
    {
        var doc = Lrc.Parse("first line\nsecond line\nthird line");
        Assert.False(doc.HasTimings);
        Assert.Equal(new[] { "first line", "second line", "third line" }, doc.Lines.Select(l => l.Text));
        Assert.All(doc.Lines, l => Assert.Equal(TimeSpan.Zero, l.Time));
    }

    [Theory]
    [InlineData("[99999999999:00.00]bad")]            // minutes fit long but overflow TimeSpan → range guard skips
    [InlineData("[123456789012345678901:00.00]bad")]  // minutes overflow long parse → TryParse skips
    public void OverflowMinutes_AreSkipped_NotThrown(string badLine)
    {
        // The parser must honour its "never throws" contract on pathological input and keep the good line.
        var doc = Lrc.Parse(badLine + "\n[00:01.00]good");
        Assert.True(doc.HasTimings);
        Assert.Single(doc.Lines);
        Assert.Equal("good", doc.Lines[0].Text);
    }

    [Fact]
    public void HugeOffset_IsIgnored_NotOverflowed()
    {
        // A pathological [offset:…] must not overflow TimeSpan — it is ignored, leaving the stamp untouched.
        var doc = Lrc.Parse("[offset:999999999999999999]\n[00:10.00]a");
        Assert.Single(doc.Lines);
        Assert.Equal(10.0, doc.Lines[0].Time.TotalSeconds, 6);
    }

    [Fact]
    public void LengthTag_IsCaptured()
    {
        var doc = Lrc.Parse("[length:03:20]\n[00:01.00]a");
        Assert.Equal(200.0, doc.Length!.Value.TotalSeconds, 6);
    }

    // ---- LyricSync.ActiveIndex ----

    private static readonly List<LrcLine> Sheet = new()
    {
        new(TimeSpan.FromSeconds(1), "one"),
        new(TimeSpan.FromSeconds(2), "two"),
        new(TimeSpan.FromSeconds(3.5), "three"),
    };

    [Theory]
    [InlineData(0.0, -1)]   // before the first line
    [InlineData(0.99, -1)]
    [InlineData(1.0, 0)]    // exactly on a stamp → that line is active
    [InlineData(1.5, 0)]
    [InlineData(2.0, 1)]
    [InlineData(3.49, 1)]
    [InlineData(3.5, 2)]
    [InlineData(120.0, 2)]  // after the last line stays on the last
    public void ActiveIndex_PicksTheLastLineAtOrBeforePosition(double pos, int expected)
    {
        Assert.Equal(expected, LyricSync.ActiveIndex(Sheet, pos));
    }

    [Fact]
    public void ActiveIndex_EmptyOrNegative_IsMinusOne()
    {
        Assert.Equal(-1, LyricSync.ActiveIndex(new List<LrcLine>(), 10));
        Assert.Equal(-1, LyricSync.ActiveIndex(Sheet, -5));
    }
}
