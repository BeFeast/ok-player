import AppKit
import XCTest
@testable import OKPlayerShell

/// The track surfaces over real AppKit objects in a window that is never shown,
/// with the session stood in by closures: what a key press, a menu item and the
/// session's answers make them send and show.
final class TrackControlsTests: XCTestCase {
    private var controls: TrackControls!
    private var dispatched: [Command] = []
    private var answer: DispatchResult = .accepted(requestId: 1)
    private var view: TracksView!

    override func setUpWithError() throws {
        try super.setUpWithError()
        _ = NSApplication.shared
        view = try TrackProtocolTests.fixtureView()
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-shortcut-table.json")
        guard case .shortcutTable(let table) = reply.result else { return XCTFail("not a shortcut table") }
        controls = TrackControls()
        controls.setShortcuts(table)
        dispatched = []
        answer = .accepted(requestId: 1)
        controls.dispatch = { [unowned self] command in
            self.dispatched.append(command)
            return self.answer
        }
        controls.query = { [unowned self] in self.view }
    }

    private func key(_ characters: String, _ modifiers: NSEvent.ModifierFlags = [], repeating: Bool = false) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: 0,
                         context: nil, characters: characters, charactersIgnoringModifiers: characters,
                         isARepeat: repeating, keyCode: 0)!
    }

    func testTheSizeKeyRunsTheRegistryCommandAndShowsTheSessionsReadout() {
        XCTAssertTrue(controls.handleKey(key("]")))
        XCTAssertEqual(dispatched, [.invokeCommand(id: "subtitle-size-up")])
        XCTAssertEqual(controls.readout.text, "Subtitle size 110%")
    }

    func testShiftZStepsTheDelayBackAndReadsTheDelay() {
        XCTAssertTrue(controls.handleKey(key("Z", .shift)))
        XCTAssertEqual(dispatched, [.invokeCommand(id: "subtitle-delay-back")])
        XCTAssertEqual(controls.readout.text, "Subtitle delay +250 ms")
    }

    func testOtherKeysAreLeftToTheWindow() {
        XCTAssertFalse(controls.handleKey(key("x")))
        XCTAssertFalse(controls.handleKey(key("z", .command)))
        XCTAssertTrue(dispatched.isEmpty)
        XCTAssertEqual(controls.readout.text, "")
    }

    func testARefusedStepShowsNoReadout() {
        answer = .rejected(reason: .noActiveMedia, message: "the command needs loaded media")
        XCTAssertTrue(controls.handleKey(key("[")), "the key is still the size key")
        XCTAssertEqual(dispatched, [.invokeCommand(id: "subtitle-size-down")])
        XCTAssertEqual(controls.readout.text, "", "nothing changed, so nothing is announced")
    }

    func testAHeldSAsksForTheFilePanelOnceAndTheSessionDecidesWhetherItOpens() {
        answer = .perform
        XCTAssertTrue(controls.handleKey(key("s")))
        XCTAssertTrue(controls.handleKey(key("s", repeating: true)), "a repeat is swallowed")
        XCTAssertEqual(dispatched, [.invokeCommand(id: "add-subtitle")])
        XCTAssertTrue(controls.carryOut("add-subtitle"), "the shell carries out the file panel")
        XCTAssertFalse(controls.carryOut("open-file"), "not a track surface's command")
    }

    func testAnAddedFileGoesToTheSessionAsItsPath() {
        controls.addSubtitle(URL(fileURLWithPath: "/Users/example/Movies/film.en.srt"))
        XCTAssertEqual(dispatched, [.addSubtitle(path: "/Users/example/Movies/film.en.srt")])
    }

    func testTheMenuBarSubmenuListsTheTracksAndSwitchesThem() {
        let menu = NSMenu()
        controls.populate(menu, kind: .subtitle)
        XCTAssertEqual(menu.items.map(\.title), ["Off", "English SDH · PGS · Image · Default", "en · SRT · EXT", "",
                                                 "Add Subtitle File…"])
        XCTAssertEqual(menu.items.map(\.state), [.off, .off, .on, .off, .off])
        XCTAssertTrue(menu.items[3].isSeparatorItem)
        menu.performActionForItem(at: 0)
        menu.performActionForItem(at: 1)
        XCTAssertEqual(dispatched, [.selectTrack(kind: .subtitle, id: nil), .selectTrack(kind: .subtitle, id: 1)])

        let audio = NSMenu()
        controls.populate(audio, kind: .audio)
        XCTAssertEqual(audio.items.map(\.title), ["English · 5.1 · EAC3", "Director's Commentary · ENG · 2.0 · AC3"])
        audio.performActionForItem(at: 1)
        XCTAssertEqual(dispatched.last, .selectTrack(kind: .audio, id: 2))
    }

    func testTheButtonsWorkOnlyWhileMediaIsLoaded() {
        controls.sessionChanged(SessionSnapshot(status: .playing))
        XCTAssertTrue(controls.audioButton.isEnabled && controls.subtitleButton.isEnabled)
        controls.sessionChanged(SessionSnapshot(status: .ended))
        XCTAssertFalse(controls.audioButton.isEnabled || controls.subtitleButton.isEnabled)
    }

    func testTheButtonsSitAtTheFrontOfTheTransportControlsTrailingGroup() {
        let osc = TransportOverlay(hideTimer: ManualHideTimer())
        let root = NSView()
        controls.install(in: osc, over: root)
        XCTAssertTrue(osc.trailingControls.arrangedSubviews.first === controls.audioButton)
        XCTAssertTrue(osc.trailingControls.arrangedSubviews.dropFirst().first === controls.subtitleButton)
        XCTAssertTrue(osc.trailingControls.arrangedSubviews.contains(osc.fullscreenButton))
        XCTAssertTrue(controls.readout.superview === root)
        XCTAssertNil(controls.readout.hitTest(NSPoint(x: 1, y: 1)), "the readout takes no clicks")
    }

    func testThePlayerWindowOffersTheKeysItsOwnCommandsDoNotTake() {
        let window = PlayerWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100),
                                  styleMask: PlayerWindow.style, backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        var commands: [WindowCommand] = []
        var offered: [String] = []
        window.onCommand = { command in commands.append(command); return true }
        window.onOtherKey = { event in offered.append(event.charactersIgnoringModifiers ?? ""); return true }
        window.keyDown(with: key("f"))
        window.keyDown(with: key("z"))
        XCTAssertEqual(commands, [.toggleFullscreen])
        XCTAssertEqual(offered, ["z"], "F is the window's; Z goes on to the track keys")
        window.close()
    }
}

