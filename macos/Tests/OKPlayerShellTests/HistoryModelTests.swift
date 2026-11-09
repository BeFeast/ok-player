import AppKit
import XCTest
@testable import OKPlayerShell

/// A session for the history views: it answers each history query with what the
/// test sets and records every history query and action it is sent.
final class RecordingHistorySession {
    var answer: (HistoryQuery) -> QueryResult? = { _ in nil }
    var reply: (HistoryAction) -> DispatchResult = { _ in .history(result: .done) }
    private(set) var queries: [HistoryQuery] = []
    private(set) var actions: [HistoryAction] = []

    var link: HistorySessionLink {
        HistorySessionLink(
            query: { [unowned self] query in
                guard case .history(let history) = query else {
                    return .rejected(reason: .malformed, message: "not a history query: \(query)")
                }
                self.queries.append(history)
                return self.answer(history)
            },
            dispatch: { [unowned self] command in
                guard case .history(let action) = command else {
                    return .rejected(reason: .malformed, message: "not a history command: \(command)")
                }
                self.actions.append(action)
                return self.reply(action)
            },
            utcOffsetSeconds: { 7200 })
    }

    func count(_ kind: (HistoryQuery) -> Bool) -> Int {
        queries.filter(kind).count
    }
}

/// The history query answers in rust/crates/okp-session/fixtures/history.
enum HistoryFixtures {
    static func result(_ name: String) throws -> QueryResult {
        try GoldenFixtures.decode(QueryReply.self, name).result
    }

    static func shelfReply() throws -> QueryResult { try result("history/query-reply-welcome-shelf.json") }

    static func pageReply() throws -> QueryResult { try result("history/query-reply-page.json") }

    struct Mismatch: Error, CustomStringConvertible {
        let description: String
    }

    /// The shelf fixture's entry: a local file in progress.
    static func localEntry() throws -> HistoryEntry {
        guard case .history(.welcomeShelf(_, let entries)) = try shelfReply(), let entry = entries.first else {
            throw Mismatch(description: "the shelf fixture lists no entry")
        }
        return entry
    }

    /// The page fixture's sections: one finished stream, earlier.
    static func pageSections() throws -> [HistorySection] {
        guard case .history(.page(_, _, _, _, _, let sections)) = try pageReply() else {
            throw Mismatch(description: "the page fixture is not a page")
        }
        return sections
    }

    static func page(privateSession: Bool = false, sections: [HistorySection] = []) -> QueryResult {
        .history(result: .page(privateSession: privateSession, readFailed: false, unreadableCopy: nil,
                               summary: "Everything you’ve opened · kept forever",
                               endCap: "End of history · kept forever", sections: sections))
    }
}

/// The Continue watching shelf, the History page and the islands' refresh rules,
/// over a recording session fed with the golden history documents.
final class HistoryModelTests: XCTestCase {
    private struct Opened: Equatable {
        var source: MediaSource
        var resumeFrom: Double?
    }

    private var session: RecordingHistorySession!
    private var opened: [Opened] = []
    private var revealed: [URL] = []

    override func setUp() {
        session = RecordingHistorySession()
        opened = []
        revealed = []
    }

    private func shelfModel() -> ContinueWatchingModel {
        ContinueWatchingModel(session: session.link) { [unowned self] source, resumeFrom in
            self.opened.append(Opened(source: source, resumeFrom: resumeFrom))
        }
    }

    private func pageModel() -> HistoryPageModel {
        HistoryPageModel(session: session.link,
                         open: { [unowned self] source, resumeFrom in
                             self.opened.append(Opened(source: source, resumeFrom: resumeFrom))
                         },
                         reveal: { [unowned self] url in self.revealed.append(url) })
    }

    // MARK: Continue watching

    func testTheShelfListsTheSessionsWelcomeShelfAnswer() throws {
        let shelf = try HistoryFixtures.shelfReply()
        session.answer = { _ in shelf }
        let model = shelfModel()
        XCTAssertTrue(model.isFirstRun, "nothing is listed before the session answers")
        model.refresh()
        XCTAssertEqual(session.queries, [.welcomeShelf(limit: 3, utcOffsetSeconds: 7200)])
        XCTAssertEqual(model.entries.map(\.title), ["Severance S02E03"])
        XCTAssertEqual(model.entries.first?.stateLabel, "30m left")
        XCTAssertEqual(model.entries.first?.whenLabel, "Today 21:14")
        XCTAssertFalse(model.isFirstRun)
        XCTAssertFalse(model.privateSession)
        XCTAssertNil(model.problem)
    }

