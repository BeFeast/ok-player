import Foundation
import XCTest
@testable import OKPlayerShell

/// The Swift client over the real okp_session_* C ABI and libmpv: commands go out as
/// Swift-encoded documents, and state comes back through the wakeup and a drain on
/// the main queue.
final class SessionClientTests: XCTestCase {
    private var root: URL!
    private var client: SessionClient!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-session-client-\(UUID().uuidString)")
        let config = SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: root.appendingPathComponent("data").path,
            configDir: root.appendingPathComponent("config").path,
            cacheDir: root.appendingPathComponent("cache").path,
            hwdec: "no",
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")],
            shellCommands: ["open-file"])
        client = try SessionClient(config: config)
    }

    override func tearDownWithError() throws {
        client.close()
        client = nil
        try? FileManager.default.removeItem(at: root)
    }

    func testAWakeupDrainsOnTheMainQueueIntoTheStore() throws {
        let drained = expectation(description: "a drained batch carries the new volume")
        drained.assertForOverFulfill = false
        var offMain = false
        let store = client.store
        client.onBatch = { _ in
            if !Thread.isMainThread { offMain = true }
            if store.snapshot.volume == 42 { drained.fulfill() }
        }
        guard case .accepted = client.dispatch(.setVolume(volume: 42)) else {
            return XCTFail("the session did not accept the volume")
        }
        wait(for: [drained], timeout: 5)
        XCTAssertFalse(offMain, "batches are drained on the main queue")
    }

    func testARefusedCommandComesBackAsTheSessionsReason() {
        guard case .rejected(.noActiveMedia, _) = client.dispatch(.togglePause) else {
            return XCTFail("toggling pause without media must be refused")
        }
    }

    func testTheSessionReadsEverySwiftEncodedCommand() {
        let missing = root.appendingPathComponent("missing.mkv").path
        // The fixture opens name media that is not there, and a URL libmpv would
        // fetch; open a missing local file instead.
        var commands = SessionProtocolTests.commands.values.filter {
            switch $0 {
            case .open, .playlist(.open): return false
            default: return true
            }
        }
        commands.append(.open(source: MediaSource(kind: .local, location: missing), resumeFrom: 3, audioTrack: 1, subtitleTrack: nil))
        commands.append(.playlist(action: .open(sources: [MediaSource(kind: .local, location: missing)])))
        for command in commands {
            switch client.dispatch(command) {
            case .rejected(let reason, let message) where reason == .malformed || reason == .unsupportedSchemaVersion:
                XCTFail("\(command) was not understood: \(message)")
            default:
                break
            }
        }
    }

    func testTheSessionAnswersTheRegistryQueriesAndInvokesItsCommands() throws {
        guard case .commandRegistry(let commands)? = client.query(.commandRegistry(fullscreen: true, miniPlayer: false)) else {
            return XCTFail("the registry query was not answered with the registry")
        }
        let fullscreen = try XCTUnwrap(commands.first { $0.id == "fullscreen" })
        XCTAssertEqual(fullscreen.label, "Exit fullscreen", "the window state passed in the query is honoured")
        XCTAssertTrue(fullscreen.checked)
        XCTAssertFalse(fullscreen.available, "a shell command this shell did not list is unavailable")
        let openFile = try XCTUnwrap(commands.first { $0.id == "open-file" })
        XCTAssertTrue(openFile.available, "the config's shell_commands reached the session")
        XCTAssertEqual(openFile.shortcut?.label, "Cmd+O")

        guard case .shortcutTable(let shortcuts)? = client.query(.shortcutTable) else {
            return XCTFail("the shortcut table query was not answered with the table")
        }
        XCTAssertEqual(shortcuts.first { $0.action == "save-screenshot" }?.command, "save-frame")

        guard case .accepted = client.dispatch(.invokeCommand(id: "volume-up")) else {
            return XCTFail("a command the session runs must be dispatched")
        }
        guard case .perform = client.dispatch(.invokeCommand(id: "open-file")) else {
            return XCTFail("a command this shell carries out must come back as perform")
        }
        guard case .rejected(.unavailable, _) = client.dispatch(.invokeCommand(id: "open-url")) else {
            return XCTFail("a command nobody carries out must be refused as unavailable")
        }
        guard case .rejected(.unknownCommand, _) = client.dispatch(.invokeCommand(id: "no-such-command")) else {
            return XCTFail("an id outside the registry must be refused as unknown")
        }
    }

    func testTheSessionKeepsTheHistoryAndReportsThePrivateSession() throws {
        guard case .history(.welcomeShelf(false, let entries))? =
            client.query(.history(query: .welcomeShelf(limit: 8, utcOffsetSeconds: 0))) else {
            return XCTFail("the shelf query was not answered with a public shelf")
        }
        XCTAssertEqual(entries, [], "a new data directory has no history")

        let reported = expectation(description: "a drained batch reports the private session")
        reported.assertForOverFulfill = false
        client.onBatch = { batch in
            if batch.events.contains(.history(event: .privateSession(enabled: true))) { reported.fulfill() }
        }
        XCTAssertEqual(client.dispatch(.history(action: .setPrivate(enabled: true))), .history(result: .done))
        XCTAssertEqual(client.dispatch(.history(action: .setPrivate(enabled: true))), .history(result: .unchanged))
        wait(for: [reported], timeout: 5)
        XCTAssertTrue(client.store.privateSession, "the drained private session reached the store")
        guard case .history(.page(true, _, _, _, _, _))? =
            client.query(.history(query: .page(utcOffsetSeconds: 0))) else {
            return XCTFail("the page does not report the private session")
        }
    }

    func testTheSessionAnswersThePlaylistQueryAndReportsItsChanges() {
        guard case .playlist(let empty)? = client.query(.playlist) else {
            return XCTFail("the playlist query was not answered with the playlist")
        }
        XCTAssertEqual(empty, PlaylistState(), "a new session has an empty playlist")

        let reported = expectation(description: "a drained batch carries the new repeat mode")
        reported.assertForOverFulfill = false
        client.onBatch = { batch in
            for case .playlistChanged(let playlist) in batch.events where playlist.repeat == .all {
                reported.fulfill()
            }
        }
        XCTAssertEqual(client.dispatch(.playlist(action: .setRepeat(mode: .all))), .applied)
        XCTAssertEqual(client.dispatch(.playlist(action: .setRepeat(mode: .all))), .noOp)
        wait(for: [reported], timeout: 5)
        XCTAssertEqual(client.store.playlist.repeat, .all, "the drained playlist reached the store")
        guard case .playlist(let playlist)? = client.query(.playlist) else {
            return XCTFail("the playlist query was not answered with the playlist")
        }
        XCTAssertEqual(playlist.repeat, .all)
    }

    func testAClosedClientRefusesCommandsWithoutTouchingTheSession() {
        client.close()
        guard case .rejected(.invalidArgument, _) = client.dispatch(.setVolume(volume: 10)) else {
            return XCTFail("a closed client must refuse commands")
        }
        XCTAssertNil(client.handle)
    }
}
