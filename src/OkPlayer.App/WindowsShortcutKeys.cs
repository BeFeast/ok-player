using System;
using System.Collections.Generic;
using Microsoft.UI.Input;
using OkPlayer.Core;
using Windows.System;
using Windows.UI.Core;

namespace OkPlayer.App;

/// <summary>WinUI key-event adapter for the canonical key names used by <see cref="ShortcutModel"/>.</summary>
internal sealed class WindowsShortcutKeys : IShortcutKeyNames
{
    public static WindowsShortcutKeys Instance { get; } = new();

    private static readonly HashSet<string> ExtraNames = new(StringComparer.Ordinal)
    {
        "BackSpace", "Tab", "Return", "Pause", "Caps_Lock", "Home", "End", "Insert", "Delete",
        "Super_L", "Super_R", "Num_Lock", "Scroll_Lock", "Print",
        "semicolon", "equal", "minus", "slash", "grave", "backslash", "apostrophe",
        "KP_Multiply", "KP_Add", "KP_Separator", "KP_Subtract", "KP_Decimal", "KP_Divide",
        "KP_0", "KP_1", "KP_2", "KP_3", "KP_4", "KP_5", "KP_6", "KP_7", "KP_8", "KP_9",
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    };

    private WindowsShortcutKeys() { }

    public string? CanonicalizeExtra(string token) => ExtraNames.Contains(token) ? token : null;

    public static ShortcutModifiers CurrentModifiers() => new(
        Ctrl: IsDown((VirtualKey)0x11),
        Alt: IsDown((VirtualKey)0x12),
        Shift: IsDown((VirtualKey)0x10));

    public static bool IsSystemMediaKey(VirtualKey key)
    {
        int code = (int)key;
        // Volume mute/down/up, media next/previous/stop/play-pause, and launch-media-select are owned by SMTC.
        return code is >= 0xAD and <= 0xB5;
    }

    public static string? CanonicalName(VirtualKey key)
    {
        int code = (int)key;
        if (IsSystemMediaKey(key))
            return null;
        if (code is >= 0x30 and <= 0x39)
            return ((char)code).ToString();
        if (code is >= 0x41 and <= 0x5A)
            return char.ToLowerInvariant((char)code).ToString();
        if (code is >= 0x60 and <= 0x69)
            return $"KP_{code - 0x60}";
        if (code is >= 0x70 and <= 0x87)
            return $"F{code - 0x6F}";

        return code switch
        {
            0x08 => "BackSpace",
            0x09 => "Tab",
            0x0D => "Return",
            0x10 => "Shift_L",
            0x11 => "Control_L",
            0x12 => "Alt_L",
            0x13 => "Pause",
            0x14 => "Caps_Lock",
            0x1B => "Escape",
            0x20 => "space",
            0x21 => "Page_Up",
            0x22 => "Page_Down",
            0x23 => "End",
            0x24 => "Home",
            0x25 => "Left",
            0x26 => "Up",
            0x27 => "Right",
            0x28 => "Down",
            0x2C => "Print",
            0x2D => "Insert",
            0x2E => "Delete",
            0x5B => "Super_L",
            0x5C => "Super_R",
            0x6A => "KP_Multiply",
            0x6B => "KP_Add",
            0x6C => "KP_Separator",
            0x6D => "KP_Subtract",
            0x6E => "KP_Decimal",
            0x6F => "KP_Divide",
            0x90 => "Num_Lock",
            0x91 => "Scroll_Lock",
            0xBA => "semicolon",
            0xBB => "equal",
            0xBC => "comma",
            0xBD => "minus",
            0xBE => "period",
            0xBF => "slash",
            0xC0 => "grave",
            0xDB => "bracketleft",
            0xDC => "backslash",
            0xDD => "bracketright",
            0xDE => "apostrophe",
            _ => null,
        };
    }

    private static bool IsDown(VirtualKey key)
        => InputKeyboardSource.GetKeyStateForCurrentThread(key).HasFlag(CoreVirtualKeyStates.Down);
}
