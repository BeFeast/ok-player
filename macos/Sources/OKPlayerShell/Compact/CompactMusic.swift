import AppKit

/// Switches presentation on the current window and session.
final class CompactMusic: NSObject {
    static let commandID = "compact-music"
    let view = MusicView()
    weak var session: SessionClient?
    var automaticallyEnters = true
    var showCommands: ((NSPoint) -> Void)?
    var onModeChange: ((Bool) -> Void)?
    var onQueueToggle: (() -> Void)?
    var accessibilityOverride: MiniPlayerAccessibility? {
        didSet { updateAccessibility() }
    }

    private weak var controller: PlayerWindowController?
    private weak var video: NSView?
    private var standard: (frame: CGRect, minimum: CGSize)?
    private var snapshot = SessionSnapshot()
    private var suppressedForSource = false
    private var artworkTask: Task<Void, Never>?
    private let autoHide = AutoHide()
    private var pointerPin = false
    private var contextPin = false
    private var scrub = ScrubState()
    private var scroll = VolumeScrollAccumulator()

    var savedFrame: CGRect? { standard?.frame }
    var isActive: Bool { standard != nil }
    var canToggle: Bool {
        guard let controller, controller.canChangePresentation, !controller.window.isMiniPlayer else { return false }
        return isActive || snapshot.status == .playing || snapshot.status == .paused
    }

