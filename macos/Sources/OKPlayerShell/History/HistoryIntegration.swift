import AppKit
import SwiftUI

/// The idle Welcome view in the player window and the History window, both bound to
/// the session's history (docs/session-abi.md, "Watch history"). The app tells it
/// when the welcome view is due and hands it every drained batch; the session's
/// `history.changed` and `history.private_session` events refresh whichever of the
/// two is on screen.
final class HistoryIntegration: NSObject, NSWindowDelegate {
    let welcome: ContinueWatchingModel
    let page: HistoryPageModel
    /// The Welcome view's hosting view, hidden until `setWelcomeShown(true)`.
    let welcomeView: NSView
    private(set) var showsWelcome = false
    /// The History window is open.
    private(set) var showsHistory = false
    private var historyWindow: NSWindow?

    /// `open` opens a source with that `resume_from`, `openFile` runs the Open panel,
    /// and `reveal` shows a local file in Finder.
    init(session: HistorySessionLink, open: @escaping (MediaSource, Double?) -> Void,
         openFile: @escaping () -> Void,
         reveal: @escaping (URL) -> Void = { NSWorkspace.shared.activateFileViewerSelecting([$0]) }) {
        let welcome = ContinueWatchingModel(session: session, open: open)
        self.welcome = welcome
        page = HistoryPageModel(session: session, open: open, reveal: reveal)
        var host: NSView?
        MainActor.assumeIsolated {
            let hosting = NSHostingView(rootView: WelcomeView(model: welcome))
            // Only the content's natural size: the window's own size constraints decide
            // how much room it gets, and a narrow window shows fewer cards.
            hosting.sizingOptions = [.intrinsicContentSize]
            host = hosting
        }
        let view = host ?? NSView()
        view.isHidden = true
        welcomeView = view
        super.init()
        welcome.onOpenFile = openFile
        welcome.onShowHistory = { [weak self] in self?.showHistory() }
    }

    /// Whether the player window shows the Welcome view: nothing is loaded or being
    /// read, and no error is up.
    static func welcomeDue(_ snapshot: SessionSnapshot, scanning: Bool, errorShown: Bool) -> Bool {
        snapshot.source == nil && !scanning && !errorShown
    }

    /// Puts the Welcome view in the middle of the player window, between the title
    /// strip and the transport controls.
    func install(in root: NSView, above controls: NSView) {
        let host = welcomeView
        host.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(host)
        // Centred when it fits; squeezed (fewer cards, clipped text) rather than over
        // the title strip or the controls when it does not.
        let centred = host.centerYAnchor.constraint(equalTo: root.centerYAnchor)
        centred.priority = NSLayoutConstraint.Priority(240)
        NSLayoutConstraint.activate([
            host.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            host.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 16),
            host.topAnchor.constraint(greaterThanOrEqualTo: root.safeAreaLayoutGuide.topAnchor, constant: 8),
            host.bottomAnchor.constraint(lessThanOrEqualTo: controls.topAnchor, constant: -8),
            centred,
        ])
        host.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        host.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
    }

    /// Show or hide the Welcome view; a shelf that comes into view is read again.
    func setWelcomeShown(_ shown: Bool) {
        guard shown != showsWelcome else { return }
        showsWelcome = shown
        welcomeView.isHidden = !shown
        if shown { welcome.refresh() }
    }

    /// A drained batch: a history change refreshes what is on screen.
    func sessionChanged(_ batch: EventBatch) {
        let changed = batch.events.contains { event in
            switch event {
            case .history(.changed), .history(.privateSession): return true
            default: return false
            }
        }
        guard changed else { return }
        if showsWelcome { welcome.refresh() }
        if showsHistory { page.refresh() }
    }

    // MARK: History window

    /// File > History…
    func makeMenuItem() -> NSMenuItem {
        let item = NSMenuItem(title: "History…", action: #selector(showHistoryFromMenu(_:)), keyEquivalent: "y")
        item.target = self
        return item
    }

    @objc func showHistoryFromMenu(_ sender: Any?) {
        showHistory()
    }

    /// Open the History window, or bring it to the front, with a fresh page.
    func showHistory() {
        let window = historyWindow ?? makeHistoryWindow()
        historyWindow = window
        historyShown()
        window.makeKeyAndOrderFront(nil)
    }

    /// The History window opened: it lists the session's page from now on.
    func historyShown() {
        showsHistory = true
        page.refresh()
    }

    func windowWillClose(_ notification: Notification) {
        guard (notification.object as? NSWindow) === historyWindow else { return }
        showsHistory = false
    }

    private func makeHistoryWindow() -> NSWindow {
        var made: NSWindow?
        let model = page
        MainActor.assumeIsolated {
            let hosting = NSHostingController(rootView: HistoryView(model: model))
            hosting.sizingOptions = [.minSize]
            made = NSWindow(contentViewController: hosting)
        }
        let window = made ?? NSWindow()
        window.title = "History"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.isReleasedWhenClosed = false
        window.setContentSize(NSSize(width: 640, height: 560))
        window.delegate = self
        _ = window.setFrameAutosaveName("OKPlayerHistory")
        return window
    }

    // MARK: Smoke

    /// Smoke, while idle: the Welcome view shows the seeded Continue watching shelf
    /// (`SmokeHistory`) with the fixture's card first, and its baseline snapshots are
    /// written. The smoke then opens the fixture through the open router (Open/), so
    /// the card itself is not activated here; `HistoryModelTests` covers a card's
    /// open. Returns what failed.
    func smokeWelcomeFailure(fixture: String, snapshotDirectory: String?) -> String? {
        guard showsWelcome, !welcomeView.isHidden else { return "The Welcome view is not shown while nothing plays" }
        let entries = welcome.entries
        guard entries.count == SmokeHistory.seededCount, let card = entries.first,
              card.source == MediaSource(kind: .local, location: fixture) else {
            return "Continue watching lists \(entries.map(\.title)) instead of the seeded history"
                + (welcome.problem.map { " (\($0))" } ?? "")
        }
        if let snapshotDirectory { smokeCaptureWelcome(to: snapshotDirectory) }
        print("OKP_SMOKE welcome=shown continue_watching=\(entries.count) first_card=\(card.state.rawValue)")
        fflush(stdout)
        return nil
    }

    /// Smoke evidence: the Welcome view in the light and the dark appearance, as
    /// `welcome-light.png` and `welcome-dark.png`.
    private func smokeCaptureWelcome(to directory: String) {
        for (name, appearanceName) in [("light", NSAppearance.Name.aqua), ("dark", NSAppearance.Name.darkAqua)] {
            guard let appearance = NSAppearance(named: appearanceName) else { continue }
            let file = URL(fileURLWithPath: directory).appendingPathComponent("welcome-\(name).png")
            var png: Data?
            let model = welcome
            appearance.performAsCurrentDrawingAppearance {
                png = MainActor.assumeIsolated { Self.renderWelcome(model, dark: name == "dark") }
            }
            do {
                guard let png else { throw CocoaError(.fileWriteUnknown) }
                try png.write(to: file)
                print("OKP_SNAPSHOT welcome-\(name)=\(file.lastPathComponent)")
            } catch {
                fputs("OK Player: could not write \(file.lastPathComponent): \(error)\n", stderr)
            }
        }
    }

    @MainActor
    private static func renderWelcome(_ model: ContinueWatchingModel, dark: Bool) -> Data? {
        let renderer = ImageRenderer(content: WelcomeView(model: model)
            .background(PaletteRole.canvas.swiftUI)
            .environment(\.colorScheme, dark ? .dark : .light))
        renderer.scale = 2
        guard let image = renderer.cgImage else { return nil }
        return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
    }
}

