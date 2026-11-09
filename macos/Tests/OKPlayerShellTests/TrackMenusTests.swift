import AppKit
import XCTest
@testable import OKPlayerShell

/// The track menus and keys as values: what the popovers and the menu bar list for
/// the session's `tracks` view, what choosing an item sends, and which key presses
/// are the track keys of the session's shortcut table.
final class TrackMenusTests: XCTestCase {
    private var view: TracksView!

    override func setUpWithError() throws {
        try super.setUpWithError()
        view = try TrackProtocolTests.fixtureView()
    }

    func testTheAudioMenuListsEveryTrackWithTheSessionsLabelAndChecksThePlayingOne() {
        XCTAssertEqual(TrackMenus.audioItems(view), [
            TrackMenuItem(kind: .choice, title: "English · 5.1 · EAC3", checked: true, enabled: true,
                          action: .select(kind: .audio, id: 1)),
            TrackMenuItem(kind: .choice, title: "Director's Commentary · ENG · 2.0 · AC3", checked: false, enabled: true,
                          action: .select(kind: .audio, id: 2)),
        ])
    }

    func testTheSubtitlesMenuStartsWithOffAndEndsWithAddSubtitleFile() {
        let items = TrackMenus.subtitleItems(view)
        XCTAssertEqual(items.map(\.title), ["Off", "English SDH · PGS · Image · Default", "en · SRT · EXT", "",
                                            "Add Subtitle File…"])
        XCTAssertEqual(items.map(\.checked), [false, false, true, false, false])
        XCTAssertEqual(items[0].action, .select(kind: .subtitle, id: nil), "Off turns subtitles off")
        XCTAssertEqual(items[2].action, .select(kind: .subtitle, id: 2))
        XCTAssertEqual(items[3].kind, .separator)
        XCTAssertEqual(items[4], TrackMenuItem(kind: .command, title: "Add Subtitle File…", enabled: true,
                                               action: .addSubtitle))
    }

    func testWithoutLoadedMediaNothingCanBeChosen() {
        view.adjustable = false
        view.audio = []
        view.subtitles = [TrackChoice(id: nil, label: "Off", selected: true)]
        XCTAssertEqual(TrackMenus.audioItems(view), [TrackMenuItem(kind: .placeholder, title: "No audio tracks")])
        let subtitles = TrackMenus.subtitleItems(view)
        XCTAssertTrue(subtitles.allSatisfy { !$0.enabled }, "\(subtitles)")
        XCTAssertTrue(TrackMenus.adjustments(view).allSatisfy { !$0.enabled })
        XCTAssertEqual(TrackMenus.subtitleItems(nil).first?.checked, true, "before any answer subtitles read Off")
    }

    func testTheAdjustmentsShowTheSessionsReadoutsAndStepLikeTheKeys() {
        let rows = TrackMenus.adjustments(view)
        XCTAssertEqual(rows.map(\.title), ["Delay", "Size"])
        XCTAssertEqual(rows.map(\.value), ["+250 ms", "110%"])
        XCTAssertEqual(rows[0].decrease, .invoke(id: "subtitle-delay-back"))
        XCTAssertEqual(rows[0].increase, .invoke(id: "subtitle-delay-forward"))
        XCTAssertEqual(rows[1].decrease, .invoke(id: "subtitle-size-down"))
        XCTAssertEqual(rows[1].increase, .invoke(id: "subtitle-size-up"))
    }

    func testEachActionIsTheSessionCommandItStandsFor() {
        XCTAssertEqual(TrackMenus.command(for: .select(kind: .audio, id: 2)), .selectTrack(kind: .audio, id: 2))
        XCTAssertEqual(TrackMenus.command(for: .select(kind: .subtitle, id: nil)), .selectTrack(kind: .subtitle, id: nil))
        XCTAssertEqual(TrackMenus.command(for: .invoke(id: "subtitle-size-up")), .invokeCommand(id: "subtitle-size-up"))
        XCTAssertNil(TrackMenus.command(for: .addSubtitle), "the file panel is the shell's")
    }

    func testTheReadoutNamesTheValueTheStepChanged() {
        XCTAssertEqual(TrackMenus.readout(after: "subtitle-delay-forward", in: view), "Subtitle delay: +250 ms")
        XCTAssertEqual(TrackMenus.readout(after: "subtitle-delay-back", in: view), "Subtitle delay: +250 ms")
        XCTAssertEqual(TrackMenus.readout(after: "subtitle-size-down", in: view), "Subtitle size: 110%")
        XCTAssertNil(TrackMenus.readout(after: "volume-up", in: view))
    }

    /// The command the shortcut table runs for a key event with these characters.
    private func command(_ characters: String, keyCode: UInt16, _ modifiers: NSEvent.ModifierFlags = []) throws
        -> String? {
        let event = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers,
                                                   timestamp: 0, windowNumber: 0, context: nil,
                                                   characters: characters, charactersIgnoringModifiers: characters,
                                                   isARepeat: false, keyCode: keyCode))
        guard let press = KeyPress(event: event), case .run(let id)? = try MenuFixtures.keys().match(press) else {
            return nil
        }
        return id
    }

    func testTheTrackKeysRunThroughTheShortcutTable() throws {
        XCTAssertEqual(try command("z", keyCode: 6), "subtitle-delay-forward")
        XCTAssertEqual(try command("Z", keyCode: 6, .shift), "subtitle-delay-back",
                       "Shift keeps the capital in charactersIgnoringModifiers")
        XCTAssertEqual(try command("[", keyCode: 33), "subtitle-size-down")
        XCTAssertEqual(try command("]", keyCode: 30), "subtitle-size-up")
        XCTAssertEqual(try command("s", keyCode: 1), "add-subtitle")
        XCTAssertNil(try command("z", keyCode: 6, .command), "Cmd+Z is the menu's Undo, not a subtitle key")
        XCTAssertNil(try command("{", keyCode: 33, .shift), "Shift+[ is not [")
    }

    func testARussianOrHebrewLayoutReachesTheTrackKeyInTheSamePlace() throws {
        XCTAssertEqual(try command("ы", keyCode: 1), "add-subtitle")
        XCTAssertEqual(try command("ד", keyCode: 1), "add-subtitle")
        XCTAssertEqual(try command("я", keyCode: 6), "subtitle-delay-forward")
        XCTAssertEqual(try command("Я", keyCode: 6, .shift), "subtitle-delay-back")
        XCTAssertEqual(try command("х", keyCode: 33), "subtitle-size-down")
        XCTAssertEqual(try command("ъ", keyCode: 30), "subtitle-size-up")
    }
}
