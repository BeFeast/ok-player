import AppKit
import XCTest
@testable import OKPlayerShell

private func entry(_ name: String) -> PlaylistEntry {
    PlaylistEntry(source: MediaSource(kind: .local, location: "/Users/example/Movies/\(name)"), title: name)
}

/// A playlist of `names` with the item at `current` playing.
func upNextPlaylist(_ names: [String], current: Int?) -> PlaylistState {
    PlaylistState(items: names.map(entry), currentIndex: current)
}

/// What the panel shows for a playlist, and what its rows and keys ask the session for.
final class UpNextListTests: XCTestCase {
    func testTheRowsListThePlaylistInOrderWithTheCurrentItemMarked() {
        let list = UpNextList(playlist: upNextPlaylist(["a.mkv", "b.mkv", "c.mkv"], current: 1), mediaTitle: nil)
        XCTAssertEqual(list.rows.map(\.title), ["a.mkv", "b.mkv", "c.mkv"])
        XCTAssertEqual(list.rows.map(\.number), ["1", "2", "3"])
        XCTAssertEqual(list.rows.map(\.isCurrent), [false, true, false])
        XCTAssertEqual(list.rows[2].location, "/Users/example/Movies/c.mkv", "the tooltip names the file")
        XCTAssertEqual(list.heading, "3 ITEMS")
    }

    func testNowPlayingShowsTheMediaTitleOrElseTheItemsLabel() {
        let playlist = upNextPlaylist(["a.mkv", "b.mkv"], current: 1)
        XCTAssertEqual(UpNextList(playlist: playlist, mediaTitle: "The Film").nowPlaying,
                       UpNextList.NowPlaying(title: "The Film", location: "/Users/example/Movies/b.mkv"))
        XCTAssertEqual(UpNextList(playlist: playlist, mediaTitle: nil).nowPlaying?.title, "b.mkv")
        XCTAssertEqual(UpNextList(playlist: playlist, mediaTitle: "").nowPlaying?.title, "b.mkv")
    }

    func testWithoutACurrentItemNothingIsPinnedOrMarked() {
        for playlist in [PlaylistState(), upNextPlaylist(["a.mkv"], current: nil), upNextPlaylist(["a.mkv"], current: 4)] {
            let list = UpNextList(playlist: playlist, mediaTitle: "Stale title")
            XCTAssertNil(list.nowPlaying, "\(playlist)")
            XCTAssertFalse(list.rows.contains(where: \.isCurrent), "\(playlist)")
        }
        XCTAssertEqual(UpNextList().heading, "", "an empty list has no heading")
        XCTAssertEqual(UpNextList(playlist: upNextPlaylist(["a.mkv"], current: 0), mediaTitle: nil).heading, "1 ITEM")
    }

    func testTheHeadingSaysWhileAFolderIsStillBeingRead() {
        var playlist = upNextPlaylist(["a.mkv"], current: 0)
        playlist.scanning = true
        XCTAssertEqual(UpNextList(playlist: playlist, mediaTitle: nil).heading, "1 ITEM · SCANNING…")
    }

    func testARowPlaysAndIsRemovedThroughTheSessionPlaylistButNeverTheCurrentOne() {
        let list = UpNextList(playlist: upNextPlaylist(["a.mkv", "b.mkv", "c.mkv"], current: 1), mediaTitle: nil)
        XCTAssertEqual(list.activation(ofRow: 2), .playlist(action: .jump(index: 2)))
        XCTAssertEqual(list.removal(ofRow: 0), .playlist(action: .remove(index: 0)))
        XCTAssertNil(list.activation(ofRow: 1), "the current item is already playing")
        XCTAssertNil(list.removal(ofRow: 1), "the session keeps the current item")
        for row in [-1, 3] {
            XCTAssertNil(list.activation(ofRow: row), "no row \(row)")
            XCTAssertNil(list.removal(ofRow: row), "no row \(row)")
        }
    }