extension HistoryIntegration {
    /// The app's islands: the session behind `app.client`, and opens that go through
    /// `app.play` and bring the player window forward.
    convenience init(app: PlayerApp) {
        let noSession = DispatchResult.rejected(reason: .noActiveMedia, message: "There is no playback session.")
        self.init(session: HistorySessionLink(query: { [weak app] query in app?.client?.query(query) },
                                              dispatch: { [weak app] command in app?.client?.dispatch(command) ?? noSession }),
                  open: { [weak app] source, resumeFrom in
                      app?.play(source, resumeFrom: resumeFrom)
                      app?.window?.makeKeyAndOrderFront(nil)
                  },
                  openFile: { [weak app] in app?.openFile() })
    }
}

extension PlayerApp {
    /// The Welcome view takes the open prompt's place while nothing is loaded.
    func updateWelcome(_ snapshot: SessionSnapshot) {
        let scanning = client?.store.playlist.scanning ?? false
        history.setWelcomeShown(HistoryIntegration.welcomeDue(snapshot, scanning: scanning, errorShown: errorShown))
        if history.showsWelcome { prompt.isHidden = true }
    }
}

/// The smoke's watch history. The bounded smoke (`--smoke-test`) runs its session on
/// a history of its own, in a new temporary directory, so it starts from a known
/// shelf and never touches the user's: a `history.json` in the core's document
/// format (`okp_core::history::History`) holding the fixture, finished so that it
/// opens from the start, and a stream left halfway a day earlier.
enum SmokeHistory {
    static let seededCount = 2
    static let seededStream = "https://example.com/okp-smoke/seeded-talk.mp4"
    /// The directory `config(fixture:)` made, removed by `cleanUp()`.
    private static var directory: URL?

    /// The app's session config, with `shellCommands`, and the history in a seeded
    /// temporary directory.
    static func config(fixture: String?, shellCommands: [String] = []) throws -> SessionConfig {
        var config = SessionConfig.forApp(shellCommands: shellCommands)
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("okp-smoke-history-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.directory = directory
        try seed(directory: directory, fixture: fixture, now: Date())
        config.dataDir = directory.path
        return config
    }

    /// Write the seeded `history.json` into `directory`.
    static func seed(directory: URL, fixture: String?, now: Date) throws {
        let unix = Int64(now.timeIntervalSince1970)
        var files: [String: Any] = [
            seededStream: ["position": 1325.5, "duration": 3120.0, "finished": false,
                           "updated_at_unix": unix - 26 * 3600, "title": "Seeded talk"] as [String: Any],
        ]
        if let fixture {
            files[fixture] = ["position": 0.0, "duration": 12.0, "finished": true,
                              "updated_at_unix": unix - 60] as [String: Any]
        }
        let document: [String: Any] = ["version": 2, "files": files]
        let data = try JSONSerialization.data(withJSONObject: document, options: [.sortedKeys])
        try data.write(to: directory.appendingPathComponent("history.json"), options: .atomic)
    }

    /// Remove the smoke's history once the session is freed.
    static func cleanUp() {
        guard let directory else { return }
        self.directory = nil
        try? FileManager.default.removeItem(at: directory)
    }
}