    override init() {
        super.init()
        autoHide.onChange = { [weak self] in self?.view.showControls($0) }
        for (button, action) in [
            (view.play, #selector(playPause)), (view.previous, #selector(previousItem)),
            (view.next, #selector(nextItem)), (view.mute, #selector(mute)),
            (view.queue, #selector(toggleQueue)), (view.more, #selector(openCommands)),
            (view.switchToVideo, #selector(restoreStandard))
        ] {
            button.target = self
            button.action = action
        }
        view.seek.onScrub = { [weak self] phase, fraction in self?.scrubbed(phase, fraction) }
        view.seek.onAccessibilitySeek = { [weak self] fraction in
            guard let self, let duration = self.snapshot.duration else { return }
            self.session?.dispatch(.seek(seconds: duration * fraction, mode: .absolute, precision: .exact))
        }
        view.volume.onFraction = { [weak self] fraction in
            guard let self, let maximum = self.view.volume.volume?.max else { return }
            self.session?.dispatch(.setVolume(volume: VolumeTrack.level(atFraction: fraction, maximum: maximum)))
        }
        view.volume.onNudge = { [weak self] delta in self?.session?.dispatch(.nudgeVolume(delta: delta)) }
        NSWorkspace.shared.notificationCenter.addObserver(
            self, selector: #selector(accessibilityChanged), name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil)
    }

    deinit {
        artworkTask?.cancel()
        NSWorkspace.shared.notificationCenter.removeObserver(self)
        NotificationCenter.default.removeObserver(self)
    }

    func install(in root: NSView, video: NSView, controller: PlayerWindowController) {
        self.video = video
        self.controller = controller
        view.frame = root.bounds
        view.autoresizingMask = [.width, .height]
        root.addSubview(view)
        updateAccessibility()
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowAvailable), name: NSWindow.didEndSheetNotification, object: controller.window)
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowAvailable), name: NSWindow.didExitFullScreenNotification, object: controller.window)
    }

    func sessionChanged(_ updated: SessionSnapshot) {
        let newLoad = snapshot.source != updated.source || (updated.status == .opening && snapshot.status != .opening)
        if newLoad {
            suppressedForSource = false
            scrub.cancel()
            view.volume.cancelPress()
            artworkTask?.cancel()
            artworkTask = nil
            view.artwork.show(nil)
        }
        snapshot = updated
        if updated.source == nil || updated.endReason == .error ||
            (updated.video != nil && updated.audioMetadata == nil && (newLoad || awaitingNewSource)) {
            leave()
        }
        if newLoad { awaitingNewSource = isActive }
        if updated.audioMetadata != nil { awaitingNewSource = false }

        enterAutomatically()
        if isActive, artworkTask == nil { loadArtwork() }
        refresh()
    }

    private var awaitingNewSource = false

    func toggle() {
        guard canToggle else { return }
        if isActive { restoreStandard() } else { enter() }
    }

    @objc func restoreStandard() {
        suppressedForSource = true
        leave()
    }

    func noteActivity() {
        if isActive { autoHide.activity() }
    }

    func setQueueShown(_ shown: Bool) {
        view.queueShown = shown
        refresh()
    }

    func setContextShown(_ shown: Bool) {
        guard isActive, contextPin != shown else { return }
        contextPin = shown
        if shown { autoHide.pin() } else { autoHide.unpin() }
    }

    func handle(_ event: NSEvent) -> Bool {
        switch event.type {
        case .mouseMoved, .mouseEntered, .mouseExited, .leftMouseDown, .leftMouseUp, .scrollWheel:
            noteActivity()
            let overControls = event.type != .mouseExited && view.pointerOverControls(event.locationInWindow)
            if pointerPin != overControls {
                pointerPin = overControls
                if overControls { autoHide.pin() } else { autoHide.unpin() }
            }
        default: break
        }
        guard event.type == .scrollWheel else { return false }
        let point = view.queueHost.convert(event.locationInWindow, from: nil)
        if view.queueShown && view.queueHost.bounds.contains(point) { return false }
        if let command = VolumeScrollAccumulator.command(steps: scroll.steps(for: ScrollInput(event: event))) {
            session?.dispatch(command)
        }
        return true
    }

    func project(_ row: RegistryCommand) -> RegistryCommand {
        var row = row
        if row.id == Self.commandID {
            row.enabled = row.enabled && canToggle
            row.checked = isActive
        } else if isActive, row.id == "mini-player" {
            row.enabled = false
        }
        return row
    }

    @objc private func windowAvailable() { enterAutomatically() }

    private func enterAutomatically() {
        guard automaticallyEnters, !suppressedForSource, !isActive, snapshot.audioMetadata != nil, canToggle else { return }
        enter()
    }

    private func enter() {
        guard let controller else { return }
        let window = controller.window
        standard = (window.frame, window.minSize)
        window.isCompactMusic = true
        controller.presentationChanged()
        onModeChange?(true)
        video?.isHidden = true
        view.isHidden = false
        window.minSize = MusicView.minimumSize
        let fitted = CGRect(x: window.frame.midX - MusicView.defaultSize.width / 2,
                            y: window.frame.midY - MusicView.defaultSize.height / 2,
                            width: MusicView.defaultSize.width, height: MusicView.defaultSize.height)
        setFrame(fitted)
        window.makeFirstResponder(view)
        loadArtwork()
        autoHide.activity()
        refresh()
    }

    private func leave() {
        guard let standard, let controller else { return }
        self.standard = nil
        awaitingNewSource = false
        scrub.cancel()
        view.volume.cancelPress()
        artworkTask?.cancel()
        artworkTask = nil
        view.artwork.show(nil)
        pointerPin = false
        contextPin = false
        autoHide.unpinAll()
        autoHide.update(allowed: false, hideAfter: 0)
        view.isHidden = true
        let window = controller.window
        window.isCompactMusic = false
        window.minSize = standard.minimum
        controller.presentationChanged()
        setFrame(standard.frame)
        video?.isHidden = false
        window.makeFirstResponder(video)
        onModeChange?(false)
        (video as? VideoView)?.redraw()
    }

    private func setFrame(_ frame: CGRect) {
        guard let window = controller?.window else { return }
        let area = window.screen?.visibleFrame ?? NSScreen.main?.visibleFrame
        window.setFrame(area.map { frame.fitted(inside: $0) } ?? frame,
                        display: true, animate: !accessibility.reduceMotion)
    }

    private func loadArtwork() {
        guard let source = snapshot.source, snapshot.audioMetadata != nil else { return }
        artworkTask = Task { @MainActor [weak self] in
            let artwork = await EmbeddedMusicArtwork.load(source)
            guard !Task.isCancelled, let self, self.isActive, self.snapshot.source == source else { return }
            self.view.artwork.show(artwork)
            self.refresh()
        }
    }

    private var accessibility: MiniPlayerAccessibility { accessibilityOverride ?? .system }

    @objc private func accessibilityChanged() { updateAccessibility() }

    private func updateAccessibility() {
        view.reduceTransparency = accessibility.reduceTransparency || accessibility.increaseContrast
        view.seek.highContrast = accessibility.increaseContrast
        refresh()
    }

    private func refresh() {
        guard isActive, let source = snapshot.source,
              case .transport(let transport)? = session?.query(.transport(
                at: scrub.dragPosition(duration: snapshot.duration))) else { return }
        let audio = snapshot.audioMetadata
        let title = nonempty(audio?.title) ?? snapshot.title ?? (source.location as NSString).lastPathComponent
        let detail = [nonempty(audio?.artist), nonempty(audio?.album)].compactMap { $0 }.joined(separator: " · ")
        view.show(title: title, detail: detail, hasVideo: snapshot.video != nil)
        let playing = snapshot.status == .playing
        view.play.isEnabled = playing || snapshot.status == .paused
        view.showTransport(transport, playing: playing, canStep: (session?.store.playlist.items.count ?? 0) > 1)
        view.artwork.updateMotion(playing: playing && !snapshot.buffering, reduceMotion: accessibility.reduceMotion)
        autoHide.update(allowed: transport.osc.autoHide && !view.queueShown && !scrub.isDragging &&
                        !view.volume.isPressed, hideAfter: transport.osc.hideAfter)
    }

    private func nonempty(_ tag: String?) -> String? {
        guard let tag = tag?.trimmingCharacters(in: .whitespacesAndNewlines), !tag.isEmpty else { return nil }
        return tag
    }

    private func scrubbed(_ phase: ScrubberView.Phase, _ fraction: Double) {
        let duration = snapshot.duration
        let command: Command?
        switch phase {
        case .began: command = scrub.begin(at: fraction, duration: duration)
        case .moved: command = scrub.move(to: fraction, duration: duration)
        case .ended: command = scrub.end(at: fraction, duration: duration)
        }
        if let command { session?.dispatch(command) }
        refresh()
    }

    @objc private func playPause() { session?.dispatch(.togglePause) }
    @objc private func previousItem() { session?.dispatch(.playlist(action: .previous)) }
    @objc private func nextItem() { session?.dispatch(.playlist(action: .next)) }
    @objc private func mute() { session?.dispatch(.toggleMute) }
    @objc private func toggleQueue() { onQueueToggle?() }
    @objc private func openCommands() {
        showCommands?(view.more.convert(CGPoint(x: view.more.bounds.midX, y: view.more.bounds.midY), to: nil))
    }
}
