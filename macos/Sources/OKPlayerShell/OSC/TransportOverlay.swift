import AppKit

/// The bottom transport surface and the input that reveals it over video.
final class TransportOverlay: NSView, NSMenuDelegate {
    enum Pin: Hashable { case pointer, scrub, volume, menu, panel, tracks }

    struct Slot {
        let id: String
        let view: NSView
        let width: CGFloat
        // A higher priority folds into More earlier, following the core's OSC order.
        let collapse: Int
        let menuItem: (() -> NSMenuItem)?
    }

    /// Sends a command to the session.
    var dispatch: ((Command) -> Void)?
    /// The session's transport view, at a drag position when one is given.
    var query: ((Double?) -> TransportView?)?
    /// Previous and next item. A button is enabled while its action is set and the
    /// playlist has an item to step to (`playlistSteps`).
    var onPrevious: (() -> Void)? { didSet { updateStepButtons() } }
    var onNext: (() -> Void)? { didSet { updateStepButtons() } }
    /// Whether the session playlist can step back and forward. The session wraps
    /// at both ends, so both are true exactly when it holds two items or more.
    var playlistSteps = (previous: false, next: false) { didSet { updateStepButtons() } }
    /// Called whenever the controls show or hide.
    var onRevealChange: ((Bool) -> Void)?
    /// The session owning subtitles for the video surface this overlay fills.
    weak var subtitleSession: SessionClient?
    /// Where keyboard focus goes when the scrubber may no longer hold it (the
    /// controls hid, or the media cannot be sought): the video.
    weak var focusReturn: NSView?

    let autoHide: AutoHide
    let scrim = TransportScrim()
    let panel = PalettePanel(fill: NSColor.fromSRGB(0x161619).withAlphaComponent(0.50),
                             border: PaletteRole.text.overVideo.withAlphaComponent(0.14), radius: 14)
    let scrubber = ScrubberView()
    let elapsedLabel = NSTextField(labelWithString: "00:00")
    let trailingLabel = NSButton(title: "-00:00", target: nil, action: nil)
    let previousButton = TransportOverlay.iconButton("backward.end.fill", "Previous item")
    let playButton = TransportOverlay.iconButton("play.fill", "Play")
    let nextButton = TransportOverlay.iconButton("forward.end.fill", "Next item")
    let speedButton = TransportTextButton(title: "1.00×", label: "Playback speed")
    let muteButton = TransportOverlay.iconButton("speaker.wave.2.fill", "Mute")
    let volumeWick = VolumeWick()
    let volumeSlider = VolumeSliderView()
    let volumeReadout = NSTextField(labelWithString: "")
    let volumeCapsule = PalettePanel(fill: NSColor.fromSRGB(0x1c1c20).withAlphaComponent(0.76),
                                     border: PaletteRole.borderSubtle.overVideo, radius: 13)
    let fullscreenButton = TransportOverlay.iconButton("arrow.up.left.and.arrow.down.right", "Enter Full Screen")
    let moreButton = TransportOverlay.iconButton("ellipsis", "More")
    private(set) var slots: [Slot] = []
    private(set) var collapsed: [Slot] = []
    private var pins: Set<Pin> = []
    private var openMenus: Set<ObjectIdentifier> = []
    private let volumeClose = MainQueueHideTimer()
    private var volumeSuppressed = false
    private var pointerInside = false
    private(set) var cursorHidden = false
    var onMenuOpen: ((NSMenu) -> Void)?
    var moreActions: (() -> [NSMenuItem])?
    var acceptsVolumeWheel = true
    var collapsesSlots = true

    /// The last transport view the session gave.
    private(set) var transport: TransportView?
    private var snapshot = SessionSnapshot()
    private var scrub = ScrubState()
    private var trailing = TrailingTime.remaining
    private var scrollSteps = VolumeScrollAccumulator()
    private var surface: OscSurface?
    private var subtitleClearance: (height: CGFloat, bottom: CGFloat)?
    private var windowObservers: [NSObjectProtocol] = []

