using System;
using System.Collections.Generic;
using System.Globalization;

namespace OkPlayer.Core;

/// <summary>Parses the player's command line for the companion-library launch contract (PRD §13.1):
/// a media file/URL plus optional <c>--resume &lt;time&gt;</c> and <c>--sub</c>/<c>--audio</c> track
/// preselection the library uses to open the player at an exact position with a chosen subtitle/audio track.
/// Pure and engine-agnostic so it can be unit-tested; the caller validates which positional is a real file
/// (URL / exists on disk) and applies the result.</summary>
public static class LaunchArgs
{
    /// <param name="args">The command-line arguments <b>excluding</b> the executable (i.e. what a normal
    /// <c>Main(string[] args)</c> receives — call <c>Environment.GetCommandLineArgs()[1..]</c>).</param>
    /// <returns><c>Files</c>: positional tokens in order (the caller picks the first that is a URL or an
    /// existing file). <c>ResumeSeconds</c>: the parsed <c>--resume</c> value in seconds, or null when absent
    /// or malformed (a resume of 0 is meaningful — "start from the beginning", overriding remembered position).
    /// <c>Sub</c>/<c>Audio</c>: the <c>--sub</c>/<c>--audio</c> track id to preselect (mpv track id ≥ 0), or
    /// <c>-1</c> for an explicit <c>no</c>/<c>off</c>, or null when absent/malformed.</returns>
    public static (IReadOnlyList<string> Files, double? ResumeSeconds, int? Sub, int? Audio) Parse(IReadOnlyList<string>? args)
    {
        var files = new List<string>();
        double? resume = null;
        int? sub = null, audio = null;
        if (args is null)
            return (files, resume, sub, audio);

        for (int i = 0; i < args.Count; i++)
        {
            string a = args[i];
            if (string.IsNullOrEmpty(a))
                continue;

            if (TryMatchOption(a, "resume", out string? inlineValue))
            { resume = Consume(inlineValue, args, ref i, TimeCode.Parse); continue; }
            if (TryMatchOption(a, "sub", out inlineValue))
            { sub = Consume(inlineValue, args, ref i, ParseTrackId); continue; }
            if (TryMatchOption(a, "audio", out inlineValue))
            { audio = Consume(inlineValue, args, ref i, ParseTrackId); continue; }

            if (a[0] == '-' || a[0] == '/')
                continue; // an unknown switch — ignore (file associations may append flags)

            files.Add(a);
        }
        return (files, resume, sub, audio);
    }

    /// <summary>Resolve an option's value: the inline part (<c>--opt=value</c>) if present, else the following
    /// token — but only consume that token when it actually parses, so a path after a bare <c>--opt</c> stays
    /// a positional instead of being silently swallowed.</summary>
    private static T? Consume<T>(string? inlineValue, IReadOnlyList<string> args, ref int i, Func<string, T?> parse)
        where T : struct
    {
        if (inlineValue is not null)
            return parse(inlineValue);
        if (i + 1 < args.Count && parse(args[i + 1]) is { } next)
        {
            i++;
            return next;
        }
        return null;
    }

    /// <summary>A track id is a non-negative integer, or <c>no</c>/<c>off</c> (→ -1, "select none"). Anything
    /// else is null (ignored).</summary>
    private static int? ParseTrackId(string? s)
    {
        if (string.IsNullOrWhiteSpace(s))
            return null;
        s = s.Trim();
        if (s.Equals("no", StringComparison.OrdinalIgnoreCase) || s.Equals("off", StringComparison.OrdinalIgnoreCase))
            return -1;
        return int.TryParse(s, NumberStyles.None, CultureInfo.InvariantCulture, out int id) ? id : null;
    }

    /// <summary>Matches <c>--name</c>, <c>-name</c> or <c>/name</c> (case-insensitive). When the token carries
    /// an inline value (<c>--name=value</c> or <c>--name:value</c>), returns it via <paramref name="value"/>;
    /// otherwise <paramref name="value"/> is null and the value (if any) is the following token.</summary>
    private static bool TryMatchOption(string token, string name, out string? value)
    {
        value = null;
        int dashes = 0;
        while (dashes < token.Length && (token[dashes] == '-' || token[dashes] == '/'))
            dashes++;
        if (dashes == 0)
            return false;

        string body = token.Substring(dashes);
        int sep = body.IndexOfAny(new[] { '=', ':' });
        string key = sep >= 0 ? body.Substring(0, sep) : body;
        if (!key.Equals(name, StringComparison.OrdinalIgnoreCase))
            return false;

        if (sep >= 0)
            value = body.Substring(sep + 1);
        return true;
    }
}
