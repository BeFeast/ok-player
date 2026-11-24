import AppKit
import QuartzCore

/// The Up Next panel in the player window (#884), with the transport controls'
/// Up Next button and the menu bar's registry item that show and hide it.
///
/// Both run the registry command `chapters-up-next`, which this shell carries out
/// (registered with `PlayerCommands`, so the session config lists it): the panel
/// opens when the registry enables the command (media is open) and the session
/// answers `perform`. It closes from the button, the menu item while it is enabled,
/// or its own close button. Playlist events invalidate the shared queue view.
final class UpNextController: NSObject {
    /// The registry command that shows and hides the panel.
    static let commandID = "chapters-up-next"

    let panel = UpNextPanel()
    /// The transport controls' Up Next button.
    let button = TransportOverlay.iconButton("list.bullet.rectangle", "Chapters & Up Next")
    /// Sends a command to the session.
    var dispatch: ((Command) -> DispatchResult)?
    /// Asks the session.
    var query: ((Query) -> QueryResult?)?
    /// Whether the window is in fullscreen, for the registry query.
    var isFullscreen: () -> Bool = { false }
    private(set) var isShown = false
    var onVisibilityChange: ((Bool) -> Void)?
    var onQueueError: ((String) -> Void)?
    /// The verifier can supply a selection while exercising the same picker completion.
    var chooseFiles: ((QueueMode, @escaping ([URL]) -> Void) -> Void)?
    var verifyMovesDisabled = false
    var verifyAddDisabled = false
    private weak var standardRoot: NSView?
    private weak var standardBar: NSView?
    private var standardLayout: [NSLayoutConstraint] = []
    private var musicLayout: [NSLayoutConstraint] = []
    private var shownPlaylist = PlaylistState()
    private var shownTitle: String?

    override init() {
        super.init()
        panel.isHidden = true
        panel.dispatch = { [weak self] command in
            guard let self else { return }
            if self.verifyMovesDisabled, case .playlist(action: .move) = command { return }
            self.send(command)
        }
        panel.onAddFiles = { [weak self] in self?.presentAddFiles(mode: .append) }
        panel.onDragInvalidated = { [weak self] in self?.onQueueError?("Queue changed. Drag the item again.") }
        panel.onClose = { [weak self] in self?.hide() }
        button.target = self
        button.action = #selector(toggleUpNext(_:))
    }