    func testReturnAndEnterPlayDeleteAndForwardDeleteRemoveAndChordsAreNotTheLists() {
        XCTAssertEqual(UpNextKey(keyCode: UpNextKey.returnKeyCode, modifiers: []), .activate)
        XCTAssertEqual(UpNextKey(keyCode: UpNextKey.enterKeyCode, modifiers: [.numericPad]), .activate)
        XCTAssertEqual(UpNextKey(keyCode: UpNextKey.deleteKeyCode, modifiers: []), .remove)
        XCTAssertEqual(UpNextKey(keyCode: UpNextKey.forwardDeleteKeyCode, modifiers: [.function]), .remove)
        XCTAssertNil(UpNextKey(keyCode: UpNextKey.deleteKeyCode, modifiers: [.command]))
        XCTAssertNil(UpNextKey(keyCode: UpNextKey.returnKeyCode, modifiers: [.option]))
        XCTAssertNil(UpNextKey(keyCode: 49, modifiers: []), "Space belongs to the video")
        XCTAssertNil(UpNextKey(keyCode: 3, modifiers: []), "F belongs to the window")
    }
}

/// Stands in for the session: answers the registry query with the Up Next command
/// in a chosen state and records every command.
private final class FakeUpNextSession {
    var available = true
    var enabled = true
    var invokeResult = DispatchResult.perform
    private(set) var dispatched: [Command] = []
    private(set) var registryQueries = 0

    func dispatch(_ command: Command) -> DispatchResult {
        dispatched.append(command)
        if case .invokeCommand = command { return invokeResult }
        return .applied
    }

    func query(_ query: Query) -> QueryResult? {
        guard case .commandRegistry = query else { return nil }
        registryQueries += 1
        return .commandRegistry(commands: [
            RegistryCommand(id: "play-pause", group: .playbackNavigation, menu: .topLevel, label: "Play / Pause",
                            available: true, enabled: true, checked: false, shortcut: nil),
            RegistryCommand(id: UpNextController.commandID, group: .tracksSubtitles, menu: .topLevel,
                            label: "Chapters & Up Next", available: available, enabled: enabled, checked: false,
                            shortcut: nil),
        ])
    }
}

/// Stands in for the video: takes keyboard focus and records the keys it is sent.
final class UpNextFocusTarget: NSView {
    private(set) var keys: [UInt16] = []
    override var acceptsFirstResponder: Bool { true }
    override func keyDown(with event: NSEvent) { keys.append(event.keyCode) }
}

/// The panel's place in the window: the transport controls' button and the menu
/// bar's registry item open it through the registry command, and it shows the playlist the
/// session drained. Real AppKit views in a window that is never shown.
final class UpNextControllerTests: XCTestCase {
    private var session: FakeUpNextSession!
    private var controller: UpNextController!
    private var window: NSWindow!
    private var video: UpNextFocusTarget!

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
        session = FakeUpNextSession()
        controller = UpNextController()
        controller.dispatch = { [unowned self] command in self.session.dispatch(command) }
        controller.query = { [unowned self] query in self.session.query(query) }
        let frame = NSRect(x: 0, y: 0, width: 960, height: 580)
        window = NSWindow(contentRect: frame, styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        let root = NSView(frame: frame)
        window.contentView = root
        video = UpNextFocusTarget(frame: root.bounds)
        root.addSubview(video)
        let bar = NSView()
        bar.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(bar)
        root.addSubview(controller.button)
        NSLayoutConstraint.activate([
            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            bar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            bar.heightAnchor.constraint(equalToConstant: 76),
        ])
        controller.install(in: root, above: bar)
        controller.panel.focusReturn = video
    }

    override func tearDown() {
        window.close()
        super.tearDown()
    }

    func testTheCommandOpensThePanelWhenTheRegistryEnablesItAndTheShellCarriesItOut() {
        XCTAssertTrue(controller.toggle())
        XCTAssertTrue(controller.isShown)
        XCTAssertFalse(controller.panel.isHidden)
        XCTAssertEqual(session.dispatched, [.invokeCommand(id: "chapters-up-next")])

        controller.hide()
        session.enabled = false
        XCTAssertFalse(controller.toggle(), "without media the registry disables the command")
        XCTAssertTrue(controller.panel.isHidden)
        session.enabled = true
        session.available = false
        XCTAssertFalse(controller.toggle(), "a command this shell does not carry out")
        XCTAssertEqual(session.dispatched.count, 1, "a command the registry does not offer is not invoked")

        session.available = true
        session.invokeResult = .rejected(reason: .unavailable, message: "not here")
        XCTAssertFalse(controller.toggle(), "only a perform answer opens the panel")
        XCTAssertTrue(controller.panel.isHidden)
    }

