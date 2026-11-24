import AppKit

/// The player window: the content view runs under a transparent title bar, and
/// the window handles F, Escape and Control-Command-F itself. Create it with
/// `PlayerWindowController`, which applies what the keys ask for.
class PlayerWindow: NSWindow {
    static let style: NSWindow.StyleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]

    /// Carries out a command from a key; returns whether it did anything.
    var onCommand: ((WindowCommand) -> Bool)?

    /// Called for every key press the window receives, before anything handles it:
    /// the transport controls count it as activity.
    var onKeyDown: (() -> Void)?
    var onPlayerInput: ((NSEvent) -> Bool)?
    var inputRouter: PlayerInputRouter?
    private var dispatchingWindowEvent: NSEvent?

    /// The single-key shortcuts (`PlayerCommands.handle`), offered every key press
    /// no view took; returns whether it ran a command.
    var shortcutKeys: ((NSEvent) -> Bool)?

    /// The window is in native fullscreen. (Tests stand in a flagged window.)
    var isInFullscreen: Bool { styleMask.contains(.fullScreen) }
    var isMiniPlayer = false

    override var canBecomeKey: Bool { isMiniPlayer || super.canBecomeKey }
    override var canBecomeMain: Bool { isMiniPlayer || super.canBecomeMain }

    override func toggleFullScreen(_ sender: Any?) {
        if isMiniPlayer {
            _ = onCommand?(.toggleFullscreen)
            return
        }
        super.toggleFullScreen(sender)
    }

    override func sendEvent(_ event: NSEvent) {
        if event.type == .keyDown {
            onKeyDown?()
            if inputRouter?.handleKey(event, in: self) == true { return }
            if inputRouter != nil, handleFullscreenEquivalent(event) { return }
        }
        if inputRouter?.handlePointer(event, in: self) == true || onPlayerInput?(event) == true { return }
        let previous = dispatchingWindowEvent
        dispatchingWindowEvent = event
        defer { dispatchingWindowEvent = previous }
        super.sendEvent(event)
    }

    override func keyDown(with event: NSEvent) {
        if let inputRouter {
            // A key forwarded from a native transient (such as the track
            // popover) may enter the responder chain without sendEvent.
            if dispatchingWindowEvent !== event, inputRouter.handleKey(event, in: self) { return }
            super.keyDown(with: event)
            return
        }
        if shortcutKeys?(event) == true { return }
        if let command = WindowKeys.command(forKey: event.charactersIgnoringModifiers, keyCode: event.keyCode,
                                            modifiers: event.modifierFlags, isRepeat: event.isARepeat,
                                            isFullscreen: isInFullscreen),
           onCommand?(command) == true {
            return
        }
        super.keyDown(with: event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.modifierFlags.contains(.command), inputRouter?.handleKey(event, in: self) == true { return true }
        if handleFullscreenEquivalent(event) { return true }
        return super.performKeyEquivalent(with: event)
    }

    private func handleFullscreenEquivalent(_ event: NSEvent) -> Bool {
        guard attachedSheet == nil else { return false }
        if let command = WindowKeys.command(forKeyEquivalent: event.charactersIgnoringModifiers,
                                            modifiers: event.modifierFlags) {
            return event.isARepeat || onCommand?(command) == true
        }
        return false
    }

    /// Escape (and Command-period) reach the window as `cancelOperation:` when
    /// no view takes them. In fullscreen that leaves fullscreen.
    override func cancelOperation(_ sender: Any?) {
        if inputRouter?.cancel(in: self) == true { return }
        _ = onCommand?(.exitFullscreen)
    }
}