    /// Lay the panel over the right of `root`, from under the title bar down to just
    /// above `bar` (the transport controls), so it never covers them. Call it after
    /// the video and the controls are in `root`, so the panel is above both.
    func install(in root: NSView, below strip: NSView? = nil, above bar: NSView) {
        panel.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(panel)
        // Narrower than this only in a window too small to hold it.
        let width = panel.widthAnchor.constraint(equalToConstant: 316)
        width.priority = .defaultHigh
        standardRoot = root
        standardBar = bar
        standardLayout = [
            panel.topAnchor.constraint(equalTo: strip?.bottomAnchor ?? root.safeAreaLayoutGuide.topAnchor, constant: 4),
            panel.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            panel.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -2),
            panel.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor),
            width,
        ]
        NSLayoutConstraint.activate(standardLayout)
    }

    func useMusicLayout(_ host: NSView?) {
        NSLayoutConstraint.deactivate(standardLayout + musicLayout)
        panel.removeFromSuperview()
        musicLayout.removeAll()
        if let host {
            host.addSubview(panel)
            musicLayout = [
                panel.topAnchor.constraint(equalTo: host.topAnchor),
                panel.leadingAnchor.constraint(equalTo: host.leadingAnchor),
                panel.trailingAnchor.constraint(equalTo: host.trailingAnchor),
                panel.bottomAnchor.constraint(equalTo: host.bottomAnchor),
            ]
            NSLayoutConstraint.activate(musicLayout)
        } else if let standardRoot {
            // The OSC's bar is a child of the OSC, which is a sibling of the panel.
            standardRoot.addSubview(panel, positioned: .above, relativeTo: standardBar?.superview)
            NSLayoutConstraint.activate(standardLayout)
        }
    }

    /// Carry out the registry command when the menu bar or a key runs it: register
    /// it with `commands` before the session starts.
    func register(with commands: PlayerCommands) {
        commands.register(Self.commandID) { [weak self] in self?.registryCommandRan() }
        commands.register("add-to-queue") { [weak self] in self?.presentAddFiles(mode: .append) }
        commands.register("play-next") { [weak self] in self?.presentAddFiles(mode: .playNext) }
    }

    /// `PlayerCommands` ran the command and the session handed it to this shell:
    /// show a hidden panel, hide a shown one. The registry already enabled it.
    func registryCommandRan() {
        setShown(!isShown)
    }

    /// The session reported a batch; `playlist` and `snapshot` are the store's state
    /// after it.
    func sessionChanged(playlist: PlaylistState, snapshot: SessionSnapshot, playlistChanged: Bool = false) {
        panel.showChapters(snapshot.chapters)
        guard playlistChanged || playlist != shownPlaylist || snapshot.title != shownTitle else { return }
        if case .queueView(let view)? = query?(.queueView) {
            shownPlaylist = playlist
            shownTitle = snapshot.title
            panel.show(view)
        }
    }

    private func presentAddFiles(mode: QueueMode) {
        let append: ([URL]) -> Void = { [weak self] urls in
            let sources = urls.filter(\.isFileURL).map { MediaSource(kind: .local, location: $0.path) }
            guard !sources.isEmpty else { return }
            guard let self, !self.verifyAddDisabled else { return }
            self.send(.playlist(action: .queue(sources: sources, mode: mode)))
        }
        if let chooseFiles { chooseFiles(mode, append); return }
        guard let window = panel.window, window.attachedSheet == nil else { return }
        var formats: OpenFormats?
        if case .openFormats(var media)? = query?(.openFormats) {
            media.subtitle = []
            media.playlist = []
            formats = media
        }
        OpenPanelConfiguration.files(formats).present(on: window, prompt: mode == .append ? "Add" : "Play Next", open: append)
    }

    private func send(_ command: Command) {
        switch dispatch?(command) {
        case .rejected(_, let message), .failed(_, let message): onQueueError?(message)
        default: break
        }
    }

    /// Whether the registry offers the command now: this shell carries it out, and
    /// the session's state enables it.
    var isOffered: Bool {
        guard case .commandRegistry(let commands)? = query?(.commandRegistry(fullscreen: isFullscreen(), miniPlayer: false)),
              let command = commands.first(where: { $0.id == Self.commandID }) else { return false }
        return command.available && command.enabled
    }

    /// Hide a shown panel, or show a hidden one. Returns whether anything changed.
    @discardableResult
    func toggle() -> Bool {
        if isShown {
            hide()
            return true
        }
        return show()
    }

    /// Show the panel when the registry offers the command and the session leaves it
    /// to this shell. Returns whether it is shown.
    @discardableResult
    func show() -> Bool {
        guard !isShown else { return true }
        guard isOffered, case .perform? = dispatch?(.invokeCommand(id: Self.commandID)) else { return false }
        setShown(true)
        return true
    }

    func hide() {
        guard isShown else { return }
        setShown(false)
    }

    @objc func toggleUpNext(_ sender: Any?) {
        toggle()
    }

    private func setShown(_ shown: Bool) {
        guard isShown != shown else { return }
        isShown = shown
        onVisibilityChange?(shown)
        if !shown {
            panel.returnFocus()
            panel.dismissRowMenu()
        }
        panel.isHidden = !shown
        if shown, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            panel.wantsLayer = true
            let slide = CABasicAnimation(keyPath: "transform.translation.x")
            slide.fromValue = panel.bounds.width
            slide.toValue = 0
            slide.duration = 0.16
            slide.timingFunction = CAMediaTimingFunction(name: .easeOut)
            panel.layer?.add(slide, forKey: "up-next-reveal")
        }
        button.contentTintColor = (shown ? PaletteRole.link : PaletteRole.text).overVideo
        let label = shown ? "Hide Chapters & Up Next" : "Chapters & Up Next"
        button.toolTip = label
        button.setAccessibilityLabel(label)
    }
}

// The smoke's check (`--smoke-test`, run by the macOS workflows).
extension UpNextController {
    /// Open the panel if the menu bar has not, and check that it lists `playlist`
    /// (at least two items) under its Now Playing card, with the current item marked.
    /// Returns what failed.
    func smokeShowFailure(playlist: PlaylistState) -> String? {
        guard show(), !panel.isHidden else { return "The Up Next command did not open the panel" }
        panel.select(.upNext)
        panel.superview?.layoutSubtreeIfNeeded()
        guard panel.frame.width > 0, panel.frame.height > 0 else { return "The Up Next panel has no size" }
        let rows = panel.tableView.numberOfRows
        guard playlist.items.count >= 2, rows == playlist.items.count else {
            return "Up Next lists \(rows) rows for a playlist of \(playlist.items.count) items"
        }
        guard let current = playlist.currentIndex, panel.list.rows.indices.contains(current),
              panel.list.rows[current].isCurrent, let nowPlaying = panel.list.nowPlaying,
              panel.nowPlayingTitle.stringValue == nowPlaying.title else {
            return "Up Next does not show the current item under Now Playing"
        }
        return nil
    }

    /// `up_next=shown up_next_rows=N up_next_current=I` for the smoke line.
    var smokeSummary: String {
        let current = panel.list.rows.firstIndex(where: \.isCurrent).map { "\($0)" } ?? "none"
        return "up_next=\(isShown ? "shown" : "hidden") up_next_rows=\(panel.tableView.numberOfRows) "
            + "up_next_current=\(current)"
    }
}
