using System;

namespace OkPlayer.Core;

/// <summary>Pure layout math for the welcome "Continue watching" shelf: how many fixed-width cards fit a given
/// row width. The shelf shows exactly this many so it never needs a horizontal scrollbar (the design's
/// elegance bar); any remaining resumable files stay reachable via History. Kept here, engine- and
/// UI-agnostic, so the fit rule is unit-tested rather than buried in the view.</summary>
public static class RecentsShelf
{
    /// <summary>How many cards to show: as many whole cards as fit <paramref name="rowWidth"/>, capped by how
    /// many are actually <paramref name="available"/>.
    ///
    /// n cards laid out with (n-1) gaps need n*card + (n-1)*spacing ≤ width, i.e. n ≤ (width + spacing) /
    /// (card + spacing); we take the floor. This is 0 when not even one card fits — important because the row
    /// no longer scrolls, so on a side-snapped or very narrow window we must show nothing (and route the items
    /// to the overflow control) rather than clip a full-width card. Before the row is measured
    /// (<paramref name="rowWidth"/> ≤ 0) we fall back to <paramref name="unmeasuredDefault"/> so the first
    /// paint is sensible; a SizeChanged then corrects it. The result is always clamped to [0, available].</summary>
    public static int VisibleCount(double rowWidth, int available, double cardWidth, double spacing,
                                   int unmeasuredDefault = 3)
    {
        if (available <= 0)
            return 0;
        int fit = rowWidth <= 0
            ? Math.Max(0, unmeasuredDefault)
            : (int)((rowWidth + spacing) / (cardWidth + spacing)); // 0 when one card doesn't fit -> no clipping
        return Math.Min(fit, available);
    }
}