    func testAClickOnACardOpensItAndLeavesThePositionToTheSession() throws {
        let shelf = try HistoryFixtures.shelfReply()
        session.answer = { _ in shelf }
        let model = shelfModel()
        model.refresh()
        let card = try XCTUnwrap(model.entries.first)
        model.resume(card)
        XCTAssertEqual(opened, [Opened(source: MediaSource(kind: .local,
                                                           location: "/Users/example/Movies/Severance/Season 02/Severance S02E03.mkv"),
                                       resumeFrom: nil)])
        XCTAssertTrue(session.actions.isEmpty, "opening is not a history action")
    }

    func testAnEmptyOrUnreadableShelfShowsTheFirstRunCard() {
        let model = shelfModel()
        session.answer = { _ in .history(result: .welcomeShelf(privateSession: true, entries: [])) }
        model.refresh()
        XCTAssertTrue(model.isFirstRun)
        XCTAssertTrue(model.privateSession)
        XCTAssertNil(model.problem)

        session.answer = { _ in .rejected(reason: .invalidArgument, message: "the session is closed") }
        model.refresh()
        XCTAssertTrue(model.isFirstRun)
        XCTAssertEqual(model.problem, "the session is closed")

        session.answer = { _ in nil }
        model.refresh()
        XCTAssertEqual(model.problem, "There is no playback session.")
    }

    // MARK: History page

    func testTheHistoryPageLoadsThenShowsTheSessionsDayGroups() throws {
        let page = try HistoryFixtures.pageReply()
        session.answer = { _ in page }
        let model = pageModel()
        XCTAssertEqual(model.content, .loading)
        XCTAssertTrue(session.queries.isEmpty)

        model.refresh()
        XCTAssertEqual(session.queries, [.page(search: "", utcOffsetSeconds: 7200)])
        let sections = try HistoryFixtures.pageSections()
        XCTAssertEqual(model.content, .sections(sections))
        XCTAssertEqual(sections.map(\.header), ["EARLIER"])
        XCTAssertEqual(model.summary, "Everything you’ve opened · kept forever")
        XCTAssertEqual(model.endCap, "End of history · kept forever")
        XCTAssertTrue(model.privateSession, "the fixture's session is private, so the banner shows")
        XCTAssertNil(model.readFailure)
        XCTAssertNil(model.notice)
    }

    func testEverySearchChangeAsksTheSessionWithTheNewText() throws {
        let page = try HistoryFixtures.pageReply()
        session.answer = { _ in page }
        let model = pageModel()
        model.refresh()
        model.setSearch("sev")
        model.setSearch("severance")
        model.setSearch("severance")
        XCTAssertEqual(session.queries, [
            .page(search: "", utcOffsetSeconds: 7200),
            .page(search: "sev", utcOffsetSeconds: 7200),
            .page(search: "severance", utcOffsetSeconds: 7200),
        ], "the session filters; an unchanged search is not asked again")
        XCTAssertEqual(model.search, "severance")
    }

    func testNothingRememberedIsEmptyAndNothingFoundIsNoMatch() {
        session.answer = { _ in HistoryFixtures.page() }
        let model = pageModel()
        model.refresh()
        XCTAssertEqual(model.content, .empty)
        model.setSearch("  zebra ")
        XCTAssertEqual(model.content, .noMatch("zebra"))
        model.setSearch("   ")
        XCTAssertEqual(model.content, .empty)
    }

    func testAFailedPageShowsTheErrorAndRetryAsksAgain() throws {
        session.answer = { _ in .rejected(reason: .malformed, message: "unknown field `page`") }
        let model = pageModel()
        model.refresh()
        XCTAssertEqual(model.content, .failed("unknown field `page`"))

        let page = try HistoryFixtures.pageReply()
        session.answer = { _ in page }
        model.retry()
        XCTAssertEqual(session.queries.count, 2)
        XCTAssertEqual(model.content, .sections(try HistoryFixtures.pageSections()))
    }

