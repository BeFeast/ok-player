using System.Collections.Generic;
using System.Linq;
using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

public class NaturalComparerTests
{
    [Theory]
    [InlineData("ep2", "ep10", -1)]    // numeric, not lexical
    [InlineData("ep10", "ep2", 1)]
    [InlineData("a.mkv", "a2.mkv", -1)] // '.' (0x2E) before a digit run
    [InlineData("Show", "show", 0)]     // case-insensitive
    [InlineData("ep2", "ep02", -1)]     // equal value → fewer leading zeros first
    [InlineData("file", "file", 0)]
    public void Compare_OrdersNaturally(string a, string b, int expectedSign)
    {
        int actual = NaturalComparer.Instance.Compare(a, b);
        Assert.Equal(expectedSign, System.Math.Sign(actual));
    }

    [Fact]
    public void Sort_PutsEpisodesInHumanOrder()
    {
        var files = new List<string> { "ep10.mkv", "ep2.mkv", "ep1.mkv", "ep20.mkv", "ep3.mkv" };
        files.Sort(NaturalComparer.Instance);
        Assert.Equal(new[] { "ep1.mkv", "ep2.mkv", "ep3.mkv", "ep10.mkv", "ep20.mkv" }, files);
    }
}

public class PlaylistTests
{
    private static readonly string[] Folder =
    {
        @"C:\v\ep10.mkv", @"C:\v\ep1.mkv", @"C:\v\ep2.mkv", // intentionally unsorted input
    };

    [Fact]
    public void Construct_SortsAndLandsOnCurrent()
    {
        var p = new Playlist(Folder, @"C:\v\ep2.mkv");
        Assert.Equal(3, p.Count);
        Assert.Equal(new[] { @"C:\v\ep1.mkv", @"C:\v\ep2.mkv", @"C:\v\ep10.mkv" }, p.Items.ToArray());
        Assert.Equal(1, p.CurrentIndex);
        Assert.Equal(@"C:\v\ep2.mkv", p.Current);
    }

    [Fact]
    public void NextPrev_WalkTheList()
    {
        var p = new Playlist(Folder, @"C:\v\ep1.mkv");
        Assert.True(p.HasNext);
        Assert.False(p.HasPrev);
        Assert.Equal(@"C:\v\ep2.mkv", p.Next());
        Assert.Equal(@"C:\v\ep10.mkv", p.Next());
        Assert.False(p.HasNext);
        Assert.Null(p.Next());                 // at the end
        Assert.Equal(@"C:\v\ep2.mkv", p.Prev());
        Assert.Equal(@"C:\v\ep1.mkv", p.Prev());
        Assert.Null(p.Prev());                 // at the start
    }

    [Fact]
    public void CurrentNotInFolder_HasNoNeighbours()
    {
        var p = new Playlist(Folder, @"C:\other\x.mkv");
        Assert.Equal(-1, p.CurrentIndex);
        Assert.False(p.HasNext);
        Assert.False(p.HasPrev);
        Assert.Null(p.Current);
    }

    [Fact]
    public void SetCurrent_RepointsWhenPresent_IgnoresCase()
    {
        var p = new Playlist(Folder, @"C:\v\ep1.mkv");
        Assert.True(p.SetCurrent(@"C:\V\EP10.MKV")); // case-insensitive path match
        Assert.Equal(@"C:\v\ep10.mkv", p.Current);
        Assert.False(p.SetCurrent(@"C:\v\missing.mkv"));
        Assert.Equal(@"C:\v\ep10.mkv", p.Current); // unchanged on miss
    }

    [Fact]
    public void Peek_ReturnsNeighboursWithoutMovingCursor()
    {
        var p = new Playlist(Folder, @"C:\v\ep1.mkv");
        Assert.Equal(@"C:\v\ep2.mkv", p.PeekNext);
        Assert.Null(p.PeekPrev);          // at the start
        Assert.Equal(0, p.CurrentIndex);  // peeking did not move the cursor
        p.SetCurrent(@"C:\v\ep10.mkv");    // last item
        Assert.Null(p.PeekNext);
        Assert.Equal(@"C:\v\ep2.mkv", p.PeekPrev);
        Assert.Equal(2, p.CurrentIndex);
    }

    [Fact]
    public void RepeatOff_StopsAtEnd()
    {
        var p = new Playlist(Folder, @"C:\v\ep10.mkv"); // last item, Repeat.Off (default)
        Assert.Null(p.PeekNext);
        Assert.Null(p.AutoAdvanceTarget);
    }

    [Fact]
    public void RepeatAll_WrapsAtBothEnds()
    {
        var last = new Playlist(Folder, @"C:\v\ep10.mkv") { Repeat = RepeatMode.All };
        Assert.Equal(@"C:\v\ep1.mkv", last.PeekNext);   // end → first
        var first = new Playlist(Folder, @"C:\v\ep1.mkv") { Repeat = RepeatMode.All };
        Assert.Equal(@"C:\v\ep10.mkv", first.PeekPrev); // start → last
    }

    [Fact]
    public void RepeatOne_ReplaysOnAutoAdvance_ButManualNextStillMoves()
    {
        var p = new Playlist(Folder, @"C:\v\ep2.mkv") { Repeat = RepeatMode.One };
        Assert.Equal(@"C:\v\ep2.mkv", p.AutoAdvanceTarget); // EOF replays the current file
        Assert.Equal(@"C:\v\ep10.mkv", p.PeekNext);         // a manual hop still advances
    }

