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
    func command(characters: String?, modifiers: NSEvent.ModifierFlags) -> String? {
        guard let characters, let key = Self.keyName(characters) else { return nil }
        let pressed = ShortcutModifiers(ctrl: modifiers.contains(.control), alt: modifiers.contains(.option),
                                        shift: modifiers.contains(.shift), cmd: modifiers.contains(.command))
        return bindings.first { $0.chord.key.lowercased() == key && $0.chord.modifiers == pressed }?.command
    }

    /// The shortcut table's key name for the characters a key gave: a letter or
    /// digit in lower case, the brackets by name.
    static func keyName(_ characters: String) -> String? {
        switch characters {
        case "[", "{": return "bracketleft"
        case "]", "}": return "bracketright"
        default:
            guard characters.count == 1, let character = characters.first,
                  character.isLetter || character.isNumber else { return nil }
            return characters.lowercased()
        }
    }
}
