using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace OkPlayer.App.Services;

/// <summary>A user-authored chapter mark (time + title), stored in the sidecar and merged with the
/// file's own chapters for display, seeking and seek-bar markers.</summary>
public sealed class ChapterMark
{
    public double Time { get; set; }
    public string Title { get; set; } = string.Empty;
}

/// <summary>Persisted per-file playback state: resume position, bookmarks, last-opened time.</summary>
public sealed class FileRecord
{
    public double Position { get; set; }
    public double Duration { get; set; }
    /// <summary>True once the file was watched to the end. Distinct from <see cref="Position"/> == 0,
    /// which a finished file also stores (so it re-opens from the start): without this flag a consumer
    /// — the playlist's watched-marker, or a companion library reading history.json — can't tell a
    /// completed file from one that was never started. <c>percent watched ≈ Finished ? 1 : Position/Duration</c>.</summary>
    public bool Finished { get; set; }
    public string LastOpenedUtc { get; set; } = string.Empty;
    public string? Title { get; set; }
    public string? PosterPath { get; set; } // cached poster frame for continue-watching
    /// <summary>Remembered per-file track choice (mirrors the §13.1 launch preselect convention): <c>null</c>
    /// = not recorded, so restore leaves mpv's default; <c>-1</c> = explicitly OFF/none; <c>&gt;= 1</c> = the
    /// mpv track id to reselect. Nullable + <see cref="JsonIgnoreCondition.WhenWritingNull"/> keeps old records
    /// clean and back-compatible (they load with null and don't force a track).</summary>
    public int? SubtitleId { get; set; }
    public int? AudioId { get; set; }
    public List<double> Bookmarks { get; set; } = new();
    public List<ChapterMark> UserChapters { get; set; } = new(); // user-added chapters (the file's own are read-only)
}

/// <summary>
/// Per-file watch history persisted as human-readable JSON under %APPDATA%/OkPlayer (no database,
/// per the storage decision). Keyed by full path; drives resume-on-open, bookmarks and the
/// continue-watching recents. Local files only — network streams/URLs are not tracked.
/// </summary>
public sealed class HistoryService
{
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private readonly string? _path; // null when AppData is unavailable — persistence is disabled, not fatal
    private readonly object _lock = new();
    private Dictionary<string, FileRecord> _records = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Private (incognito) session: when true, no new playback data is recorded — <see cref="Record"/>,
    /// <see cref="SetPoster"/>, <see cref="AddBookmark"/> and the user-chapter writes all become no-ops,
    /// so watching leaves no trace (no resume position, no recents entry). Existing history stays
    /// readable (resume from before still works), and deletions (<see cref="Clear"/>, removes) still
    /// apply. Session-scoped: defaults off every launch, opt-in per session.
    /// </summary>
    public bool Private { get; set; }

    /// <summary>Raised after records are removed out-of-band (<see cref="Clear"/> or
    /// <see cref="PruneOlderThan"/>) so other windows — the player's continue-watching recents — can
    /// refresh instead of showing stale entries.</summary>
    public event Action? Changed;