    [Fact]
    public void Shuffle_VisitsEveryFileOnce_StartingFromCurrent()
    {
        var p = new Playlist(Folder, @"C:\v\ep2.mkv") { Repeat = RepeatMode.All, Shuffle = true };
        var seen = new List<string> { p.Current! };
        for (int i = 0; i < Folder.Length - 1; i++) { p.Next(); seen.Add(p.Current!); }
        Assert.Equal(@"C:\v\ep2.mkv", seen[0]);                       // the playing file stays first
        Assert.Equal(Folder.Length, new HashSet<string>(seen).Count); // a full permutation, no repeats
    }

    [Fact]
    public void ShuffleOff_RestoresNaturalOrder()
    {
        var p = new Playlist(Folder, @"C:\v\ep1.mkv") { Shuffle = true };
        p.Shuffle = false;
        Assert.Equal(@"C:\v\ep2.mkv", p.PeekNext); // back to natural ep1 → ep2 → ep10
    }

    [Fact]
    public void Shuffle_DirectJump_StrandsNoFileUnderRepeatOff()
    {
        var folder = new[] { @"C:\v\1.mkv", @"C:\v\2.mkv", @"C:\v\3.mkv", @"C:\v\4.mkv", @"C:\v\5.mkv", @"C:\v\6.mkv" };
        var p = new Playlist(folder, @"C:\v\1.mkv", new System.Random(3)) { Shuffle = true };
        p.SetCurrent(@"C:\v\6.mkv"); // a direct jump (clicking an Up-Next row), not a sequential step
        var seen = new HashSet<string> { p.Current! };
        while (p.Next() is string n) seen.Add(n); // Repeat.Off — walk to the end of the cycle
        Assert.Equal(folder.Length, seen.Count); // the jump re-shuffled, so nothing is stranded this cycle
    }

    [Fact]
    public void QueueAppend_AddsOnlyNewItemsAtEnd()
    {
        var p = new Playlist(new[] { @"C:\v\current.mkv", @"C:\v\queued.mkv" }, @"C:\v\current.mkv", sort: false);

        int? added = p.QueueInsert(@"C:\v\current.mkv", new[]
        {
            @"C:\v\current.mkv", @"C:\V\QUEUED.MKV", @"C:\v\new.mp4", @"C:\v\new.mp4",
        }, QueueInsertMode.Append);

        Assert.Equal(1, added);
        Assert.Equal(new[] { @"C:\v\current.mkv", @"C:\v\queued.mkv", @"C:\v\new.mp4" }, p.Items);
        Assert.Equal(0, p.CurrentIndex);
    }

    [Fact]
    public void QueuePlayNext_MovesExistingItemsAndPreservesSelectionOrder()
    {
        var p = new Playlist(new[]
        {
            @"C:\v\previous.mkv", @"C:\v\current.mkv", @"C:\v\later.mkv", @"C:\v\final.mkv",
        }, @"C:\v\current.mkv", sort: false);

        int? added = p.QueueInsert(@"C:\v\current.mkv",
            new[] { @"C:\v\later.mkv", @"C:\v\new.mp4" }, QueueInsertMode.PlayNext);

        Assert.Equal(2, added);
        Assert.Equal(new[]
        {
            @"C:\v\previous.mkv", @"C:\v\current.mkv", @"C:\v\later.mkv", @"C:\v\new.mp4", @"C:\v\final.mkv",
        }, p.Items);
        Assert.Equal(1, p.CurrentIndex);
    }

    [Fact]
    public void QueueInsert_GrowsSingleUrlQueueAroundCurrent()
    {
        const string stream = "https://example.test/live.m3u8";
        var p = new Playlist(new[] { stream }, stream, sort: false);

        Assert.Equal(1, p.QueueInsert(stream, new[] { @"C:\v\next.mkv" }, QueueInsertMode.PlayNext));
        Assert.Equal(new[] { stream, @"C:\v\next.mkv" }, p.Items);
        Assert.Equal(stream, p.Current);
    }

    [Fact]
    public void Reorder_PlayNext_RemoveAndClearKeepCursorOnPlayingItem()
    {
        var p = new Playlist(new[] { "a", "b", "c", "d" }, "b", sort: false);

        Assert.True(p.Reorder(3, 0));
        Assert.Equal(new[] { "d", "a", "b", "c" }, p.Items);
        Assert.Equal("b", p.Current);
        Assert.True(p.PlayNext(0));
        Assert.Equal(new[] { "a", "b", "d", "c" }, p.Items);
        Assert.False(p.Remove(p.CurrentIndex));
        Assert.True(p.Remove(0));
        Assert.Equal("b", p.Current);
        Assert.True(p.ClearQueue());
        Assert.Equal(new[] { "b" }, p.Items);
        Assert.Equal(0, p.CurrentIndex);
        Assert.False(p.ClearQueue());
    }

    [Theory]
    [InlineData(0, 2, false, 1)]
    [InlineData(0, 2, true, 2)]
    [InlineData(3, 1, false, 1)]
    [InlineData(3, 1, true, 2)]
    [InlineData(2, 2, false, null)]
    [InlineData(1, 2, false, null)]
    public void DropTargetIndex_MapsInsertionLineToReorderDestination(int source, int row, bool after, int? expected)
    {
        Assert.Equal(expected, Playlist.DropTargetIndex(source, row, after));
    }
}
