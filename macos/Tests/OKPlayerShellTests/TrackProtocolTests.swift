import Foundation
import XCTest
@testable import OKPlayerShell

/// The track documents (rust/crates/okp-session/fixtures/tracks), read and written
/// by the Swift models.
final class TrackProtocolTests: XCTestCase {
    /// Every track fixture, by its name in `tracks/`.
    static let covered: Set<String> = [
        "command-add-subtitle.json", "command-set-subtitle-scale.json", "command-nudge-subtitle-scale.json",
        "query-tracks.json", "query-reply-tracks.json",
    ]

    static let commands: [String: Command] = [
        "command-add-subtitle.json": .addSubtitle(path: "/Users/example/Movies/film.en.srt"),
        "command-set-subtitle-scale.json": .setSubtitleScale(scale: 1.2),
        "command-nudge-subtitle-scale.json": .nudgeSubtitleScale(delta: -0.1),
    ]

    /// The view the reply fixture carries.
    static func fixtureView() throws -> TracksView {
        let reply = try GoldenFixtures.decode(QueryReply.self, "tracks/query-reply-tracks.json")
        guard case .tracks(let view) = reply.result else { throw NotATracksReply() }
        return view
    }

    private struct NotATracksReply: Error {}

    func testEveryTrackFixtureIsCoveredHere() throws {
        let directory = GoldenFixtures.directory.appendingPathComponent("tracks")
        let names = try FileManager.default.contentsOfDirectory(atPath: directory.path).filter { $0.hasSuffix(".json") }
        XCTAssertEqual(Set(names), Self.covered, "a track fixture was added or removed without a Swift test")
    }

    func testTheTrackCommandsRoundTrip() throws {
        for (name, command) in Self.commands {
            let path = "tracks/\(name)"
            let envelope = CommandEnvelope(command: command)
            XCTAssertEqual(try GoldenFixtures.decode(CommandEnvelope.self, path), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(path), name)
        }
    }

    func testTheTracksQueryRoundTrips() throws {
        let envelope = QueryEnvelope(query: .tracks)
        XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, "tracks/query-tracks.json"), envelope)
        XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object("tracks/query-tracks.json"))
    }

    func testTheTracksReplyDecodesEveryField() throws {
        let name = "tracks/query-reply-tracks.json"
        let reply = try GoldenFixtures.decode(QueryReply.self, name)
        let expected = TracksView(
            audio: [
                TrackChoice(id: 1, label: "English · 5.1 · EAC3", selected: true),
                TrackChoice(id: 2, label: "Director's Commentary · ENG · 2.0 · AC3", selected: false),
            ],
            subtitles: [
                TrackChoice(id: nil, label: "Off", selected: false),
                TrackChoice(id: 1, label: "English SDH · PGS · Image · Default", selected: false),
                TrackChoice(id: 2, label: "en · SRT · EXT", selected: true),
            ],
            subtitleDelay: SubtitleValue(value: 0.25, label: "+250 ms", reported: 0.25),
            subtitleScale: SubtitleValue(value: 1.1, label: "110%", reported: 1.0),
            adjustable: true)
        XCTAssertEqual(reply.result, .tracks(tracks: expected))
        // Written back, it is the fixture; Off's `null` id is left out, which Rust reads the same way.
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object(name))
    }
}
