using System;
using System.Xml.Linq;

namespace OkPlayer.Core;

/// <summary>
/// Parsed fields from a Kodi/Jellyfin/Emby <c>.nfo</c> sidecar — the local-library metadata convention (an XML
/// file next to the media, or a <c>movie.nfo</c> in the movie's folder). A pure, tolerant parse: reads the
/// common fields from whatever root the file uses (<c>movie</c>, <c>episodedetails</c>, <c>musicvideo</c>,
/// <c>tvshow</c>, …) and ignores the rest. Returns null for a non-XML <c>.nfo</c> (some are just a bare scraper
/// URL) or one with no usable title. Engine- and UI-free so it can be unit-tested headlessly.
/// </summary>
public sealed record NfoMetadata(string Title, int? Year, string? Plot)
{
    /// <summary>Parse a <c>.nfo</c> document. Null when the text isn't XML, has no root, or carries no title.</summary>
    public static NfoMetadata? Parse(string? xml)
    {
        if (string.IsNullOrWhiteSpace(xml))
            return null;
        XDocument doc;
        try { doc = XDocument.Parse(xml, LoadOptions.None); }
        catch { return null; } // not XML (e.g. a legacy .nfo that's just an IMDb URL) — nothing to read
        if (doc.Root is not { } root)
            return null;

        // Title is required to be useful; <title> first, then <originaltitle>.
        string? title = Child(root, "title") ?? Child(root, "originaltitle");
        if (string.IsNullOrWhiteSpace(title))
            return null;

        int? year = null;
        if (int.TryParse(Child(root, "year"), out int y) && y > 0)
            year = y;
        else if (Child(root, "premiered") is { Length: >= 4 } prem && int.TryParse(prem.AsSpan(0, 4), out int py) && py > 0)
            year = py; // <premiered>2020-05-01</premiered>
        else if (Child(root, "aired") is { Length: >= 4 } aired && int.TryParse(aired.AsSpan(0, 4), out int ay) && ay > 0)
            year = ay;

        string? plot = Child(root, "plot") ?? Child(root, "outline");
        return new NfoMetadata(title!.Trim(), year, string.IsNullOrWhiteSpace(plot) ? null : plot!.Trim());
    }

    // First DIRECT child element with the given local name (namespace-agnostic), non-empty trimmed value or null.
    // Direct children only, so a nested <title> (inside <set>, <actor>, …) can't be mistaken for the item title.
    private static string? Child(XElement root, string name)
    {
        foreach (var e in root.Elements())
            if (string.Equals(e.Name.LocalName, name, StringComparison.OrdinalIgnoreCase))
            {
                string? v = (string?)e;
                if (!string.IsNullOrWhiteSpace(v))
                    return v;
            }
        return null;
    }
}