    func testAnUnreadableHistoryFileIsReportedWithWhereItWasKept() {
        let kept = "/Users/me/Library/Application Support/uk.oklabs.ok-player/history.json.unreadable-1700000000"
        session.answer = { _ in
            .history(result: .page(privateSession: false, readFailed: true, unreadableCopy: kept,
                                   summary: "", endCap: "", sections: []))
        }
        let model = pageModel()
        model.refresh()
        XCTAssertEqual(model.content, .empty)
        let failure = model.readFailure
        XCTAssertTrue(failure?.hasSuffix("The old file was kept as \(kept).") == true, String(describing: failure))
    }

    func testRemoveSendsTheEntrysSourceAndListsTheSessionsAnswer() throws {
        // A session that forgets what it is asked to remove.
        var remembered = try HistoryFixtures.pageSections().flatMap(\.entries)
        session.answer = { _ in
            HistoryFixtures.page(sections: remembered.isEmpty
                                     ? [] : [HistorySection(bucket: .earlier, header: "EARLIER", entries: remembered)])
        }
        session.reply = { action in
            guard case .remove(let source) = action else { return .history(result: .unchanged) }
            remembered.removeAll { $0.source == source }
            return .history(result: .done)
        }
        let model = pageModel()
        model.refresh()
        let entry = try XCTUnwrap(remembered.first)
        model.remove(entry)
        XCTAssertEqual(session.actions, [.remove(source: MediaSource(kind: .url, location: "https://example.com/watch?v=abc123"))])
        XCTAssertEqual(session.queries.count, 2, "the page is asked for again after the removal")
        XCTAssertEqual(model.content, .empty)
        XCTAssertNil(model.notice)
    }

    func testARemovalTheSessionCouldNotSaveSaysSoAndKeepsTheRow() throws {
        let page = try HistoryFixtures.pageReply()
        session.answer = { _ in page }
        session.reply = { _ in .history(result: .failed(message: "the history could not be saved: Permission denied")) }
        let model = pageModel()
        model.refresh()
        let entry = try XCTUnwrap(HistoryFixtures.pageSections().first?.entries.first)
        model.remove(entry)
        XCTAssertEqual(model.notice,
                       "“Example talk” was not removed from history: the history could not be saved: Permission denied")
        XCTAssertEqual(model.content, .sections(try HistoryFixtures.pageSections()))
    }

    func testThePrivateSwitchTellsTheSessionAndTheBannerFollowsItsAnswer() {
        // A session that keeps the private flag it is sent.
        var privateSession = false
        session.answer = { _ in HistoryFixtures.page(privateSession: privateSession) }
        session.reply = { action in
            guard case .setPrivate(let enabled) = action else { return .history(result: .done) }
            defer { privateSession = enabled }
            return .history(result: enabled == privateSession ? .unchanged : .done)
        }
        let model = pageModel()
        model.refresh()
        XCTAssertFalse(model.privateSession)

        model.setPrivate(true)
        XCTAssertEqual(session.actions, [.setPrivate(enabled: true)])
        XCTAssertTrue(model.privateSession, "the banner shows")

        model.setPrivate(false)
        XCTAssertEqual(session.actions, [.setPrivate(enabled: true), .setPrivate(enabled: false)])
        XCTAssertFalse(model.privateSession, "the banner goes")
        XCTAssertNil(model.notice)

        session.reply = { _ in .rejected(reason: .invalidArgument, message: "the session is closed") }
        model.setPrivate(true)
        XCTAssertEqual(model.notice, "The private session was not turned on: the session is closed")
        XCTAssertFalse(model.privateSession)
    }

