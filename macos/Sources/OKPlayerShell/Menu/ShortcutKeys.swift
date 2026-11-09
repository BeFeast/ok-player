import AppKit

// The player window's single-key shortcuts (#880). A key press is named the way
// the core's shortcut table names keys, and the table (the session's
// `shortcut_table` query) says which registry command it runs. Only chords
// without Command are window keys: Command chords are menu key equivalents
// (`MenuBarLayout`). Nothing here knows a player command by name.

/// A key press as the shortcut table compares it.
struct KeyPress: Equatable {
    /// The canonical key name (`space`, `Left`, `m`, `comma`).
    var key: String
    var modifiers: ShortcutModifiers
    /// AppKit flags the press as an auto-repeat of a held key.
    var isRepeat: Bool

    init(key: String, modifiers: ShortcutModifiers = ShortcutModifiers(), isRepeat: Bool = false) {
        self.key = key
        self.modifiers = modifiers
        self.isRepeat = isRepeat
    }

    /// The press a key-down event is, or nil for a key the table cannot name.
    init?(event: NSEvent) {
        guard event.type == .keyDown,
              let key = KeyNames.canonical(characters: event.charactersIgnoringModifiers, keyCode: event.keyCode)
        else { return nil }
        self.init(key: key, modifiers: ShortcutModifiers(flags: event.modifierFlags), isRepeat: event.isARepeat)
    }
}

extension ShortcutModifiers {
    /// The modifiers of an event that take part in chords; Caps Lock, Fn and the
    /// keypad flag do not.
    init(flags: NSEvent.ModifierFlags) {
        self.init(ctrl: flags.contains(.control), alt: flags.contains(.option), shift: flags.contains(.shift),
                  cmd: flags.contains(.command))
    }

    /// The same modifiers as AppKit flags, for a menu item's key equivalent.
    var flags: NSEvent.ModifierFlags {
        var flags: NSEvent.ModifierFlags = []
        if ctrl { flags.insert(.control) }
        if alt { flags.insert(.option) }
        if shift { flags.insert(.shift) }
        if cmd { flags.insert(.command) }
        return flags
    }
}

/// Key names: from an AppKit key event to the core's canonical names (GDK keysym
/// names, `rust/crates/okp-core/src/shortcuts.rs`), and from those names to a
/// menu item's key equivalent.
enum KeyNames {
    /// Keys named by their virtual key code: their characters are control or
    /// private-use codes.
    static let byKeyCode: [UInt16: String] = [
        49: "space", 36: "Return", 76: "KP_Enter", 48: "Tab", 51: "BackSpace", 117: "Delete", 53: "Escape",
        123: "Left", 124: "Right", 125: "Down", 126: "Up", 116: "Page_Up", 121: "Page_Down", 115: "Home", 119: "End",
        122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6", 98: "F7", 100: "F8", 101: "F9", 109: "F10",
        103: "F11", 111: "F12",
    ]

    /// The keysym names of the ASCII punctuation characters.
    static let punctuation: [Character: String] = [
        ",": "comma", ".": "period", "[": "bracketleft", "]": "bracketright", "-": "minus", "=": "equal",
        "/": "slash", "\\": "backslash", ";": "semicolon", "'": "apostrophe", "`": "grave", "<": "less",
        ">": "greater", "{": "braceleft", "}": "braceright", "_": "underscore", "+": "plus", "?": "question",
        "|": "bar", ":": "colon", "\"": "quotedbl", "~": "asciitilde", "!": "exclam", "@": "at", "#": "numbersign",
        "$": "dollar", "%": "percent", "^": "asciicircum", "&": "ampersand", "*": "asterisk", "(": "parenleft",
        ")": "parenright",
    ]