    public HistoryService()
    {
        try
        {
            string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "OkPlayer");
            Directory.CreateDirectory(dir);
            _path = Path.Combine(dir, "history.json");
        }
        catch { _path = null; } // can't reach AppData: run without persistence rather than failing to start
        Load();
    }

    /// <summary>Test seam: persist to a caller-supplied path (or null for in-memory only) instead of %APPDATA%.</summary>
    internal HistoryService(string? path)
    {
        _path = path;
        Load();
    }

    private void Load()
    {
        try
        {
            if (_path is null || !File.Exists(_path))
                return;
            var data = JsonSerializer.Deserialize<Dictionary<string, FileRecord>>(File.ReadAllText(_path), JsonOpts);
            if (data is not null)
                _records = new Dictionary<string, FileRecord>(data, StringComparer.OrdinalIgnoreCase);
        }
        catch { /* corrupt/unreadable history is non-fatal — start fresh */ }
    }

    private void Save()
    {
        if (_path is null)
            return; // persistence disabled this session
        string json;
        lock (_lock)
            json = JsonSerializer.Serialize(_records, JsonOpts);
        string tmp = _path + ".tmp";
        // Files under %APPDATA% take brief exclusive locks from Defender and the Search indexer, which scan
        // each newly written file. That made the atomic replace throw a sharing violation and — because the
        // failure was swallowed — silently drop the save, losing the resume position and the remembered track
        // choice. Retry across the transient lock so the save actually lands (same fix as SettingsService.Save).
        for (int attempt = 0; ; attempt++)
        {
            try
            {
                File.WriteAllText(tmp, json);
                File.Move(tmp, _path, overwrite: true); // replace in one step so a crash can't truncate history
                break;
            }
            catch (Exception ex) when (attempt < 8 && ex is IOException or UnauthorizedAccessException)
            {
                System.Threading.Thread.Sleep(30); // up to ~240ms; the scanner's lock clears well within this
            }
            catch
            {
                break; // give up after the retries (or an unexpected error) — Save stays best-effort, never throws
            }
        }
    }

    public FileRecord? Get(string path)
    {
        if (string.IsNullOrEmpty(path))
            return null;
        lock (_lock)
            return _records.TryGetValue(path, out var r) ? r : null;
    }

    /// <summary>Record the latest position/duration for a local file (creates the entry if needed).
    /// <paramref name="finished"/> marks the file watched-to-end; it always overwrites the prior value,
    /// so re-watching a completed file from the start clears the flag. <paramref name="subtitleId"/> and
    /// <paramref name="audioId"/> remember the user's per-file track choice and are written ONLY when supplied
    /// (a <c>null</c> arg leaves the stored value untouched, so a caller that doesn't know the selection — or
    /// a record made before a track was picked — never clears a previously remembered one). See
    /// <see cref="FileRecord.SubtitleId"/> for the value convention.</summary>
    public void Record(string path, double position, double duration, bool finished = false,
                       int? subtitleId = null, int? audioId = null)
    {
        if (Private || !IsTrackable(path))
            return;
        lock (_lock)
        {
            FileRecord r = GetOrCreate(path);
            r.Position = position;
            if (duration > 0)
                r.Duration = duration;
            r.Finished = finished;
            r.Title = Path.GetFileNameWithoutExtension(path);
            r.LastOpenedUtc = DateTime.UtcNow.ToString("o");
            if (subtitleId.HasValue)
                r.SubtitleId = subtitleId;
            if (audioId.HasValue)
                r.AudioId = audioId;
        }
        Save();
    }

    /// <summary>Add a bookmark; returns false (no toast) for untrackable paths such as URLs, or while
    /// a <see cref="Private"/> session is active (nothing is persisted in incognito).</summary>
    public bool AddBookmark(string path, double time)
    {
        if (Private || !IsTrackable(path))
            return false;
        lock (_lock)
        {
            FileRecord r = GetOrCreate(path);
            // Keep a bookmark-first record complete so recents ordering/metadata stay correct.
            if (string.IsNullOrEmpty(r.Title))
                r.Title = Path.GetFileNameWithoutExtension(path);
            if (string.IsNullOrEmpty(r.LastOpenedUtc))
                r.LastOpenedUtc = DateTime.UtcNow.ToString("o");
            if (!r.Bookmarks.Any(b => Math.Abs(b - time) < 0.5))
            {
                r.Bookmarks.Add(time);
                r.Bookmarks.Sort();
            }
        }
        Save();
        return true;
    }

    public void SetPoster(string path, string posterPath)
    {
        if (Private || !IsTrackable(path))
            return;
        lock (_lock)
            GetOrCreate(path).PosterPath = posterPath;
        Save();
    }

    public IReadOnlyList<double> GetBookmarks(string path)
    {
        lock (_lock)
            return _records.TryGetValue(path, out var r) ? r.Bookmarks.ToList() : new List<double>();
    }

    public void RemoveBookmark(string path, double time)
    {
        lock (_lock)
        {
            if (_records.TryGetValue(path, out var r))
                r.Bookmarks.RemoveAll(b => Math.Abs(b - time) < 0.01);
        }
        Save();
    }

    public bool AddUserChapter(string path, double time, string title)
    {
        if (Private || !IsTrackable(path))
            return false;
        bool added = false;
        lock (_lock)
        {
            FileRecord r = GetOrCreate(path);
            if (string.IsNullOrEmpty(r.Title))
                r.Title = Path.GetFileNameWithoutExtension(path);
            if (string.IsNullOrEmpty(r.LastOpenedUtc))
                r.LastOpenedUtc = DateTime.UtcNow.ToString("o");
            if (!r.UserChapters.Any(c => Math.Abs(c.Time - time) < 0.5))
            {
                r.UserChapters.Add(new ChapterMark { Time = time, Title = title });
                r.UserChapters.Sort((a, b) => a.Time.CompareTo(b.Time));
                added = true;
            }
        }
        if (added)
            Save();
        return added; // false when a chapter already sits within 0.5s, so the caller won't claim it added one
    }

    public void RenameUserChapter(string path, double time, string title)
    {
        lock (_lock)
        {
            if (_records.TryGetValue(path, out var r) &&
                r.UserChapters.FirstOrDefault(c => Math.Abs(c.Time - time) < 0.01) is { } mark)
                mark.Title = title;
        }
        Save();
    }

    public void RemoveUserChapter(string path, double time)
    {
        lock (_lock)
        {
            if (_records.TryGetValue(path, out var r))
                r.UserChapters.RemoveAll(c => Math.Abs(c.Time - time) < 0.01);
        }
        Save();
    }

    /// <summary>The user's own chapters for a file (copies, so callers can't mutate the stored list).</summary>
    public IReadOnlyList<ChapterMark> GetUserChapters(string path)
    {
        lock (_lock)
            return _records.TryGetValue(path, out var r)
                ? r.UserChapters.Select(c => new ChapterMark { Time = c.Time, Title = c.Title }).ToList()
                : new List<ChapterMark>();
    }

    /// <summary>Most-recently-opened existing files, newest first, for continue-watching.</summary>
    public IReadOnlyList<(string Path, FileRecord Record)> Recents(int count)
    {
        lock (_lock)
            return _records
                .Where(kv => IsListable(kv.Key))
                .OrderByDescending(kv => kv.Value.LastOpenedUtc, StringComparer.Ordinal)
                .Take(count)
                .Select(kv => (kv.Key, kv.Value))
                .ToList();
    }

    /// <summary>Every tracked file that is still present, newest-opened first — the full History list. Unlike
    /// <see cref="Recents"/> this keeps finished files and applies no resume-progress threshold. A genuinely
    /// local-and-missing file is hidden so History never shows a dead path, but a network/URL entry is kept
    /// (see <see cref="IsListable"/>) so a transiently-offline NFS/SMB file doesn't vanish.</summary>
    public IReadOnlyList<(string Path, FileRecord Record)> All()
    {
        lock (_lock)
            return _records
                .Where(kv => IsListable(kv.Key))
                .OrderByDescending(kv => kv.Value.LastOpenedUtc, StringComparer.Ordinal)
                .Select(kv => (kv.Key, kv.Value))
                .ToList();
    }

    /// <summary>Whether a tracked path should still be listed in History / recents. Local files are listed only
    /// while they exist (a deleted file shouldn't linger). But <see cref="File.Exists"/> is unreliable for
    /// network paths — an NFS/SMB share that is briefly slow, offline, or auth-gated returns <c>false</c> even
    /// when the file is there (Exists swallows the IO/timeout exception) — so a URL or a network path (UNC or a
    /// mapped network drive) is never hidden on a false Exists; only a genuinely local-and-missing file is
    /// dropped. Without this, an NFS file vanished from History/recents on any transient blip.</summary>
    private static bool IsListable(string path)
        => path.Contains("://", StringComparison.Ordinal) || IsNetworkPath(path) || File.Exists(path);

    /// <summary>True for a UNC path (<c>\\server\share\…</c>) or a path on a <b>mapped network drive</b> (e.g. an
    /// NFS/SMB mount surfaced as <c>Z:\</c>, whose <see cref="DriveType.Network"/> stays reported even while the
    /// share is disconnected). A removable/fixed <b>local</b> drive that is currently unplugged or unmounted
    /// reports <see cref="DriveType.NoRootDirectory"/> (or fails to probe) — that is a local-and-missing file, so
    /// it is deliberately <i>not</i> treated as network: it falls through to <see cref="File.Exists"/> and drops
    /// off, rather than lingering in History as if it were a flaky network share.</summary>
    public static bool IsNetworkPath(string path)
        => IsNetworkPath(path, ProbeRootDriveType);

    /// <summary>Testable core of <see cref="IsNetworkPath(string)"/>: the root drive-type probe is injected so the
    /// classification can be unit-tested without depending on the volumes actually mounted on the test machine
    /// (the probe returns <c>null</c> when the root can't be classified).</summary>
    internal static bool IsNetworkPath(string path, Func<string, DriveType?> rootDriveType)
    {
        if (string.IsNullOrEmpty(path))
            return false;
        if (path.StartsWith(@"\\", StringComparison.Ordinal))
            return true; // UNC, including \\server\share and \\?\UNC\
        if (!Path.IsPathRooted(path))
            return false;
        string? root = Path.GetPathRoot(path);
        if (string.IsNullOrEmpty(root))
            return false;
        return rootDriveType(root) == DriveType.Network; // only a mapped network drive bypasses File.Exists
    }

    private static DriveType? ProbeRootDriveType(string root)
    {
        try { return new DriveInfo(root).DriveType; }
        catch { return null; } // unclassifiable root -> treat as local; File.Exists is the decider
    }

    /// <summary>Wipe all watch history (resume positions, recents, bookmarks, user chapters). Returns
    /// the number of file records removed. Persists the empty store so it survives restart.</summary>
    public int Clear()
    {
        int removed;
        lock (_lock)
        {
            removed = _records.Count;
            if (removed == 0)
                return 0;
            _records = new Dictionary<string, FileRecord>(StringComparer.OrdinalIgnoreCase);
        }
        Save();
        Changed?.Invoke();
        return removed;
    }

    /// <summary>Remove a single file's entry (resume position, bookmarks, user chapters, poster).
    /// Returns true if a record existed and was dropped. Persists and fires <see cref="Changed"/> so
    /// open surfaces — the recents shelf and the History list — refresh instead of showing it again.</summary>
    public bool Remove(string path)
    {
        if (string.IsNullOrEmpty(path))
            return false;
        bool removed;
        lock (_lock)
            removed = _records.Remove(path);
        if (removed)
        {
            Save();
            Changed?.Invoke();
        }
        return removed;
    }

    /// <summary>Retention: drop records last opened more than <paramref name="days"/> days ago.
    /// <paramref name="days"/> &lt;= 0 keeps everything (the default). Records with no/unparseable
    /// timestamp are kept. Returns the number pruned; persists only when something was removed.</summary>
    public int PruneOlderThan(int days)
    {
        if (days <= 0)
            return 0;
        DateTime cutoff = DateTime.UtcNow.AddDays(-days);
        int removed;
        lock (_lock)
        {
            var stale = _records
                .Where(kv => DateTime.TryParse(
                    kv.Value.LastOpenedUtc, null,
                    System.Globalization.DateTimeStyles.RoundtripKind, out var t) && t < cutoff)
                .Select(kv => kv.Key)
                .ToList();
            foreach (string key in stale)
                _records.Remove(key);
            removed = stale.Count;
        }
        if (removed > 0)
        {
            Save();
            Changed?.Invoke(); // a retention change can prune visible recents — let the player refresh
        }
        return removed;
    }

    private FileRecord GetOrCreate(string path)
    {
        if (!_records.TryGetValue(path, out var r))
        {
            r = new FileRecord();
            _records[path] = r;
        }
        return r;
    }

    private static bool IsTrackable(string path)
        => !string.IsNullOrEmpty(path) && !path.Contains("://", StringComparison.Ordinal);
}
