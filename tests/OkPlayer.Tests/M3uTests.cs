using System.Collections.Generic;
using OkPlayer.Core;
using Xunit;

namespace OkPlayer.Tests;

public class M3uTests
{
    [Fact]
    public void Write_EmitsHeaderThenOnePathPerLine()
    {
        string text = M3u.Write(new[] { @"C:\v\a.mkv", @"C:\v\b.mkv" });
        Assert.Equal("#EXTM3U\n" + @"C:\v\a.mkv" + "\n" + @"C:\v\b.mkv" + "\n", text);
    }

    [Fact]
    public void Parse_KeepsOrder_SkipsDirectivesAndBlanks()
    {
        string text = "#EXTM3U\n#EXTINF:123,Title\n\n  C:\\v\\b.mkv  \nC:\\v\\a.mkv\n";
        var entries = M3u.Parse(text, null);
        Assert.Equal(new[] { @"C:\v\b.mkv", @"C:\v\a.mkv" }, entries); // order preserved, comments/blank dropped, trimmed
    }

    [Fact]
    public void Parse_ResolvesRelative_PassesThroughAbsoluteAndUrls()
    {
        // Unit tests run on Linux CI too, so build OS-appropriate paths rather than hard-coding Windows ones.
        string baseDir = System.IO.Path.GetTempPath().TrimEnd(System.IO.Path.DirectorySeparatorChar);
        string absolute = System.OperatingSystem.IsWindows() ? @"C:\other\ep2.mkv" : "/other/ep2.mkv";
        string text = $"ep1.mkv\n{absolute}\nhttps://host/ep3.mp4\n";
        var entries = M3u.Parse(text, baseDir);
        Assert.Equal(System.IO.Path.GetFullPath(System.IO.Path.Combine(baseDir, "ep1.mkv")), entries[0]); // relative → resolved
        Assert.Equal(absolute, entries[1]);                 // absolute → unchanged
        Assert.Equal("https://host/ep3.mp4", entries[2]);   // URL → unchanged
    }

    [Fact]
    public void RoundTrips()
    {
        var paths = new List<string> { @"C:\v\1.mkv", @"C:\v\2.mkv", @"C:\v\3.mkv" };
        var back = M3u.Parse(M3u.Write(paths), null);
        Assert.Equal(paths, back);
    }
}
