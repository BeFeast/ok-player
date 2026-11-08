import Foundation
import XCTest
@testable import OKPlayerShell

/// The transport documents (rust/crates/okp-session/fixtures/transport), read and
/// written by the Swift models.
final class TransportProtocolTests: XCTestCase {
    private static let covered: Set<String> = [
        "query-transport.json", "query-transport-at.json", "query-reply-transport.json", "command-nudge-volume.json",
        "command-toggle-mute.json",
    ]

    func testEveryTransportFixtureIsCoveredHere() throws {
        let directory = GoldenFixtures.directory.appendingPathComponent("transport")
        let names = try FileManager.default.contentsOfDirectory(atPath: directory.path).filter { $0.hasSuffix(".json") }
        XCTAssertEqual(Set(names), Self.covered, "a transport fixture was added or removed without a Swift test")
    }

    func testTheTransportQueriesRoundTrip() throws {
        for (name, query) in [("query-transport.json", Query.transport(at: nil)),
                              ("query-transport-at.json", Query.transport(at: 95.5))] {
            let path = "transport/\(name)"
            let envelope = QueryEnvelope(query: query)
            XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, path), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(path), name)
        }
    }

    func testTheVolumeCommandsRoundTrip() throws {
        for (name, command) in [("command-nudge-volume.json", Command.nudgeVolume(delta: -2)),
                                ("command-toggle-mute.json", Command.toggleMute)] {
            let path = "transport/\(name)"
            let envelope = CommandEnvelope(command: command)
            XCTAssertEqual(try GoldenFixtures.decode(CommandEnvelope.self, path), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(path), name)
        }
    }

    func testTheTransportReplyDecodesEveryField() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "transport/query-reply-transport.json")
        let shown = OscSurface(visible: true, mapped: true, focusable: true, hitTestable: true)
        let hidden = OscSurface(visible: true, mapped: true, focusable: false, hitTestable: false)
        let speeds: [SpeedChoice] = [
            SpeedChoice(speed: 0.5, label: "0.50×", selected: false),
            SpeedChoice(speed: 0.75, label: "0.75×", selected: false),
            SpeedChoice(speed: 1.0, label: "1.00×", selected: false),
            SpeedChoice(speed: 1.25, label: "1.25×", selected: true),
            SpeedChoice(speed: 1.5, label: "1.50×", selected: false),
            SpeedChoice(speed: 2.0, label: "2.00×", selected: false),
        ]
        let expected = TransportView(
            elapsed: "16:40", remaining: "-50:00", total: "01:06:40",
            progress: 0.25, buffered: 0.4, chapterTicks: [0, 0.5, 0.75], seekable: true,
            volume: VolumeView(level: 117, max: 130, fraction: 0.9, unityFraction: 100.0 / 130.0,
                               muted: false, boosted: true, readout: "117%"),
            speed: "1.25×", speeds: speeds,
            osc: OscView(revealed: shown, hidden: hidden, autoHide: true, hideAfterMs: 2500))
        XCTAssertEqual(reply.result, .transport(transport: expected))
        XCTAssertEqual(expected.osc.surface(revealed: false), hidden)
        XCTAssertEqual(expected.osc.hideAfter, 2.5)
    }
}
