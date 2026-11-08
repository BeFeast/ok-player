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
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")])
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
        var commands = SessionProtocolTests.commands.values.filter {
            if case .open = $0 { return false }
            return true
        }
        commands.append(.open(source: MediaSource(kind: .local, location: missing), resumeFrom: 3, audioTrack: 1, subtitleTrack: nil))
        for command in commands {
            switch client.dispatch(command) {
            case .rejected(let reason, let message) where reason == .malformed || reason == .unsupportedSchemaVersion:
                XCTFail("\(command) was not understood: \(message)")
            default:
                break
            }
        }
    }

    func testAClosedClientRefusesCommandsWithoutTouchingTheSession() {
        client.close()
        guard case .rejected(.invalidArgument, _) = client.dispatch(.setVolume(volume: 10)) else {
            return XCTFail("a closed client must refuse commands")
        }
        XCTAssertNil(client.handle)
    }
}
