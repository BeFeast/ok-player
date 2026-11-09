import Foundation
import XCTest
@testable import OKPlayerShell

/// The status documents (rust/crates/okp-session/fixtures/status), read and written
/// by the Swift models.
final class StatusProtocolTests: XCTestCase {
    /// Every status fixture, by its name in `status/`.
    static let covered: Set<String> = [
        "query-status-surface.json", "query-reply-clear.json", "query-reply-loading.json",
        "query-reply-buffering.json", "query-reply-failed.json", "events.json", "enums.json",
    ]

    static let film = MediaSource(kind: .local, location: "/Users/example/Movies/film.mkv")

    func testEveryStatusFixtureIsCoveredHere() throws {
        let directory = GoldenFixtures.directory.appendingPathComponent("status")
        let names = try FileManager.default.contentsOfDirectory(atPath: directory.path).filter { $0.hasSuffix(".json") }
        XCTAssertEqual(Set(names), Self.covered, "a status fixture was added or removed without a Swift test")
    }

    func testTheStatusQueryRoundTrips() throws {
        let envelope = QueryEnvelope(query: .statusSurface)
        XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, "status/query-status-surface.json"), envelope)
        XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object("status/query-status-surface.json"))
    }

    func testEveryStatusReplyDecodesEveryField() throws {
        let card = FailureCard(
            kind: .application, title: "Playback failed",
            message: "OK Player could not open this source. You can retry or choose another.",
            detail: "OK Player could not open the media.\nPath: /Users/example/Movies/film.mkv\nReason: libmpv error -13",
            source: Self.film,
            actions: [FailureActionView(action: .retry, label: "Retry"),
                      FailureActionView(action: .openAnother, label: "Open another"),
                      FailureActionView(action: .copyDetails, label: "Copy details")])
        let expected: [String: StatusSurface] = [
            "query-reply-clear.json": .clear,
            "query-reply-loading.json": .loading(label: "Opening film.mkv"),
            "query-reply-buffering.json": .buffering(percent: 45, label: "Buffering 45%"),
            "query-reply-failed.json": .failed(card: card),
        ]
        for (name, surface) in expected {
            let reply = try GoldenFixtures.decode(QueryReply.self, "status/\(name)")
            XCTAssertEqual(reply.result, .statusSurface(surface: surface), name)
            // Written back, the surface is the fixture's: no field was dropped.
            let written = try GoldenFixtures.encodedObject(QueryReply(schemaVersion: 1, result: .statusSurface(surface: surface)))
            XCTAssertEqual(written, try GoldenFixtures.object("status/\(name)"), name)
        }
    }

    func testNoticesDecodeInOrderAndLeaveTheSnapshotAlone() throws {
        let batch = try GoldenFixtures.decode(EventBatch.self, "status/events.json")
        XCTAssertEqual(batch.events, [.notice(text: "Volume 72%"), .notice(text: "Resumed at 12:34")])
        let store = SessionStore()
        store.apply(batch)
        XCTAssertEqual(store.snapshot, SessionSnapshot())
        XCTAssertNil(store.lastError)
    }

    func testEveryFailureEnumValueMatchesTheRustList() throws {
        let lists = try XCTUnwrap(JSONSerialization.jsonObject(with: GoldenFixtures.data("status/enums.json")) as? [String: [String]])
        XCTAssertEqual(lists, [
            "failure_kind": FailureKind.allCases.map(\.rawValue),
            "failure_action": FailureAction.allCases.map(\.rawValue),
        ])
    }
}