/// The player window's behaviour (#881): native fullscreen from F, Escape,
/// Control-Command-F and a double click on the video; the window fitted to the
/// video when a file opens; the video area's aspect kept while video shows; and
/// the transparent title bar whose traffic lights and title follow the OSC.
///
/// The fit, the fit each opened source is owed and the click rules are
/// okp-core's, asked through the session (`WindowRules`).
final class PlayerWindowController: NSObject, WindowChromeVisibility {
    let window: PlayerWindow
    let titleChrome = TitleChrome()
    let pinButton = NSButton()
    private let pinAccessory = NSTitlebarAccessoryViewController()
    private(set) var isPinned = false
    private var idleFrame: CGRect?
    private var normalFrame: CGRect?
    private var fullscreenTransition = false
    var onExitMiniPlayer: (() -> Void)?
    var canEnterMiniPlayer: Bool {
        !fullscreenTransition && !window.isInFullscreen && window.attachedSheet == nil
    }
    /// The menu and header expose the same player window preference.
    var onPinChanged: ((Bool) -> Void)?
    var dismissTransient: (() -> Bool)?
    /// Optional work area supplied by hosted verification.
    var verificationWorkArea: CGRect?
    var applyInitialFit = true
    var allowVideoFullscreen = true
    var applyFloatingLevel = true
    private var workArea: CGRect? { verificationWorkArea ?? window.screen?.visibleFrame ?? NSScreen.main?.visibleFrame }
    /// Called when the window has entered (true) or left (false) fullscreen.
    var onFullscreenChanged: ((Bool) -> Void)?
    /// Enters or leaves native fullscreen. Tests replace it, so they never move
    /// a window to a new Space.
    var toggleFullscreen: () -> Void
    /// The content frame the latest fit gave the window.
    private(set) var lastFit: CGRect?
    /// The aspect the video area keeps while video shows.
    private(set) var aspect: CGSize?

    private weak var rules: WindowRules?
    private weak var videoArea: NSView?
    private var clicks: VideoClickArbiter?
    private var chrome = WindowChromeState()
    /// Counts chrome changes, so a fade that finishes after a newer change
    /// leaves the newer one alone.
    private var chromeChanges = 0
    private var smokeRoundTrip: ((String?) -> Void)?
    private var smokeVideoAreas: [CGSize] = []

    convenience init(contentRect: NSRect) {
        self.init(window: PlayerWindow(contentRect: contentRect, styleMask: PlayerWindow.style,
                                       backing: .buffered, defer: false))
    }

