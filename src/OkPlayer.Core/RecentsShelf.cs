using System;

namespace OkPlayer.Core;

/// <summary>Pure rules for the welcome "Continue watching" shelf: how many fixed-width cards fit a given
/// row width, and what state a card shows. The shelf shows exactly as many cards as fit so it never needs a
/// horizontal scrollbar (the design's elegance bar); any remaining recent files stay reachable via History.
/// Kept here, engine- and UI-agnostic, so the rules are unit-tested rather than buried in the view.</summary>
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

    /// <summary>Card state for the welcome shelf (#776). The shelf selects by last-opened recency alone, so
    /// finished files appear on it: a finished card shows the Finished state — History's language for it —
    /// instead of a time-left chip, with an empty progress bar, and reopens from zero (resume refuses
    /// finished records, #767). An unfinished card keeps its time-left chip and fractional progress,
    /// unchanged. A record without a measured duration shows neither.</summary>
    public static (string Chip, double Progress) CardState(double position, double duration, bool finished,
                                                           string timeLeft)
        => finished ? ("Finished", 0)
         : duration <= 0 ? (string.Empty, 0)
         : (timeLeft, Math.Clamp(position / duration, 0, 1));
}
