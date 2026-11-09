import AppKit
import XCTest
@testable import OKPlayerShell

/// The player window's single keys (#880): key events named like the core's
/// shortcut table, the fixture table's keys mapped to their registry commands,
/// and a held toggle's auto-repeat taken without running it again.
final class ShortcutKeyTests: XCTestCase {
    /// The fixtures this file checks, for `SessionProtocolTests`' coverage test.
    static let fixtures: Set<String> = ["query-repeating-shortcuts.json", "query-reply-repeating-shortcuts.json"]

    private func press(_ key: String, _ modifiers: ShortcutModifiers = ShortcutModifiers(),
                       repeat isRepeat: Bool = false) -> KeyPress {
        KeyPress(key: key, modifiers: modifiers, isRepeat: isRepeat)
    }

    private func event(_ characters: String, keyCode: UInt16, modifiers: NSEvent.ModifierFlags = [],
                       repeat isRepeat: Bool = false) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0,
                                       windowNumber: 0, context: nil, characters: characters,
                                       charactersIgnoringModifiers: characters, isARepeat: isRepeat, keyCode: keyCode))
    }

    // MARK: The documents

    func testTheRepeatingShortcutsDocumentsRoundTrip() throws {
        let query = QueryEnvelope(query: .repeatingShortcuts)
        XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, "query-repeating-shortcuts.json"), query)
        XCTAssertEqual(try GoldenFixtures.encodedObject(query), try GoldenFixtures.object("query-repeating-shortcuts.json"))

        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-repeating-shortcuts.json")
        XCTAssertEqual(reply.schemaVersion, SessionJSON.schemaVersion)
        let actions = try MenuFixtures.repeating()
        XCTAssertEqual(actions.count, 12)
        XCTAssertTrue(actions.contains("seek-forward"))
        XCTAssertTrue(actions.contains("volume-up"))
        XCTAssertFalse(actions.contains("play-pause"))
        XCTAssertFalse(actions.contains("mute"))
        let table = try MenuFixtures.shortcutTable()
        XCTAssertTrue(actions.allSatisfy { action in table.contains { $0.action == action } },
                      "every repeating action is a row of the table")
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply),
                       try GoldenFixtures.object("query-reply-repeating-shortcuts.json"))
    }

    // MARK: Key names

    func testKeyEventsAreNamedLikeTheShortcutTable() throws {
        XCTAssertEqual(KeyNames.canonical(characters: " ", keyCode: 49), "space")
        XCTAssertEqual(KeyNames.canonical(characters: "\u{F702}", keyCode: 123), "Left")
        XCTAssertEqual(KeyNames.canonical(characters: "\u{F700}", keyCode: 126), "Up")
        XCTAssertEqual(KeyNames.canonical(characters: "\u{F72D}", keyCode: 121), "Page_Down")
        XCTAssertEqual(KeyNames.canonical(characters: "\u{1b}", keyCode: 53), "Escape")
        XCTAssertEqual(KeyNames.canonical(characters: "M", keyCode: 46), "m", "Shift folds to the letter")
        XCTAssertEqual(KeyNames.canonical(characters: ",", keyCode: 43), "comma")
        XCTAssertEqual(KeyNames.canonical(characters: "[", keyCode: 33), "bracketleft")
        XCTAssertEqual(KeyNames.canonical(characters: "7", keyCode: 26), "7")
        // A Russian or Hebrew layout: the key in M's place is still M.
        XCTAssertEqual(KeyNames.canonical(characters: "ь", keyCode: 46), "m")
        XCTAssertEqual(KeyNames.canonical(characters: "צ", keyCode: 46), "m")
        XCTAssertEqual(KeyNames.canonical(characters: "б", keyCode: 43), "comma")
        XCTAssertNil(KeyNames.canonical(characters: "", keyCode: 63), "Fn alone names no key")
    }

    func testAKeyEventCarriesItsChordModifiersAndRepeatFlag() throws {
        let shiftZ = try XCTUnwrap(KeyPress(event: try event("Z", keyCode: 6, modifiers: [.shift])))
        XCTAssertEqual(shiftZ, press("z", ShortcutModifiers(shift: true)))
        let held = try XCTUnwrap(KeyPress(event: try event(" ", keyCode: 49, repeat: true)))
        XCTAssertEqual(held, press("space", repeat: true))
        let arrow = try XCTUnwrap(KeyPress(event: try event("\u{F703}", keyCode: 124,
                                                            modifiers: [.numericPad, .function, .capsLock])))
        XCTAssertEqual(arrow, press("Right"), "keypad, Fn and Caps Lock are not chord modifiers")
        let command = try XCTUnwrap(KeyPress(event: try event("o", keyCode: 31, modifiers: [.command, .option])))
        XCTAssertEqual(command.modifiers, ShortcutModifiers(alt: true, cmd: true))
    }

    func testMenuKeyEquivalentsOfTableKeys() {
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "o"), "o")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "comma"), ",")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "bracketright"), "]")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "space"), " ")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "Left"), "\u{F702}")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "Page_Down"), "\u{F72D}")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "F1"), "\u{F704}")
        XCTAssertEqual(KeyNames.keyEquivalent(forKey: "Escape"), "\u{1b}")
        XCTAssertNil(KeyNames.keyEquivalent(forKey: "Multi_key"))
    }

    // MARK: The fixture table

    func testKeysMapToTheirRegistryCommandsThroughTheFixtureTable() throws {
        let keys = try MenuFixtures.keys()
        let expected: [(KeyPress, String)] = [
            (press("space"), "play-pause"),
            (press("Left"), "seek-back"),
            (press("Right"), "seek-forward"),
            (press("Up"), "volume-up"),
            (press("Down"), "volume-down"),
            (press("m"), "mute"),
            (press("f"), "fullscreen"),
            (press("Escape"), "escape-fullscreen"),
            (press("Page_Up"), "previous-item"),
            (press("Page_Down"), "next-item"),
            (press("x"), "close-media"),
            (press("comma"), "frame-back"),
            (press("period"), "frame-forward"),
            (press("z"), "subtitle-delay-forward"),
            (press("z", ShortcutModifiers(shift: true)), "subtitle-delay-back"),
            // The action is save-screenshot, the registry command it runs save-frame.
            (press("c"), "save-frame"),
            (press("c", ShortcutModifiers(shift: true)), "copy-frame"),
        ]
        for (key, command) in expected {
            XCTAssertEqual(keys.match(key), .run(command), "\(key)")
        }
    }

    func testCommandChordsAndUnboundKeysAreNotWindowKeys() throws {
        let keys = try MenuFixtures.keys()
        XCTAssertNil(keys.match(press("o", ShortcutModifiers(cmd: true))), "Cmd-O is File > Open's key equivalent")
        XCTAssertNil(keys.match(press("Left", ShortcutModifiers(cmd: true))), "a menu key equivalent too")
        XCTAssertNil(keys.match(press("comma", ShortcutModifiers(cmd: true))))
        XCTAssertNil(keys.match(press("q")))
        XCTAssertNil(keys.match(press("m", ShortcutModifiers(ctrl: true))), "the modifiers must match exactly")
        XCTAssertNil(keys.match(press("m", ShortcutModifiers(shift: true))))
        XCTAssertNil(ShortcutKeyMap().match(press("space")), "no table yet, no keys")
    }

    // MARK: Held keys

    func testAHeldToggleRunsOncePerPressAndAHeldStepRepeats() throws {
        let keys = try MenuFixtures.keys()
        for (key, command) in [("space", "play-pause"), ("m", "mute"), ("f", "fullscreen"), ("x", "close-media"),
                               ("Page_Down", "next-item")] {
            XCTAssertEqual(keys.match(press(key)), .run(command), key)
            XCTAssertEqual(keys.match(press(key, repeat: true)), .heldRepeat(command), key)
        }
        for (key, command) in [("Left", "seek-back"), ("Right", "seek-forward"), ("Up", "volume-up"),
                               ("Down", "volume-down")] {
            XCTAssertEqual(keys.match(press(key, repeat: true)), .run(command), key)
        }
        let everyKeyOnce = ShortcutKeyMap(table: try MenuFixtures.shortcutTable(), repeating: [])
        XCTAssertEqual(everyKeyOnce.match(press("Right", repeat: true)), .heldRepeat("seek-forward"),
                       "the session decides which keys repeat")
    }
}