    func testTheButtonOpensAndClosesThePanelAndClosingNeedsNoSession() {
        controller.button.performClick(nil)
        XCTAssertTrue(controller.isShown)
        let invoked = session.dispatched.count
        session.enabled = false
        controller.button.performClick(nil)
        XCTAssertFalse(controller.isShown, "the panel closes even once the command is disabled")
        XCTAssertTrue(controller.panel.isHidden)
        XCTAssertEqual(session.dispatched.count, invoked, "closing asks the session nothing")

        session.enabled = true
        XCTAssertTrue(controller.show())
        controller.panel.closeButton.performClick(nil)
        XCTAssertFalse(controller.isShown, "the panel's own close button")
    }

    func testTheMenuBarsRegistryItemOpensAndClosesThePanel() {
        let commands = PlayerCommands()
        controller.register(with: commands)
        XCTAssertEqual(commands.shellCommands, [UpNextController.commandID])
        commands.connect(session: { [unowned self] command in self.session.dispatch(command) },
                         registry: { [unowned self] _ in
                             guard case .commandRegistry(let rows)? = self.session.query(.commandRegistry()) else {
                                 return []
                             }
                             return rows
                         },
                         keys: ShortcutKeyMap())
        XCTAssertEqual(commands.run(UpNextController.commandID), .performed)
        XCTAssertTrue(controller.isShown)
        XCTAssertFalse(controller.panel.isHidden)
        XCTAssertEqual(session.dispatched, [.invokeCommand(id: "chapters-up-next")], "invoked once, by the menu bar")
        XCTAssertEqual(commands.run(UpNextController.commandID), .performed)
        XCTAssertFalse(controller.isShown, "the same item closes it")
        session.enabled = false
        XCTAssertEqual(commands.run(UpNextController.commandID), .disabled, "nothing to show without media")
        XCTAssertFalse(controller.isShown)
    }

    func testThePanelShowsTheDrainedPlaylistUnderNowPlaying() {
        var snapshot = SessionSnapshot()
        snapshot.title = "First film"
        controller.sessionChanged(playlist: upNextPlaylist(["first.mkv", "second.mkv"], current: 0), snapshot: snapshot)
        let panel = controller.panel
        XCTAssertEqual(panel.tableView.numberOfRows, 2)
        XCTAssertEqual(panel.nowPlayingTitle.stringValue, "First film")
        XCTAssertEqual(panel.nowPlayingState.stringValue, "NOW PLAYING")
        XCTAssertEqual(panel.headingLabel.stringValue, "2 ITEMS")
        XCTAssertTrue(panel.emptyLabel.isHidden)

        controller.sessionChanged(playlist: PlaylistState(), snapshot: SessionSnapshot())
        XCTAssertEqual(panel.tableView.numberOfRows, 0, "closing the media empties the playlist")
        XCTAssertEqual(panel.nowPlayingTitle.stringValue, "Nothing is playing")
        XCTAssertFalse(panel.emptyLabel.isHidden)
    }

    func testHidingThePanelGivesTheListsKeyboardFocusBackToTheVideo() {
        controller.sessionChanged(playlist: upNextPlaylist(["first.mkv", "second.mkv"], current: 0),
                                  snapshot: SessionSnapshot())
        XCTAssertTrue(controller.show())
        XCTAssertTrue(window.makeFirstResponder(controller.panel.tableView))
        controller.hide()
        XCTAssertTrue(window.firstResponder === video, "Space would not reach the video")
    }
}

