import AppKit
import XCTest
@testable import OKPlayerShell

/// A session that answers `invoke_command` as the real one does for the fixture
/// shell: the commands it runs itself are accepted, the shell's come back as
/// `perform`. It records everything dispatched.
private final class StandInSession {
    static let runsItself: Set<String> = [
        "play-pause", "close-media", "seek-back", "seek-forward", "volume-down", "volume-up", "mute",
        "subtitle-delay-forward", "subtitle-delay-back",
    ]
    private(set) var dispatched: [Command] = []

    func dispatch(_ command: Command) -> DispatchResult {
        dispatched.append(command)
        guard case .invokeCommand(let id) = command else { return .accepted(requestId: 9) }
        return Self.runsItself.contains(id) ? .accepted(requestId: 1) : .perform
    }

    var invoked: [String] {
        dispatched.compactMap { command -> String? in
            if case .invokeCommand(let id) = command { return id }
            return nil
        }
    }
}

/// `PlayerCommands` (#880): a command runs through the session or this shell's
/// handler only while the registry says it can, keys go through the shortcut
/// table, and a held toggle's auto-repeat is taken without running it again.
final class PlayerCommandsTests: XCTestCase {
    private var session: StandInSession!
    private var commands: PlayerCommands!
    private var fullscreen = false
    private var performed: [String] = []

    override func setUpWithError() throws {
        session = StandInSession()
        commands = PlayerCommands()
        let rows = try MenuFixtures.registry()
        commands.isFullscreen = { [unowned self] in self.fullscreen }
        for id in ["open-file", "fullscreen", "escape-fullscreen", "next-item"] {
            commands.register(id) { [unowned self] in self.performed.append(id) }
        }
        // The fixture is resolved for a fullscreen window; the window commands
        // follow the state the query passes, as the session resolves them.
        commands.connect(session: { [unowned self] in self.session.dispatch($0) },
                         registry: { fullscreen in
                             MenuFixtures.changing(rows, ["escape-fullscreen"]) { $0.enabled = fullscreen }
                         },
                         keys: try MenuFixtures.keys())
    }

    func testASessionCommandRunsThroughInvokeCommand() {
        XCTAssertEqual(commands.run("seek-forward"), .ran(.accepted(requestId: 1)))
        XCTAssertEqual(session.dispatched, [.invokeCommand(id: "seek-forward")])
        XCTAssertEqual(performed, [])
        XCTAssertEqual(commands.lastInvocation, PlayerCommands.Invocation(id: "seek-forward",
                                                                          outcome: .ran(.accepted(requestId: 1))))
    }

    func testAShellCommandRunsItsHandlerWhenTheSessionHandsItBack() {
        XCTAssertEqual(commands.run("fullscreen"), .performed)
        XCTAssertEqual(session.invoked, ["fullscreen"])
        XCTAssertEqual(performed, ["fullscreen"])
        XCTAssertEqual(commands.performedCounts, ["fullscreen": 1])
    }

    func testUnavailableAndDisabledCommandsReachNothing() {
        XCTAssertEqual(commands.run("zoom-out"), .unavailable)
        XCTAssertEqual(commands.run("no-such-command"), .unavailable)
        XCTAssertEqual(commands.run("escape-fullscreen"), .disabled, "not in fullscreen")
        XCTAssertEqual(session.dispatched, [])
        XCTAssertEqual(performed, [])
        fullscreen = true
        XCTAssertEqual(commands.run("escape-fullscreen"), .performed)
        XCTAssertEqual(performed, ["escape-fullscreen"])
    }

    func testTheShellCommandsAreEveryHandlerAndSubmenu() {
        commands.register(StandInSubmenu(commandID: "audio-track", titles: []))
        XCTAssertEqual(commands.shellCommands, ["audio-track", "escape-fullscreen", "fullscreen", "next-item", "open-file"])
        XCTAssertEqual(commands.submenuCommands, ["audio-track"])
    }

