import AppKit

/// The Up Next panel in the player window (#884), with the transport controls'
/// Up Next button and the menu bar's registry item that show and hide it.
///
/// Both run the registry command `chapters-up-next`, which this shell carries out
/// (registered with `PlayerCommands`, so the session config lists it): the panel
/// opens when the registry enables the command (media is open) and the session
/// answers `perform`. It closes from the button, the menu item while it is enabled,
/// or its own close button. Each drained batch hands it the session's playlist
/// (`SessionStore.playlist`).
final class UpNextController: NSObject {
    /// The registry command that shows and hides the panel.
    static let commandID = "chapters-up-next"

    let panel = UpNextPanel()
    /// The transport controls' Up Next button.
    let button = TransportOverlay.iconButton("list.bullet", "Up Next")
    /// Sends a command to the session.
    var dispatch: ((Command) -> DispatchResult)?
    /// Asks the session.
    var query: ((Query) -> QueryResult?)?
    /// Whether the window is in fullscreen, for the registry query.
    var isFullscreen: () -> Bool = { false }
    private(set) var isShown = false
    private var shownPlaylist = PlaylistState()
    private var shownTitle: String?

    override init() {
        super.init()
        panel.isHidden = true
        panel.dispatch = { [weak self] command in _ = self?.dispatch?(command) }
        panel.onClose = { [weak self] in self?.hide() }
        button.target = self
        button.action = #selector(toggleUpNext(_:))
    }

    /// Lay the panel over the right of `root`, from under the title bar down to just
    /// above `bar` (the transport controls), so it never covers them. Call it after
    /// the video and the controls are in `root`, so the panel is above both.
    func install(in root: NSView, above bar: NSView) {
        panel.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(panel)
        // Narrower than this only in a window too small to hold it.
        let width = panel.widthAnchor.constraint(equalToConstant: 300)
        width.priority = .defaultHigh
        NSLayoutConstraint.activate([
            panel.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 8),
            panel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            panel.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -10),
            panel.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 12),
            width,
        ])
    }

    /// Carry out the registry command when the menu bar or a key runs it: register
    /// it with `commands` before the session starts.
    func register(with commands: PlayerCommands) {
        commands.register(Self.commandID) { [weak self] in self?.registryCommandRan() }
    }

    /// `PlayerCommands` ran the command and the session handed it to this shell:
    /// show a hidden panel, hide a shown one. The registry already enabled it.
    func registryCommandRan() {
        setShown(!isShown)
    }

    /// The session reported a batch; `playlist` and `snapshot` are the store's state
    /// after it.
    func sessionChanged(playlist: PlaylistState, snapshot: SessionSnapshot) {
        guard playlist != shownPlaylist || snapshot.title != shownTitle else { return }
        shownPlaylist = playlist
        shownTitle = snapshot.title
        panel.show(UpNextList(playlist: playlist, mediaTitle: snapshot.title))
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
        isShown = shown
        if !shown { panel.returnFocus() }
        panel.isHidden = !shown
        button.contentTintColor = (shown ? PaletteRole.accent : PaletteRole.text).color
        let label = shown ? "Hide Up Next" : "Up Next"
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
