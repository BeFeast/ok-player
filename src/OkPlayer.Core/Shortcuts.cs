using System;
using System.Collections.Generic;
using System.Linq;

namespace OkPlayer.Core;

/// <summary>
/// Cross-shell keyboard shortcut contract. This mirrors <c>okp_core::shortcuts</c>: stable action ids,
/// default chords, the line-oriented <c>action-id=Chord</c> storage format, conflict detection, and the
/// editor's primary/secondary-slot rules. Platform shells only translate native key events to canonical
/// key names and render the resulting state.
/// </summary>
public enum ShortcutAction
{
    PlayPause,
    SeekBack,
    SeekForward,
    FrameForward,
    FrameBack,
    PreviousItem,
    NextItem,
    VolumeDown,
    VolumeUp,
    Mute,
    OpenFile,
    AddSubtitle,
    OpenUrl,
    CloseMedia,
    SaveScreenshot,
    CopyFrame,
    MediaInfo,
    GoToTime,
    AbLoop,
    SubtitleDelayForward,
    SubtitleDelayBack,
    SubtitleSizeDown,
    SubtitleSizeUp,
    SubtitlePreviousCue,
    SubtitleNextCue,
    Fullscreen,
    EscapeFullscreen,
    OpenSettings,
}

public enum ShortcutSlot { Primary, Secondary }

public readonly record struct ShortcutModifiers(bool Ctrl = false, bool Alt = false, bool Shift = false);

public sealed record ShortcutChord
{
    internal ShortcutChord(string key, ShortcutModifiers modifiers)
    {
        Key = ShortcutModel.FoldedKeyName(key);
        Modifiers = modifiers;
    }

    internal string Key { get; }
    public ShortcutModifiers Modifiers { get; }

    public bool Matches(string keyName, ShortcutModifiers modifiers)
        => Key == ShortcutModel.FoldedKeyName(keyName) && Modifiers == modifiers;

    public string Label
    {
        get
        {
            var parts = new List<string>(4);
            if (Modifiers.Ctrl) parts.Add("Ctrl");
            if (Modifiers.Alt) parts.Add("Alt");
            if (Modifiers.Shift) parts.Add("Shift");
            parts.Add(ShortcutModel.DisplayKeyName(Key));
            return string.Join("+", parts);
        }
    }
}

public sealed record ShortcutBinding(ShortcutAction Action, ShortcutChord Chord);

public sealed record ActionChords(ShortcutAction Action, ShortcutChord Primary, ShortcutChord? Secondary);

public sealed class ShortcutConfigException : Exception
{
    public ShortcutConfigException(int line, string message) : base(message) => Line = line;
    public int Line { get; }
}

/// <summary>Platform key namespace for config tokens outside the shared portable set.</summary>
public interface IShortcutKeyNames
{
    string? CanonicalizeExtra(string token);
}

public sealed class PortableShortcutKeyNames : IShortcutKeyNames
{
    public static PortableShortcutKeyNames Instance { get; } = new();
    private PortableShortcutKeyNames() { }
    public string? CanonicalizeExtra(string token) => null;
}

public static class ShortcutModel
{
    public const int MaxShortcutsPerAction = 2;

    public static IReadOnlyList<ShortcutAction> Actions { get; } = new[]
    {
        ShortcutAction.PlayPause,
        ShortcutAction.SeekBack,
        ShortcutAction.SeekForward,
        ShortcutAction.FrameForward,
        ShortcutAction.FrameBack,
        ShortcutAction.PreviousItem,
        ShortcutAction.NextItem,
        ShortcutAction.VolumeDown,
        ShortcutAction.VolumeUp,
        ShortcutAction.Mute,
        ShortcutAction.OpenFile,
        ShortcutAction.AddSubtitle,
        ShortcutAction.OpenUrl,
        ShortcutAction.CloseMedia,
        ShortcutAction.SaveScreenshot,
        ShortcutAction.CopyFrame,
        ShortcutAction.MediaInfo,
        ShortcutAction.GoToTime,
        ShortcutAction.AbLoop,
        ShortcutAction.SubtitleDelayForward,
        ShortcutAction.SubtitleDelayBack,
        ShortcutAction.SubtitleSizeDown,
        ShortcutAction.SubtitleSizeUp,
        ShortcutAction.SubtitlePreviousCue,
        ShortcutAction.SubtitleNextCue,
        ShortcutAction.Fullscreen,
        ShortcutAction.EscapeFullscreen,
        ShortcutAction.OpenSettings,
    };

