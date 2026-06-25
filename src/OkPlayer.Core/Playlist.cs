using System;
using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>How the playlist behaves at its ends.</summary>
public enum RepeatMode
{
    Off, // stop at the last item
    One, // replay the current item on auto-advance
    All, // wrap around (last → first, first → last)
}

/// <summary>
/// A per-window playlist: the folder's files in natural order, a cursor, and the play modes (repeat,
/// shuffle). Navigation reads the neighbour in the active <em>play order</em> (shuffled or natural), wrapping
/// when Repeat=All; auto-advance additionally honours Repeat=One by replaying the current file. The cursor
/// moves only through SetCurrent, so a caller can peek-then-open without ever desyncing. Pure/testable.
/// </summary>
public sealed class Playlist
{
    private readonly List<string> _items;  // files, natural-sorted (stable identity order)
    private List<int> _order;              // playback order: indices into _items (identity unless shuffled)
    private readonly Random _rng;
    private bool _shuffle;

    /// <summary>Build a playlist from a set of paths, cursor on <paramref name="current"/>. <paramref name="sort"/>
    /// natural-sorts the items (the folder case); pass false to keep the given order (an .m3u's order matters).</summary>
    public Playlist(IEnumerable<string> items, string current, bool sort = true) : this(items, current, new Random(), sort) { }

    // Seam for deterministic shuffle tests.
    internal Playlist(IEnumerable<string> items, string current, Random rng, bool sort = true)
    {
        _items = new List<string>(items);
        if (sort)
            _items.Sort(NaturalComparer.Instance);
        _rng = rng;
        CurrentIndex = IndexOf(current);
        _order = new List<int>();
        RebuildOrder();
    }

    public IReadOnlyList<string> Items => _items;
    public int Count => _items.Count;
    public int CurrentIndex { get; private set; } // index into _items, or -1
    public RepeatMode Repeat { get; set; } = RepeatMode.Off;

    /// <summary>Shuffle the play order (current file stays first); turning it off restores natural order.</summary>
    public bool Shuffle
    {
        get => _shuffle;
        set { if (_shuffle != value) { _shuffle = value; RebuildOrder(); } }
    }

    public string? Current => CurrentIndex >= 0 && CurrentIndex < _items.Count ? _items[CurrentIndex] : null;

    /// <summary>The next path in play order without moving the cursor (wraps when Repeat=All), or null at the
    /// end. Repeat=One does not affect manual next — see <see cref="AutoAdvanceTarget"/>.</summary>
    public string? PeekNext => Neighbour(+1);
    public string? PeekPrev => Neighbour(-1);
    public bool HasNext => PeekNext is not null;
    public bool HasPrev => PeekPrev is not null;

    /// <summary>What to play when the current file ends: the same file when Repeat=One, otherwise PeekNext.</summary>
    public string? AutoAdvanceTarget => Repeat == RepeatMode.One ? Current : PeekNext;

    /// <summary>Advance the cursor to the next item and return it (null at the end). Equivalent to opening PeekNext.</summary>
    public string? Next() { var n = PeekNext; if (n is not null) SetCurrent(n); return n; }
    public string? Prev() { var p = PeekPrev; if (p is not null) SetCurrent(p); return p; }

    /// <summary>Re-point the cursor at <paramref name="path"/> if present (case-insensitive). A sequential
    /// step keeps the order; jumping elsewhere while shuffled re-shuffles the remaining order (new current
    /// first) so a click never skips the files between, and a wrap reshuffles the next cycle.</summary>
    public bool SetCurrent(string path)
    {
        int i = IndexOf(path);
        if (i < 0 || i == CurrentIndex)
            return i >= 0; // not found → false; already current → no-op
        if (_shuffle && CurrentIndex >= 0)
        {
            int oldPos = _order.IndexOf(CurrentIndex);
            bool sequential = oldPos >= 0 &&
                ((oldPos + 1 < _order.Count && _order[oldPos + 1] == i) ||
                 (oldPos - 1 >= 0 && _order[oldPos - 1] == i));
            CurrentIndex = i;
            if (!sequential)
                RebuildOrder();
        }
        else
        {
            CurrentIndex = i;
        }
        return true;
    }

    private string? Neighbour(int step)
    {
        if (CurrentIndex < 0 || _order.Count == 0)
            return null;
        int pos = _order.IndexOf(CurrentIndex) + step;
        if (pos < 0 || pos >= _order.Count)
        {
            if (Repeat != RepeatMode.All)
                return null;
            pos = (pos + _order.Count) % _order.Count; // wrap
        }
        return _items[_order[pos]];
    }

    private void RebuildOrder()
    {
        _order = new List<int>(_items.Count);
        for (int i = 0; i < _items.Count; i++)
            _order.Add(i);
        if (!_shuffle)
            return;
        for (int i = _order.Count - 1; i > 0; i--) // Fisher–Yates
        {
            int j = _rng.Next(i + 1);
            (_order[i], _order[j]) = (_order[j], _order[i]);
        }
        if (CurrentIndex >= 0) // keep the playing file at the front so it isn't skipped
        {
            _order.Remove(CurrentIndex);
            _order.Insert(0, CurrentIndex);
        }
    }

    private int IndexOf(string path) =>
        _items.FindIndex(p => string.Equals(p, path, StringComparison.OrdinalIgnoreCase));
}
