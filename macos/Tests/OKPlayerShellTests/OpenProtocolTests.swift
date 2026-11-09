import Foundation
import XCTest
@testable import OKPlayerShell

/// The open documents (rust/crates/okp-session/fixtures/open), read and written by
/// the Swift models.
final class OpenProtocolTests: XCTestCase {
    /// Every open fixture, by its name in `open/`.
    static let covered: Set<String> = [
        "query-open-formats.json", "query-reply-open-formats.json", "query-open-paths.json",
        "query-reply-open-paths-open.json", "query-reply-open-paths-add-subtitles.json",
        "query-reply-open-paths-nothing.json",
    ]

    func testEveryOpenFixtureIsCoveredHere() throws {
        let directory = GoldenFixtures.directory.appendingPathComponent("open")
        let names = try FileManager.default.contentsOfDirectory(atPath: directory.path).filter { $0.hasSuffix(".json") }
        XCTAssertEqual(Set(names), Self.covered, "an open fixture was added or removed without a Swift test")
    }

    func testTheOpenQueriesRoundTrip() throws {
        let dropped = ["/Users/example/Movies/Show", "/Users/example/Movies/film.mkv",
                       "/Users/example/Movies/film.en.srt", "/Users/example/Music/queue.m3u8",
                       "/Users/example/Documents/notes.pdf"]
        let opened = Query.openPaths(paths: dropped, folders: ["/Users/example/Movies/Show"])
        for (name, query) in [("query-open-formats.json", Query.openFormats), ("query-open-paths.json", opened)] {
            let path = "open/\(name)"
            let envelope = QueryEnvelope(query: query)
            XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, path), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(path), name)
        }
    }

    func testThePlanRepliesDecodeToEveryPlan() throws {
        let movies = "/Users/example/Movies"
        let plans: [String: OpenPlan] = [
            "query-reply-open-paths-open.json": .open(sources: [
                MediaSource(kind: .local, location: "\(movies)/Show"),
                MediaSource(kind: .local, location: "\(movies)/film.mkv"),
                MediaSource(kind: .local, location: "/Users/example/Music/queue.m3u8"),
            ]),
            "query-reply-open-paths-add-subtitles.json": .addSubtitles(paths: ["\(movies)/film.en.srt"]),
            "query-reply-open-paths-nothing.json": .nothing,
        ]
        for (name, plan) in plans {
            let path = "open/\(name)"
            let reply = try GoldenFixtures.decode(QueryReply.self, path)
            XCTAssertEqual(reply.result, .openPaths(plan: plan), name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object(path), name)
        }
    }

    func testTheFormatsReplyListsTheCoreTypesByKind() throws {
        let path = "open/query-reply-open-formats.json"
        let reply = try GoldenFixtures.decode(QueryReply.self, path)
        guard case .openFormats(let formats) = reply.result else { return XCTFail("not an open_formats reply") }
        for ext in ["mkv", "webm", "mp4"] { XCTAssertTrue(formats.video.contains(ext), ext) }
        for ext in ["mp3", "flac"] { XCTAssertTrue(formats.audio.contains(ext), ext) }
        XCTAssertTrue(formats.subtitle.contains("srt"))
        XCTAssertEqual(formats.playlist, ["m3u", "m3u8"])
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object(path))
    }
}
