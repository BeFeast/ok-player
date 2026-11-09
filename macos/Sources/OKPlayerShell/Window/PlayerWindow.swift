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

    /// Offered every key press no view and none of the window's own keys took;
    /// returns whether it was used.
    var onOtherKey: ((NSEvent) -> Bool)?

    /// The window is in native fullscreen. (Tests stand in a flagged window.)
    var isInFullscreen: Bool { styleMask.contains(.fullScreen) }

    override func sendEvent(_ event: NSEvent) {
        if event.type == .keyDown { onKeyDown?() }
        super.sendEvent(event)
    }

    override func keyDown(with event: NSEvent) {
        if let command = WindowKeys.command(forKey: event.charactersIgnoringModifiers, keyCode: event.keyCode,
                                            modifiers: event.modifierFlags, isRepeat: event.isARepeat,
                                            isFullscreen: isInFullscreen),
           onCommand?(command) == true {
            return
        }
        if onOtherKey?(event) == true { return }
        super.keyDown(with: event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if let command = WindowKeys.command(forKeyEquivalent: event.charactersIgnoringModifiers,
                                            modifiers: event.modifierFlags),
           onCommand?(command) == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    /// Escape (and Command-period) reach the window as `cancelOperation:` when
    /// no view takes them. In fullscreen that leaves fullscreen.
    override func cancelOperation(_ sender: Any?) {
        _ = onCommand?(.exitFullscreen)
    }
}

/// The strip along the top of the window, under the transparent title bar: the
/// media title over a soft scrim, so it reads over any video. It takes no
/// clicks; the title bar above it drags the window.
final class TitleStripView: NSView {
    private let label = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        label.font = NSFont.titleBarFont(ofSize: 0)
        label.textColor = .white
        label.alignment = .center
        label.lineBreakMode = .byTruncatingMiddle
        label.translatesAutoresizingMaskIntoConstraints = false
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            // Clear of the traffic lights on the left, and as far from the right.
            label.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 84),
            label.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -84),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    /// The media title; empty shows neither text nor scrim.
    var title: String {
        get { label.stringValue }
        set {
            guard newValue != label.stringValue else { return }
            label.stringValue = newValue
            needsDisplay = true
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !label.stringValue.isEmpty else { return }
        // Darkest along the top edge, clear where the strip meets the video.
        NSGradient(starting: NSColor.black.withAlphaComponent(0.45), ending: NSColor.black.withAlphaComponent(0))?
            .draw(in: bounds, angle: -90)
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
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
    let titleStrip = TitleStripView()
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
        window.collectionBehavior.insert(.fullScreenPrimary)
        window.onCommand = { [weak self] command in self?.perform(command) ?? false }
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(willEnterFullScreen(_:)),
                           name: NSWindow.willEnterFullScreenNotification, object: window)
        center.addObserver(self, selector: #selector(didEnterFullScreen(_:)),
                           name: NSWindow.didEnterFullScreenNotification, object: window)
        center.addObserver(self, selector: #selector(didExitFullScreen(_:)),
                           name: NSWindow.didExitFullScreenNotification, object: window)
    }

    deinit { NotificationCenter.default.removeObserver(self) }

    /// Lay the title strip over the top of `root` (call it after `videoArea` is
    /// added, so the strip is above it), and take the clicks of `videoArea` when
    /// it is the video view. The fit and the resize keep `videoArea`, not the
    /// whole content, at the video's aspect.
    func install(in root: NSView, videoArea: NSView) {
        self.videoArea = videoArea
        titleStrip.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(titleStrip)
        NSLayoutConstraint.activate([
            titleStrip.topAnchor.constraint(equalTo: root.topAnchor),
            titleStrip.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            titleStrip.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            // Under a transparent title bar the safe area starts below the bar.
            titleStrip.bottomAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor),
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
        clicks.onToggleFullscreen = { [weak self] in _ = self?.perform(.toggleFullscreen) }
        self.clicks = clicks
    }

    /// The session reported a batch; `snapshot` is the state after it.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        titleStrip.title = snapshot.title ?? ""
        aspect = VideoAreaAspect.lock(for: snapshot.video)
        // The session knows which source the size belongs to and answers with a
        // fit once per opened source, so asking on every batch with video is safe.
        if snapshot.video != nil { takeInitialFit() }
    }

    /// The frame size a resize towards `frameSize` settles on: while video
    /// shows, the video area keeps the video's aspect. Called from the window
    /// delegate's `windowWillResize(_:to:)`.
    func windowWillResize(to frameSize: NSSize) -> NSSize {
        guard let aspect, !window.isInFullscreen else { return frameSize }
        let size = VideoAreaAspect.constrain(proposed: contentSize(forFrameSize: frameSize),
                                             current: contentSize(forFrameSize: window.frame.size),
                                             video: aspect, chrome: videoChrome,
                                             minimum: contentSize(forFrameSize: window.minSize))
        return window.frameRect(forContentRect: NSRect(origin: .zero, size: size)).size
    }

    /// Carry out a window command; returns whether it did anything.
    @discardableResult
    func perform(_ command: WindowCommand) -> Bool {
        guard command.togglesFullscreen(isFullscreen: window.isInFullscreen) else { return false }
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
        guard let rules, let screen = window.screen ?? NSScreen.main,
              let content = rules.initialWindowFit(backingScale: screen.backingScaleFactor,
                                                   visibleFrame: screen.visibleFrame,
                                                   minSize: contentSize(forFrameSize: window.minSize),
                                                   chrome: videoChrome),
              !window.isInFullscreen else { return }
        window.setFrame(window.frameRect(forContentRect: content), display: true)
        lastFit = content
    }

    private var trafficLights: [NSButton] {
        [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
            .compactMap { window.standardWindowButton($0) }
    }

    /// Show or hide the traffic lights and the title strip. Hidden ones are
    /// `isHidden`, not only transparent: a transparent button still takes clicks.
    private func applyChrome(animated: Bool) {
        chromeChanges += 1
        let change = chromeChanges
        let showLights = chrome.trafficLightsVisible
        let showStrip = chrome.titleStripVisible
        let lights = trafficLights
        let strip = titleStrip
        // Whatever fades in is unhidden first, so the fade can be seen.
        if showLights { for button in lights { button.isHidden = false } }
        if showStrip { strip.isHidden = false }
        let hideFaded = {
            if !showLights { for button in lights { button.isHidden = true } }
            if !showStrip { strip.isHidden = true }
        }
        guard animated else {
            for button in lights { button.alphaValue = showLights ? 1 : 0 }
            strip.alphaValue = showStrip ? 1 : 0
            hideFaded()
            return
        }
        NSAnimationContext.runAnimationGroup({ context in
            context.duration = 0.2
            for button in lights { button.animator().alphaValue = showLights ? 1 : 0 }
            strip.animator().alphaValue = showStrip ? 1 : 0
        }, completionHandler: { [weak self] in
            guard let self, self.chromeChanges == change else { return }
            hideFaded()
        })
    }

    @objc private func willEnterFullScreen(_ notification: Notification) {
        chrome.fullscreen = true
        applyChrome(animated: false)
    }

    @objc private func didEnterFullScreen(_ notification: Notification) {
        onFullscreenChanged?(true)
        // The smoke leaves again with Escape, once the new Space has settled.
        guard smokeRoundTrip != nil else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            self?.smokeSendKey("\u{1b}", keyCode: WindowKeys.escapeKeyCode)
        }
    }

    @objc private func didExitFullScreen(_ notification: Notification) {
        chrome.fullscreen = false
        applyChrome(animated: false)
        onFullscreenChanged?(false)
        finishSmokeRoundTrip(nil)
    }
}

// The smoke's checks (`--smoke-test`, run by the macOS workflows).
extension PlayerWindowController {
    /// What is wrong with the window after a file with `video` opened, or nil
    /// when the fit sized it, the video view has the video's aspect, a resize
    /// keeps that aspect, and the title strip has room under the title bar.
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
        guard titleStrip.frame.height > 0 else { return "The title strip has no height under the title bar" }
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

    /// `fit=WxH video_area=WxH,WxH aspect=W:H title_strip=H` for the smoke line:
    /// the fitted content, the video view after the fit and after a resize.
    var smokeFitSummary: String {
        let fit = lastFit.map { "\(Int($0.width))x\(Int($0.height))" } ?? "none"
        let areas = smokeVideoAreas.map { "\(Int($0.width))x\(Int($0.height))" }.joined(separator: ",")
        let lock = aspect.map { "\(Int($0.width)):\(Int($0.height))" } ?? "none"
        return "fit=\(fit) video_area=\(areas.isEmpty ? "none" : areas) aspect=\(lock) "
            + "title_strip=\(Int(titleStrip.frame.height))"
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
