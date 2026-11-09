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
        XCTAssertEqual(TrackMenus.readout(after: "subtitle-delay-forward", in: view), "Subtitle delay +250 ms")
        XCTAssertEqual(TrackMenus.readout(after: "subtitle-delay-back", in: view), "Subtitle delay +250 ms")
        XCTAssertEqual(TrackMenus.readout(after: "subtitle-size-down", in: view), "Subtitle size 110%")
        XCTAssertNil(TrackMenus.readout(after: "volume-up", in: view))
    }

    func testTheTrackKeysAreTheShortcutTablesChords() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-shortcut-table.json")
        guard case .shortcutTable(let table) = reply.result else { return XCTFail("not a shortcut table") }
        let keys = TrackKeys(table: table)
        XCTAssertEqual(keys.command(characters: "z", modifiers: []), "subtitle-delay-forward")
        XCTAssertEqual(keys.command(characters: "Z", modifiers: .shift), "subtitle-delay-back",
                       "Shift keeps the capital in charactersIgnoringModifiers")
        XCTAssertEqual(keys.command(characters: "[", modifiers: []), "subtitle-size-down")
        XCTAssertEqual(keys.command(characters: "]", modifiers: []), "subtitle-size-up")
        XCTAssertEqual(keys.command(characters: "s", modifiers: []), "add-subtitle")
        XCTAssertNil(keys.command(characters: "z", modifiers: .command), "Cmd+Z is not a subtitle key")
        XCTAssertNil(keys.command(characters: "{", modifiers: .shift), "Shift+[ is not [")
        XCTAssertNil(keys.command(characters: "x", modifiers: []), "close-media is not handled here")
        XCTAssertNil(TrackKeys(table: []).command(characters: "z", modifiers: []), "no table, no keys")
    }
}
