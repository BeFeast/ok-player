import AppKit
import XCTest
@testable import OKPlayerShell

/// A session that answers `open_paths` with a scripted plan and records what the
/// router asked and sent.
final class FakeOpenSession: OpenSession {
    var plan: OpenPlan
    var reply = DispatchResult.accepted(requestId: 1)
    /// The paths of each `open_paths` query, and the folders among them.
    private(set) var asked: [[String]] = []
    private(set) var folders: [[String]] = []
    private(set) var dispatched: [Command] = []

    init(plan: OpenPlan) { self.plan = plan }

    func query(_ query: Query) -> QueryResult? {
        guard case .openPaths(let paths, let folders) = query else { return nil }
        asked.append(paths)
        self.folders.append(folders)
        return .openPaths(plan: plan)
    }

    func dispatch(_ command: Command) -> DispatchResult {
        dispatched.append(command)
        return reply
    }
}

private func file(_ path: String) -> URL { URL(fileURLWithPath: path) }
/// A folder URL as open panels, Finder and Launch Services hand it over: ending in a slash.
private func folder(_ path: String) -> URL { URL(fileURLWithPath: path, isDirectory: true) }
private func local(_ path: String) -> MediaSource { MediaSource(kind: .local, location: path) }

/// Open events, open panels and drops through the router, against a scripted session.
final class OpenRouterTests: XCTestCase {
    func testOpenEventsAtLaunchAreHeldAndOpenedTogetherOnceTheSessionExists() throws {
        let router = OpenRouter()
        // Finder's launch events arrive before applicationDidFinishLaunching.
        XCTAssertEqual(router.open([file("/m/a.mkv")]), .held)
        XCTAssertEqual(router.open([file("/m/b.mp3"), URL(string: "https://example.com/c.mp4")!]), .held)
        XCTAssertEqual(router.held, [file("/m/a.mkv"), file("/m/b.mp3")])

        let session = FakeOpenSession(plan: .open(sources: [local("/m/a.mkv"), local("/m/b.mp3")]))
        let outcome = try XCTUnwrap(router.connect(session))

        XCTAssertEqual(session.asked, [["/m/a.mkv", "/m/b.mp3"]], "one question for every held file, in order")
        let command = Command.playlist(action: .open(sources: [local("/m/a.mkv"), local("/m/b.mp3")]))
        XCTAssertEqual(session.dispatched, [command], "one playlist open")
        XCTAssertEqual(outcome, .opened(command: command, result: .accepted(requestId: 1), title: "a.mkv"))
        XCTAssertEqual(router.lastOutcome, outcome, "the router remembers what the open came to")
        XCTAssertTrue(router.held.isEmpty)
        XCTAssertNil(router.connect(session), "nothing is opened twice")
        XCTAssertEqual(session.dispatched.count, 1)
    }

    func testAnOpenEventWhileRunningOpensAtOnce() {
        let router = OpenRouter()
        let session = FakeOpenSession(plan: .open(sources: [local("/m/Show")]))
        XCTAssertNil(router.connect(session), "nothing was held")

        let outcome = router.open([folder("/m/Show")])

        XCTAssertEqual(session.asked, [["/m/Show"]])
        XCTAssertEqual(session.folders, [["/m/Show"]], "the session is told it is a folder")
        let command = Command.playlist(action: .open(sources: [local("/m/Show")]))
        XCTAssertEqual(session.dispatched, [command])
        XCTAssertEqual(outcome, .opened(command: command, result: .accepted(requestId: 1), title: "Show"))
    }

    func testTheSmokeTakesTheHeldEventsWithoutOpeningThem() {
        let router = OpenRouter()
        router.open([file("/m/fixture.mp4")])
        XCTAssertEqual(router.takeHeld(), [file("/m/fixture.mp4")])
        let session = FakeOpenSession(plan: .nothing)
        XCTAssertNil(router.connect(session))
        XCTAssertTrue(session.dispatched.isEmpty)
    }

    func testTheSessionsRefusalComesBackInTheOutcome() {
        let router = OpenRouter()
        let session = FakeOpenSession(plan: .open(sources: [local("/m/a.mkv")]))
        session.reply = .rejected(reason: .invalidArgument, message: "no")
        router.connect(session)
        guard case .opened(_, .rejected(.invalidArgument, "no"), _) = router.open([file("/m/a.mkv")]) else {
            return XCTFail("the refusal was not reported")
        }
    }

    func testSubtitlesOntoPlayingMediaGoToTheSubtitleLoaderOnly() {
        let router = OpenRouter()
        let paths = ["/m/film.en.srt", "/m/film.de.srt"]
        let session = FakeOpenSession(plan: .addSubtitles(paths: paths))
        router.connect(session)
        let urls = paths.map(file)

        // Until the session has a command for it, a subtitle drop is refused.
        XCTAssertFalse(router.accepts(urls))
        XCTAssertEqual(router.open(urls), .unsupported(.addSubtitles(paths: paths)))

        var loaded: [String] = []
        router.addSubtitle = { path in
            loaded.append(path)
            return .accepted(requestId: 7)
        }
        XCTAssertTrue(router.accepts(urls))
        XCTAssertEqual(router.open(urls), .subtitlesAdded(results: [.accepted(requestId: 7), .accepted(requestId: 7)]))
        XCTAssertEqual(loaded, paths)
        XCTAssertTrue(session.dispatched.isEmpty, "the playlist is left alone")
    }