    /// The US layout's character for each letter, digit and punctuation key. A
    /// layout whose characters are not ASCII (Cyrillic, Hebrew) still reaches the
    /// shortcut of the key in the same place.
    static let usLayout: [UInt16: Character] = [
        0: "a", 11: "b", 8: "c", 2: "d", 14: "e", 3: "f", 5: "g", 4: "h", 34: "i", 38: "j", 40: "k", 37: "l",
        46: "m", 45: "n", 31: "o", 35: "p", 12: "q", 15: "r", 1: "s", 17: "t", 32: "u", 9: "v", 13: "w", 7: "x",
        16: "y", 6: "z", 29: "0", 18: "1", 19: "2", 20: "3", 21: "4", 23: "5", 22: "6", 26: "7", 28: "8", 25: "9",
        43: ",", 47: ".", 33: "[", 30: "]", 27: "-", 24: "=", 44: "/", 42: "\\", 41: ";", 39: "'", 50: "`",
    ]

    /// Menu key equivalents of the named keys whose characters are not their names.
    static let menuKeys: [String: String] = {
        let function = KeyNames.functionKey
        var keys: [String: String] = [
            "space": " ", "Return": "\r", "KP_Enter": "\u{3}", "Tab": "\t", "BackSpace": "\u{8}", "Escape": "\u{1b}",
            "Delete": function(0xF728), "Up": function(0xF700), "Down": function(0xF701), "Left": function(0xF702),
            "Right": function(0xF703), "Home": function(0xF729), "End": function(0xF72B), "Page_Up": function(0xF72C),
            "Page_Down": function(0xF72D),
        ]
        for number in 1...12 { keys["F\(number)"] = function(0xF703 + UInt32(number)) }
        return keys
    }()

    /// The canonical name of the key an event pressed, or nil for one the table
    /// cannot name. `characters` is the event's `charactersIgnoringModifiers`.
    static func canonical(characters: String?, keyCode: UInt16) -> String? {
        if let name = byKeyCode[keyCode] { return name }
        if let characters, characters.count == 1, let character = characters.first, let name = name(of: character) {
            return name
        }
        return usLayout[keyCode].flatMap(name(of:))
    }

    /// The key equivalent a menu item shows for a canonical key name, or nil for
    /// a key a menu cannot show.
    static func keyEquivalent(forKey key: String) -> String? {
        if let special = menuKeys[key] { return special }
        if key.count == 1, let character = key.first, character.isASCII, character.isLetter || character.isNumber {
            return key.lowercased()
        }
        return punctuation.first { $0.value == key }.map { String($0.key) }
    }

    /// A lowercase ASCII letter or digit, or the keysym name of an ASCII punctuation character.
    private static func name(of character: Character) -> String? {
        guard character.isASCII else { return nil }
        if character.isLetter || character.isNumber { return String(character).lowercased() }
        return punctuation[character]
    }

    private static func functionKey(_ code: UInt32) -> String {
        Unicode.Scalar(code).map { String(Character($0)) } ?? ""
    }
}

/// What a key press in the player window does.
enum ShortcutKeyMatch: Equatable {
    /// Run this registry command.
    case run(String)
    /// An auto-repeat of a held key whose command runs once per physical press:
    /// the press is taken and nothing runs.
    case heldRepeat(String)
}

/// The shortcut table as the player window reads it: the session's
/// `shortcut_table` rows and its `repeating_shortcuts` actions.
struct ShortcutKeyMap: Equatable {
    private(set) var rows: [ShortcutRow]
    /// The actions a held key repeats; every other one runs once per press.
    private(set) var repeating: Set<String>

    init(table: [ShortcutRow] = [], repeating: [String] = []) {
        rows = table
        self.repeating = Set(repeating)
    }

    /// What `press` does, or nil when it is not a window key: no row binds it,
    /// or it holds Command, which only menu key equivalents use.
    func match(_ press: KeyPress) -> ShortcutKeyMatch? {
        guard !press.modifiers.cmd else { return nil }
        let bound = rows.first { row in
            [row.primary, row.secondary].contains { slot in slot.map { Self.binds($0, press) } ?? false }
        }
        guard let row = bound else { return nil }
        if press.isRepeat && !repeating.contains(row.action) { return .heldRepeat(row.command) }
        return .run(row.command)
    }

    private static func binds(_ chord: ShortcutChord, _ press: KeyPress) -> Bool {
        // The table folds letters to lowercase already; fold again for a hand-written table.
        let key = chord.key.count == 1 ? chord.key.lowercased() : chord.key
        return key == press.key && chord.modifiers == press.modifiers
    }
}
