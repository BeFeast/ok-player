using System;
using System.Collections.Generic;

namespace OkPlayer.Core;

/// <summary>Parses the player's command line for the companion-library launch contract (PRD §13.1):
/// a media file/URL plus an optional <c>--resume &lt;time&gt;</c> the library uses to open the player at an
/// exact position. Pure and engine-agnostic so it can be unit-tested; the caller validates which positional
/// is a real file (URL / exists on disk) and applies the resume.</summary>
public static class LaunchArgs
{
    /// <param name="args">The command-line arguments <b>excluding</b> the executable (i.e. what a normal
    /// <c>Main(string[] args)</c> receives — call <c>Environment.GetCommandLineArgs()[1..]</c>).</param>
    /// <returns><c>Files</c>: positional tokens in order (the caller picks the first that is a URL or an
    /// existing file). <c>ResumeSeconds</c>: the parsed <c>--resume</c> value in seconds, or null when absent
    /// or malformed. A resume of 0 is meaningful — "start from the beginning", overriding remembered position.</returns>
    public static (IReadOnlyList<string> Files, double? ResumeSeconds) Parse(IReadOnlyList<string>? args)
    {
        var files = new List<string>();
        double? resume = null;
        if (args is null)
            return (files, resume);

        for (int i = 0; i < args.Count; i++)
        {
            string a = args[i];
            if (string.IsNullOrEmpty(a))
                continue;

            if (TryMatchOption(a, "resume", out string? inlineValue))
            {
                if (inlineValue is not null)
                {
                    resume = TimeCode.Parse(inlineValue);
                }
                // Only consume the *next* token as the value if it's actually a timecode — otherwise a path
                // following a bare "--resume" would be silently swallowed instead of opened.
                else if (i + 1 < args.Count && TimeCode.Parse(args[i + 1]) is { } next)
                {
                    resume = next;
                    i++;
                }
                continue;
            }

            if (a[0] == '-' || a[0] == '/')
                continue; // an unknown switch — ignore (file associations may append flags)

            files.Add(a);
        }
        return (files, resume);
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
