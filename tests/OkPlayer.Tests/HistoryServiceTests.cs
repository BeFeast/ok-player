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
}
