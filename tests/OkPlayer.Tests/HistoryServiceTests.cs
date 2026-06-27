using OkPlayer.App.Services;

namespace OkPlayer.Tests;

public class HistoryServiceTests : IDisposable
{
    private readonly string _path = Path.Combine(Path.GetTempPath(), $"okplayer-history-{Guid.NewGuid():N}.json");
    private const string Movie = @"C:\media\movie.mkv";

    private HistoryService New() => new(_path); // internal test ctor: persist to a throwaway file

    public void Dispose()
    {
        try { File.Delete(_path); } catch { }
    }

    [Fact]
    public void AddUserChapter_RoundTripsThroughGet()
    {
        var h = New();
        Assert.True(h.AddUserChapter(Movie, 12.0, "Intro"));

        var chapters = h.GetUserChapters(Movie);
        Assert.Single(chapters);
        Assert.Equal(12.0, chapters[0].Time);
        Assert.Equal("Intro", chapters[0].Title);
    }

    [Fact]
    public void AddUserChapter_DuplicateWithinHalfSecond_ReturnsFalseAndIsNotAdded()
    {
        var h = New();
        Assert.True(h.AddUserChapter(Movie, 30.0, "First"));
        Assert.False(h.AddUserChapter(Movie, 30.3, "Too close")); // within 0.5s -> rejected
        Assert.Single(h.GetUserChapters(Movie));
    }

    [Fact]
    public void AddUserChapter_RejectsUrls()
        => Assert.False(New().AddUserChapter("https://example.com/stream", 5, "x"));

    [Fact]
    public void Chapters_AreKeptSortedByTime()
    {
        var h = New();
        h.AddUserChapter(Movie, 50, "late");
        h.AddUserChapter(Movie, 10, "early");
        Assert.Equal(new[] { 10.0, 50.0 }, h.GetUserChapters(Movie).Select(c => c.Time));
    }

    [Fact]
    public void RenameUserChapter_ChangesTitleAtThatTime()
    {
        var h = New();
        h.AddUserChapter(Movie, 20, "old");
        h.RenameUserChapter(Movie, 20, "new");
        Assert.Equal("new", h.GetUserChapters(Movie).Single().Title);
    }

    [Fact]
    public void RemoveUserChapter_DropsIt()
    {
        var h = New();
        h.AddUserChapter(Movie, 20, "x");
        h.RemoveUserChapter(Movie, 20);
        Assert.Empty(h.GetUserChapters(Movie));
    }

    [Fact]
    public void Bookmarks_AddDedupeRemove()
    {
        var h = New();
        Assert.True(h.AddBookmark(Movie, 100));
        h.AddBookmark(Movie, 100.2); // within 0.5s -> deduped
        Assert.Single(h.GetBookmarks(Movie));
        h.RemoveBookmark(Movie, 100);
        Assert.Empty(h.GetBookmarks(Movie));
    }

    [Fact]
    public void Data_PersistsAcrossInstances()
    {
        var a = New();
        a.AddUserChapter(Movie, 42, "Saved");
        a.AddBookmark(Movie, 7);

        var b = New(); // a fresh service reads the same file
        Assert.Equal("Saved", b.GetUserChapters(Movie).Single().Title);
        Assert.Equal(new[] { 7.0 }, b.GetBookmarks(Movie));
    }

    [Fact]
    public void Get_ReturnsCopies_SoCallerCannotMutateStoredState()
    {
        var h = New();
        h.AddUserChapter(Movie, 5, "orig");
        var first = h.GetUserChapters(Movie).Single();
        first.Title = "mutated by caller";
        Assert.Equal("orig", h.GetUserChapters(Movie).Single().Title);
    }

    [Fact]
    public void Private_SuppressesAllWrites_AndPersistsNothing()
    {
        var h = New();
        h.Private = true;
        h.Record(Movie, 120, 600);
        Assert.False(h.AddBookmark(Movie, 30));
        Assert.False(h.AddUserChapter(Movie, 40, "x"));
        h.SetPoster(Movie, @"C:\poster.png");

        Assert.Null(h.Get(Movie));
        Assert.Empty(h.Recents(10));
        var fresh = New(); // nothing reached disk
        Assert.Null(fresh.Get(Movie));
    }

    [Fact]
    public void Private_StillReadsExistingHistory_AndAllowsDeletion()
    {
        var seed = New();
        seed.Record(Movie, 90, 600); // recorded normally

        var h = New();
        h.Private = true;
        Assert.NotNull(h.Get(Movie));    // existing history stays readable in incognito
        Assert.Equal(1, h.Clear());      // deletions still apply
        Assert.Null(h.Get(Movie));
    }