    init(window: PlayerWindow) {
        self.window = window
        toggleFullscreen = { [weak window] in window?.toggleFullScreen(nil) }
        super.init()
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.titlebarSeparatorStyle = .none
        let accessoryView = NSView(frame: NSRect(x: 0, y: 0, width: 44, height: 24))
        pinButton.frame = accessoryView.bounds.insetBy(dx: 5, dy: 1)
        pinButton.isBordered = false
        pinButton.setButtonType(.onOff)
        pinButton.target = self
        pinButton.action = #selector(togglePin)
        accessoryView.addSubview(pinButton)
        titleChrome.pinButton = pinButton
        titleChrome.trafficLights = { [weak self] in self?.trafficLights ?? [] }
        pinAccessory.view = accessoryView
        pinAccessory.layoutAttribute = .right
        window.addTitlebarAccessoryViewController(pinAccessory)
        updatePin()
        window.collectionBehavior.insert(.fullScreenPrimary)
        window.onCommand = { [weak self] command in self?.perform(command) ?? false }
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(willEnterFullScreen(_:)),
                           name: NSWindow.willEnterFullScreenNotification, object: window)
        center.addObserver(self, selector: #selector(didEnterFullScreen(_:)),
                           name: NSWindow.didEnterFullScreenNotification, object: window)
        center.addObserver(self, selector: #selector(willExitFullScreen(_:)),
                           name: NSWindow.willExitFullScreenNotification, object: window)
        center.addObserver(self, selector: #selector(didExitFullScreen(_:)),
                           name: NSWindow.didExitFullScreenNotification, object: window)
    }

    deinit { NotificationCenter.default.removeObserver(self) }

    /// Put the title over the full-window content and connect playback clicks.
    func install(in root: NSView, videoArea: NSView) {
        self.videoArea = videoArea
        titleChrome.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(titleChrome)
        NSLayoutConstraint.activate([
            titleChrome.topAnchor.constraint(equalTo: root.topAnchor),
            titleChrome.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            titleChrome.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            titleChrome.trailingAnchor.constraint(equalTo: root.trailingAnchor),
        ])
        (videoArea as? VideoView)?.onClick = { [weak self] clickCount in
            self?.clicks?.released(clickCount: clickCount)
        }
        applyChrome(animated: false)
    }

    /// Ask `rules` for fits and click meanings; a single click calls
    /// `togglePause`. `schedule` holds a single click back for the double-click
    /// interval.
    func connect(_ rules: WindowRules, togglePause: @escaping () -> Void,
                 schedule: @escaping VideoClickArbiter.Schedule = VideoClickArbiter.onMainQueue) {
        self.rules = rules
        let clicks = VideoClickArbiter(
            interval: { NSEvent.doubleClickInterval },
            classify: { [weak rules] clickCount in rules?.videoClickIntent(clickCount: clickCount) ?? .ignore },
            schedule: schedule)
        clicks.onTogglePause = togglePause
        clicks.onToggleFullscreen = { [weak self] in
            guard let self else { return }
            if self.window.isMiniPlayer {
                self.onExitMiniPlayer?()
                return
            }
            guard self.allowVideoFullscreen else { return }
            self.perform(.toggleFullscreen)
        }
        self.clicks = clicks
    }

    func miniPlayerChanged() {
        clicks?.cancel()
        applyChrome(animated: false)
    }

    func cancelVideoClick() { clicks?.cancel() }

    func compactVideoClick(count: Int) { clicks?.released(clickCount: count) }

    /// The session reported a batch; `snapshot` is the state after it.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        if let source = snapshot.source {
            if !chrome.hasMedia && !chrome.fullscreen { idleFrame = window.frame }
            titleChrome.title = snapshot.title ?? (source.location as NSString).lastPathComponent
        } else if chrome.hasMedia {
            titleChrome.title = "OK Player"
        }
        let overVideo = snapshot.video != nil
        if titleChrome.overVideo != overVideo {
            titleChrome.overVideo = overVideo
            updatePin()
        }
        let hasMedia = snapshot.source != nil
        if chrome.hasMedia != hasMedia {
            chrome.hasMedia = hasMedia
            if !hasMedia { lastFit = nil }
            applyChrome(animated: false)
        }
        let name = hasMedia ? "\(titleChrome.title) — OK Player" : "OK Player"
        if window.title != name { window.title = name }
        aspect = VideoAreaAspect.lock(for: snapshot.video)
        if !hasMedia { restoreIdleFrame() }
        // The session owns which source is owed a fit.
        if snapshot.video != nil { takeInitialFit() }
    }

    private func restoreIdleFrame() {
        guard !chrome.hasMedia, !chrome.fullscreen, !fullscreenTransition, let frame = idleFrame else { return }
        idleFrame = nil
        restore(frame)
    }

    private func restore(_ frame: CGRect) {
        let safe = workArea.map { frame.fitted(inside: $0) } ?? frame
        if window.frame != safe { window.setFrame(safe, display: true) }
    }

    var canTogglePin: Bool {
        !chrome.fullscreen && !fullscreenTransition && !window.isInFullscreen && !window.isMiniPlayer
    }

    @objc func togglePin() {
        guard canTogglePin else { return }
        isPinned.toggle()
        updatePin()
        onPinChanged?(isPinned)
    }

    private func updatePin() {
        let level: NSWindow.Level = window.isMiniPlayer || (isPinned && canTogglePin) ? .floating : .normal
        if applyFloatingLevel, window.level != level { window.level = level }
        pinButton.state = isPinned ? .on : .off
        let label = isPinned ? "Disable Always on Top" : "Always on Top"
        pinButton.toolTip = label
        pinButton.setAccessibilityLabel(label)
        pinButton.image = NSImage(systemSymbolName: isPinned ? "pin.fill" : "pin", accessibilityDescription: label)
        pinButton.contentTintColor = isPinned
            ? (titleChrome.overVideo ? PaletteRole.link.overVideo : PaletteRole.link.color)
            : (titleChrome.overVideo ? PaletteRole.onAccent.overVideo : PaletteRole.text.color)
    }

    /// The frame size a resize towards `frameSize` settles on: while video
    /// shows, the video area keeps the video's aspect. Called from the window
    /// delegate's `windowWillResize(_:to:)`.
    func windowWillResize(to frameSize: NSSize) -> NSSize {
        guard !chrome.fullscreen, !window.isInFullscreen, !fullscreenTransition, !window.isMiniPlayer else {
            return frameSize
        }
        let proposed = contentSize(forFrameSize: frameSize)
        let minimum = contentSize(forFrameSize: window.minSize)
        let maximum = workArea.map { contentSize(forFrameSize: $0.size) }
        let size: CGSize
        if let aspect {
            size = VideoAreaAspect.constrain(proposed: proposed, current: contentSize(forFrameSize: window.frame.size),
                                             video: aspect, chrome: videoChrome, minimum: minimum, maximum: maximum)
        } else if let maximum {
            size = CGSize(width: min(proposed.width, maximum.width), height: min(proposed.height, maximum.height))
        } else {
            size = proposed
        }
        return window.frameRect(forContentRect: NSRect(origin: .zero, size: size)).size
    }

    /// AppKit chooses the frame origin from the edge being dragged. After the
    /// gesture, keep the resulting title and controls on the active display.
    func windowDidEndLiveResize() {
        guard !chrome.fullscreen, !fullscreenTransition, !window.isMiniPlayer else { return }
        restore(window.frame)
    }

    /// Carry out a window command; returns whether it did anything.
    @discardableResult
    func perform(_ command: WindowCommand) -> Bool {
        if command == .exitFullscreen, dismissTransient?() == true { return true }
        if window.isMiniPlayer {
            onExitMiniPlayer?()
            if command == .exitFullscreen { return !window.isMiniPlayer }
        }
        guard !window.isMiniPlayer, !fullscreenTransition,
              command.togglesFullscreen(isFullscreen: window.isInFullscreen) else { return false }
        toggleFullscreen()
        return true
    }

    func oscVisibilityChanged(_ visible: Bool, animated: Bool) {
        guard chrome.oscVisible != visible else { return }
        chrome.oscVisible = visible
        applyChrome(animated: animated)
    }

    /// The room the window's own controls take outside the video area: the
    /// content's size less the video area's, as laid out now.
    var videoChrome: CGSize {
        guard let videoArea, let content = window.contentView else { return .zero }
        content.layoutSubtreeIfNeeded()
        let area = videoArea.convert(videoArea.bounds, to: content).size
        return CGSize(width: max(0, content.bounds.width - area.width),
                      height: max(0, content.bounds.height - area.height))
    }

    private func contentSize(forFrameSize size: NSSize) -> CGSize {
        window.contentRect(forFrameRect: NSRect(origin: .zero, size: size)).size
    }

    /// Take the fit the current source is owed, if the session has one. In
    /// fullscreen the fit is spent without resizing: like core's initial fit, it
    /// never leaves a window state the user chose.
    private func takeInitialFit() {
        guard let rules, let area = workArea, let screen = window.screen ?? NSScreen.main,
              !fullscreenTransition,
              let content = rules.initialWindowFit(backingScale: screen.backingScaleFactor,
                                                   visibleFrame: area,
                                                   minSize: contentSize(forFrameSize: window.minSize),
                                                   chrome: videoChrome),
              !window.isInFullscreen, !window.isMiniPlayer, applyInitialFit else { return }
        var target = aspectFittedFrame(content: content, in: area)
        window.setFrame(target, display: true)
        if target.size != window.frame.size {
            // AppKit may enlarge a narrow frame for its native title controls.
            target = aspectFittedFrame(content: window.contentRect(forFrameRect: window.frame), in: area)
            window.setFrame(target, display: true)
        }
        lastFit = window.contentRect(forFrameRect: target)
    }

    private func aspectFittedFrame(content: CGRect, in area: CGRect) -> CGRect {
        let size = aspect.map { video in
            // The core reserves desktop space in the initial fit. A narrow
            // title-safe window can use that reserve to avoid pillarboxing.
            VideoAreaAspect.constrain(proposed: content.size, current: content.size, video: video,
                                      chrome: videoChrome, minimum: contentSize(forFrameSize: window.minSize),
                                      maximum: contentSize(forFrameSize: area.size))
        } ?? content.size
        let centered = CGRect(x: (content.midX - size.width / 2).rounded(),
                              y: (content.midY - size.height / 2).rounded(), width: size.width, height: size.height)
        return window.frameRect(forContentRect: centered).fitted(inside: area)
    }

    var trafficLights: [NSButton] {
        [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
            .compactMap { window.standardWindowButton($0) }
    }

    /// Show or hide the traffic lights, title and pin. Hidden ones are
    /// `isHidden`, not only transparent: a transparent button still takes clicks.
    private func applyChrome(animated: Bool) {
        chromeChanges += 1
        let change = chromeChanges
        updatePin()
        pinAccessory.isHidden = chrome.fullscreen || window.isMiniPlayer
        let showLights = chrome.trafficLightsVisible
        let showTitle = chrome.titleChromeVisible && !window.isMiniPlayer
        let lights = trafficLights
        titleChrome.needsLayout = true
        let views: [(NSView, Bool)] = lights.map { ($0 as NSView, showLights) }
            + [(pinButton, showTitle), (titleChrome, showTitle)]
        if !showTitle, window.firstResponder === pinButton { window.makeFirstResponder(videoArea) }
        pinButton.isEnabled = showTitle && canTogglePin
        for (view, show) in views where show { view.isHidden = false }
        guard animated, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else {
            for (view, show) in views {
                view.alphaValue = show ? 1 : 0
                view.isHidden = !show
            }
            return
        }
        NSAnimationContext.runAnimationGroup({ context in
            context.duration = 0.18
            for (view, show) in views { view.animator().alphaValue = show ? 1 : 0 }
        }, completionHandler: { [weak self] in
            guard let self, self.chromeChanges == change else { return }
            for (view, show) in views where !show { view.isHidden = true }
        })
    }

    @objc private func willEnterFullScreen(_ notification: Notification) {
        normalFrame = window.frame
        fullscreenTransition = true
        chrome.fullscreen = true
        applyChrome(animated: false)
    }

    @objc private func didEnterFullScreen(_ notification: Notification) {
        fullscreenTransition = false
        onFullscreenChanged?(true)
        // The smoke leaves again with Escape, once the new Space has settled.
        guard smokeRoundTrip != nil else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            self?.smokeSendKey("\u{1b}", keyCode: WindowKeys.escapeKeyCode)
        }
    }

    @objc private func willExitFullScreen(_ notification: Notification) {
        fullscreenTransition = true
    }

    @objc private func didExitFullScreen(_ notification: Notification) {
        fullscreenTransition = false
        chrome.fullscreen = false
        applyChrome(animated: false)
        if let frame = normalFrame { restore(frame) }
        normalFrame = nil
        restoreIdleFrame()
        onFullscreenChanged?(false)
        finishSmokeRoundTrip(nil)
    }

    func fullscreenFailed() {
        fullscreenTransition = false
        chrome.fullscreen = window.isInFullscreen
        if !chrome.fullscreen { normalFrame = nil }
        applyChrome(animated: false)
        restoreIdleFrame()
        finishSmokeRoundTrip("AppKit could not change fullscreen state")
    }
}