/// The app's own session config over the real okp_session_* ABI, with its files
/// under `root` and no audio or video output.
/// The app's session config, with the shell commands the app registers.
private func appSessionConfig(root: URL) -> SessionConfig {
    let app = PlayerApp()
    app.registerPlayerCommands()
    var config = SessionConfig.forApp(shellCommands: app.commands.shellCommands)
    config.dataDir = root.appendingPathComponent("data").path
    config.configDir = root.appendingPathComponent("config").path
    config.cacheDir = root.appendingPathComponent("cache").path
    config.hwdec = "no"
    config.mpvOptions = [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")]
    return config
}

/// The app's own session config over the real okp_session_* ABI: the Up Next
/// command is the shell's to carry out.
final class UpNextSessionTests: XCTestCase {
    func testTheAppsSessionLeavesTheUpNextCommandToTheShell() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-up-next-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let client = try SessionClient(config: appSessionConfig(root: root))
        defer { client.close() }

        guard case .commandRegistry(let commands)? = client.query(.commandRegistry(fullscreen: false, miniPlayer: false)) else {
            return XCTFail("the registry query was not answered with the registry")
        }
        let command = try XCTUnwrap(commands.first { $0.id == UpNextController.commandID })
        XCTAssertTrue(command.available, "the app's config lists the command among its shell commands")
        XCTAssertFalse(command.enabled, "nothing is open")
        XCTAssertEqual(client.dispatch(.invokeCommand(id: UpNextController.commandID)), .perform)

        let controller = UpNextController()
        controller.dispatch = { client.dispatch($0) }
        controller.query = { client.query($0) }
        XCTAssertFalse(controller.show(), "the panel waits for media")
    }
}

/// The app hands every drained batch to the panel, also while an error it reported
/// is shown: the rows send playlist commands by index into the session's playlist,
/// and a jump from the panel never goes through File > Open, which clears the error.
final class UpNextAppTests: XCTestCase {
    func testThePanelFollowsThePlaylistAfterTheSessionReportedAnError() throws {
        _ = NSApplication.shared
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-up-next-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let client = try SessionClient(config: appSessionConfig(root: root))
        defer { client.close() }
        let app = PlayerApp()
        app.system = SystemIntegration(session: { _ in .applied }, open: { _ in },
                                       nowPlayingCenter: RecordingNowPlayingCenter(),
                                       remoteSource: FakeRemoteCommandSource(), power: FakePowerAssertions(),
                                       recentList: FakeRecentDocumentList(), artwork: nil, clock: { 0 })
        app.client = client
        func deliver(_ events: [Event]) {
            let batch = EventBatch(schemaVersion: SessionJSON.schemaVersion, events: events)
            client.store.apply(batch)
            app.sessionChanged(batch)
        }
        let rows = { app.upNext.panel.list.rows.map(\.title) }

        deliver([.playlistChanged(playlist: upNextPlaylist(["a.mkv", "b.mkv", "c.mkv", "d.mkv"], current: 1))])
        XCTAssertEqual(rows(), ["a.mkv", "b.mkv", "c.mkv", "d.mkv"])

        // a.mkv, played from the panel, does not open: the session moved to it and
        // reported the error, and the app put up its alert (not run here).
        app.errorShown = true
        deliver([.playlistChanged(playlist: upNextPlaylist(["a.mkv", "b.mkv", "c.mkv", "d.mkv"], current: 0)),
                 .error(kind: .loadFailed, message: "a.mkv: unrecognized file format", requestId: nil)])
        XCTAssertEqual(app.upNext.panel.list.rows.firstIndex(where: \.isCurrent), 0, "the batch with the error")

        // The alert is dismissed and the panel removes b.mkv.
        deliver([.playlistChanged(playlist: upNextPlaylist(["a.mkv", "c.mkv", "d.mkv"], current: 0))])
        XCTAssertEqual(rows(), ["a.mkv", "c.mkv", "d.mkv"], "a batch after the error")
        XCTAssertEqual(app.upNext.panel.tableView.numberOfRows, 3)
        XCTAssertEqual(app.upNext.panel.list.activation(ofRow: 1), .playlist(action: .jump(index: 1)),
                       "row 2 on screen is c.mkv, the session's item 1")
    }
}