    func testANothingPlanIsRefusedAndSendsNothing() {
        let router = OpenRouter()
        let session = FakeOpenSession(plan: .nothing)
        router.connect(session)
        XCTAssertFalse(router.accepts([file("/m/film.srt")]))
        XCTAssertEqual(router.open([file("/m/film.srt")]), .nothing)
        XCTAssertTrue(session.dispatched.isEmpty)
    }

    func testFoldersAreToldApartByTheFileSystemOrTheURLForm() throws {
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("okp-folders-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: scratch) }
        let files = FileManager.default
        let show = scratch.appendingPathComponent("Show")
        let bundle = scratch.appendingPathComponent("Tool.app")
        let link = scratch.appendingPathComponent("Linked Show")
        let film = scratch.appendingPathComponent("film.mkv")
        try files.createDirectory(at: show, withIntermediateDirectories: true)
        try files.createDirectory(at: bundle, withIntermediateDirectories: true)
        try files.createSymbolicLink(at: link, withDestinationURL: show)
        try Data().write(to: film)

        // On disk the file system decides, whatever the URL looks like.
        XCTAssertTrue(OpenRouter.isFolder(URL(fileURLWithPath: show.path, isDirectory: false)))
        XCTAssertTrue(OpenRouter.isFolder(URL(fileURLWithPath: link.path, isDirectory: false)), "a link to a folder")
        XCTAssertFalse(OpenRouter.isFolder(URL(fileURLWithPath: bundle.path, isDirectory: true)),
                       "an app bundle is a file to the user")
        XCTAssertFalse(OpenRouter.isFolder(URL(fileURLWithPath: film.path, isDirectory: false)))
        // Not on disk, the URL's form decides.
        XCTAssertTrue(OpenRouter.isFolder(folder("/okp-missing/Show")))
        XCTAssertFalse(OpenRouter.isFolder(file("/okp-missing/doc.pdf")))

        let urls = [URL(fileURLWithPath: show.path, isDirectory: false), file(film.path), folder("/okp-missing/Show")]
        XCTAssertEqual(OpenRouter.folders(urls), [show.standardizedFileURL.path, "/okp-missing/Show"])
    }

    func testOnlyLocalFilesAreAskedAbout() {
        let router = OpenRouter()
        let session = FakeOpenSession(plan: .open(sources: [local("/m/a.mkv")]))
        router.connect(session)
        XCTAssertEqual(router.open([URL(string: "https://example.com/a.mp4")!]), .nothing)
        XCTAssertTrue(session.asked.isEmpty, "a web URL is not opened from here")
        XCTAssertEqual(OpenRouter.paths([file("/m/x/../a.mkv"), URL(string: "https://example.com/b")!]), ["/m/a.mkv"])
        XCTAssertEqual(router.open([]), .nothing)
        XCTAssertEqual(OpenRouter().open([URL(string: "https://example.com/a.mp4")!]), .nothing,
                       "a web URL is not held at launch either")
    }
}

/// The router over the real okp_session_* C ABI: what a dropped or opened payload
/// does is okp-core's rule, answered by the session.
final class OpenSessionRoutingTests: XCTestCase {
    private var root: URL!
    private var client: SessionClient!
    private var router: OpenRouter!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory.appendingPathComponent("okp-open-\(UUID().uuidString)")
        let config = SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: root.appendingPathComponent("data").path,
            configDir: root.appendingPathComponent("config").path,
            cacheDir: root.appendingPathComponent("cache").path,
            hwdec: "no",
            mpvOptions: [MpvOption(name: "ao", value: "null"), MpvOption(name: "vo", value: "null")])
        client = try SessionClient(config: config)
        router = OpenRouter()
        router.connect(client)
    }

    override func tearDownWithError() throws {
        client.close()
        client = nil
        try? FileManager.default.removeItem(at: root)
    }

    func testFilesAFolderAndAnM3uBecomeOnePlaylistOpen() {
        let payload = [folder("/m/Show"), file("/m/b.mkv"), file("/m/a.MP3"), file("/m/queue.m3u8"),
                       file("/m/notes.pdf")]
        XCTAssertEqual(router.plan(for: payload), .open(sources: [
            local("/m/Show"), local("/m/b.mkv"), local("/m/a.MP3"), local("/m/queue.m3u8"),
        ]), "the document is left out")
    }

    func testADocumentOrAnythingNotKnownAsAFolderIsRefused() {
        for payload in [[file("/m/report.pdf")], [file("/m/report.pdf"), file("/m/Show")]] {
            XCTAssertEqual(router.plan(for: payload), .nothing, "\(payload)")
            XCTAssertFalse(router.accepts(payload), "no drop highlight for \(payload)")
            XCTAssertEqual(router.open(payload), .nothing)
        }
        XCTAssertTrue(router.accepts([folder("/m/Show")]))
    }

    func testASubtitleBesideMediaIsLeftOutAndASubtitleAloneNeedsPlayingMedia() {
        XCTAssertEqual(router.plan(for: [file("/m/film.srt"), file("/m/film.mkv")]),
                       .open(sources: [local("/m/film.mkv")]))
        // Nothing plays in this session.
        XCTAssertEqual(router.plan(for: [file("/m/film.srt")]), .nothing)
        XCTAssertFalse(router.accepts([file("/m/film.srt")]))
        XCTAssertTrue(router.accepts([file("/m/film.mkv")]))
    }

    func testADroppedFolderReachesTheSessionPlaylist() throws {
        let folder = root.appendingPathComponent("Empty Show", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let outcome = router.open([folder])
        let command = Command.playlist(action: .open(sources: [local(folder.standardizedFileURL.path)]))
        // A folder is read on the session's scan thread: the open is applied, not refused.
        XCTAssertEqual(outcome, .opened(command: command, result: .applied, title: "Empty Show"))
    }

    func testTheSessionListsTheCoreFormats() throws {
        guard case .openFormats(let formats)? = client.query(.openFormats) else {
            return XCTFail("open_formats was not answered")
        }
        for ext in ["mkv", "webm", "mp4"] { XCTAssertTrue(formats.video.contains(ext), ext) }
        for ext in ["mp3", "flac"] { XCTAssertTrue(formats.audio.contains(ext), ext) }
        XCTAssertEqual(formats.playlist, ["m3u", "m3u8"])
        XCTAssertEqual(try GoldenFixtures.decode(QueryReply.self, "open/query-reply-open-formats.json").result,
                       .openFormats(formats: formats), "the fixture is what the session answers")
    }
}