    func testRowsResumePlayFromStartAndRevealOnlyLocalFiles() throws {
        let local = try HistoryFixtures.localEntry()
        let stream = try XCTUnwrap(HistoryFixtures.pageSections().first?.entries.first)
        let model = pageModel()
        model.open(local, .resume)
        model.open(local, .fromStart)
        XCTAssertEqual(opened, [Opened(source: local.source, resumeFrom: nil), Opened(source: local.source, resumeFrom: 0)])

        XCTAssertTrue(model.canReveal(local))
        XCTAssertFalse(model.canReveal(stream), "a stream has no place in Finder")
        model.reveal(stream)
        model.reveal(local)
        XCTAssertEqual(revealed, [URL(fileURLWithPath: "/Users/example/Movies/Severance/Season 02/Severance S02E03.mkv")])
        XCTAssertTrue(session.actions.isEmpty)
    }

    // MARK: Islands

    func testTheWelcomeViewIsDueOnlyWhileNothingIsLoadedReadOrFailed() {
        let idle = SessionSnapshot()
        var playing = SessionSnapshot()
        playing.status = .playing
        playing.source = MediaSource(kind: .local, location: "/Users/me/Movies/Film.mkv")
        var ended = playing
        ended.status = .ended
        XCTAssertTrue(HistoryIntegration.welcomeDue(idle, scanning: false, errorShown: false))
        XCTAssertFalse(HistoryIntegration.welcomeDue(playing, scanning: false, errorShown: false))
        XCTAssertFalse(HistoryIntegration.welcomeDue(ended, scanning: false, errorShown: false), "the last frame stays")
        XCTAssertFalse(HistoryIntegration.welcomeDue(idle, scanning: true, errorShown: false), "a folder is being read")
        XCTAssertFalse(HistoryIntegration.welcomeDue(idle, scanning: false, errorShown: true), "the error stays up")
    }

    func testHistoryEventsRefreshOnlyWhatIsOnScreen() throws {
        let shelf = try HistoryFixtures.shelfReply()
        let page = try HistoryFixtures.pageReply()
        session.answer = { query in
            switch query {
            case .welcomeShelf: return shelf
            case .page: return page
            }
        }
        let history = HistoryIntegration(session: session.link, open: { _, _ in }, openFile: {}, reveal: { _ in })
        let shelfQueries = { self.session.count { if case .welcomeShelf = $0 { return true } else { return false } } }
        let pageQueries = { self.session.count { if case .page = $0 { return true } else { return false } } }
        let changed = EventBatch(schemaVersion: 1, events: [.history(event: .changed)])
        let unrelated = EventBatch(schemaVersion: 1, events: [.snapshotChanged(changes: [.volume(value: 40)]),
                                                              .history(event: .resumed(position: 3))])

        XCTAssertTrue(history.welcomeView.isHidden)
        history.sessionChanged(changed)
        XCTAssertEqual(session.queries, [], "nothing is on screen")

        history.setWelcomeShown(true)
        XCTAssertFalse(history.welcomeView.isHidden)
        XCTAssertEqual(shelfQueries(), 1, "a shelf that comes into view is read")
        XCTAssertEqual(history.welcome.entries.count, 1)
        history.setWelcomeShown(true)
        history.sessionChanged(unrelated)
        XCTAssertEqual(shelfQueries(), 1)
        history.sessionChanged(changed)
        XCTAssertEqual(shelfQueries(), 2)

        history.setWelcomeShown(false)
        XCTAssertTrue(history.welcomeView.isHidden)
        history.sessionChanged(changed)
        XCTAssertEqual(shelfQueries(), 2, "a hidden shelf is not read")

        history.historyShown()
        XCTAssertEqual(pageQueries(), 1)
        history.sessionChanged(EventBatch(schemaVersion: 1, events: [.history(event: .privateSession(enabled: true))]))
        XCTAssertEqual(pageQueries(), 2, "the private switch changes the page's banner")
        XCTAssertEqual(shelfQueries(), 2)
    }

    func testTheHistoryMenuItemIsCommandY() {
        let history = HistoryIntegration(session: session.link, open: { _, _ in }, openFile: {}, reveal: { _ in })
        let item = history.makeMenuItem()
        XCTAssertEqual(item.title, "History…")
        XCTAssertEqual(item.keyEquivalent, "y")
        XCTAssertEqual(item.keyEquivalentModifierMask, .command)
        XCTAssertTrue(item.target === history)
        XCTAssertEqual(item.action, #selector(HistoryIntegration.showHistoryFromMenu(_:)))
    }
}
