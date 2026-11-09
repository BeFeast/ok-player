import Foundation
import Observation

// The view models of the Welcome view's Continue watching shelf and of the History
// window. Everything they list comes from the session's history queries
// (docs/session-abi.md, "Watch history"): the shelf selection, the day buckets, the
// search, the labels and the resume positions are the core's. The models only ask,
// hold the answer for the SwiftUI views and turn clicks into session commands.

/// The session calls the history views make, as `SessionClient` answers them.
struct HistorySessionLink {
    var query: (Query) -> QueryResult?
    var dispatch: (Command) -> DispatchResult
    /// The shell's current offset from UTC; the session's days and `when` labels
    /// are local to it.
    var utcOffsetSeconds: () -> Int32 = { Int32(TimeZone.current.secondsFromGMT()) }

    /// One history query: the session's answer, or why there is none.
    func history(_ query: HistoryQuery) -> Result<HistoryQueryResult, HistoryProblem> {
        switch self.query(.history(query: query)) {
        case .history(let result)?:
            return .success(result)
        case .rejected(_, let message)?:
            return .failure(HistoryProblem(message: message))
        case nil:
            return .failure(HistoryProblem(message: "There is no playback session."))
        case let other?:
            return .failure(HistoryProblem(message: "The session answered the history query with \(other)."))
        }
    }
}

struct HistoryProblem: Error, Equatable {
    var message: String
}

/// How a remembered entry is opened.
enum HistoryOpening: Equatable {
    /// As the session's resume rule says: from the remembered position when the rule
    /// allows it (never in a private session), else from the start.
    case resume
    /// From the start, whatever is remembered.
    case fromStart

    /// The open's `resume_from`: `nil` leaves the position to the session, and a
    /// `resume_from` in the open wins over the remembered one.
    var resumeFrom: Double? {
        switch self {
        case .resume: return nil
        case .fromStart: return 0
        }
    }
}

extension HistoryEntry: Identifiable {
    /// The source is the entry's identity in the history file.
    var id: String { "\(source.kind.rawValue):\(source.location)" }
}

/// The Welcome view's Continue watching shelf: the session's `welcome_shelf` answer.
/// With nothing listed the view shows the first-run card instead.
@Observable
final class ContinueWatchingModel {
    /// How many cards the shelf asks for, as on Linux.
    static let shelfLimit: UInt32 = 3

    private(set) var entries: [HistoryEntry] = []
    private(set) var privateSession = false
    /// Why the shelf could not be read; the first-run card shows meanwhile.
    private(set) var problem: String?

    private let session: HistorySessionLink
    private let opener: (MediaSource, Double?) -> Void
    /// The Open button.
    @ObservationIgnored var onOpenFile: () -> Void = {}
    /// The History button beside the cards.
    @ObservationIgnored var onShowHistory: () -> Void = {}

    /// `open` opens a source with that `resume_from`.
    init(session: HistorySessionLink, open: @escaping (MediaSource, Double?) -> Void) {
        self.session = session
        opener = open
    }

    /// Nothing to continue: the first-run card.
    var isFirstRun: Bool { entries.isEmpty }

    /// Ask the session for the shelf again.
    func refresh() {
        let query = HistoryQuery.welcomeShelf(limit: Self.shelfLimit, utcOffsetSeconds: session.utcOffsetSeconds())
        switch session.history(query) {
        case .success(.welcomeShelf(let privateSession, let entries)):
            self.entries = entries
            self.privateSession = privateSession
            problem = nil
        case .success(let other):
            entries = []
            problem = "The session answered the shelf with \(other)."
        case .failure(let failure):
            entries = []
            problem = failure.message
        }
    }

    /// A click on a card.
    func resume(_ entry: HistoryEntry) {
        opener(entry.source, HistoryOpening.resume.resumeFrom)
    }

    func openFile() { onOpenFile() }

    func showHistory() { onShowHistory() }
}