/// The files on a drag's pasteboard.
final class DropPayloadTests: XCTestCase {
    func testOnlyTheLocalFilesComeOffThePasteboardInOrder() {
        let pasteboard = NSPasteboard(name: NSPasteboard.Name("okp-drop-\(UUID().uuidString)"))
        defer { pasteboard.releaseGlobally() }
        pasteboard.clearContents()
        let files = [file("/m/b.mkv"), folder("/m/Show"), file("/m/queue.m3u")]
        let web = URL(string: "https://example.com/a.mp4")!
        XCTAssertTrue(pasteboard.writeObjects((files + [web]).map { $0 as NSURL }))
        let dropped = DropPayload.urls(from: pasteboard)
        XCTAssertEqual(dropped.map(\.path), files.map(\.path))
        // A folder keeps its trailing slash through the pasteboard, so it is still known
        // as a folder where the file system cannot tell.
        XCTAssertEqual(dropped.map(\.hasDirectoryPath), [false, true, false])
        XCTAssertEqual(OpenRouter.folders(dropped), ["/m/Show"])
    }
}

/// The window's drop target, driven with the files a drag carries.
final class FileDropViewTests: XCTestCase {
    func testAnAcceptedDragIsCopiedUnderTheHighlightAndDroppedIntoTheRouter() {
        let view = FileDropView(content: NSView())
        var accepted: [[URL]] = []
        var opened: [[URL]] = []
        view.accepts = { urls in accepted.append(urls); return urls.first?.pathExtension == "mkv" }
        view.perform = { urls in opened.append(urls); return true }
        XCTAssertTrue(view.registeredDraggedTypes.contains(.fileURL))
        XCTAssertTrue(view.highlight.isHidden)

        XCTAssertEqual(view.enter([file("/m/a.mkv")]), .copy)
        XCTAssertFalse(view.highlight.isHidden)
        XCTAssertTrue(view.drop([file("/m/a.mkv")]))
        XCTAssertTrue(view.highlight.isHidden)
        XCTAssertEqual(opened, [[file("/m/a.mkv")]])

        XCTAssertEqual(view.enter([file("/m/a.srt")]), [], "a refused drag is not copied")
        XCTAssertTrue(view.highlight.isHidden)
        XCTAssertEqual(view.enter([file("/m/a.mkv")]), .copy)
        view.leave()
        XCTAssertTrue(view.highlight.isHidden, "leaving the window drops the highlight")
        XCTAssertEqual(accepted.count, 3)
    }

    func testTheHighlightTakesNoClicksAndTheContentFillsTheTarget() {
        let content = NSView()
        let view = FileDropView(content: content)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.borderless],
                              backing: .buffered, defer: true)
        window.contentView = view
        view.layoutSubtreeIfNeeded()
        XCTAssertEqual(content.frame, view.bounds)
        XCTAssertEqual(view.highlight.frame, view.bounds)
        XCTAssertNil(view.highlight.hitTest(NSPoint(x: 10, y: 10)))
        XCTAssertTrue(view.subviews.last === view.highlight, "the highlight lies over the content")
    }
}
