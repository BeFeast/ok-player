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
}
