import AppKit

/// The floating transport controls (OSC) over the video: a rounded bar on a
/// visual-effect material at the bottom of the window with the scrubber, previous,
/// play/pause and next, the time readout, speed, mute, volume and fullscreen.
///
/// The overlay covers the whole video so it sees every pointer movement, but only
/// the bar takes clicks; everything else falls through to the video. Scrolling
/// anywhere over it changes the volume. Each session batch is turned into the
/// session's transport view (`query`), and every control sends a session command
/// (`dispatch`); nothing here changes playback state itself. The controls hide
/// with the cursor after the session's idle timeout while media plays (`AutoHide`).
final class TransportOverlay: NSView {
    /// Sends a command to the session.
    var dispatch: ((Command) -> Void)?
    /// The session's transport view, at a drag position when one is given.
    var query: ((Double?) -> TransportView?)?
    /// Previous and next item. The buttons stay disabled while these are nil.
    var onPrevious: (() -> Void)? { didSet { previousButton.isEnabled = onPrevious != nil } }
    var onNext: (() -> Void)? { didSet { nextButton.isEnabled = onNext != nil } }
    /// Called whenever the controls show or hide.
    var onRevealChange: ((Bool) -> Void)?

    let autoHide = AutoHide()
    let panel = NSVisualEffectView()
    let scrubber = ScrubberView()
    let elapsedLabel = NSTextField(labelWithString: "00:00")
    let trailingLabel = NSTextField(labelWithString: "-00:00")
    let previousButton = TransportOverlay.iconButton("backward.end.fill", "Previous item")
    let playButton = TransportOverlay.iconButton("play.fill", "Play")
    let nextButton = TransportOverlay.iconButton("forward.end.fill", "Next item")
    let speedButton = NSButton(title: "1.00×", target: nil, action: nil)
    let muteButton = TransportOverlay.iconButton("speaker.wave.2.fill", "Mute")
    let volumeSlider = VolumeSliderView()
    let fullscreenButton = TransportOverlay.iconButton("arrow.up.left.and.arrow.down.right", "Enter Full Screen")

    /// The last transport view the session gave.
    private(set) var transport: TransportView?
    private var snapshot = SessionSnapshot()
    private var scrub = ScrubState()
    private var trailing = TrailingTime.remaining
    private var scrollSteps = VolumeScrollAccumulator()
    private var surface: OscSurface?
    private var panelArea: NSTrackingArea?
    private var scrollMonitor: Any?
    private var windowObservers: [NSObjectProtocol] = []

