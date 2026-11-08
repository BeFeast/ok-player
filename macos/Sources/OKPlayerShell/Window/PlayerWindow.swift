import AppKit

/// The player window: the content view runs under a transparent title bar, and
/// the window handles F, Escape and Control-Command-F itself. Create it with
/// `PlayerWindowController`, which applies what the keys ask for.
final class PlayerWindow: NSWindow {
    static let style: NSWindow.StyleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]

    /// Carries out a command from a key; returns whether it did anything.
    var onCommand: ((WindowCommand) -> Bool)?

    var isInFullscreen: Bool { styleMask.contains(.fullScreen) }

    override func keyDown(with event: NSEvent) {
        if let command = WindowKeys.command(forKey: event.charactersIgnoringModifiers, keyCode: event.keyCode,
                                            modifiers: event.modifierFlags, isRepeat: event.isARepeat,
                                            isFullscreen: isInFullscreen),
           onCommand?(command) == true {
            return
        }
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
/// video when a file opens; the aspect kept while video shows; and the
/// transparent title bar whose traffic lights and title follow the OSC.
///
/// The fit and the click rules are okp-core's, asked through the session.
final class PlayerWindowController: NSObject, WindowChromeVisibility {
    let window: PlayerWindow
    let titleStrip = TitleStripView()
    /// Called when the window has entered (true) or left (false) fullscreen.
    var onFullscreenChanged: ((Bool) -> Void)?
    /// The frame the latest fit gave the window.
    private(set) var lastFit: CGRect?

    private weak var client: SessionClient?
    private var clicks: VideoClickArbiter?
    private var initialFit = InitialWindowFit()
    private var aspect: CGSize?
    private var chrome = WindowChromeState()
    private var smokeRoundTrip: ((String?) -> Void)?

    init(contentRect: NSRect) {
        window = PlayerWindow(contentRect: contentRect, styleMask: PlayerWindow.style, backing: .buffered, defer: false)
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

    /// Lay the title strip over the top of `root` (call it after the video view
    /// is added, so the strip is above it) and take the video's clicks.
    func install(in root: NSView, video: VideoView?) {
        titleStrip.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(titleStrip)
        NSLayoutConstraint.activate([
            titleStrip.topAnchor.constraint(equalTo: root.topAnchor),
            titleStrip.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            titleStrip.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            // Under a transparent title bar the safe area starts below the bar.
            titleStrip.bottomAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor),
        ])
        video?.onClick = { [weak self] clickCount in self?.clicks?.released(clickCount: clickCount) }
        applyChrome(animated: false)
    }

    /// Ask `client` for fits and click rules; a single click calls `togglePause`.
    func connect(_ client: SessionClient, togglePause: @escaping () -> Void) {
        self.client = client
        let clicks = VideoClickArbiter(
            interval: { NSEvent.doubleClickInterval },
            classify: { [weak client] clickCount in client?.videoClickIntent(clickCount: clickCount) ?? .ignore },
            schedule: VideoClickArbiter.onMainQueue)
        clicks.onTogglePause = togglePause
        clicks.onToggleFullscreen = { [weak self] in _ = self?.perform(.toggleFullscreen) }
        self.clicks = clicks
    }

    /// A file is being opened: fit the window to its video once its size is known.
    func beginOpen() {
        initialFit.beginOpen()
    }

    /// The session reported a batch; `snapshot` is the state after it.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        titleStrip.title = snapshot.title ?? ""
        let lock = WindowAspect.lock(for: snapshot.video)
        if lock != aspect {
            aspect = lock
            if let lock {
                window.contentAspectRatio = lock
            } else {
                // Resize increments and the aspect ratio exclude each other.
                window.contentResizeIncrements = NSSize(width: 1, height: 1)
            }
        }
        if let video = initialFit.take(snapshot) { fit(to: video) }
    }

    /// Carry out a window command; returns whether it did anything.
    @discardableResult
    func perform(_ command: WindowCommand) -> Bool {
        guard command.togglesFullscreen(isFullscreen: window.isInFullscreen) else { return false }
        window.toggleFullScreen(nil)
        return true
    }

    func oscVisibilityChanged(_ visible: Bool, animated: Bool) {
        guard chrome.oscVisible != visible else { return }
        chrome.oscVisible = visible
        applyChrome(animated: animated)
    }

    /// Fullscreen keeps the window's own size; a fit applies only to a window.
    private func fit(to video: VideoGeometry) {
        guard !window.isInFullscreen, let client, let screen = window.screen ?? NSScreen.main,
              let frame = client.windowFit(video: video, backingScale: screen.backingScaleFactor,
                                           visibleFrame: screen.visibleFrame, minSize: window.minSize) else { return }
        window.setFrame(frame, display: true)
        lastFit = frame
    }

    private func applyChrome(animated: Bool) {
        let lightsAlpha: CGFloat = chrome.trafficLightsVisible ? 1 : 0
        let stripAlpha: CGFloat = chrome.titleStripVisible ? 1 : 0
        let lights = [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
            .compactMap { window.standardWindowButton($0) }
        guard animated else {
            for button in lights { button.alphaValue = lightsAlpha }
            titleStrip.alphaValue = stripAlpha
            return
        }
        let strip = titleStrip
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.2
            for button in lights { button.animator().alphaValue = lightsAlpha }
            strip.animator().alphaValue = stripAlpha
        }
    }

    @objc private func willEnterFullScreen(_ notification: Notification) {
        chrome.fullscreen = true
        applyChrome(animated: false)
    }

    @objc private func didEnterFullScreen(_ notification: Notification) {
        onFullscreenChanged?(true)
        // The smoke leaves again the way Escape does, once the new Space has settled.
        guard smokeRoundTrip != nil else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in self?.window.cancelOperation(nil) }
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
    /// What is wrong with the window after a file with `video` opened, or nil when
    /// the fit sized it, the aspect lock matches the video and the title strip
    /// has room under the title bar.
    func smokeFitFailure(video: VideoGeometry?) -> String? {
        guard let video else { return "The session reported no video size" }
        guard let fit = lastFit else { return "The window was not fitted to the \(video.width)x\(video.height) video" }
        guard window.frame.size == fit.size else {
            return "The window is \(window.frame.size), not the fitted \(fit.size)"
        }
        let expected = CGSize(width: CGFloat(video.width), height: CGFloat(video.height))
        guard window.contentAspectRatio == expected else {
            return "The aspect lock is \(window.contentAspectRatio), not the video's \(expected)"
        }
        guard abs(fit.width - fit.height * expected.width / expected.height) <= 1 else {
            return "The fitted \(fit.size) does not have the video's aspect"
        }
        guard titleStrip.frame.height > 0 else { return "The title strip has no height under the title bar" }
        return nil
    }

    /// `fit=WxH aspect=W:H title_strip=H` for the smoke line.
    var smokeFitSummary: String {
        let fit = lastFit.map { "\(Int($0.width))x\(Int($0.height))" } ?? "none"
        let lock = aspect.map { "\(Int($0.width)):\(Int($0.height))" } ?? "none"
        return "fit=\(fit) aspect=\(lock) title_strip=\(Int(titleStrip.frame.height))"
    }

    /// Enter fullscreen the way F does and leave it the way Escape does. Calls
    /// `completion` once the window is back, or with what went wrong.
    func smokeFullscreenRoundTrip(timeout: TimeInterval, completion: @escaping (String?) -> Void) {
        smokeRoundTrip = completion
        DispatchQueue.main.asyncAfter(deadline: .now() + timeout) { [weak self] in
            guard let self else { return }
            self.finishSmokeRoundTrip("Fullscreen did not enter and leave within \(Int(timeout)) s "
                                      + "(fullscreen now: \(self.window.isInFullscreen))")
        }
        let keyCommand = WindowKeys.command(forKey: "f", keyCode: 3, modifiers: [], isRepeat: false,
                                            isFullscreen: window.isInFullscreen)
        guard let keyCommand, perform(keyCommand) else {
            finishSmokeRoundTrip("F did not toggle fullscreen")
            return
        }
    }

    private func finishSmokeRoundTrip(_ failure: String?) {
        guard let completion = smokeRoundTrip else { return }
        smokeRoundTrip = nil
        // Not inside AppKit's fullscreen notification: the smoke closes the window next.
        DispatchQueue.main.async { completion(failure) }
    }
}
