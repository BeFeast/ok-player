using System.Collections.Generic;
using System.Text;

namespace OkPlayer.App.Services;

/// <summary>One mpv option as a <c>key=value</c> pair, for the Settings → Advanced key-value editor.</summary>
public readonly record struct MpvOption(string Key, string Value);

/// <summary>Parses/serialises the user mpv.conf escape-hatch file as plain <c>key=value</c> options for the
/// Advanced editor. Pure (no I/O, no WinUI) so it is unit-testable headlessly. The engine-side loader
/// (<c>MpvVideoPanel.ApplyUserConfig</c>) is the source of truth for the on-disk format and this mirrors it
/// exactly so a round-trip is faithful: one option per line; blank lines and lines beginning with <c>#</c>
/// are ignored; the value is everything after the first <c>=</c> (so values may themselves contain <c>=</c>);
/// a bare key with no <c>=</c> means <c>key=yes</c>; keys and values are trimmed. A <c>#</c> that is not the
/// first character is part of the value (e.g. <c>sub-color=#FFFFFF</c>), not a comment.</summary>
public static class MpvConfText
{
    /// <summary>Parse mpv.conf text into options, in file order. Comments and blank lines are dropped (the
    /// editor is key/value only); a bare key becomes <c>yes</c>. Tolerant of CRLF and LF.</summary>
    public static IReadOnlyList<MpvOption> Parse(string? text)
    {
        var options = new List<MpvOption>();
        if (string.IsNullOrEmpty(text))
            return options;

        foreach (string rawLine in text.Split('\n'))
        {
            string line = rawLine.Trim(); // also strips a trailing '\r' from CRLF endings
            if (line.Length == 0 || line[0] == '#')
                continue;

            int eq = line.IndexOf('=');
            string key = (eq >= 0 ? line[..eq] : line).Trim();
            if (key.Length == 0)
                continue;

            string value = eq >= 0 ? line[(eq + 1)..].Trim() : "yes";
            options.Add(new MpvOption(key, value));
        }

        return options;
    }

    /// <summary>Serialise options back to mpv.conf text: one <c>key=value</c> per line with a trailing
    /// newline. Options with a blank key are skipped; keys and values are trimmed. Stable, so re-saving an
    /// unchanged document is a no-op diff and <c>Parse(Serialize(x))</c> round-trips.</summary>
    public static string Serialize(IEnumerable<MpvOption> options)
    {
        var sb = new StringBuilder();
        foreach (MpvOption option in options)
        {
            string key = option.Key?.Trim() ?? string.Empty;
            if (key.Length == 0)
                continue;
            string value = (option.Value ?? string.Empty).Trim();
            sb.Append(key).Append('=').Append(value).Append('\n');
        }
        return sb.ToString();
    }
}