    init(hideTimer: HideTimer = MainQueueHideTimer()) {
        autoHide = AutoHide(timer: hideTimer)
        super.init(frame: .zero)
        autoHide.onChange = { [weak self] revealed in self?.revealChanged(revealed) }
        buildPanel()
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                       owner: self, userInfo: nil))
        for view in [scrim, panel, volumeCapsule] { view.isHidden = true }
    }

    required init?(coder: NSCoder) { fatalError("Use init(hideTimer:)") }

    override func layout() {
        super.layout()
        layoutSlots()
        updateSubtitleClearance()
    }

    private func updateSubtitleClearance() {
        guard let subtitleSession, bounds.height > 0 else { return }
        let coversVideo = surface?.hitTestable == true && !panel.isHidden
        let top = volumeCapsule.isHidden ? panel.frame.maxY : volumeCapsule.frame.maxY
        guard !coversVideo || panel.frame.height > 0 else { return }
        // Both the panel and the video use view points, including on Retina.
        let bottom = coversVideo ? top - bounds.minY + 8 : 0
        let height = bounds.height
        if let previous = subtitleClearance, previous.height == height, previous.bottom == bottom { return }
        if subtitleSession.setSubtitleClearance(surfaceHeight: height, obscuredBottom: bottom) {
            subtitleClearance = (height, bottom)
        }
    }

    // MARK: Session

    /// The session reported a batch; `snapshot` is the store's state after it.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        if self.snapshot.source != snapshot.source || (snapshot.status == .opening && self.snapshot.status != .opening) {
            endScrub()
            closeVolume()
            volumeSuppressed = false
        }
        self.snapshot = snapshot
        refresh()
    }

    /// The controls are shown.
    var isRevealed: Bool { autoHide.revealed }

    /// Show the controls as pointer movement would.
    func noteActivity() {
        autoHide.activity()
        showCursor()
    }

    func setPinned(_ pinned: Bool, by owner: Pin) {
        if pinned {
            if pins.insert(owner).inserted { autoHide.pin() }
        } else if pins.remove(owner) != nil {
            autoHide.unpin()
        }
    }

    private func refresh() {
        let at = scrub.dragPosition(duration: snapshot.duration)
        guard var view = query?(at) else { return }
        if !view.seekable && scrub.isDragging {
            // The media stopped being seekable under the drag (the file ended): the
            // drag is over, and the next file shows its own playhead.
            endScrub()
            if at != nil, let current = query?(nil) { view = current }
        }
        transport = view
        elapsedLabel.stringValue = view.elapsed
        updateTrailingTitle(view)
        scrubber.isEnabled = view.seekable
        scrubber.progress = view.progress
        scrubber.buffered = view.buffered
        scrubber.ticks = view.chapterTicks
        volumeSlider.volume = view.volume
        volumeSlider.toolTip = view.volume.readout
        volumeSlider.setAccessibilityValue(view.volume.readout)
        volumeReadout.stringValue = view.volume.readout
        volumeReadout.textColor = (view.volume.boosted ? PaletteRole.warning : PaletteRole.text).overVideo
        volumeWick.volume = view.volume
        Self.setSymbol(muteButton, view.volume.muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
                       view.volume.muted ? "Unmute" : "Mute")
        speedButton.value = view.speed
        let playing = snapshot.status == .playing
        Self.setSymbol(playButton, playing ? "pause.fill" : "play.fill", playing ? "Pause" : "Play")
        playButton.isEnabled = snapshot.source != nil
        speedButton.isEnabled = playing || snapshot.status == .paused
        updateFullscreenButton()
        autoHide.update(allowed: view.osc.autoHide, hideAfter: view.osc.hideAfter)
        applySurface()
    }

    private func applySurface() {
        guard let view = transport else { return }
        let next = view.osc.surface(revealed: autoHide.revealed)
        let previous = surface
        surface = next
        if !next.mapped {
            endScrub()
            closeVolume()
            setPinned(false, by: .pointer)
        }
        if next.hitTestable != previous?.hitTestable || next.mapped != previous?.mapped {
            for view in [scrim, panel] { present(view, visible: next.hitTestable, mapped: next.mapped) }
        }
        updateSubtitleClearance()
        scrubber.allowsFocus = next.focusable
        if !next.focusable || !scrubber.isEnabled { returnFocus(onlyScrubber: next.focusable) }
    }

    private func present(_ view: NSView, visible: Bool, mapped: Bool) {
        let fade = !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        NSAnimationContext.runAnimationGroup { context in
            context.duration = fade ? (visible ? 0.15 : 0.12) : 0
            view.animator().alphaValue = visible ? 1 : 0
        }
        view.isHidden = !mapped
    }

    private func returnFocus(onlyScrubber: Bool = false) {
        guard let window, let focused = window.firstResponder as? NSView,
              onlyScrubber ? focused === scrubber : focused.isDescendant(of: self) else { return }
        _ = window.makeFirstResponder(focusReturn)
    }

    private func revealChanged(_ revealed: Bool) {
        applySurface()
        if revealed { showCursor() } else { hideCursorIfOver() }
        onRevealChange?(revealed)
    }

    private func hideCursorIfOver() {
        guard !cursorHidden, pointerInside, window?.isKeyWindow == true, NSApp.isActive else { return }
        NSCursor.hide()
        cursorHidden = true
    }

    private func showCursor() {
        guard cursorHidden else { return }
        NSCursor.unhide()
        cursorHidden = false
    }

    // MARK: Pointer and scrolling

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard surface?.hitTestable == true else { return nil }
        let hit = super.hitTest(point)
        return hit === self ? nil : hit
    }

    override func mouseMoved(with event: NSEvent) {
        pointerMoved(to: event.locationInWindow)
    }

    override func mouseEntered(with event: NSEvent) {
        pointerMoved(to: event.locationInWindow)
    }

    override func mouseExited(with event: NSEvent) {
        pointerLeft()
    }

    private func pointerMoved(to point: NSPoint) {
        let local = convert(point, from: nil)
        guard bounds.contains(local) else { pointerLeft(); return }
        pointerInside = true
        noteActivity()
        guard surface?.hitTestable == true else { return }
        let overPanel = panel.frame.contains(local)
        setPinned(overPanel, by: .pointer)
        let overSpeaker = overPanel && muteButton.frame.contains(panel.convert(local, from: self))
        if !overSpeaker { volumeSuppressed = false }
        if overSpeaker && !volumeSuppressed {
            openVolume()
        } else if !volumeCapsule.isHidden {
            if volumeCapsule.frame.insetBy(dx: 0, dy: -10).contains(local) {
                volumeClose.cancel()
            } else if !volumeSlider.isPressed {
                scheduleVolumeClose()
            }
        }
    }

    private func pointerLeft() {
        pointerInside = false
        volumeSuppressed = false
        showCursor()
        setPinned(false, by: .pointer)
        if !volumeSlider.isPressed { scheduleVolumeClose() }
    }

    override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        for observer in windowObservers { NotificationCenter.default.removeObserver(observer) }
        windowObservers = []
        scrollSteps = VolumeScrollAccumulator()
        pointerLeft()
        closeVolume()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        let center = NotificationCenter.default
        windowObservers = [NSWindow.didEnterFullScreenNotification, NSWindow.didExitFullScreenNotification].map { name in
            center.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in self?.updateFullscreenButton() }
        }
        windowObservers.append(center.addObserver(forName: NSWindow.didResignKeyNotification,
                                                  object: window, queue: .main) { [weak self] _ in
            self?.pointerLeft()
            self?.closeVolume()
        })
    }

    func handleWindowInput(_ event: NSEvent) -> Bool {
        let point: NSPoint
        if let eventWindow = event.window {
            guard eventWindow === window else { return false }
            point = event.locationInWindow
        } else if let window {
            // Windowless CGEvents are positioned in screen coordinates.
            point = window.convertPoint(fromScreen: event.locationInWindow)
        } else {
            return false
        }
        switch event.type {
        case .scrollWheel:
            return scrolled(event, at: point)
        case .leftMouseDown, .rightMouseDown:
            if takesScroll(at: point) { noteActivity() }
            if !volumeCapsule.isHidden {
                let local = convert(point, from: nil)
                if !volumeCapsule.frame.contains(local) &&
                    !muteButton.frame.contains(panel.convert(local, from: self)) {
                    closeVolume()
                }
            }
        default:
            break
        }
        return false
    }

    /// A scroll over video or the OSC adjusts the session's gain, even with the OSC hidden.
    private func scrolled(_ event: NSEvent, at point: NSPoint) -> Bool {
        guard surface?.mapped == true, takesScroll(at: point) else { return false }
        let input = ScrollInput(event: event)
        if input.deltaY != 0 && !input.momentum { noteActivity() }
        if acceptsVolumeWheel, let command = VolumeScrollAccumulator.command(steps: scrollSteps.steps(for: input)) {
            if event.modifierFlags.contains(.shift), case .nudgeVolume(let delta) = command {
                dispatch?(.nudgeVolume(delta: delta / 10))
            } else {
                dispatch?(command)
            }
        }
        return true
    }

    /// Whether a scroll at `point` (window coordinates) changes the volume: over the
    /// video or these controls, but not over a view laid over them, such as the Up
    /// Next panel, which scrolls its own list.
    func takesScroll(at point: NSPoint) -> Bool {
        guard bounds.contains(convert(point, from: nil)) else { return false }
        guard let root = window?.contentView else { return true }
        guard let hit = root.hitTest(root.superview?.convert(point, from: nil) ?? point) else { return true }
        return hit === focusReturn || hit.isDescendant(of: self)
    }

    /// Register an existing player action before Full Screen, in canonical order.
    func addTrailingControl(_ control: NSButton, id: String, collapse: Int,
                            menuItem: (() -> NSMenuItem)? = nil) {
        let slot = Slot(id: id, view: control, width: 34, collapse: collapse,
                        menuItem: menuItem ?? { [weak self, weak control] in
            guard let self, let control else { return NSMenuItem() }
            return self.buttonItem(control)
        })
        slots.insert(slot, at: slots.count - 2)
        panel.addSubview(control)
        needsLayout = true
    }

    // MARK: Controls

    @objc private func togglePause() { dispatch?(.togglePause) }
    @objc private func previousItem() { onPrevious?() }
    @objc private func nextItem() { onNext?() }

    private func updateStepButtons() {
        previousButton.isEnabled = onPrevious != nil && playlistSteps.previous
        nextButton.isEnabled = onNext != nil && playlistSteps.next
    }
    @objc private func toggleMute() {
        dispatch?(.toggleMute)
        if surface?.hitTestable == true { openVolume() }
    }

    @objc private func toggleFullscreen() { window?.toggleFullScreen(nil) }

    @objc private func toggleTrailing() {
        trailing = trailing.toggled
        if let transport { updateTrailingTitle(transport) }
    }

    private func updateTrailingTitle(_ view: TransportView) {
        trailingLabel.title = trailing.text(in: view)
        trailingLabel.toolTip = trailing.toolTip
        trailingLabel.setAccessibilityLabel(trailing.toolTip)
        trailingLabel.setAccessibilityValue(trailingLabel.title)
    }

    @objc private func showSpeedMenu() {
        popUp(speedMenu(), from: speedButton)
    }

    func speedMenu() -> NSMenu {
        let menu = NSMenu()
        for choice in transport?.speeds ?? [] {
            let item = NSMenuItem(title: choice.label, action: #selector(chooseSpeed(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = choice.speed
            item.state = choice.selected ? .on : .off
            menu.addItem(item)
        }
        return menu
    }

    private func buttonItem(_ button: NSButton) -> NSMenuItem {
        let item = NSMenuItem(title: button.toolTip ?? button.title, action: button.action, keyEquivalent: "")
        item.target = button.target
        item.isEnabled = button.isEnabled
        return item
    }

    private func popUp(_ menu: NSMenu, from button: NSView) {
        closeVolume()
        menu.delegate = self
        menu.autoenablesItems = false
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: button.bounds.height + 4), in: button)
    }

    @objc private func showMore() {
        popUp(moreMenu(), from: moreButton)
    }

    func moreMenu() -> NSMenu {
        let menu = NSMenu()
        for slot in collapsed {
            guard let item = slot.menuItem?() else { continue }
            item.representedObject = slot.id
            menu.addItem(item)
        }
        let actions = moreActions?() ?? []
        if menu.numberOfItems > 0 && !actions.isEmpty { menu.addItem(.separator()) }
        actions.forEach { menu.addItem($0) }
        return menu
    }

    /// The video context menu uses existing commands and stays pinned during tracking.
    func contextMenu() -> NSMenu? {
        guard surface?.mapped == true else { return nil }
        closeVolume()
        noteActivity()
        let menu = NSMenu()
        menu.delegate = self
        menu.autoenablesItems = false
        for item in [buttonItem(playButton), buttonItem(muteButton), buttonItem(fullscreenButton)] {
            menu.addItem(item)
        }
        for item in moreActions?() ?? [] { menu.addItem(item) }
        return menu
    }

    func menuWillOpen(_ menu: NSMenu) {
        openMenus.insert(ObjectIdentifier(menu))
        noteActivity()
        setPinned(true, by: .menu)
        onMenuOpen?(menu)
    }

    func menuDidClose(_ menu: NSMenu) {
        openMenus.remove(ObjectIdentifier(menu))
        setPinned(!openMenus.isEmpty, by: .menu)
        if let window { pointerMoved(to: window.mouseLocationOutsideOfEventStream) }
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
            command = scrub.begin(at: fraction, duration: duration)
            setPinned(scrub.isDragging, by: .scrub)
        case .moved:
            command = scrub.move(to: fraction, duration: duration)
        case .ended:
            command = scrub.end(at: fraction, duration: duration)
            setPinned(false, by: .scrub)
        }
        if let command { dispatch?(command) }
        refresh()
    }

    /// The drag ended without a release.
    private func endScrub() {
        scrub.cancel()
        setPinned(false, by: .scrub)
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

    func openVolume() {
        guard surface?.hitTestable == true else { return }
        volumeClose.cancel()
        guard volumeCapsule.isHidden else { return }
        volumeCapsule.isHidden = false
        positionVolume()
        setPinned(true, by: .volume)
        updateSubtitleClearance()
    }

    private func scheduleVolumeClose() {
        guard !volumeCapsule.isHidden else { return }
        volumeClose.schedule(after: 0.22) { [weak self] in self?.closeVolume() }
    }

    @discardableResult
    func dismissVolume() -> Bool {
        guard !volumeCapsule.isHidden else { return false }
        volumeSuppressed = true
        closeVolume()
        return true
    }

    private func closeVolume() {
        volumeClose.cancel()
        volumeCapsule.isHidden = true
        volumeSlider.cancelPress()
        if let focused = window?.firstResponder as? NSView, focused.isDescendant(of: volumeCapsule) {
            returnFocus()
        }
        setPinned(false, by: .volume)
        updateSubtitleClearance()
    }

    // MARK: Layout

    private func buildPanel() {
        for view in [scrim, panel, volumeCapsule] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        for view in [panel, volumeCapsule] { view.appearance = NSAppearance(named: .darkAqua) }
        let fill = panel.widthAnchor.constraint(equalTo: widthAnchor, constant: -32)
        fill.priority = NSLayoutConstraint.Priority(NSLayoutConstraint.Priority.windowSizeStayPut.rawValue - 10)
        NSLayoutConstraint.activate([
            scrim.bottomAnchor.constraint(equalTo: bottomAnchor),
            scrim.leadingAnchor.constraint(equalTo: leadingAnchor),
            scrim.trailingAnchor.constraint(equalTo: trailingAnchor),
            scrim.heightAnchor.constraint(equalToConstant: 190),
            panel.centerXAnchor.constraint(equalTo: centerXAnchor),
            panel.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -14),
            panel.widthAnchor.constraint(lessThanOrEqualToConstant: 1280),
            panel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 8),
            panel.heightAnchor.constraint(equalToConstant: 52),
            fill,
        ])
        for label in [elapsedLabel, volumeReadout] {
            label.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium)
            label.textColor = PaletteRole.text.overVideo
            label.alignment = .center
        }
        trailingLabel.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium)
        trailingLabel.isBordered = false
        trailingLabel.refusesFirstResponder = true
        trailingLabel.toolTip = trailing.toolTip
        for (button, action) in [(previousButton, #selector(previousItem)), (playButton, #selector(togglePause)),
                                 (nextButton, #selector(nextItem)), (muteButton, #selector(toggleMute)),
                                 (fullscreenButton, #selector(toggleFullscreen)), (moreButton, #selector(showMore)),
                                 (speedButton, #selector(showSpeedMenu)), (trailingLabel, #selector(toggleTrailing))] {
            button.target = self
            button.action = action
        }
        previousButton.isEnabled = false
        nextButton.isEnabled = false
        scrubber.onScrub = { [weak self] phase, fraction in self?.scrubbed(phase, fraction) }
        volumeSlider.onFraction = { [weak self] fraction in self?.volumeDragged(to: fraction) }
        volumeSlider.onRelease = { [weak self] event in self?.pointerMoved(to: event.locationInWindow) }
        volumeSlider.onNudge = { [weak self] delta in self?.dispatch?(.nudgeVolume(delta: delta)) }
        volumeCapsule.addSubview(volumeSlider)
        volumeCapsule.addSubview(volumeReadout)

        func slot(_ id: String, _ view: NSView, width: CGFloat = 34, collapse: Int = 0,
                  menuItem: (() -> NSMenuItem)? = nil) -> Slot {
            Slot(id: id, view: view, width: width, collapse: collapse, menuItem: menuItem)
        }
        slots = [
            slot("play", playButton), slot("previous", previousButton), slot("next", nextButton),
            slot("elapsed", elapsedLabel, width: 54),
            slot("seek", scrubber, width: 156),
            slot("duration", trailingLabel, width: 62),
            slot("volume", muteButton),
            slot("speed", speedButton, width: 68, collapse: 5) { [unowned self] in
                let item = NSMenuItem(title: "Playback Speed  " + speedButton.value, action: nil, keyEquivalent: "")
                item.submenu = speedMenu()
                item.isEnabled = speedButton.isEnabled
                return item
            },
            slot("fullscreen", fullscreenButton, collapse: 4) { [unowned self] in buttonItem(fullscreenButton) },
            slot("more", moreButton),
        ]
        for slot in slots { panel.addSubview(slot.view) }
        muteButton.addSubview(volumeWick)
    }

    private func layoutSlots() {
        let gap: CGFloat = 6
        let padding: CGFloat = 10
        let available = Swift.max(0, panel.bounds.width - 2 * padding)
        var rowWidth = slots.reduce(-gap) { $0 + $1.width + gap }
        var folded: Set<String> = []
        if collapsesSlots {
            for slot in slots.filter({ $0.collapse > 0 }).sorted(by: { $0.collapse > $1.collapse }) {
                guard rowWidth > available else { break }
                folded.insert(slot.id)
                rowWidth -= slot.width + gap
            }
        }
        collapsed = slots.filter { folded.contains($0.id) }
        var x = padding
        for slot in slots {
            slot.view.isHidden = folded.contains(slot.id)
            if slot.view.isHidden {
                if let focused = window?.firstResponder as? NSView, focused === slot.view { returnFocus() }
                continue
            }
            let width = slot.id == "seek" ? Swift.max(14, slot.width + available - rowWidth) : slot.width
            let height: CGFloat = slot.id == "elapsed" ? 16 : 34
            slot.view.frame = NSRect(x: x, y: (panel.bounds.height - height) / 2, width: width, height: height)
            x += width + gap
        }
        volumeWick.frame = NSRect(x: 8, y: 3, width: 18, height: 3)
        if !volumeCapsule.isHidden { positionVolume() }
    }

    private func positionVolume() {
        let width: CGFloat = 198
        let speaker = muteButton.convert(muteButton.bounds, to: self)
        let x = Swift.min(Swift.max(8, speaker.midX - width / 2), bounds.width - width - 8)
        volumeCapsule.frame = NSRect(x: x, y: panel.frame.maxY + 10, width: width, height: 36)
        volumeSlider.frame = NSRect(x: 10, y: 3, width: 132, height: 30)
        volumeReadout.frame = NSRect(x: 145, y: 10, width: 47, height: 16)
    }

    private static func symbol(_ name: String, _ label: String) -> NSImage? {
        NSImage(systemSymbolName: name, accessibilityDescription: label)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 14, weight: .medium))
    }

    /// A borderless 26-point button showing the SF Symbol `name` in the text colour.
    static func iconButton(_ name: String, _ label: String) -> NSButton {
        let button = TransportIconButton()
        button.title = ""
        button.isBordered = false
        button.refusesFirstResponder = true
        button.imagePosition = .imageOnly
        button.contentTintColor = PaletteRole.text.overVideo
        setSymbol(button, name, label)
        button.frame.size = NSSize(width: 34, height: 34)
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

/// Draws the transport button's highlighted state and keyboard focus.
private final class TransportIconButton: NSButton {
    init() {
        super.init(frame: .zero)
        focusRingType = .none
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func draw(_ dirtyRect: NSRect) {
        let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 6, yRadius: 6)
        if isHighlighted {
            PaletteRole.selected.color.setFill()
            shape.fill()
        }
        super.draw(dirtyRect)
        if window?.isKeyWindow == true, window?.firstResponder === self {
            PaletteRole.focus.color.setStroke()
            shape.lineWidth = 2
            shape.stroke()
        }
    }

    override func becomeFirstResponder() -> Bool {
        let accepted = super.becomeFirstResponder()
        needsDisplay = true
        return accepted
    }

    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        needsDisplay = true
        return resigned
    }
}
