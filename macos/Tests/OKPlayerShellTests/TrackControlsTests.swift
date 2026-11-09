import AppKit
import XCTest
@testable import OKPlayerShell

/// The track surfaces over real AppKit objects in a window that is never shown,
/// with the session stood in by closures and the keys and menu bar going through
/// `PlayerCommands`, wired as the app wires them: what a key press, a menu item
/// and the session's answers make them send and show.
final class TrackControlsTests: XCTestCase {
    private var controls: TrackControls!
    private var commands: PlayerCommands!
    private var dispatched: [Command] = []
    private var answer: DispatchResult = .accepted(requestId: 1)
    private var view: TracksView!

    override func setUpWithError() throws {
        try super.setUpWithError()
        _ = NSApplication.shared
        view = try TrackProtocolTests.fixtureView()
        controls = TrackControls()
        dispatched = []
        answer = .accepted(requestId: 1)
        controls.dispatch = { [unowned self] command in self.session(command) }
        controls.query = { [unowned self] in self.view }
        commands = PlayerCommands()
        controls.register(with: commands)
        // The fixture registry is resolved for a shell without the file panel.
        let rows = MenuFixtures.changing(try MenuFixtures.registry(), [TrackMenus.addSubtitle]) { $0.available = true }
        commands.connect(session: { [unowned self] command in self.session(command) }, registry: { _ in rows },
                         keys: try MenuFixtures.keys())
        commands.onRun = { [unowned self] invocation in self.controls.commandRan(invocation) }
    }

    /// The session: it hands the shell's `add-subtitle` back and answers the rest with `answer`.
    private func session(_ command: Command) -> DispatchResult {
        dispatched.append(command)
        if command == .invokeCommand(id: TrackMenus.addSubtitle) { return .perform }
        return answer
    }

    private var invoked: [String] {
        dispatched.compactMap { command -> String? in
            if case .invokeCommand(let id) = command { return id }
            return nil
        }
    }