    init() {
        super.init(frame: .zero)
        autoHide.onChange = { [weak self] revealed in self?.revealChanged(revealed) }
        buildPanel()
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                       owner: self, userInfo: nil))
        let panelArea = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect, .enabledDuringMouseDrag],
                                       owner: self, userInfo: nil)
        panel.addTrackingArea(panelArea)
        self.panelArea = panelArea
        panel.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    // MARK: Session

    /// The session reported a batch; `snapshot` is the store's state after it.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        self.snapshot = snapshot
        refresh()
    }

    /// The controls are shown.
    var isRevealed: Bool { autoHide.revealed }

    /// Show the controls as pointer movement would.
    func noteActivity() { autoHide.activity() }

    private func refresh() {
        guard let view = query?(scrub.dragPosition(duration: snapshot.duration)) else { return }
        transport = view
        elapsedLabel.stringValue = view.elapsed
        trailingLabel.stringValue = trailing.text(in: view)
        scrubber.isEnabled = view.seekable
        scrubber.progress = view.progress
        scrubber.buffered = view.buffered
        scrubber.ticks = view.chapterTicks
        volumeSlider.volume = view.volume
        volumeSlider.toolTip = view.volume.readout
        Self.setSymbol(muteButton, view.volume.muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
                       view.volume.muted ? "Unmute" : "Mute")
        setSpeedTitle(view.speed)
        let playing = snapshot.status == .playing
        Self.setSymbol(playButton, playing ? "pause.fill" : "play.fill", playing ? "Pause" : "Play")
        playButton.isEnabled = snapshot.source != nil
        updateFullscreenButton()
        autoHide.update(allowed: view.osc.autoHide, hideAfter: view.osc.hideAfter)
        applySurface()
    }

    private func applySurface() {
        guard let view = transport else { return }
        let next = view.osc.surface(revealed: autoHide.revealed)
        if !next.visible && surface?.visible == true { autoHide.unpinAll() }
        surface = next
        panel.isHidden = !next.visible
        panel.alphaValue = autoHide.revealed ? 1 : 0
        scrubber.allowsFocus = next.focusable
    }

    private func revealChanged(_ revealed: Bool) {
        applySurface()
        if !revealed { hideCursorIfOver() }
        onRevealChange?(revealed)
    }

    /// The cursor hides with the controls while it is over the video; moving it
    /// brings both back.
    private func hideCursorIfOver() {
        guard let window, NSApp.isActive else { return }
        let point = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        if bounds.contains(point) { NSCursor.setHiddenUntilMouseMoves(true) }
    }

    // MARK: Pointer and scrolling

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard surface?.hitTestable == true else { return nil }
        let hit = super.hitTest(point)
        return hit === self ? nil : hit
    }

    override func mouseMoved(with event: NSEvent) { autoHide.activity() }

    override func mouseEntered(with event: NSEvent) {
        if event.trackingArea === panelArea { autoHide.pin() } else { autoHide.activity() }
    }

    override func mouseExited(with event: NSEvent) {
        if event.trackingArea === panelArea { autoHide.unpin() }
    }

    override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        if let scrollMonitor { NSEvent.removeMonitor(scrollMonitor) }
        scrollMonitor = nil
        for observer in windowObservers { NotificationCenter.default.removeObserver(observer) }
        windowObservers = []
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        scrollMonitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
            guard let self, self.scrolled(event) else { return event }
            return nil
        }
        windowObservers = [NSWindow.didEnterFullScreenNotification, NSWindow.didExitFullScreenNotification].map { name in
            NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                self?.updateFullscreenButton()
            }
        }
    }

    /// A scroll over the video changes the volume. True when it was used.
    private func scrolled(_ event: NSEvent) -> Bool {
        guard event.window === window, surface?.visible == true else { return false }
        guard bounds.contains(convert(event.locationInWindow, from: nil)) else { return false }
        autoHide.activity()
        if let command = VolumeScrollAccumulator.command(steps: scrollSteps.steps(for: ScrollInput(event: event))) {
            dispatch?(command)
        }
        return true
    }

    // MARK: Controls

    @objc private func togglePause() { dispatch?(.togglePause) }
    @objc private func previousItem() { onPrevious?() }
    @objc private func nextItem() { onNext?() }
    @objc private func toggleMute() { dispatch?(.setMute(muted: !snapshot.mute)) }
    @objc private func toggleFullscreen() { window?.toggleFullScreen(nil) }

    @objc private func toggleTrailing() {
        trailing = trailing.toggled
        trailingLabel.toolTip = trailing.toolTip
        if let transport { trailingLabel.stringValue = trailing.text(in: transport) }
    }

    @objc private func showSpeedMenu(_ sender: NSButton) {
        guard let transport else { return }
        let menu = NSMenu()
        for choice in transport.speeds {
            let item = NSMenuItem(title: choice.label, action: #selector(chooseSpeed(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = choice.speed
            item.state = choice.selected ? .on : .off
            menu.addItem(item)
        }
        autoHide.pin()
        _ = menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 4), in: sender)
        autoHide.unpin()
    }

    @objc private func chooseSpeed(_ item: NSMenuItem) {
        guard let speed = item.representedObject as? Double else { return }
        dispatch?(.setSpeed(speed: speed))
    }

    private func scrubbed(_ phase: ScrubberView.Phase, _ fraction: Double) {
        let duration = transport?.seekable == true ? snapshot.duration : nil
        let command: Command?
        switch phase {
        case .began:
            autoHide.pin()
            command = scrub.begin(at: fraction, duration: duration)
        case .moved:
            command = scrub.move(to: fraction, duration: duration)
        case .ended:
            command = scrub.end(at: fraction, duration: duration)
            autoHide.unpin()
        }
        if let command { dispatch?(command) }
        refresh()
    }

    private func volumeDragged(to fraction: Double) {
        guard let volume = transport?.volume else { return }
        dispatch?(.setVolume(volume: VolumeTrack.level(atFraction: fraction, maximum: volume.max)))
    }

    private func updateFullscreenButton() {
        let fullscreen = window?.styleMask.contains(.fullScreen) == true
        Self.setSymbol(fullscreenButton,
                       fullscreen ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right",
                       fullscreen ? "Exit Full Screen" : "Enter Full Screen")
    }

    private func setSpeedTitle(_ title: String) {
        guard speedButton.title != title else { return }
        speedButton.attributedTitle = NSAttributedString(string: title, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium),
            .foregroundColor: PaletteRole.text.color,
        ])
    }

    // MARK: Layout

    private func buildPanel() {
        panel.material = .popover
        panel.blendingMode = .withinWindow
        panel.state = .active
        panel.wantsLayer = true
        panel.layer?.cornerRadius = 12
        panel.layer?.masksToBounds = true
        panel.translatesAutoresizingMaskIntoConstraints = false
        addSubview(panel)

        for label in [elapsedLabel, trailingLabel] {
            label.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium)
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        elapsedLabel.textColor = PaletteRole.text.color
        trailingLabel.textColor = PaletteRole.textMuted.color
        trailingLabel.toolTip = trailing.toolTip
        trailingLabel.addGestureRecognizer(NSClickGestureRecognizer(target: self, action: #selector(toggleTrailing)))

        speedButton.isBordered = false
        speedButton.toolTip = "Playback speed"
        speedButton.target = self
        speedButton.action = #selector(showSpeedMenu(_:))
        setSpeedTitle("1.00×")
        for (button, action) in [(previousButton, #selector(previousItem)), (playButton, #selector(togglePause)),
                                 (nextButton, #selector(nextItem)), (muteButton, #selector(toggleMute)),
                                 (fullscreenButton, #selector(toggleFullscreen))] {
            button.target = self
            button.action = action
        }
        previousButton.isEnabled = false
        nextButton.isEnabled = false
        scrubber.onScrub = { [weak self] phase, fraction in self?.scrubbed(phase, fraction) }
        scrubber.onStep = { [weak self] seconds in
            self?.dispatch?(.seek(seconds: seconds, mode: .relative, precision: .keyframe))
        }
        volumeSlider.onFraction = { [weak self] fraction in self?.volumeDragged(to: fraction) }

        let leading = NSStackView(views: [previousButton, playButton, nextButton, elapsedLabel, trailingLabel])
        leading.spacing = 6
        leading.setCustomSpacing(12, after: nextButton)
        let trailingControls = NSStackView(views: [speedButton, muteButton, volumeSlider, fullscreenButton])
        trailingControls.spacing = 6
        for stack in [leading, trailingControls] {
            stack.orientation = .horizontal
            stack.alignment = .centerY
            stack.translatesAutoresizingMaskIntoConstraints = false
            panel.addSubview(stack)
        }
        scrubber.translatesAutoresizingMaskIntoConstraints = false
        panel.addSubview(scrubber)

        let fill = panel.widthAnchor.constraint(equalTo: widthAnchor, constant: -32)
        fill.priority = .defaultHigh
        NSLayoutConstraint.activate([
            panel.centerXAnchor.constraint(equalTo: centerXAnchor),
            panel.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -14),
            panel.widthAnchor.constraint(lessThanOrEqualToConstant: 760),
            panel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 8),
            fill,
            scrubber.topAnchor.constraint(equalTo: panel.topAnchor, constant: 8),
            scrubber.leadingAnchor.constraint(equalTo: panel.leadingAnchor, constant: 8),
            scrubber.trailingAnchor.constraint(equalTo: panel.trailingAnchor, constant: -8),
            leading.topAnchor.constraint(equalTo: scrubber.bottomAnchor, constant: 2),
            leading.leadingAnchor.constraint(equalTo: panel.leadingAnchor, constant: 10),
            leading.bottomAnchor.constraint(equalTo: panel.bottomAnchor, constant: -8),
            trailingControls.centerYAnchor.constraint(equalTo: leading.centerYAnchor),
            trailingControls.trailingAnchor.constraint(equalTo: panel.trailingAnchor, constant: -10),
            trailingControls.leadingAnchor.constraint(greaterThanOrEqualTo: leading.trailingAnchor, constant: 8),
        ])
    }

    private static func symbol(_ name: String, _ label: String) -> NSImage? {
        NSImage(systemSymbolName: name, accessibilityDescription: label)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 14, weight: .medium))
    }

    private static func iconButton(_ name: String, _ label: String) -> NSButton {
        let button = NSButton(title: "", target: nil, action: nil)
        button.isBordered = false
        button.imagePosition = .imageOnly
        button.contentTintColor = PaletteRole.text.color
        setSymbol(button, name, label)
        button.widthAnchor.constraint(equalToConstant: 26).isActive = true
        button.heightAnchor.constraint(equalToConstant: 26).isActive = true
        return button
    }

    private static func setSymbol(_ button: NSButton, _ name: String, _ label: String) {
        guard button.toolTip != label else { return }
        button.image = symbol(name, label)
        if button.image == nil { button.title = label }
        button.toolTip = label
        button.setAccessibilityLabel(label)
    }
}