/// The new documents over the real okp_session_* C ABI and libmpv, without media.
final class TrackSessionTests: XCTestCase {
    private var root: URL!
    private var client: SessionClient!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-tracks-\(UUID().uuidString)")
        client = try SessionClient(config: SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: root.appendingPathComponent("data").path,
            configDir: root.appendingPathComponent("config").path,
            cacheDir: root.appendingPathComponent("cache").path,
            hwdec: "no",
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")],
            shellCommands: TrackControls.shellCommands))
    }

    override func tearDownWithError() throws {
        client.close()
        client = nil
        try? FileManager.default.removeItem(at: root)
    }

    func testTheSessionAnswersTheTracksQueryAndGatesTheNewCommands() throws {
        guard case .tracks(let view)? = client.query(.tracks) else {
            return XCTFail("the tracks query was not answered with the menus")
        }
        XCTAssertEqual(view.audio, [])
        XCTAssertEqual(view.subtitles, [TrackChoice(id: nil, label: "Off", selected: true)])
        XCTAssertEqual(view.subtitleDelay.label, "0 ms")
        XCTAssertEqual(view.subtitleScale.label, "100%")
        XCTAssertFalse(view.adjustable)

        guard case .rejected(.noActiveMedia, _) = client.dispatch(.nudgeSubtitleScale(delta: 0.1)) else {
            return XCTFail("a size step without media must be refused")
        }
        guard case .rejected(.invalidArgument, _) = client.dispatch(.addSubtitle(path: root.appendingPathComponent("film.mkv").path)) else {
            return XCTFail("a file that is not a subtitle must be refused")
        }
        guard case .rejected(.noActiveMedia, _) = client.dispatch(.addSubtitle(path: root.appendingPathComponent("film.srt").path)) else {
            return XCTFail("a subtitle without media must be refused")
        }
        guard case .perform = client.dispatch(.invokeCommand(id: "add-subtitle")) else {
            return XCTFail("the file panel is this shell's to open")
        }
        guard case .rejected(.noActiveMedia, _) = client.dispatch(.invokeCommand(id: "subtitle-size-up")) else {
            return XCTFail("the session runs the size keys and refuses them without media")
        }
    }
}