    public static string Id(this ShortcutAction action) => action switch
    {
        ShortcutAction.PlayPause => "play-pause",
        ShortcutAction.SeekBack => "seek-back",
        ShortcutAction.SeekForward => "seek-forward",
        ShortcutAction.FrameForward => "frame-forward",
        ShortcutAction.FrameBack => "frame-back",
        ShortcutAction.PreviousItem => "previous-item",
        ShortcutAction.NextItem => "next-item",
        ShortcutAction.VolumeDown => "volume-down",
        ShortcutAction.VolumeUp => "volume-up",
        ShortcutAction.Mute => "mute",
        ShortcutAction.OpenFile => "open-file",
        ShortcutAction.AddSubtitle => "add-subtitle",
        ShortcutAction.OpenUrl => "open-url",
        ShortcutAction.CloseMedia => "close-media",
        ShortcutAction.SaveScreenshot => "save-screenshot",
        ShortcutAction.CopyFrame => "copy-frame",
        ShortcutAction.MediaInfo => "media-info",
        ShortcutAction.GoToTime => "go-to-time",
        ShortcutAction.AbLoop => "ab-loop",
        ShortcutAction.SubtitleDelayForward => "subtitle-delay-forward",
        ShortcutAction.SubtitleDelayBack => "subtitle-delay-back",
        ShortcutAction.SubtitleSizeDown => "subtitle-size-down",
        ShortcutAction.SubtitleSizeUp => "subtitle-size-up",
        ShortcutAction.SubtitlePreviousCue => "subtitle-previous-cue",
        ShortcutAction.SubtitleNextCue => "subtitle-next-cue",
        ShortcutAction.Fullscreen => "fullscreen",
        ShortcutAction.EscapeFullscreen => "escape-fullscreen",
        ShortcutAction.OpenSettings => "open-settings",
        _ => throw new ArgumentOutOfRangeException(nameof(action)),
    };

    public static string Label(this ShortcutAction action) => action switch
    {
        ShortcutAction.PlayPause => "Play / Pause",
        ShortcutAction.SeekBack => "Seek Back",
        ShortcutAction.SeekForward => "Seek Forward",
        ShortcutAction.FrameForward => "Frame Forward",
        ShortcutAction.FrameBack => "Frame Back",
        ShortcutAction.PreviousItem => "Previous Item",
        ShortcutAction.NextItem => "Next Item",
        ShortcutAction.VolumeDown => "Volume Down",
        ShortcutAction.VolumeUp => "Volume Up",
        ShortcutAction.Mute => "Mute",
        ShortcutAction.OpenFile => "Open File",
        ShortcutAction.AddSubtitle => "Add Subtitle",
        ShortcutAction.OpenUrl => "Open URL",
        ShortcutAction.CloseMedia => "Close Media",
        ShortcutAction.SaveScreenshot => "Save Screenshot",
        ShortcutAction.CopyFrame => "Copy Frame",
        ShortcutAction.MediaInfo => "Media Info",
        ShortcutAction.GoToTime => "Go to Time",
        ShortcutAction.AbLoop => "A-B Loop",
        ShortcutAction.SubtitleDelayForward => "Subtitle Delay Forward",
        ShortcutAction.SubtitleDelayBack => "Subtitle Delay Back",
        ShortcutAction.SubtitleSizeDown => "Subtitle Size Down",
        ShortcutAction.SubtitleSizeUp => "Subtitle Size Up",
        ShortcutAction.SubtitlePreviousCue => "Previous Subtitle Cue",
        ShortcutAction.SubtitleNextCue => "Next Subtitle Cue",
        ShortcutAction.Fullscreen => "Fullscreen",
        ShortcutAction.EscapeFullscreen => "Exit Fullscreen",
        ShortcutAction.OpenSettings => "Settings",
        _ => throw new ArgumentOutOfRangeException(nameof(action)),
    };

    public static string DefaultShortcut(this ShortcutAction action) => action switch
    {
        ShortcutAction.PlayPause => "Space",
        ShortcutAction.SeekBack => "Left",
        ShortcutAction.SeekForward => "Right",
        ShortcutAction.FrameForward => ".",
        ShortcutAction.FrameBack => ",",
        ShortcutAction.PreviousItem => "PageUp",
        ShortcutAction.NextItem => "PageDown",
        ShortcutAction.VolumeDown => "Down",
        ShortcutAction.VolumeUp => "Up",
        ShortcutAction.Mute => "M",
        ShortcutAction.OpenFile => "O",
        ShortcutAction.AddSubtitle => "S",
        ShortcutAction.OpenUrl => "U",
        ShortcutAction.CloseMedia => "X",
        ShortcutAction.SaveScreenshot => "C",
        ShortcutAction.CopyFrame => "Shift+C",
        ShortcutAction.MediaInfo => "I",
        ShortcutAction.GoToTime => "J",
        ShortcutAction.AbLoop => "L",
        ShortcutAction.SubtitleDelayForward => "Z",
        ShortcutAction.SubtitleDelayBack => "Shift+Z",
        ShortcutAction.SubtitleSizeDown => "[",
        ShortcutAction.SubtitleSizeUp => "]",
        ShortcutAction.SubtitlePreviousCue => "Ctrl+Left",
        ShortcutAction.SubtitleNextCue => "Ctrl+Right",
        ShortcutAction.Fullscreen => "F",
        ShortcutAction.EscapeFullscreen => "Escape",
        ShortcutAction.OpenSettings => "Ctrl+,",
        _ => throw new ArgumentOutOfRangeException(nameof(action)),
    };

