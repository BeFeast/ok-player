import Foundation
import XCTest
@testable import OKPlayerShell

/// The window queries over the real okp_session_* C ABI: the window fit and the
/// click rules the shell uses are okp-core's answers, and the Swift models read
/// and write the golden documents Rust pins.
final class WindowSessionQueryTests: XCTestCase {
    /// The golden documents of the window queries.
    static let fixtures: Set<String> = [
        "query-window-fit.json", "query-reply-window-fit.json",
        "query-video-click.json", "query-reply-video-click.json",
    ]

    private var root: URL!
    private var client: SessionClient!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-window-query-\(UUID().uuidString)")
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

    // MARK: Core fit result to window frame

    func testA4KVideoOnARetinaLaptopGetsCoresFrame() {
        let frame = client.windowFit(video: VideoGeometry(width: 3840, height: 2160, displayAspect: 16 / 9),
                                     backingScale: 2, visibleFrame: CGRect(x: 0, y: 0, width: 1512, height: 945),
                                     minSize: CGSize(width: 420, height: 280))
        XCTAssertEqual(frame, CGRect(x: 45, y: 73, width: 1421, height: 799))
    }

    func testAVideoIsShownAtOneVideoPixelPerDevicePixelAndNeverBelowTheMinimum() {
        let laptop = CGRect(x: 0, y: 0, width: 1512, height: 945)
        let minimum = CGSize(width: 420, height: 280)
        XCTAssertEqual(client.windowFit(video: VideoGeometry(width: 1280, height: 720, displayAspect: 16 / 9),
                                        backingScale: 2, visibleFrame: laptop, minSize: minimum),
                       CGRect(x: 436, y: 292, width: 640, height: 360))
        // 320x180 points would be below the window's minimum: it grows with the aspect.
        XCTAssertEqual(client.windowFit(video: VideoGeometry(width: 640, height: 360, displayAspect: 16 / 9),
                                        backingScale: 2, visibleFrame: laptop, minSize: minimum),
                       CGRect(x: 507, y: 332, width: 498, height: 280))
    }

    func testTheFrameStaysOnAScreenLeftOfTheMainOneAboveTheDock() throws {
        let screen = CGRect(x: -1920, y: 70, width: 1920, height: 1010)
        let frame = try XCTUnwrap(client.windowFit(video: VideoGeometry(width: 1920, height: 1080, displayAspect: 16 / 9),
                                                   backingScale: 1, visibleFrame: screen,
                                                   minSize: CGSize(width: 420, height: 280)))
        XCTAssertTrue(screen.contains(frame), "\(frame) is not inside \(screen)")
    }

    func testNothingIsFittedWithoutAVideoSize() {
        XCTAssertNil(client.windowFit(video: VideoGeometry(width: 0, height: 0, displayAspect: 0), backingScale: 2,
                                      visibleFrame: CGRect(x: 0, y: 0, width: 1512, height: 945),
                                      minSize: CGSize(width: 420, height: 280)))
    }

    // MARK: Single click versus double click

    func testTheSessionClassifiesClicksWithCoresRules() {
        XCTAssertEqual(client.videoClickIntent(clickCount: 1), .schedulePlayPause)
        XCTAssertEqual(client.videoClickIntent(clickCount: 2), .cancelPlayPauseAndToggleFullscreen)
        XCTAssertEqual(client.videoClickIntent(clickCount: 3), .ignore)
        XCTAssertEqual(client.videoClickIntent(clickCount: Int.max), .ignore)
        XCTAssertEqual((1...3).map(WindowBehaviourTests.coreRules),
                       (1...3).map { client.videoClickIntent(clickCount: $0) },
                       "the hand-written rules in WindowBehaviourTests match the session's")
    }

    func testADoubleClickClassifiedByTheSessionTogglesFullscreenWithoutPausing() {
        var timers: [DispatchWorkItem] = []
        var pauses = 0
        var fullscreens = 0
        let client = self.client!
        let arbiter = VideoClickArbiter(interval: { 0.35 },
                                        classify: { client.videoClickIntent(clickCount: $0) }) { _, body in
            let item = DispatchWorkItem(block: body)
            timers.append(item)
            return item
        }
        arbiter.onTogglePause = { pauses += 1 }
        arbiter.onToggleFullscreen = { fullscreens += 1 }
        arbiter.released(clickCount: 1)
        arbiter.released(clickCount: 2)
        for item in timers where !item.isCancelled { item.perform() }
        XCTAssertEqual(fullscreens, 1)
        XCTAssertEqual(pauses, 0)

        arbiter.released(clickCount: 1)
        for item in timers where !item.isCancelled { item.perform() }
        XCTAssertEqual(pauses, 1, "a lone click still toggles pause")
    }

    // MARK: Golden documents

    func testTheWindowFitQueryAndReplyRoundTrip() throws {
        let query = QueryEnvelope(query: .windowFit(video: WindowSize(width: 3840, height: 2160), backingScale: 2,
                                                    visibleFrame: WindowRect(x: 0, y: 0, width: 1512, height: 945),
                                                    minSize: WindowSize(width: 420, height: 280)))
        XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, "query-window-fit.json"), query)
        XCTAssertEqual(try GoldenFixtures.encodedObject(query), try GoldenFixtures.object("query-window-fit.json"))
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-window-fit.json")
        XCTAssertEqual(reply.result, .windowFit(frame: WindowRect(x: 45, y: 73, width: 1421, height: 799)))
    }

    func testTheVideoClickQueryAndReplyRoundTrip() throws {
        let query = QueryEnvelope(query: .videoClick(pressCount: 2))
        XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, "query-video-click.json"), query)
        XCTAssertEqual(try GoldenFixtures.encodedObject(query), try GoldenFixtures.object("query-video-click.json"))
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-video-click.json")
        XCTAssertEqual(reply.result, .videoClick(intent: .cancelPlayPauseAndToggleFullscreen))
    }

    func testAFitWithNothingToFitDecodesAsNoFrame() throws {
        let text = #"{"schema_version":1,"result":{"window_fit":{"frame":null}}}"#
        let reply = try SessionJSON.makeDecoder().decode(QueryReply.self, from: Data(text.utf8))
        XCTAssertEqual(reply.result, .windowFit(frame: nil))
    }
}