    private func key(_ characters: String, keyCode: UInt16, _ modifiers: NSEvent.ModifierFlags = [],
                     repeating: Bool = false) -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: 0,
                         context: nil, characters: characters, charactersIgnoringModifiers: characters,
                         isARepeat: repeating, keyCode: keyCode)!
    }

    func testTheSizeKeyRunsTheRegistryCommandAndShowsTheSessionsReadout() {
        XCTAssertTrue(commands.handle(key("]", keyCode: 30)))
        XCTAssertEqual(dispatched, [.invokeCommand(id: "subtitle-size-up")])
        XCTAssertEqual(controls.readout.text, "Subtitle size: 110%")
    }

    func testWithAToastTheReadoutsAreToasts() {
        var toasts: [String] = []
        controls.toast = { toasts.append($0) }
        XCTAssertTrue(commands.handle(key("]", keyCode: 30)))
        answer = .accepted(requestId: 7)
        controls.addSubtitle(URL(fileURLWithPath: "/Users/example/Movies/broken.srt"))
        XCTAssertTrue(controls.claimError(requestId: 7))
        XCTAssertEqual(toasts, ["Subtitle size: 110%", TrackControls.addFailed])
        XCTAssertEqual(controls.readout.text, "", "nothing shows in the track readout itself")
        XCTAssertEqual(controls.lastShown, TrackControls.addFailed)
    }

    func testShiftZStepsTheDelayBackAndReadsTheDelay() {
        XCTAssertTrue(commands.handle(key("Z", keyCode: 6, .shift)))
        XCTAssertEqual(dispatched, [.invokeCommand(id: "subtitle-delay-back")])
        XCTAssertEqual(controls.readout.text, "Subtitle delay: +250 ms")
    }

    func testAnotherCommandIsNotATrackRequest() {
        answer = .accepted(requestId: 5)
        XCTAssertTrue(commands.handle(key("x", keyCode: 7)), "X closes the media")
        XCTAssertEqual(invoked, ["close-media"])
        XCTAssertEqual(controls.readout.text, "")
        XCTAssertFalse(controls.claimError(requestId: 5), "its error stays the app's")
    }

    func testARefusedStepShowsNoReadout() {
        answer = .rejected(reason: .noActiveMedia, message: "the command needs loaded media")
        XCTAssertTrue(commands.handle(key("[", keyCode: 33)), "the key is still the size key")
        XCTAssertEqual(dispatched, [.invokeCommand(id: "subtitle-size-down")])
        XCTAssertEqual(controls.readout.text, "", "nothing changed, so nothing is announced")
    }

    func testAHeldSAsksForTheFilePanelOnceAndTheSessionDecidesWhetherItOpens() {
        XCTAssertTrue(commands.handle(key("s", keyCode: 1)))
        XCTAssertTrue(commands.handle(key("s", keyCode: 1, repeating: true)), "a repeat is swallowed")
        XCTAssertEqual(dispatched, [.invokeCommand(id: "add-subtitle")])
        XCTAssertEqual(commands.performedCounts, ["add-subtitle": 1], "the shell carries out the file panel")
        XCTAssertFalse(controls.carryOut("open-file"), "not a track surface's command")
    }

    func testAnAddedFileGoesToTheSessionAsItsPath() {
        controls.addSubtitle(URL(fileURLWithPath: "/Users/example/Movies/film.en.srt"))
        XCTAssertEqual(dispatched, [.addSubtitle(path: "/Users/example/Movies/film.en.srt")])
    }

    func testTheMenuBarHostsTheTrackSubmenusAndTheySwitchTracks() throws {
        XCTAssertEqual(commands.submenuCommands, ["audio-track", "subtitles"])
        XCTAssertTrue(commands.shellCommands.contains("add-subtitle"))
        let bar = MenuBar(commands: commands, openRecent: nil)
        bar.rebuild()
        let subtitles = try submenu(of: .subtitles, in: bar)
        XCTAssertEqual(subtitles.items.map(\.title), ["Off", "English SDH · PGS · Image · Default", "en · SRT · EXT", "",
                                                      "Add Subtitle File…"])
        XCTAssertEqual(subtitles.items.map(\.state), [.off, .off, .on, .off, .off])
        XCTAssertTrue(subtitles.items[3].isSeparatorItem)
        subtitles.performActionForItem(at: 0)
        subtitles.performActionForItem(at: 1)
        XCTAssertEqual(dispatched, [.selectTrack(kind: .subtitle, id: nil), .selectTrack(kind: .subtitle, id: 1)])
        XCTAssertTrue(bar.menus[.subtitles]?.items.contains(where: { $0.playerCommandID == "add-subtitle" }) ?? false,
                      "S has its registry item in the Subtitles menu")

        let audio = try submenu(of: .audio, in: bar)
        XCTAssertEqual(audio.items.map(\.title), ["English · 5.1 · EAC3", "Director's Commentary · ENG · 2.0 · AC3"])
        audio.performActionForItem(at: 1)
        XCTAssertEqual(dispatched.last, .selectTrack(kind: .audio, id: 2))

        view.adjustable = false
        bar.menuNeedsUpdate(subtitles)
        XCTAssertFalse(subtitles.items.filter { !$0.isSeparatorItem }.contains(where: \.isEnabled),
                       "nothing to choose without loaded media")
    }

    func testAMenuItemsStepShowsItsReadout() throws {
        let bar = MenuBar(commands: commands, openRecent: nil)
        bar.rebuild()
        let menu = try XCTUnwrap(bar.menus[.subtitles])
        let index = try XCTUnwrap(menu.items.firstIndex { $0.playerCommandID == "subtitle-size-up" })
        commands.perform(menu.items[index])
        XCTAssertEqual(invoked, ["subtitle-size-up"])
        XCTAssertEqual(controls.readout.text, "Subtitle size: 110%")
    }

    /// The submenu a provider fills in `menu`, filled as when it opens.
    private func submenu(of menu: MenuBarMenu, in bar: MenuBar) throws -> NSMenu {
        let item = try XCTUnwrap(bar.menus[menu]?.items.first { $0.submenu != nil }, "\(menu) has no track submenu")
        let submenu = try XCTUnwrap(item.submenu)
        bar.menuNeedsUpdate(submenu)
        return submenu
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

    func testThePlayerWindowRunsEachTrackKeyOnceThroughTheShortcutTable() {
        let window = PlayerWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100),
                                  styleMask: PlayerWindow.style, backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        var windowCommands: [WindowCommand] = []
        window.onCommand = { command in windowCommands.append(command); return true }
        window.shortcutKeys = { [unowned self] event in self.commands.handle(event) }
        window.keyDown(with: key("s", keyCode: 1))
        window.keyDown(with: key("z", keyCode: 6))
        window.keyDown(with: key("Z", keyCode: 6, .shift))
        window.keyDown(with: key("[", keyCode: 33))
        window.keyDown(with: key("]", keyCode: 30))
        window.keyDown(with: key("я", keyCode: 6))
        XCTAssertEqual(invoked, ["add-subtitle", "subtitle-delay-forward", "subtitle-delay-back", "subtitle-size-down",
                                 "subtitle-size-up", "subtitle-delay-forward"], "each key ran its command once")
        XCTAssertEqual(windowCommands, [], "and none went on to the window's own keys")
        window.close()
    }

    // MARK: Errors and refusals

    func testAnErrorAgainstATrackRequestShowsInTheReadoutAndNoOtherErrorIsClaimed() {
        answer = .accepted(requestId: 7)
        controls.addSubtitle(URL(fileURLWithPath: "/Users/example/Movies/broken.srt"))
        XCTAssertFalse(controls.claimError(requestId: nil), "an error of no request stays the app's")
        XCTAssertFalse(controls.claimError(requestId: 8), "so does one of another request")
        XCTAssertEqual(controls.readout.text, "")
        XCTAssertTrue(controls.claimError(requestId: 7))
        XCTAssertEqual(controls.readout.text, TrackControls.addFailed)
        XCTAssertFalse(controls.claimError(requestId: 7), "a request is answered once")

        answer = .failed(requestId: 9, message: "track not found")
        controls.choose(.select(kind: .audio, id: 3))
        XCTAssertTrue(controls.claimError(requestId: 9), "the session reports a failed choice as an error event too")
        XCTAssertEqual(controls.readout.text, TrackControls.changeFailed)

        answer = .accepted(requestId: 11)
        XCTAssertTrue(commands.handle(key("]", keyCode: 30)))
        XCTAssertTrue(controls.claimError(requestId: 11), "a step from the shortcut table is a track request")
        XCTAssertEqual(controls.readout.text, TrackControls.changeFailed)
    }

    func testSOpensTheFilePanelOnlyWhileMediaIsLoaded() {
        let window = offscreenWindow()
        defer { window.close() }
        var panels = 0
        controls.window = window
        controls.presentFilePanel = { _, _ in panels += 1 }
        // The session answers `perform` for the shell's command whatever plays.
        view.adjustable = false
        XCTAssertTrue(commands.handle(key("s", keyCode: 1)))
        XCTAssertEqual(panels, 0, "nothing plays to add a file to")
        view.adjustable = true
        XCTAssertTrue(commands.handle(key("s", keyCode: 1)))
        XCTAssertEqual(panels, 1)
    }

    func testAFileTheSessionRefusesSaysSoInTheReadout() {
        let window = offscreenWindow()
        defer { window.close() }
        var chosen: ((URL) -> Void)?
        controls.window = window
        controls.presentFilePanel = { _, done in chosen = done }
        XCTAssertTrue(commands.handle(key("s", keyCode: 1)))
        // Playback ended while the panel was open.
        answer = .rejected(reason: .noActiveMedia, message: "the command needs loaded media")
        chosen?(URL(fileURLWithPath: "/Users/example/Movies/film.en.srt"))
        XCTAssertEqual(dispatched.last, .addSubtitle(path: "/Users/example/Movies/film.en.srt"))
        XCTAssertEqual(controls.readout.text, TrackControls.addFailed)
    }

    // MARK: Popover

    func testThePopoverKeepsItsControlsWhileTheTracksStayTheSame() {
        let popover = controls.popoverContent
        popover.update(kind: .subtitle, view: view)
        let rows = popover.controls
        let buttons = Self.buttons(in: rows)
        XCTAssertEqual(buttons.count, 3 + 1 + 4, "three choices, Add Subtitle File… and two steps per row")
        XCTAssertTrue(buttons.allSatisfy(\.refusesFirstResponder), "no control of the popover takes keyboard focus")
        // Batches with nothing new for the popover, as during playback.
        popover.update(kind: .subtitle, view: view)
        popover.update(kind: .subtitle, view: view)
        XCTAssertTrue(Self.same(Self.buttons(in: popover.controls), buttons), "the buttons under the pointer stay")

        var later = view!
        later.subtitleDelay = SubtitleValue(value: 0.3, label: "+300 ms", reported: 0.3)
        popover.update(kind: .subtitle, view: later)
        XCTAssertFalse(Self.same(popover.controls, rows), "a new delay is shown")
        let shown = popover.controls
        // Closed after a choice the session did not take: reopened, it shows the session's state.
        popover.invalidate()
        popover.update(kind: .subtitle, view: later)
        XCTAssertFalse(Self.same(popover.controls, shown))
    }

    func testAfterAPopoverClosesSpacePausesInsteadOfClickingTheTrackButton() throws {
        let rig = try PlayerRig(controls: controls, commands: commands)
        defer { rig.close() }
        XCTAssertTrue(controls.audioButton.refusesFirstResponder && controls.subtitleButton.refusesFirstResponder,
                      "a click on a track button leaves keyboard focus where it was")
        // Where a click would have left focus with Keyboard navigation on.
        _ = rig.window.makeFirstResponder(controls.subtitleButton)
        controls.popoverDidClose(Notification(name: NSPopover.didCloseNotification))
        rig.window.sendEvent(try rig.key(" ", keyCode: 49))
        XCTAssertEqual(invoked, ["play-pause"])
    }

    func testKeysPressedInThePopoverGoToThePlayerWindow() throws {
        let rig = try PlayerRig(controls: controls, commands: commands)
        defer { rig.close() }
        let content = controls.popoverContent.view
        XCTAssertTrue(content.acceptsFirstResponder, "the popover's content holds keyboard focus, not its Off button")
        content.keyDown(with: try rig.key(" ", keyCode: 49))
        XCTAssertEqual(dispatched, [.invokeCommand(id: "play-pause")],
                       "Space pauses as it does on the video, and turns no subtitle off")
        content.keyDown(with: try rig.key("]", keyCode: 30))
        XCTAssertEqual(dispatched.last, .invokeCommand(id: "subtitle-size-up"), "] steps the size the popover shows")
        XCTAssertEqual(controls.readout.text, "Subtitle size: 110%")
        XCTAssertFalse(controls.popoverKey(try rig.key("\u{1b}", keyCode: WindowKeys.escapeKeyCode)),
                       "Escape is left to the popover, which closes")
    }

    private func offscreenWindow() -> NSWindow {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100), styleMask: [.titled],
                              backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        return window
    }

    private static func buttons(in views: [NSView]) -> [NSButton] {
        views.flatMap { view in (view as? NSButton).map { [$0] } ?? buttons(in: view.subviews) }
    }

    private static func same(_ left: [NSView], _ right: [NSView]) -> Bool {
        left.count == right.count && zip(left, right).allSatisfy { $0 === $1 }
    }
}

