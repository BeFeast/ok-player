using System;
using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>
/// A linear, per-window playlist: the files to play in natural order, plus a cursor at the current one.
/// The folder-as-playlist behavior builds one from the opened file's folder. Next/Prev move the cursor and
/// hand back the path to open (or null at the ends); HasNext/HasPrev drive the Prev/Next button enabled
/// state. Queue, shuffle, repeat, and .m3u are layered on later — this is the MVP core. Pure/testable.
/// </summary>
public sealed class Playlist
{
    private readonly List<string> _items;

    /// <summary>Build a playlist from a set of paths (sorted naturally), with the cursor on <paramref name="current"/>.</summary>
    public Playlist(IEnumerable<string> items, string current)
    {
        _items = new List<string>(items);
        _items.Sort(NaturalComparer.Instance);
        CurrentIndex = IndexOf(current);
    }

    public IReadOnlyList<string> Items => _items;
    public int Count => _items.Count;
    public int CurrentIndex { get; private set; }

    public string? Current => CurrentIndex >= 0 && CurrentIndex < _items.Count ? _items[CurrentIndex] : null;
    public bool HasNext => CurrentIndex >= 0 && CurrentIndex + 1 < _items.Count;
    public bool HasPrev => CurrentIndex > 0;

    /// <summary>Advance the cursor and return the next path, or null if already at the end.</summary>
    public string? Next() => HasNext ? _items[++CurrentIndex] : null;

    /// <summary>Step the cursor back and return the previous path, or null if already at the start.</summary>
    public string? Prev() => HasPrev ? _items[--CurrentIndex] : null;

    /// <summary>Re-point the cursor at <paramref name="path"/> if it's in the list (e.g. a queued item was opened directly).</summary>
    public bool SetCurrent(string path)
    {
        int i = IndexOf(path);
        if (i < 0)
            return false;
        CurrentIndex = i;
        return true;
    }

    private int IndexOf(string path) =>
        _items.FindIndex(p => string.Equals(p, path, StringComparison.OrdinalIgnoreCase));
}