    // MARK: Keys

    func testKeysRunTheirCommandsAndAHeldToggleIsTakenWithoutRunning() {
        XCTAssertTrue(commands.handle(KeyPress(key: "space")))
        XCTAssertTrue(commands.handle(KeyPress(key: "space", isRepeat: true)), "the repeat is taken")
        XCTAssertEqual(session.invoked, ["play-pause"], "but runs nothing")
        XCTAssertEqual(commands.lastInvocation, PlayerCommands.Invocation(id: "play-pause", outcome: .heldRepeat))

        XCTAssertTrue(commands.handle(KeyPress(key: "Right")))
        XCTAssertTrue(commands.handle(KeyPress(key: "Right", isRepeat: true)), "a held seek repeats")
        XCTAssertTrue(commands.handle(KeyPress(key: "m")))
        XCTAssertTrue(commands.handle(KeyPress(key: "m", isRepeat: true)))
        XCTAssertTrue(commands.handle(KeyPress(key: "Up")))
        XCTAssertTrue(commands.handle(KeyPress(key: "f")))
        XCTAssertTrue(commands.handle(KeyPress(key: "f", isRepeat: true)))
        XCTAssertEqual(session.invoked, ["play-pause", "seek-forward", "seek-forward", "mute", "volume-up", "fullscreen"])
        XCTAssertEqual(performed, ["fullscreen"])
    }

    func testAKeyThatRunsNothingNowIsLeftToTheWindow() {
        XCTAssertFalse(commands.handle(KeyPress(key: "Escape")), "Escape outside fullscreen still cancels")
        XCTAssertFalse(commands.handle(KeyPress(key: "Escape", isRepeat: true)))
        XCTAssertFalse(commands.handle(KeyPress(key: "j")), "Go to time is unavailable here")
        XCTAssertFalse(commands.handle(KeyPress(key: "q")), "unbound")
        XCTAssertFalse(commands.handle(KeyPress(key: "o", modifiers: ShortcutModifiers(cmd: true))), "a menu's key")
        XCTAssertEqual(session.dispatched, [])
    }

    // MARK: The speed submenu

    func testTheSpeedSubmenuOffersTheTransportsSpeeds() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "transport/query-reply-transport.json")
        guard case .transport(let view) = reply.result else { return XCTFail("not a transport reply") }
        var sent: [Command] = []
        let menu = PlaybackSpeedMenu(transport: { view }, dispatch: { sent.append($0) })
        let row = try XCTUnwrap(try MenuFixtures.registry().first { $0.id == "playback-speed" })
        let items = menu.submenuItems(for: row)
        XCTAssertEqual(items.map(\.title), view.speeds.map(\.label))
        XCTAssertEqual(items.map { $0.state == .on }, view.speeds.map(\.selected))
        XCTAssertTrue(items.allSatisfy(\.isEnabled))
        let last = try XCTUnwrap(items.last)
        menu.chooseSpeed(last)
        XCTAssertEqual(sent, [.setSpeed(speed: try XCTUnwrap(view.speeds.last).speed)])

        var idle = row
        idle.enabled = false
        XCTAssertTrue(menu.submenuItems(for: idle).allSatisfy { !$0.isEnabled }, "no media, no speed")
    }
}

/// The keys on a real player window: a key pressed on the video goes past it to
/// the window, which offers it to the shortcut table before its own F and Escape.
final class ShortcutWindowTests: XCTestCase {
    private var window: PlayerWindow!
    private var video: VideoView!
    private var session: StandInSession!
    private var commands: PlayerCommands!
    private var toggles = 0

