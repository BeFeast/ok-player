using System;
using System.Globalization;

namespace OkPlayer.Core;

/// <summary>Parses human-typed timecodes into seconds for "go to time" — "90", "1:30", "1:23:45",
/// "2:05.5" all work. Returns null when the text isn't a valid non-negative timecode, so the caller can
/// reject bad input rather than seek somewhere wrong.</summary>
public static class TimeCode
{
    public static double? Parse(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
            return null;
        string[] parts = text.Trim().Split(':');
        if (parts.Length is < 1 or > 3)
            return null;

        double total = 0;
        for (int i = 0; i < parts.Length; i++)
        {
            // Only the last field (seconds) may carry a fractional part; hours/minutes must be whole.
            bool last = i == parts.Length - 1;
            if (!double.TryParse(parts[i], NumberStyles.AllowDecimalPoint, CultureInfo.InvariantCulture, out double v) || v < 0)
                return null;
            if (!last && v != Math.Floor(v))
                return null;
            total = total * 60 + v;
        }
        return total;
    }

    /// <summary>Format seconds as a timecode: H:MM:SS past an hour, else M:SS.</summary>
    public static string Format(double seconds)
    {
        if (seconds < 0 || double.IsNaN(seconds))
            seconds = 0;
        int total = (int)Math.Round(seconds);
        int h = total / 3600, m = total % 3600 / 60, s = total % 60;
        return h > 0
            ? string.Format(CultureInfo.InvariantCulture, "{0}:{1:D2}:{2:D2}", h, m, s)
            : string.Format(CultureInfo.InvariantCulture, "{0}:{1:D2}", m, s);
    }
}