    public static ShortcutChord ParseChord(string text, int line = 0, IShortcutKeyNames? keyNames = null)
    {
        keyNames ??= PortableShortcutKeyNames.Instance;
        var modifiers = new ShortcutModifiers();
        string? key = null;

        foreach (string raw in text.Split('+'))
        {
            string token = raw.Trim();
            if (token.Length == 0)
                continue;
            switch (token.ToLowerInvariant())
            {
                case "ctrl":
                case "control": modifiers = modifiers with { Ctrl = true }; break;
                case "alt":
                case "option": modifiers = modifiers with { Alt = true }; break;
                case "shift": modifiers = modifiers with { Shift = true }; break;
                default:
                    if (key is not null)
                        throw Error(line, "Shortcut can only contain one non-modifier key.");
                    key = KeyNameFromToken(token, keyNames)
                        ?? throw Error(line, $"Unknown key `{token}`.");
                    break;
            }
        }

        if (key is null)
            throw Error(line, "Shortcut key is empty.");
        return new ShortcutChord(key, modifiers);
    }

    public static IReadOnlyList<ShortcutBinding> ResolvedBindingsFromText(
        string? text,
        IShortcutKeyNames? keyNames = null)
    {
        keyNames ??= PortableShortcutKeyNames.Instance;
        var bindings = DefaultBindings().ToList();
        var overrides = ParseConfigOverrides(text ?? string.Empty, keyNames);
        var overrideChords = overrides.Select(item => item.Chord).ToList();

        foreach (ShortcutAction action in Actions)
        {
            var actionOverrides = overrides.Where(item => item.Action == action).Select(item => item.Chord).ToList();
            if (actionOverrides.Count == 0)
                continue;
            bindings.RemoveAll(binding => binding.Action == action);
            bindings.AddRange(actionOverrides.Select(chord => new ShortcutBinding(action, chord)));
        }

        bindings.RemoveAll(binding =>
            !overrides.Any(item => item.Action == binding.Action)
            && IsUpgradeShadowableDefault(binding.Action)
            && overrideChords.Contains(binding.Chord));
        ValidateConflicts(bindings);
        return bindings;
    }

    public static string ConfigTextFromBindings(IEnumerable<ShortcutBinding> source)
    {
        var bindings = source.ToList();
        var lines = new List<string>();
        foreach (ShortcutAction action in Actions)
        {
            ShortcutChord defaultChord = DefaultChord(action);
            var chords = ChordsForAction(bindings, action);
            if (chords.Count == 1 && chords[0] == defaultChord)
                continue;
            lines.AddRange(chords.Take(MaxShortcutsPerAction).Select(chord => $"{action.Id()}={chord.Label}"));
        }
        return string.Join("\n", lines);
    }

    public static IReadOnlyList<ShortcutChord> ChordsForAction(
        IEnumerable<ShortcutBinding> source,
        ShortcutAction action)
    {
        var chords = source.Where(binding => binding.Action == action)
            .Select(binding => binding.Chord)
            .Take(MaxShortcutsPerAction)
            .ToList();
        return chords.Count == 0 ? new[] { DefaultChord(action) } : chords;
    }

    public static ShortcutChord DefaultChord(ShortcutAction action)
        => ParseChord(action.DefaultShortcut());

    public static IReadOnlyList<ShortcutBinding> DefaultBindings()
        => Actions.Select(action => new ShortcutBinding(action, DefaultChord(action))).ToList();

    public static ShortcutAction? ActionForKey(
        IEnumerable<ShortcutBinding> bindings,
        string keyName,
        ShortcutModifiers modifiers)
        => bindings.FirstOrDefault(binding => binding.Chord.Matches(keyName, modifiers))?.Action;

    public static void ValidateConflicts(IReadOnlyList<ShortcutBinding> bindings)
    {
        for (int left = 0; left < bindings.Count; left++)
        {
            for (int right = left + 1; right < bindings.Count; right++)
            {
                if (bindings[left].Chord != bindings[right].Chord)
                    continue;
                throw Error(0,
                    $"{bindings[right].Action.Id()} conflicts with {bindings[left].Action.Id()} on {bindings[left].Chord.Label}.");
            }
        }
    }

