using System;
using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>A named subtitle appearance preset — one of a small, curated set of looks the user picks from in
/// Settings → Subtitles. Each preset is a fixed map of mpv subtitle-style options, kept as pure data so the
/// option set is unit-testable and one definition drives both the engine (apply) and the UI label.
///
/// These options style mpv's OWN text-subtitle renderer (SRT / plain text). ASS/SSA subtitles carry their
/// own embedded styling, which mpv respects by design (<c>sub-ass-override</c> defaults to <c>scale</c>), so
/// a preset deliberately does NOT repaint them — the same asymmetry that forced the OSC-lift onto
/// <c>sub-pos</c> rather than <c>sub-margin-y</c> (see <see cref="SubtitleLift"/>). Every preset sets the
/// SAME set of options, so switching presets fully overrides the previous one with no residual state.</summary>
public sealed class SubtitleStyle
{
    /// <summary>Stable identifier persisted in settings (never localized). Also the preset's display text is
    /// the Settings button's own Content — the Core model only owns the key and the option set.</summary>
    public string Key { get; }

    /// <summary>Ordered mpv option → value pairs. Applied via set-property at engine init and on a live
    /// settings change. Colors are <c>#RRGGBB</c> (fully opaque) — the universally-accepted mpv color form,
    /// so they parse identically regardless of whether a build expects a leading alpha byte.</summary>
    public IReadOnlyList<KeyValuePair<string, string>> Options { get; }

    private SubtitleStyle(string key, params (string Name, string Value)[] options)
    {
        Key = key;
        var list = new List<KeyValuePair<string, string>>(options.Length);
        foreach (var (name, value) in options)
            list.Add(new KeyValuePair<string, string>(name, value));
        Options = list;
    }

    // The exact set of options every preset writes. Listing all six in each preset (rather than only the ones
    // that differ from mpv's defaults) is deliberate: switching from any preset to any other then restores
    // every field, so e.g. going Classic → Default actually repaints yellow back to white.
    public static readonly SubtitleStyle Default = new("Default",
        ("sub-color", "#FFFFFF"), ("sub-border-color", "#000000"), ("sub-border-size", "3"),
        ("sub-shadow-offset", "0"), ("sub-shadow-color", "#000000"), ("sub-bold", "no"));

    public static readonly SubtitleStyle Bold = new("Bold",
        ("sub-color", "#FFFFFF"), ("sub-border-color", "#000000"), ("sub-border-size", "3.2"),
        ("sub-shadow-offset", "0"), ("sub-shadow-color", "#000000"), ("sub-bold", "yes"));

    public static readonly SubtitleStyle Classic = new("Classic",
        ("sub-color", "#FFFF00"), ("sub-border-color", "#000000"), ("sub-border-size", "3"),
        ("sub-shadow-offset", "0"), ("sub-shadow-color", "#000000"), ("sub-bold", "no"));

    public static readonly SubtitleStyle Contrast = new("Contrast",
        ("sub-color", "#FFFFFF"), ("sub-border-color", "#000000"), ("sub-border-size", "4"),
        ("sub-shadow-offset", "1.5"), ("sub-shadow-color", "#000000"), ("sub-bold", "no"));

    /// <summary>All presets in display order (the order the Settings buttons render).</summary>
    public static readonly IReadOnlyList<SubtitleStyle> All = new[] { Default, Bold, Classic, Contrast };

    /// <summary>The preset for a stored key, falling back to <see cref="Default"/> for an unknown or empty
    /// key so settings written by another version (or a hand-edited file) degrade gracefully.</summary>
    public static SubtitleStyle FromKey(string? key)
    {
        if (!string.IsNullOrEmpty(key))
            foreach (var style in All)
                if (string.Equals(style.Key, key, StringComparison.OrdinalIgnoreCase))
                    return style;
        return Default;
    }
}
