using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>
/// Compares strings the way a file manager lists files: alphabetic runs case-insensitively, digit runs by
/// numeric value (so "ep2" precedes "ep10", not the other way round). Leading zeros don't change the value
/// ("ep02" == "ep2" numerically) but break ties so the order stays stable. Used to put a folder's files in
/// the natural playlist order. Pure and headlessly testable.
/// </summary>
public sealed class NaturalComparer : IComparer<string>
{
    public static readonly NaturalComparer Instance = new();

    public int Compare(string? x, string? y)
    {
        if (x is null)
            return y is null ? 0 : -1;
        if (y is null)
            return 1;

        int ix = 0, iy = 0;
        while (ix < x.Length && iy < y.Length)
        {
            if (char.IsDigit(x[ix]) && char.IsDigit(y[iy]))
            {
                int sx = ix; while (ix < x.Length && char.IsDigit(x[ix])) ix++;
                int sy = iy; while (iy < y.Length && char.IsDigit(y[iy])) iy++;

                // Compare by value: drop leading zeros, then longer number wins, else lexically.
                string nx = x.Substring(sx, ix - sx).TrimStart('0');
                string ny = y.Substring(sy, iy - sy).TrimStart('0');
                if (nx.Length != ny.Length)
                    return nx.Length < ny.Length ? -1 : 1;
                int cmp = string.CompareOrdinal(nx, ny);
                if (cmp != 0)
                    return cmp < 0 ? -1 : 1;
                // Equal value — fewer leading zeros first, so "ep2" precedes "ep02" deterministically.
                if ((ix - sx) != (iy - sy))
                    return (ix - sx) < (iy - sy) ? -1 : 1;
            }
            else
            {
                char cx = char.ToUpperInvariant(x[ix]), cy = char.ToUpperInvariant(y[iy]);
                if (cx != cy)
                    return cx < cy ? -1 : 1;
                ix++; iy++;
            }
        }
        // Whichever string still has characters left sorts after the shorter prefix.
        int rx = x.Length - ix, ry = y.Length - iy;
        return rx == ry ? 0 : (rx < ry ? -1 : 1);
    }
}