// The smoke's checks (`--smoke-test`, run by the macOS workflows).
extension PlayerWindowController {
    /// What is wrong with the window after a file with `video` opened, or nil
    /// when the fit sized it, the video view has the video's aspect, a resize
    /// keeps that aspect, and the video reaches every edge of the content.
    /// Leaves the window at the resized size.
    func smokeFitFailure(video: VideoGeometry?) -> String? {
        guard let video else { return "The session reported no video size" }
        guard let fit = lastFit else { return "The window was not fitted to the \(video.width)x\(video.height) video" }
        let content = contentSize(forFrameSize: window.frame.size)
        guard content == fit.size else { return "The window content is \(content), not the fitted \(fit.size)" }
        let expected = CGSize(width: CGFloat(video.width), height: CGFloat(video.height))
        if let failure = smokeVideoAreaFailure(expected, after: "the fit") { return failure }
        // Grow the window the way a drag on its corner proposes, through the
        // same rule the window delegate applies.
        let proposed = NSSize(width: window.frame.width + 160, height: window.frame.height + 20)
        window.setFrame(NSRect(origin: window.frame.origin, size: windowWillResize(to: proposed)), display: true)
        if let failure = smokeVideoAreaFailure(expected, after: "a resize") { return failure }
        guard videoChrome == .zero else { return "The video does not fill the window content" }
        return nil
    }

