import Foundation
import XCTest
@testable import OKPlayerShell

/// The history views over the real okp_session_* C ABI, on the smoke's seeded
/// history (`SmokeHistory`): what the shelf and the page list, a removal that the
/// history file forgets, and the private switch.
final class HistorySessionTests: XCTestCase {
    private var root: URL!
    private var data: URL!
    private var fixture: String!
    private var client: SessionClient!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-history-\(UUID().uuidString)")
        data = root.appendingPathComponent("data")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        // A local entry is listed only while its file exists.
        let media = root.appendingPathComponent("fixture.mp4")
        XCTAssertTrue(FileManager.default.createFile(atPath: media.path, contents: Data()))
        fixture = media.path
        try SmokeHistory.seed(directory: data, fixture: fixture, now: Date())
        client = try SessionClient(config: SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: data.path,
            configDir: root.appendingPathComponent("config").path,
            cacheDir: root.appendingPathComponent("cache").path,
            hwdec: "no",
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")]))
    }

    override func tearDownWithError() throws {
        client.close()
        client = nil
        try? FileManager.default.removeItem(at: root)
    }

    private var link: HistorySessionLink {
        HistorySessionLink(query: { [unowned self] query in self.client.query(query) },
                           dispatch: { [unowned self] command in self.client.dispatch(command) })
    }

    func testTheSmokeSeedIsTheShelfTheSessionAnswers() throws {
        let shelf = ContinueWatchingModel(session: link) { _, _ in }
        shelf.refresh()
        XCTAssertNil(shelf.problem)
        XCTAssertEqual(shelf.entries.count, SmokeHistory.seededCount)
        XCTAssertEqual(shelf.entries.map(\.source), [MediaSource(kind: .local, location: fixture),
                                                     MediaSource(kind: .url, location: SmokeHistory.seededStream)],
                       "newest first")
        let card = try XCTUnwrap(shelf.entries.first)
        XCTAssertEqual(card.state, .finished)
        XCTAssertNil(card.resumeFrom, "the finished fixture opens from the start, so the smoke plays all of it")
        let stream = try XCTUnwrap(shelf.entries.last)
        XCTAssertEqual(stream.title, "Seeded talk")
        XCTAssertEqual(stream.state, .progress)
        XCTAssertEqual(stream.resumeFrom, 1325.5)
    }

    func testARemovalFromThePageIsWrittenToTheHistoryFile() throws {
        let page = HistoryPageModel(session: link, open: { _, _ in }, reveal: { _ in })
        page.refresh()
        guard case .sections(let sections) = page.content else { return XCTFail("no sections: \(page.content)") }
        XCTAssertFalse(sections.contains { $0.header.isEmpty })
        let entries = sections.flatMap(\.entries)
        XCTAssertEqual(entries.count, 2)
        let stream = try XCTUnwrap(entries.first { $0.source.kind == .url })

        page.remove(stream)
        XCTAssertNil(page.notice)
        guard case .sections(let after) = page.content else { return XCTFail("no sections: \(page.content)") }
        XCTAssertEqual(after.flatMap(\.entries).map(\.source), [MediaSource(kind: .local, location: fixture)])
        // The removal answers once its own write is done.
        let saved = try JSONSerialization.jsonObject(with: Data(contentsOf: data.appendingPathComponent("history.json")))
        let files = try XCTUnwrap((saved as? [String: Any])?["files"] as? [String: Any])
        XCTAssertEqual(Set(files.keys), [fixture])

        page.setSearch("seeded")
        XCTAssertEqual(page.content, .noMatch("seeded"))
        page.setSearch("fixture")
        guard case .sections(let found) = page.content else { return XCTFail("no match for the fixture: \(page.content)") }
        XCTAssertEqual(found.flatMap(\.entries).map(\.title), ["fixture"])
    }

    func testThePrivateSwitchReachesTheSessionAndHidesResumePositions() {
        let page = HistoryPageModel(session: link, open: { _, _ in }, reveal: { _ in })
        let shelf = ContinueWatchingModel(session: link) { _, _ in }
        page.refresh()
        XCTAssertFalse(page.privateSession)

        page.setPrivate(true)
        XCTAssertNil(page.notice)
        XCTAssertTrue(page.privateSession)
        shelf.refresh()
        XCTAssertTrue(shelf.privateSession)
        XCTAssertEqual(shelf.entries.count, SmokeHistory.seededCount, "the history stays readable")
        XCTAssertTrue(shelf.entries.allSatisfy { $0.resumeFrom == nil }, "nothing remembered is applied")

        page.setPrivate(false)
        XCTAssertFalse(page.privateSession)
        shelf.refresh()
        XCTAssertEqual(shelf.entries.last?.resumeFrom, 1325.5)
    }
}