    public static bool TryChordFromCapturedKey(
        string? keyName,
        ShortcutModifiers modifiers,
        out ShortcutChord? chord,
        out string? error)
    {
        if (keyName is null || IsModifierKeyName(keyName))
        {
            chord = null;
            error = "Press a non-modifier key.";
            return false;
        }
        chord = new ShortcutChord(keyName, modifiers);
        error = null;
        return true;
    }

    public static ShortcutAction? SlotConflict(
        IEnumerable<ActionChords> rows,
        ShortcutAction action,
        ShortcutSlot slot,
        ShortcutChord chord)
    {
        foreach (ActionChords row in rows)
        {
            if (!(row.Action == action && slot == ShortcutSlot.Primary) && row.Primary == chord)
                return row.Action;
            if (!(row.Action == action && slot == ShortcutSlot.Secondary) && row.Secondary == chord)
                return row.Action;
        }
        return null;
    }

    public static IReadOnlyList<ShortcutBinding> BindingsFromActionChords(IEnumerable<ActionChords> rows)
    {
        var bindings = new List<ShortcutBinding>();
        foreach (ActionChords row in rows)
        {
            bindings.Add(new ShortcutBinding(row.Action, row.Primary));
            if (row.Secondary is not null)
                bindings.Add(new ShortcutBinding(row.Action, row.Secondary));
        }
        return bindings;
    }

    internal static string FoldedKeyName(string name)
        => name.Length == 1 && name[0] is >= 'A' and <= 'Z' ? name.ToLowerInvariant() : name;

    internal static string DisplayKeyName(string name) => name switch
    {
        "space" => "Space",
        "comma" => ",",
        "period" => ".",
        "bracketleft" => "[",
        "bracketright" => "]",
        "Page_Up" => "PageUp",
        "Page_Down" => "PageDown",
        _ when name.Length == 1 => name.ToUpperInvariant(),
        _ => name,
    };

    private static List<(ShortcutAction Action, ShortcutChord Chord)> ParseConfigOverrides(
        string text,
        IShortcutKeyNames keyNames)
    {
        var overrides = new List<(ShortcutAction, ShortcutChord)>();
        string[] lines = text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n').Split('\n');
        for (int index = 0; index < lines.Length; index++)
        {
            int lineNumber = index + 1;
            string trimmed = lines[index].Trim();
            if (trimmed.Length == 0 || trimmed.StartsWith('#') || trimmed.StartsWith(';'))
                continue;
            int separator = trimmed.IndexOf('=');
            if (separator < 0)
                throw Error(lineNumber, "Use action=shortcut syntax, one binding per line.");
            string actionId = trimmed[..separator].Trim();
            string shortcut = trimmed[(separator + 1)..].Trim();
            ShortcutAction? action = null;
            foreach (ShortcutAction candidate in Actions)
            {
                if (candidate.Id() == actionId)
                {
                    action = candidate;
                    break;
                }
            }
            if (action is null)
                throw Error(lineNumber, $"Unknown action `{actionId}`.");
            if (overrides.Count(item => item.Item1 == action.Value) >= MaxShortcutsPerAction)
                throw Error(lineNumber, $"Action `{actionId}` supports at most two shortcuts.");
            overrides.Add((action.Value, ParseChord(shortcut, lineNumber, keyNames)));
        }
        return overrides;
    }

    private static string? KeyNameFromToken(string token, IShortcutKeyNames keyNames)
    {
        string lower = token.ToLowerInvariant();
        return lower switch
        {
            "," or "comma" => "comma",
            "." or "period" => "period",
            "[" or "bracketleft" => "bracketleft",
            "]" or "bracketright" => "bracketright",
            "esc" or "escape" => "Escape",
            "pageup" or "page_up" => "Page_Up",
            "pagedown" or "page_down" => "Page_Down",
            "space" => "space",
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            _ when lower.Length == 1 && char.IsAsciiLetterOrDigit(lower[0]) => lower,
            _ => keyNames.CanonicalizeExtra(token),
        };
    }

    private static bool IsUpgradeShadowableDefault(ShortcutAction action)
        => action is ShortcutAction.SubtitlePreviousCue or ShortcutAction.SubtitleNextCue;

    private static bool IsModifierKeyName(string name) => name is
        "Shift_L" or "Shift_R" or "Control_L" or "Control_R" or "Alt_L" or "Alt_R"
        or "Meta_L" or "Meta_R" or "Super_L" or "Super_R" or "Hyper_L" or "Hyper_R"
        or "ISO_Level3_Shift" or "Caps_Lock";

    private static ShortcutConfigException Error(int line, string message) => new(line, message);
}