/// The History window: the session's `page` answer for the search field's text.
@Observable
final class HistoryPageModel {
    enum Content: Equatable {
        /// Nothing has been answered yet.
        case loading
        /// The query could not be answered; Retry asks again.
        case failed(String)
        /// Nothing is remembered.
        case empty
        /// Nothing matches the search (trimmed).
        case noMatch(String)
        /// The session's day groups, newest first.
        case sections([HistorySection])
    }

    /// The search field's text. Each change asks the session again.
    private(set) var search = ""
    private(set) var content = Content.loading
    private(set) var privateSession = false
    /// The line under the title, from the session.
    private(set) var summary = ""
    /// The line after the last entry, from the session.
    private(set) var endCap = ""
    /// Set while the session could not read the history file it started with.
    private(set) var readFailure: String?
    /// What the last action could not do.
    private(set) var notice: String?

    private let session: HistorySessionLink
    private let opener: (MediaSource, Double?) -> Void
    private let revealer: (URL) -> Void

    /// `open` opens a source with that `resume_from`; `reveal` shows a local file in
    /// Finder.
    init(session: HistorySessionLink, open: @escaping (MediaSource, Double?) -> Void,
         reveal: @escaping (URL) -> Void) {
        self.session = session
        opener = open
        revealer = reveal
    }

    func setSearch(_ text: String) {
        guard text != search else { return }
        search = text
        refresh()
    }

    /// Ask the session for the page again, for the current search.
    func refresh() {
        let search = self.search
        let query = HistoryQuery.page(search: search, utcOffsetSeconds: session.utcOffsetSeconds())
        switch session.history(query) {
        case .success(.page(let privateSession, let readFailed, let unreadableCopy, let summary, let endCap, let sections)):
            self.privateSession = privateSession
            self.summary = summary
            self.endCap = endCap
            readFailure = readFailed ? Self.readFailureText(unreadableCopy: unreadableCopy) : nil
            let trimmed = search.trimmingCharacters(in: .whitespacesAndNewlines)
            if !sections.isEmpty {
                content = .sections(sections)
            } else if trimmed.isEmpty {
                content = .empty
            } else {
                content = .noMatch(trimmed)
            }
        case .success(let other):
            content = .failed("The session answered the history page with \(other).")
        case .failure(let failure):
            content = .failed(failure.message)
        }
    }

    /// The error state's Retry.
    func retry() {
        refresh()
    }

    func open(_ entry: HistoryEntry, _ opening: HistoryOpening) {
        notice = nil
        opener(entry.source, opening.resumeFrom)
    }

    /// Forget the entry; the media itself is untouched.
    func remove(_ entry: HistoryEntry) {
        let reply = session.dispatch(.history(action: .remove(source: entry.source)))
        notice = Self.failure(of: reply).map { "“\(entry.title)” was not removed from history: \($0)" }
        refresh()
    }

    /// Turn the private session on or off; the banner follows the session's answer.
    func setPrivate(_ enabled: Bool) {
        let reply = session.dispatch(.history(action: .setPrivate(enabled: enabled)))
        notice = Self.failure(of: reply).map { "The private session was not turned \(enabled ? "on" : "off"): \($0)" }
        refresh()
    }

    /// Only a local file has a place in Finder.
    func canReveal(_ entry: HistoryEntry) -> Bool {
        entry.source.kind == .local
    }

    func reveal(_ entry: HistoryEntry) {
        guard canReveal(entry) else { return }
        revealer(URL(fileURLWithPath: entry.source.location))
    }

    /// What went wrong with a history command, or `nil` when it was carried out or
    /// there was nothing to change.
    private static func failure(of reply: DispatchResult) -> String? {
        switch reply {
        case .history(.done), .history(.unchanged): return nil
        case .history(.failed(let message)), .rejected(_, let message), .failed(_, let message): return message
        case .accepted, .noOp, .perform, .applied: return "the session answered \(reply)"
        }
    }

    private static func readFailureText(unreadableCopy: String?) -> String {
        let listed = "Your history file could not be read, so only what you opened since OK Player started is listed."
        guard let unreadableCopy else { return listed }
        return "\(listed) The old file was kept as \(unreadableCopy)."
    }
}
