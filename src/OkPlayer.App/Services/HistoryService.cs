using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace OkPlayer.App.Services;

/// <summary>Persisted per-file playback state: resume position, bookmarks, last-opened time.</summary>
public sealed class FileRecord
{
    public double Position { get; set; }
    public double Duration { get; set; }
    public string LastOpenedUtc { get; set; } = string.Empty;
    public string? Title { get; set; }
    public List<double> Bookmarks { get; set; } = new();
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

    private readonly string _path;
    private readonly object _lock = new();
    private Dictionary<string, FileRecord> _records = new(StringComparer.OrdinalIgnoreCase);

    public HistoryService()
    {
        string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "OkPlayer");
        Directory.CreateDirectory(dir);
        _path = Path.Combine(dir, "history.json");
        Load();
    }

    private void Load()
    {
        try
        {
            if (!File.Exists(_path))
                return;
            var data = JsonSerializer.Deserialize<Dictionary<string, FileRecord>>(File.ReadAllText(_path), JsonOpts);
            if (data is not null)
                _records = new Dictionary<string, FileRecord>(data, StringComparer.OrdinalIgnoreCase);
        }
        catch { /* corrupt/unreadable history is non-fatal — start fresh */ }
    }

    private void Save()
    {
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

    public void AddBookmark(string path, double time)
    {
        if (!IsTrackable(path))
            return;
        lock (_lock)
        {
            FileRecord r = GetOrCreate(path);
            if (!r.Bookmarks.Any(b => Math.Abs(b - time) < 0.5))
            {
                r.Bookmarks.Add(time);
                r.Bookmarks.Sort();
            }
        }
        Save();
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