/// A real player window with a video view holding keyboard focus, the track
/// buttons in the transport controls and the keys going through `commands`'s
/// shortcut table, wired as the app wires them.
private final class PlayerRig {
    let window: PlayerWindow
    let video: VideoView
    let osc = TransportOverlay(hideTimer: ManualHideTimer())

    init(controls: TrackControls, commands: PlayerCommands) throws {
        _ = NSApplication.shared
        window = PlayerWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 580), styleMask: PlayerWindow.style,
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        video = VideoView(surface: try XCTUnwrap(GLSurface.make(), "no OpenGL pixel format for the video view"))
        let root = NSView()
        window.contentView = root
        root.addSubview(video)
        root.addSubview(osc)
        osc.focusReturn = video
        controls.install(in: osc, over: root)
        controls.window = window
        window.shortcutKeys = { [weak commands] event in commands?.handle(event) ?? false }
        XCTAssertTrue(window.makeFirstResponder(video))
    }

    func key(_ characters: String, keyCode: UInt16) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                                       windowNumber: window.windowNumber, context: nil, characters: characters,
                                       charactersIgnoringModifiers: characters, isARepeat: false, keyCode: keyCode))
    }

    func close() {
        video.shutdown()
        window.close()
    }
}

/// The app's handling of a session batch, over the real okp_session_* C ABI.
final class TrackErrorRoutingTests: XCTestCase {
    func testAnUnreadableSubtitleFileLeavesThePlayerRunning() throws {
        _ = NSApplication.shared
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-track-errors-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let client = try SessionClient(config: SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: root.appendingPathComponent("data").path,
            configDir: root.appendingPathComponent("config").path,
            cacheDir: root.appendingPathComponent("cache").path,
            hwdec: "no",
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")],
            shellCommands: TrackControls.shellCommands))
        defer { client.close() }
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled],
                              backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let app = PlayerApp()
        app.window = window
        app.system = SystemIntegration(session: { _ in .noOp }, open: { _ in },
                                       nowPlayingCenter: RecordingNowPlayingCenter(),
                                       remoteSource: FakeRemoteCommandSource(), power: FakePowerAssertions(),
                                       recentList: FakeRecentDocumentList(), artwork: nil, clock: { 0 })
        app.client = client
        // The session accepted the file; libmpv then fails to read it.
        app.tracks.dispatch = { _ in .accepted(requestId: 41) }
        app.tracks.addSubtitle(URL(fileURLWithPath: "/Users/example/Movies/broken.srt"))
        app.tracks.audioButton.isEnabled = true

        app.sessionChanged(EventBatch(schemaVersion: SessionJSON.schemaVersion, events: [
            .error(kind: .commandFailed, message: "Error parsing option", requestId: 41),
        ]))
        XCTAssertFalse(app.errorShown, "no Could not play video alert over a video that plays on")
        XCTAssertEqual(app.tracks.readout.text, TrackControls.addFailed)
        XCTAssertFalse(app.tracks.audioButton.isEnabled, "the same batch still reached the track controls (idle session)")

        app.tracks.audioButton.isEnabled = true
        app.sessionChanged(EventBatch(schemaVersion: SessionJSON.schemaVersion, events: [.snapshotChanged(changes: [])]))
        XCTAssertFalse(app.tracks.audioButton.isEnabled, "later batches are still followed")
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
