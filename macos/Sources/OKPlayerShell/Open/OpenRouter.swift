import AppKit

/// The session as the open router uses it: one query and one command. The app
/// passes its `SessionClient`; tests pass a fake.
protocol OpenSession: AnyObject {
    func query(_ query: Query) -> QueryResult?
    func dispatch(_ command: Command) -> DispatchResult
}

extension SessionClient: OpenSession {}

/// What became of one open.
enum OpenOutcome: Equatable {
    /// The session was sent the playlist open the plan names; `title` names what
    /// opens (the first file or folder), for the window title.
    case opened(command: Command, result: DispatchResult, title: String)
    /// Each subtitle file went to `OpenRouter.addSubtitle`, with these replies.
    case subtitlesAdded(results: [DispatchResult])
    /// Kept until the session exists: an open event that arrived while the app
    /// was still launching.
    case held
    /// The plan names something this shell cannot carry out yet.
    case unsupported(OpenPlan)
    /// Nothing to open: no local files, or the session's plan says so.
    case nothing
}

/// Where opened files go. An open panel's selection, a drop onto the window and
/// the system's open events (Finder's Open With, a drop on the Dock icon, files
/// the app is launched with) all go through `open(_:)`: one `open_paths` query,
/// whose plan the session computes with okp-core's rules, then the command the
/// plan names. Swift decides nothing about extensions.
///
/// Open events can arrive before the session exists, while the app launches.
/// They are held and opened together, as one playlist, once `connect(_:)` hands
/// the router its session.
final class OpenRouter {
    private(set) weak var session: OpenSession?
    /// Loads one subtitle file onto the media that plays. `nil` while the session
    /// has no command for it; a subtitle-only plan is then unsupported, and a
    /// drop of subtitles is refused.
    var addSubtitle: ((String) -> DispatchResult)?
    /// Open events held until the session exists, oldest first.
    private(set) var held: [URL] = []

    /// The local file paths among `urls`, in order. Web URLs are not opened from
    /// here yet.
    static func paths(_ urls: [URL]) -> [String] {
        urls.filter(\.isFileURL).map { $0.standardizedFileURL.path }
    }

    /// The session's plan for `urls`; `.nothing` without a session or local files.
    func plan(for urls: [URL]) -> OpenPlan {
        let paths = OpenRouter.paths(urls)
        guard !paths.isEmpty, case .openPaths(let plan)? = session?.query(.openPaths(paths: paths)) else {
            return .nothing
        }
        return plan
    }

    /// Whether a drop of `urls` would do anything; the window shows its drop
    /// highlight only then.
    func accepts(_ urls: [URL]) -> Bool {
        switch plan(for: urls) {
        case .open: return true
        case .addSubtitles: return addSubtitle != nil
        case .nothing: return false
        }
    }

    /// Open `urls`, or hold them until the session exists.
    @discardableResult
    func open(_ urls: [URL]) -> OpenOutcome {
        guard session != nil else {
            let files = urls.filter(\.isFileURL)
            if files.isEmpty { return .nothing }
            held.append(contentsOf: files)
            return .held
        }
        let answer = self.plan(for: urls)
        switch answer {
        case .open(let sources):
            guard let session, let first = sources.first else { return .nothing }
            let command = Command.playlist(action: .open(sources: sources))
            let title = (first.location as NSString).lastPathComponent
            return .opened(command: command, result: session.dispatch(command), title: title)
        case .addSubtitles(let paths):
            guard let addSubtitle, !paths.isEmpty else { return .unsupported(answer) }
            return .subtitlesAdded(results: paths.map(addSubtitle))
        case .nothing:
            return .nothing
        }
    }

    /// Hand the router its session and open what was held, as one open. Returns
    /// that open's outcome, or `nil` when nothing was held.
    @discardableResult
    func connect(_ session: OpenSession) -> OpenOutcome? {
        self.session = session
        let waiting = takeHeld()
        return waiting.isEmpty ? nil : open(waiting)
    }

    /// Take the held open events without opening them (the smoke opens its
    /// fixture itself, on its own schedule).
    func takeHeld() -> [URL] {
        defer { held.removeAll() }
        return held
    }
}

/// The local files on a drag's pasteboard, in order.
enum DropPayload {
    static func urls(from pasteboard: NSPasteboard) -> [URL] {
        let objects = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
        return (objects as? [URL]) ?? []
    }
}
