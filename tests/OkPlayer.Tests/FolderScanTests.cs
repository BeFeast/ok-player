using OkPlayer.Core;

namespace OkPlayer.Tests;

public class FolderScanTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), $"okplayer-scan-{Guid.NewGuid():N}");

    public FolderScanTests() => Directory.CreateDirectory(_root);

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { }
    }

    private void Touch(params string[] relativePaths)
    {
        foreach (string rel in relativePaths)
        {
            string full = Path.Combine(_root, rel);
            Directory.CreateDirectory(Path.GetDirectoryName(full)!);
            File.WriteAllText(full, "x");
        }
    }

    [Fact]
    public void MediaFiles_CollectsRecursively_NaturalSorted_SkipsNonMedia()
    {
        Touch("b.mkv", "a.mp4", "notes.txt", Path.Combine("sub", "c.mkv"), Path.Combine("sub", "readme.nfo"));

        var media = FolderScan.MediaFiles(_root);

        Assert.Equal(3, media.Count); // a.mp4, b.mkv, sub/c.mkv — .txt/.nfo excluded
        Assert.EndsWith("a.mp4", media[0]); // natural-sorted by full path: a < b < sub
        Assert.EndsWith("b.mkv", media[1]);
        Assert.EndsWith("c.mkv", media[2]);
        Assert.DoesNotContain(media, m => m.EndsWith(".txt") || m.EndsWith(".nfo"));
    }

    [Fact]
    public void MediaFiles_RespectsMaxDepth()
    {
        Touch("top.mkv", Path.Combine("d1", "deep.mkv"), Path.Combine("d1", "d2", "tooDeep.mkv"));

        // maxDepth 1: root files (depth 0) + one level down (depth 1). d1/d2 (depth 2) is not descended.
        var media = FolderScan.MediaFiles(_root, maxDepth: 1);

        Assert.Contains(media, m => m.EndsWith("top.mkv"));
        Assert.Contains(media, m => m.EndsWith("deep.mkv"));
        Assert.DoesNotContain(media, m => m.EndsWith("tooDeep.mkv"));
    }

    [Fact]
    public void MediaFiles_MissingRoot_ReturnsEmpty()
        => Assert.Empty(FolderScan.MediaFiles(Path.Combine(_root, "does-not-exist")));
}