    override func setUpWithError() throws {
        _ = NSApplication.shared
        window = PlayerWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 400), styleMask: PlayerWindow.style,
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        video = VideoView(surface: try XCTUnwrap(GLSurface.make(), "no OpenGL pixel format for the video view"))
        window.contentView = video
        session = StandInSession()
        commands = PlayerCommands()
        commands.register("fullscreen") { [unowned self] in self.toggles += 1 }
        let rows = try MenuFixtures.registry()
        commands.connect(session: { [unowned self] in self.session.dispatch($0) }, registry: { _ in rows },
                         keys: try MenuFixtures.keys())
        window.shortcutKeys = { [unowned self] event in self.commands.handle(event) }
        XCTAssertTrue(window.makeFirstResponder(video))
    }

    override func tearDown() {
        video?.shutdown()
        window?.close()
        window = nil
        video = nil
    }

    private func key(_ characters: String, keyCode: UInt16, repeat isRepeat: Bool = false) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                                       windowNumber: window.windowNumber, context: nil, characters: characters,
                                       charactersIgnoringModifiers: characters, isARepeat: isRepeat, keyCode: keyCode))
    }

    func testSpacePressedOnTheVideoRunsPlayPauseOncePerPress() throws {
        window.sendEvent(try key(" ", keyCode: 49))
        window.sendEvent(try key(" ", keyCode: 49, repeat: true))
        window.sendEvent(try key(" ", keyCode: 49, repeat: true))
        XCTAssertEqual(session.invoked, ["play-pause"])
    }

    func testArrowsAndMOnTheVideoSeekChangeTheVolumeAndMute() throws {
        window.sendEvent(try key("\u{F703}", keyCode: 124))
        window.sendEvent(try key("\u{F702}", keyCode: 123, repeat: true))
        window.sendEvent(try key("\u{F700}", keyCode: 126))
        window.sendEvent(try key("\u{F701}", keyCode: 125))
        window.sendEvent(try key("m", keyCode: 46))
        XCTAssertEqual(session.invoked, ["seek-forward", "seek-back", "volume-up", "volume-down", "mute"])
    }

    func testFRunsTheRegistrysFullscreenCommandOnce() throws {
        window.sendEvent(try key("f", keyCode: 3))
        XCTAssertEqual(toggles, 1, "the shortcut table took F; the window's own F did not toggle again")
        XCTAssertEqual(session.invoked, ["fullscreen"])
    }
}

/// The app's registry commands against the real session: every id it registers
/// is one the session knows, so the session starts with them.
final class PlayerAppCommandsTests: XCTestCase {
    func testTheSessionAcceptsTheAppsShellCommandsAndAnswersTheKeyQueries() throws {
        _ = NSApplication.shared
        let app = PlayerApp()
        app.registerPlayerCommands()
        XCTAssertEqual(app.commands.shellCommands,
                       ["escape-fullscreen", "fullscreen", "next-item", "open-file", "playback-speed", "previous-item"])
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-menu-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let client = try SessionClient(config: SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: root.appendingPathComponent("data").path, configDir: root.appendingPathComponent("config").path,
            cacheDir: root.appendingPathComponent("cache").path, hwdec: "no",
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")],
            shellCommands: app.commands.shellCommands))
        defer { client.close() }

        guard case .repeatingShortcuts(let repeating)? = client.query(.repeatingShortcuts) else {
            return XCTFail("the repeating shortcuts query was not answered")
        }
        XCTAssertEqual(repeating, try MenuFixtures.repeating())
        guard case .commandRegistry(let rows)? = client.query(.commandRegistry(fullscreen: false)) else {
            return XCTFail("the registry query was not answered")
        }
        let models = MenuBarLayout.build(registry: rows, submenus: app.commands.submenuCommands)
        let shown = models.flatMap(\.entries).compactMap { entry -> String? in
            if case .command(let id, _, _, _) = entry { return id }
            return nil
        }
        for id in app.commands.shellCommands where id != "escape-fullscreen" {
            XCTAssertTrue(shown.contains(id), "\(id) is in the menu bar")
        }
        let next = try XCTUnwrap(rows.first { $0.id == "next-item" })
        XCTAssertTrue(next.available)
        XCTAssertFalse(next.enabled, "nothing to step to without a playlist")
    }
}
