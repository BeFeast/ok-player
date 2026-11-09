import AppKit

/// The app's ways in for files: File > Open and File > Open Folder, a drop onto the
/// window, and the system's open events. Each goes through the open router.
extension PlayerApp {
    /// Finder's Open With, a double click on a file the app is the default for, a
    /// drop on the Dock icon and files the app is launched with. Events from the
    /// launch arrive before the session exists; the router holds them until then.
    @objc func application(_ application: NSApplication, open urls: [URL]) {
        open(urls)
    }

    /// Open `urls` through the router and show the outcome; returns whether
    /// anything was opened (or held for the launch).
    @discardableResult
    func open(_ urls: [URL]) -> Bool {
        showOpenOutcome(openRouter.open(urls))
    }

    /// File > Open: several media, subtitle or playlist files.
    @objc func openFile() {
        var formats: OpenFormats?
        if case .openFormats(let answer)? = client?.query(.openFormats) { formats = answer }
        OpenPanelConfiguration.files(formats).present(on: window) { [weak self] urls in self?.open(urls) }
    }

    /// File > Open Folder: one or more folders, played as one playlist.
    @objc func openFolder() {
        OpenPanelConfiguration.folders.present(on: window) { [weak self] urls in self?.open(urls) }
    }

    /// The window's content inside the drop target, which asks the router.
    func makeDropTarget(content: NSView) -> FileDropView {
        let drop = FileDropView(content: content)
        drop.accepts = { [weak self] urls in self?.openRouter.accepts(urls) ?? false }
        drop.perform = { [weak self] urls in self?.open(urls) ?? false }
        return drop
    }

    /// Give the router the session and open what arrived while the app launched;
    /// returns whether anything was.
    @discardableResult
    func connectOpenRouter(_ client: SessionClient) -> Bool {
        // TODO(#882): once the session has its `add_subtitle {path}` command, load
        // subtitles dropped on playing media with it through openRouter.addSubtitle.
        guard let outcome = openRouter.connect(client) else { return false }
        return showOpenOutcome(outcome)
    }

    @discardableResult
    private func showOpenOutcome(_ outcome: OpenOutcome) -> Bool {
        switch outcome {
        case .opened(_, let result, let title):
            errorShown = false
            switch result {
            case .rejected(_, let message), .failed(_, let message):
                fail(message)
                return false
            case .noOp:
                return false
            case .accepted, .applied, .perform, .history:
                showOpening(title)
                return true
            }
        case .subtitlesAdded(let results):
            for case .rejected(_, let message) in results { fail(message) }
            for case .failed(_, let message) in results { fail(message) }
            return results.contains { if case .accepted = $0 { return true } else { return false } }
        case .held:
            return true
        case .unsupported, .nothing:
            return false
        }
    }
}
