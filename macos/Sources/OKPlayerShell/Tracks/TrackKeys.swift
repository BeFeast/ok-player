import AppKit

/// The keys of the track surfaces, read from the session's shortcut table: Add
/// Subtitle (S), the subtitle delay (Z, Shift+Z) and size ([, ]) on this platform.
/// Only these commands; the menu bar's general key handling (#880) replaces this
/// once it covers them.
struct TrackKeys {
    /// The registry commands handled here.
    static let commands: Set<String> = [TrackMenus.addSubtitle, TrackMenus.delayForward, TrackMenus.delayBack,
                                        TrackMenus.sizeUp, TrackMenus.sizeDown]

    private let bindings: [(chord: ShortcutChord, command: String)]

    init(table: [ShortcutRow]) {
        bindings = table.filter { Self.commands.contains($0.command) }.flatMap { row in
            [row.primary, row.secondary].compactMap { $0 }.map { (chord: $0, command: row.command) }
        }
    }

    /// The registry command for a key press, or `nil`. `characters` is the event's
    /// `charactersIgnoringModifiers`, which keeps Shift (Shift+Z gives `Z`).
    func command(characters: String?, keyCode: UInt16, modifiers: NSEvent.ModifierFlags) -> String? {
        guard let key = Self.keyName(characters, keyCode: keyCode) else { return nil }
        let pressed = ShortcutModifiers(ctrl: modifiers.contains(.control), alt: modifiers.contains(.option),
                                        shift: modifiers.contains(.shift), cmd: modifiers.contains(.command))
        return bindings.first { $0.chord.key.lowercased() == key && $0.chord.modifiers == pressed }?.command
    }

    /// The shortcut table's key name for a key press: an ASCII letter or digit in
    /// lower case, the brackets by name. A layout whose characters are not ASCII
    /// (Cyrillic, Hebrew) gives the US key in the same place.
    static func keyName(_ characters: String?, keyCode: UInt16) -> String? {
        if let characters, characters.count == 1, let character = characters.first, character.isASCII {
            switch character {
            case "[", "{": return "bracketleft"
            case "]", "}": return "bracketright"
            default: return character.isLetter || character.isNumber ? characters.lowercased() : nil
            }
        }
        return usLayout[keyCode]
    }

    /// The US layout's letter and bracket keys by virtual key code.
    static let usLayout: [UInt16: String] = [
        0: "a", 11: "b", 8: "c", 2: "d", 14: "e", 3: "f", 5: "g", 4: "h", 34: "i", 38: "j", 40: "k", 37: "l",
        46: "m", 45: "n", 31: "o", 35: "p", 12: "q", 15: "r", 1: "s", 17: "t", 32: "u", 9: "v", 13: "w", 7: "x",
        16: "y", 6: "z", 33: "bracketleft", 30: "bracketright",
    ]
}