    [Fact]
    public void Record_Finished_DefaultsFalse_AndPersists()
    {
        var a = New();
        a.Record(Movie, 120, 600); // default overload -> not finished
        Assert.False(a.Get(Movie)!.Finished);

        a.Record(Movie, 0, 600, finished: true);
        Assert.True(a.Get(Movie)!.Finished);

        var b = New(); // survives a round-trip through disk
        Assert.True(b.Get(Movie)!.Finished);
    }

    [Fact]
    public void Record_Finished_IsOverwritten_WhenReWatchedFromStart()
    {
        var h = New();
        h.Record(Movie, 0, 600, finished: true);
        Assert.True(h.Get(Movie)!.Finished);

        h.Record(Movie, 45, 600); // re-watching from the start clears the flag
        Assert.False(h.Get(Movie)!.Finished);
    }

    [Fact]
    public void Clear_WipesEverything_AndReturnsCount()
    {
        var h = New();
        h.Record(Movie, 10, 100);
        h.Record(@"C:\media\other.mkv", 20, 200);
        Assert.Equal(2, h.Clear());
        Assert.Empty(h.Recents(10));
        Assert.Equal(0, h.Clear()); // already empty
    }

    [Fact]
    public void PruneOlderThan_ZeroKeepsForever_PositiveDropsStale()
    {
        var h = New();
        h.Record(Movie, 10, 100);
        // Forge an old timestamp directly on the stored record.
        h.Get(Movie)!.LastOpenedUtc = DateTime.UtcNow.AddDays(-40).ToString("o");

        Assert.Equal(0, h.PruneOlderThan(0));         // 0 = keep forever
        Assert.Equal(1, h.PruneOlderThan(30));        // 40 days old > 30 -> pruned
        Assert.Null(h.Get(Movie));
    }

    [Fact]
    public void PruneOlderThan_KeepsUndatedRecords()
    {
        var h = New();
        h.AddBookmark(Movie, 5); // bookmark-first record may carry a timestamp; clear it to simulate undated
        h.Get(Movie)!.LastOpenedUtc = "";
        Assert.Equal(0, h.PruneOlderThan(30)); // unparseable timestamp -> kept, not silently dropped
        Assert.NotNull(h.Get(Movie));
    }

    [Fact]
    public void PruneOlderThan_KeepsEntriesWithinWindow()
    {
        var h = New();
        h.Record(Movie, 10, 100);
        h.Get(Movie)!.LastOpenedUtc = DateTime.UtcNow.AddDays(-5).ToString("o");
        Assert.Equal(0, h.PruneOlderThan(30)); // 5 days < 30 -> kept
        Assert.NotNull(h.Get(Movie));
    }

    [Fact]
    public void Remove_DropsRecord_ReturnsTrue_FiresChanged_AndPersists()
    {
        var h = New();
        h.Record(Movie, 10, 100);
        int changed = 0;
        h.Changed += () => changed++;

        Assert.True(h.Remove(Movie));
        Assert.Null(h.Get(Movie));
        Assert.Equal(1, changed);

        Assert.Null(New().Get(Movie)); // removal reached disk
    }

    [Fact]
    public void Remove_AbsentOrEmptyPath_ReturnsFalse_AndDoesNotFireChanged()
    {
        var h = New();
        int changed = 0;
        h.Changed += () => changed++;

        Assert.False(h.Remove(Movie)); // never recorded
        Assert.False(h.Remove(""));
        Assert.Equal(0, changed);
    }

    [Fact]
    public void All_ReturnsExistingFilesNewestFirst_IncludingFinished()
    {
        var h = New();
        string a = TempMedia(), b = TempMedia();
        try
        {
            h.Record(a, 30, 600);
            h.Record(b, 0, 600, finished: true);
            h.Get(a)!.LastOpenedUtc = DateTime.UtcNow.AddMinutes(-10).ToString("o"); // older
            h.Get(b)!.LastOpenedUtc = DateTime.UtcNow.ToString("o");                 // newer

            var all = h.All();
            Assert.Equal(new[] { b, a }, all.Select(x => x.Path)); // newest-opened first
            Assert.True(all.Single(x => x.Path == b).Record.Finished); // finished files are kept
        }
        finally { File.Delete(a); File.Delete(b); }
    }

    [Fact]
    public void All_HidesMissingFiles()
    {
        var h = New();
        h.Record(Movie, 10, 100); // Movie path does not exist on disk
        Assert.Empty(h.All());
    }

    private static string TempMedia()
    {
        string p = Path.Combine(Path.GetTempPath(), $"okplayer-media-{Guid.NewGuid():N}.mkv");
        File.WriteAllText(p, "x");
        return p;
    }
}
