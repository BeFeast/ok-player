using System.Threading;
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

    [Theory]
    [InlineData(@"\\nas\media\movie.mkv", true)]       // UNC share — kept even if File.Exists blips false
    [InlineData(@"\\?\UNC\nas\media\movie.mkv", true)] // extended-length UNC
    [InlineData(@"\\?\C:\media\movie.mkv", false)]     // extended-length LOCAL path — a drive, not a share
    [InlineData(@"C:\media\movie.mkv", false)]         // local fixed drive — gated on real existence
    [InlineData(@"movie.mkv", false)]                  // relative path — not rooted
    [InlineData("", false)]                            // empty
    public void IsNetworkPath_TreatsUncAndNetworkDrivesAsNetwork(string path, bool expected)
        => Assert.Equal(expected, HistoryService.IsNetworkPath(path));

    // A path rooted on whatever OS the test runs on, so the drive-type branch is actually reached: the
    // engine-agnostic unit job runs on Linux, where a Windows "Z:\" path isn't rooted and the method bails
    // before the probe. The probe is injected, so no real volume is touched on either OS.
    private static string RootedMediaPath()
        => Path.Combine(Path.GetPathRoot(Path.GetFullPath("."))!, "media", "movie.mkv");

    [Theory]
    [InlineData(DriveType.Network, true)]           // mapped network drive (NFS/SMB) — bypasses File.Exists
    [InlineData(DriveType.Fixed, false)]            // local fixed disk — gated on real existence
    [InlineData(DriveType.Removable, false)]        // USB while plugged in — gated on real existence
    [InlineData(DriveType.NoRootDirectory, false)]  // removable/local drive UNPLUGGED — must drop off, not linger
    public void IsNetworkPath_OnlyMappedNetworkDriveBypassesExistence(DriveType type, bool expected)
        => Assert.Equal(expected, HistoryService.IsNetworkPath(RootedMediaPath(), _ => type));

    [Fact] // probe couldn't classify the root -> treat as local, let File.Exists decide (don't keep it visible)
    public void IsNetworkPath_UnclassifiableRoot_IsTreatedAsLocal()
        => Assert.False(HistoryService.IsNetworkPath(RootedMediaPath(), _ => null));

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
    public void Record_RemembersSubtitleAndAudioTrack_RoundTrips()
    {
        var a = New();
        a.Record(Movie, 120, 600, finished: false, subtitleId: 3, audioId: 2);

        var b = New(); // a fresh service reads the same file
        var r = b.Get(Movie)!;
        Assert.Equal(3, r.SubtitleId);
        Assert.Equal(2, r.AudioId);
    }

    [Fact]
    public void Record_NullTrackArgs_LeaveStoredIdsUnchanged()
    {
        var h = New();
        h.Record(Movie, 100, 600, subtitleId: 5, audioId: 1);
        h.Record(Movie, 150, 600); // a later position-only save (null track args) must not wipe the choice

        var r = h.Get(Movie)!;
        Assert.Equal(150, r.Position);
        Assert.Equal(5, r.SubtitleId);
        Assert.Equal(1, r.AudioId);
    }

    [Fact]
    public void Record_SubtitleAndAudioOff_RoundTripAsMinusOne()
    {
        var a = New();
        a.Record(Movie, 10, 600, subtitleId: -1, audioId: -1); // -1 = explicitly off/none

        var b = New();
        var r = b.Get(Movie)!;
        Assert.Equal(-1, r.SubtitleId);
        Assert.Equal(-1, r.AudioId);
    }

    [Fact]
    public void Get_OldRecordWithoutTrackFields_LoadsWithNullIds()
    {
        // A history.json written before the track fields existed must load cleanly (back-compat), with the new
        // ids null so restore leaves mpv's default rather than forcing a track.
        File.WriteAllText(_path, """
            {
              "C:\\media\\movie.mkv": {
                "Position": 42,
                "Duration": 600,
                "Finished": false,
                "LastOpenedUtc": "2026-01-01T00:00:00.0000000Z"
              }
            }
            """);

        var r = New().Get(Movie)!;
        Assert.Equal(42, r.Position);
        Assert.Null(r.SubtitleId);
        Assert.Null(r.AudioId);
    }

    // Regression mirror of SettingsServiceTests: %APPDATA% files take brief exclusive locks from Defender /
    // the Search indexer. Record's Save must retry across a transient lock instead of silently dropping the
    // write (which lost the resume position AND, now, the remembered track choice).
    [Fact]
    public void Save_RetriesAcrossATransientLock_AndStillPersists()
    {
        var a = New();
        a.Record(Movie, 10, 600, subtitleId: 2); // create the file first so the next save is a replace
        Assert.True(File.Exists(_path));

        var locker = new FileStream(_path, FileMode.Open, FileAccess.Read, FileShare.None);
        var release = Task.Run(() => { Thread.Sleep(60); locker.Dispose(); }); // free it within the retry budget
        a.Record(Movie, 305, 600, subtitleId: 4); // retries until the lock clears instead of giving up
        release.Wait();

        var r = New().Get(Movie)!;
        Assert.Equal(305, r.Position);
        Assert.Equal(4, r.SubtitleId);
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
