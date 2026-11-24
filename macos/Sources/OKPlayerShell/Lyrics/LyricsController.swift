import AppKit

/// Coordinates the shared Rust local-lyrics state (`Query.lyrics`, `Event.lyricsChanged`)
/// with `LyricsPanelView` across both Compact Music and the standard player window.
final class LyricsController: NSObject {
    static let commandID = "lyrics"

    let panel = LyricsPanelView()
    var query: ((Query) -> QueryResult?)?
    var dispatch: ((Command) -> DispatchResult)?
    var onVisibilityChange: ((Bool) -> Void)?
    /// Disconnected only by the `lyrics-sidecar` negative control so the verifier
    /// proves active-line highlighting and auto-scroll are checked live.
    var syncsActiveLine = true

    private(set) var isShown = false
    private(set) var hasAudio = false
    private var lastSource: MediaSource?
    private weak var standardRoot: NSView?
    private weak var standardAbove: NSView?
    private var standardConstraints: [NSLayoutConstraint] = []
    private var musicConstraints: [NSLayoutConstraint] = []

    var canToggle: Bool { isShown || hasAudio }

    override init() {
        super.init()
        panel.onSeek = { [weak self] seconds in
            _ = self?.dispatch?(.seek(seconds: seconds, mode: .absolute, precision: .exact))
        }
        panel.onClose = { [weak self] in self?.hide() }
    }

    func install(in root: NSView, above video: NSView, below osc: NSView) {
        standardRoot = root
        standardAbove = video
        panel.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(panel, positioned: .above, relativeTo: video)
        standardConstraints = [
            panel.topAnchor.constraint(equalTo: root.topAnchor, constant: 44),
            panel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 24),
            panel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -24),
            panel.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -96),
        ]
        NSLayoutConstraint.activate(standardConstraints)
        panel.usesMusicSurface = false
    }

    func useMusicLayout(_ host: NSView?) {
        NSLayoutConstraint.deactivate(standardConstraints + musicConstraints)
        panel.removeFromSuperview()
        musicConstraints.removeAll()
        if let host {
            panel.usesMusicSurface = true
            host.addSubview(panel)
            musicConstraints = [
                panel.topAnchor.constraint(equalTo: host.topAnchor),
                panel.leadingAnchor.constraint(equalTo: host.leadingAnchor),
                panel.trailingAnchor.constraint(equalTo: host.trailingAnchor),
                panel.bottomAnchor.constraint(equalTo: host.bottomAnchor),
            ]
            NSLayoutConstraint.activate(musicConstraints)
        } else if let standardRoot {
            panel.usesMusicSurface = false
            if let standardAbove {
                standardRoot.addSubview(panel, positioned: .above, relativeTo: standardAbove)
            } else {
                standardRoot.addSubview(panel)
            }
            NSLayoutConstraint.activate(standardConstraints)
        }
    }

    func register(with commands: PlayerCommands) {
        commands.register(Self.commandID) { [weak self] in self?.toggle() }
    }

    func project(_ row: RegistryCommand) -> RegistryCommand {
        guard row.id == Self.commandID else { return row }
        var row = row
        row.enabled = row.enabled && canToggle
        row.checked = isShown
        return row
    }

    @discardableResult
    func toggle() -> Bool {
        if isShown {
            hide()
            return true
        }
        return show()
    }

    @discardableResult
    func show() -> Bool {
        guard canToggle else { return false }
        if !isShown {
            isShown = true
            panel.isHidden = false
            onVisibilityChange?(true)
        }
        refreshFromSession(expectedSource: lastSource)
        return true
    }

    func hide() {
        guard isShown else { return }
        isShown = false
        panel.isHidden = true
        onVisibilityChange?(false)
    }

    func sessionChanged(snapshot: SessionSnapshot, audioMetadata: AudioMetadata?, batch: EventBatch) {
        let sourceChanged = snapshot.source != lastSource || snapshot.status == .opening
        if sourceChanged {
            // Immediately drop previous track's lyrics so stale rows can never follow a track switch.
            panel.clear()
        }
        lastSource = snapshot.source
        hasAudio = audioMetadata != nil && snapshot.source != nil && snapshot.endReason != .error

        if snapshot.source == nil || snapshot.endReason == .error ||
            (snapshot.video != nil && audioMetadata == nil) {
            hasAudio = false
            panel.clear()
            hide()
            return
        }

        guard hasAudio else { return }

        let lyricsEvent = batch.events.contains { event in
            if case .lyricsChanged = event { return true }
            return false
        }
        let timeChanged = batch.events.contains { event in
            guard case .snapshotChanged(let changes) = event else { return false }
            return changes.contains { change in
                if case .timePos = change { return true }
                return false
            }
        }
        if sourceChanged || lyricsEvent || (isShown && timeChanged) || panel.displayedSource != snapshot.source {
            refreshFromSession(expectedSource: snapshot.source)
        }
    }

    func refreshFromSession(expectedSource: MediaSource?, at position: Double? = nil) {
        guard let expectedSource else {
            panel.clear()
            return
        }
        guard case .lyrics(var view)? = query?(.lyrics(at: position)) else { return }
        // Reject stale replies whose source does not match the active track.
        guard view.source == expectedSource || view.state == .unavailable else {
            panel.clear()
            return
        }
        if !syncsActiveLine {
            view.activeIndex = nil
        }
        panel.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        panel.show(view)
    }
}