    private func smokeVideoAreaFailure(_ video: CGSize, after step: String) -> String? {
        guard let videoArea, let content = window.contentView else { return "The window has no video view" }
        content.layoutSubtreeIfNeeded()
        let area = videoArea.bounds.size
        smokeVideoAreas.append(area)
        guard area.width > 0, area.height > 0, abs(area.width - area.height * video.width / video.height) <= 1 else {
            return "After \(step) the video view is \(Int(area.width))x\(Int(area.height)), "
                + "not at the video's \(Int(video.width)):\(Int(video.height)) aspect"
        }
        return nil
    }

    /// `fit=WxH video_area=WxH,WxH aspect=W:H` for the smoke line:
    /// the fitted content, the video view after the fit and after a resize.
    var smokeFitSummary: String {
        let fit = lastFit.map { "\(Int($0.width))x\(Int($0.height))" } ?? "none"
        let areas = smokeVideoAreas.map { "\(Int($0.width))x\(Int($0.height))" }.joined(separator: ",")
        let lock = aspect.map { "\(Int($0.width)):\(Int($0.height))" } ?? "none"
        return "fit=\(fit) video_area=\(areas.isEmpty ? "none" : areas) aspect=\(lock)"
    }

    /// Press F on the video view to enter fullscreen and Escape to leave it,
    /// as key events through the window. Calls `completion` once the window is
    /// back, or with what went wrong.
    func smokeFullscreenRoundTrip(timeout: TimeInterval, completion: @escaping (String?) -> Void) {
        smokeRoundTrip = completion
        DispatchQueue.main.asyncAfter(deadline: .now() + timeout) { [weak self] in
            guard let self else { return }
            self.finishSmokeRoundTrip("F and Escape did not enter and leave fullscreen within \(Int(timeout)) s "
                                      + "(fullscreen now: \(self.window.isInFullscreen))")
        }
        guard let videoArea, window.makeFirstResponder(videoArea) else {
            finishSmokeRoundTrip("The video view does not take key presses")
            return
        }
        smokeSendKey("f", keyCode: 3)
    }

    /// A key press delivered the way AppKit delivers one to this window: to its
    /// first responder, then up the responder chain.
    private func smokeSendKey(_ characters: String, keyCode: UInt16) {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil,
                                           characters: characters, charactersIgnoringModifiers: characters,
                                           isARepeat: false, keyCode: keyCode) else {
            finishSmokeRoundTrip("Could not make a key event for \(characters)")
            return
        }
        window.sendEvent(event)
    }

    private func finishSmokeRoundTrip(_ failure: String?) {
        guard let completion = smokeRoundTrip else { return }
        smokeRoundTrip = nil
        // Not inside AppKit's fullscreen notification: the smoke closes the window next.
        DispatchQueue.main.async { completion(failure) }
    }
}
