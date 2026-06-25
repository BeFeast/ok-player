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
    public string LastOpenedUtc { get; set; } = string.Empty;
    public string? Title { get; set; }
    public string? PosterPath { get; set; } // cached poster frame for continue-watching
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
        try
        {
            string json;
            lock (_lock)
                json = JsonSerializer.Serialize(_records, JsonOpts);
            string tmp = _path + ".tmp";
            File.WriteAllText(tmp, json);
            File.Move(tmp, _path, overwrite: true); // replace in one step so a crash can't truncate history
        }
        catch { /* best effort */ }
    }

    public FileRecord? Get(string path)
    {
        if (string.IsNullOrEmpty(path))
            return null;
        lock (_lock)
            return _records.TryGetValue(path, out var r) ? r : null;
    }

    /// <summary>Record the latest position/duration for a local file (creates the entry if needed).</summary>
    public void Record(string path, double position, double duration)
    {
        if (!IsTrackable(path))
            return;
        lock (_lock)
        {
            FileRecord r = GetOrCreate(path);
            r.Position = position;
            if (duration > 0)
                r.Duration = duration;
            r.Title = Path.GetFileNameWithoutExtension(path);
            r.LastOpenedUtc = DateTime.UtcNow.ToString("o");
        }
        Save();
    }

    /// <summary>Add a bookmark; returns false (no toast) for untrackable paths such as URLs.</summary>
    public bool AddBookmark(string path, double time)
    {
        if (!IsTrackable(path))
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
        if (!IsTrackable(path))
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
        if (!IsTrackable(path))
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
                .Where(kv => File.Exists(kv.Key))
                .OrderByDescending(kv => kv.Value.LastOpenedUtc, StringComparer.Ordinal)
                .Take(count)
                .Select(kv => (kv.Key, kv.Value))
                .ToList();
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
