import AppKit

extension PaletteRole {
    /// The role as a colour resolved against the appearance it is drawn in, so the
    /// chrome follows the system light/dark switch. `PaletteRole` and its values
    /// are generated from brand/palette.json at build time.
    var color: NSColor {
        let value = srgb
        return NSColor(name: nil) { appearance in
            let dark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return NSColor.fromSRGB(dark ? value.dark : value.light)
        }
    }
}

extension NSColor {
    static func fromSRGB(_ value: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((value >> 16) & 0xFF) / 255, green: CGFloat((value >> 8) & 0xFF) / 255,
                blue: CGFloat(value & 0xFF) / 255, alpha: 1)
    }
}

/// Paints its bounds with one palette role. The control bar adds a separator along
/// its top edge and draws the playback progress over it in the accent colour.
final class PaletteSurface: NSView {
    let fill: PaletteRole
    let separator: PaletteRole?
    var progress = 0.0 { didSet { if progress != oldValue { needsDisplay = true } } }

    init(fill: PaletteRole, separator: PaletteRole? = nil) {
        self.fill = fill
        self.separator = separator
        super.init(frame: .zero)
    }
    required init?(coder: NSCoder) { fatalError("Use init(fill:separator:)") }
    override func draw(_ dirtyRect: NSRect) {
        fill.color.setFill()
        bounds.fill()
        guard let separator else { return }
        let top = isFlipped ? bounds.minY : bounds.maxY - 1
        separator.color.setFill()
        NSRect(x: bounds.minX, y: top, width: bounds.width, height: 1).fill()
        guard progress > 0 else { return }
        PaletteRole.accent.color.setFill()
        let line = isFlipped ? bounds.minY : bounds.maxY - 2
        NSRect(x: bounds.minX, y: line, width: bounds.width * CGFloat(min(progress, 1)), height: 2).fill()
    }
}

/// A push button filled with the palette accent and labelled in onAccent. It draws
/// its own focus ring in the palette focus colour instead of the system ring.
final class AccentButton: NSButton {
    init(label: String, target: AnyObject?, action: Selector?) {
        super.init(frame: .zero)
        title = label
        self.target = target
        self.action = action
        isBordered = false
        focusRingType = .none
        setButtonType(.momentaryPushIn)
    }
    required init?(coder: NSCoder) { fatalError("Use init(label:target:action:)") }
    override var title: String { didSet { invalidateIntrinsicContentSize(); needsDisplay = true } }
    private var labelAttributes: [NSAttributedString.Key: Any] {
        [.font: font ?? NSFont.systemFont(ofSize: NSFont.systemFontSize), .foregroundColor: PaletteRole.onAccent.color]
    }
    override var intrinsicContentSize: NSSize {
        let label = (title as NSString).size(withAttributes: labelAttributes)
        return NSSize(width: ceil(label.width) + 32, height: 28)
    }
    override func draw(_ dirtyRect: NSRect) {
        let body = NSBezierPath(roundedRect: bounds.insetBy(dx: 3, dy: 3), xRadius: 6, yRadius: 6)
        PaletteRole.accent.color.setFill()
        body.fill()
        if isHighlighted {
            NSColor.black.withAlphaComponent(0.18).setFill()
            body.fill()
        }
        let label = title as NSString
        let size = label.size(withAttributes: labelAttributes)
        label.draw(at: NSPoint(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2), withAttributes: labelAttributes)
        if window?.isKeyWindow == true, window?.firstResponder === self {
            let ring = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 8, yRadius: 8)
            ring.lineWidth = 2
            PaletteRole.focus.color.setStroke()
            ring.stroke()
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

extension SessionConfig {
    /// The app's own session: per-user directories named after the bundle id, as
    /// the core's `AppIdentity::macos` does, and the session's engine defaults.
    static func forApp() -> SessionConfig {
        let files = FileManager.default
        let bundleID = "uk.oklabs.ok-player"
        let support = files.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent(bundleID)
        let caches = files.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent(bundleID)
        return SessionConfig(app: AppIdentity(name: "OK Player", platform: .macos),
                             dataDir: support.path, configDir: support.path, cacheDir: caches.path)
    }
}

final class PlayerApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    var windowBehaviour: PlayerWindowController?
    var video: VideoView?
    let status = NSTextField(labelWithString: "Open a video to begin")
    let pause = AccentButton(label: "Pause", target: nil, action: nil)
    let bar = PaletteSurface(fill: .surface, separator: .borderSubtle)
    var statusRole = PaletteRole.textMuted
    var client: SessionClient?
    var pendingPath: String?
    let smoke = CommandLine.arguments.contains("--smoke-test")
    let snapshotDirectory = CommandLine.arguments.first { $0.hasPrefix("--snapshot-dir=") }
        .map { String($0.dropFirst("--snapshot-dir=".count)) }
    var smokeRun: SmokeRun?
    var shuttingDown = false
    var closing = false
    var errorShown = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        let menu = NSMenu()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit OK Player", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appItem = NSMenuItem(); appItem.submenu = appMenu; menu.addItem(appItem)
        let fileMenu = NSMenu(title: "File")
        let openItem = fileMenu.addItem(withTitle: "Open…", action: #selector(openFile), keyEquivalent: "o")
        openItem.target = self
        let fileItem = NSMenuItem(title: "File", action: nil, keyEquivalent: ""); fileItem.submenu = fileMenu
        menu.addItem(fileItem); NSApp.mainMenu = menu
        let behaviour = PlayerWindowController(contentRect: NSRect(x: 0, y: 0, width: 960, height: 580))
        windowBehaviour = behaviour; window = behaviour.window
        window.title = "OK Player"; window.delegate = self; window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 420, height: 280)
        window.backgroundColor = PaletteRole.canvas.color
        let root = PaletteSurface(fill: .canvas); window.contentView = root
        // The video area keeps its place in the layout even when no renderer exists.
        let surface = GLSurface.make()
        let videoArea: NSView = surface.map { VideoView(surface: $0) as NSView } ?? NSView()
        video = videoArea as? VideoView
        videoArea.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(videoArea)
        bar.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(bar)
        let open = AccentButton(label: "Open…", target: self, action: #selector(openFile))
        pause.target = self; pause.action = #selector(togglePause)
        status.textColor = statusRole.color
        let controls = NSStackView(views: [open, pause, status])
        controls.orientation = .horizontal; controls.spacing = 14
        controls.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(controls)
        NSLayoutConstraint.activate([
            videoArea.topAnchor.constraint(equalTo: root.topAnchor), videoArea.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            videoArea.trailingAnchor.constraint(equalTo: root.trailingAnchor), videoArea.bottomAnchor.constraint(equalTo: controls.topAnchor, constant: -8),
            bar.topAnchor.constraint(equalTo: videoArea.bottomAnchor), bar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor), bar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            controls.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            controls.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -12),
            controls.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -10), controls.heightAnchor.constraint(equalToConstant: 30)
        ])
        behaviour.install(in: root, videoArea: videoArea)
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        guard let video else { fail("No OpenGL pixel format is available for video"); return }
        let client: SessionClient
        do {
            client = try SessionClient(config: .forApp())
        } catch {
            fail("\(error)"); return
        }
        self.client = client
        behaviour.connect(client) { [weak self] in self?.togglePause() }
        client.onBatch = { [weak self] batch in self?.sessionChanged(batch) }
        video.onTogglePause = { [weak self] in self?.togglePause() }
        video.videoLayer.onError = { [weak self] message in self?.fail(message) }
        if let handle = client.handle { video.videoLayer.attach(session: handle) }
        let args = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("--") }
        let path = pendingPath ?? args.first
        if smoke {
            smokeRun = SmokeRun(app: self, path: path)
            smokeRun?.start()
        } else if let path {
            play(path)
        }
    }
    func application(_ sender: NSApplication, openFile filename: String) -> Bool {
        if client != nil { play(filename) } else { pendingPath = filename }
        return true
    }
    @objc func openFile() {
        let panel = NSOpenPanel(); panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        panel.beginSheetModal(for: window) { [weak self] response in
            if response == .OK, let path = panel.url?.path { self?.play(path) }
        }
    }
    func play(_ path: String) {
        guard let client else { return }
        errorShown = false
        switch client.dispatch(.open(source: MediaSource(kind: .local, location: path))) {
        case .accepted, .noOp: break
        case .rejected(_, let message), .failed(_, let message): fail(message); return
        }
        window.title = "\((path as NSString).lastPathComponent) — OK Player"
        bar.progress = 0
        showStatus("Opening…", .textMuted)
    }
    @objc func togglePause() { client?.dispatch(.togglePause) }
    /// The session reported a batch; the store already holds the new state.
    func sessionChanged(_ batch: EventBatch) {
        guard !shuttingDown, let client else { return }
        for case .error(_, let message, _) in batch.events {
            fail(message); return
        }
        if batch.events.contains(.shutdown) { fail("The playback engine stopped"); return }
        guard !errorShown else { return }
        let snapshot = client.store.snapshot
        windowBehaviour?.sessionChanged(snapshot)
        video?.hasVideo = snapshot.video != nil
        let transport = snapshot.status == .paused ? "Play" : "Pause"
        if pause.title != transport { pause.title = transport }
        if let time = snapshot.timePos {
            let duration = snapshot.duration ?? 0
            showStatus(String(format: "%.0f / %.0f s", time, duration), .text)
            bar.progress = duration > 0 ? max(0, time / duration) : 0
        }
        smokeRun?.sessionChanged(snapshot)
    }
    func fail(_ message: String) {
        guard !shuttingDown, !errorShown else { return }
        errorShown = true
        showStatus(message, .danger)
        fputs("OK Player: \(message)\n", stderr)
        if smoke { shutdown(); exit(1) }
        let alert = NSAlert(); alert.messageText = "Could not play video"; alert.informativeText = message
        alert.beginSheetModal(for: window)
    }
    func showStatus(_ text: String, _ role: PaletteRole) {
        status.stringValue = text
        if role != statusRole { statusRole = role; status.textColor = role.color }
    }
    /// Smoke evidence: renders the window content in the light and the dark appearance
    /// to PNG files. Only this window's appearance is overridden, never the system's.
    /// The OpenGL video layer is not part of an AppKit cache, so it stays empty.
    func captureAppearances(to directory: String) {
        guard let view = window.contentView else { return }
        for (name, appearanceName) in [("light", NSAppearance.Name.aqua), ("dark", NSAppearance.Name.darkAqua)] {
            guard let appearance = NSAppearance(named: appearanceName) else { continue }
            window.appearance = appearance
            view.layoutSubtreeIfNeeded()
            guard let image = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { continue }
            appearance.performAsCurrentDrawingAppearance { view.cacheDisplay(in: view.bounds, to: image) }
            let file = URL(fileURLWithPath: directory).appendingPathComponent("window-\(name).png")
            do {
                guard let png = image.representation(using: .png, properties: [:]) else { throw CocoaError(.fileWriteUnknown) }
                try png.write(to: file)
                print("OKP_SNAPSHOT \(name)=\(file.lastPathComponent)")
            } catch {
                fputs("OK Player: could not write \(file.lastPathComponent): \(error)\n", stderr)
            }
        }
        window.appearance = nil
    }
    /// Close, then destroy the render context, then free the session.
    func shutdown() {
        guard !shuttingDown else { return }; shuttingDown = true
        smokeRun?.cancel()
        if let client {
            client.dispatch(.close)
            video?.shutdown()
            client.close()
        }
        client = nil
    }
    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        windowBehaviour?.windowWillResize(to: frameSize) ?? frameSize
    }
    func windowWillClose(_ notification: Notification) {
        // Terminating closes the window again; the first close does the work.
        guard !closing else { return }
        closing = true
        shutdown()
        if smokeRun?.finished == true { print("OKP_SMOKE teardown=clean"); fflush(stdout) }
        NSApp.terminate(nil)
    }
    func applicationWillTerminate(_ notification: Notification) { shutdown() }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

/// The bounded smoke the macOS workflows run (`--smoke-test`). Each step is checked
/// against what the session reported and what the video layer counted:
///
/// 1. idle: no render calls before a file is open;
/// 2. playing: the position and the render calls advance;
/// 3. paused: the position holds and the render calls stay flat for two seconds;
/// 4. resumed: both advance again;
/// 5. resized: after repeated window resizes Core Animation holds exactly one GL
///    context and has drawn into no context but the layer's own;
/// 6. presented: every frame drawn reported a swap that reached libmpv (the swap
///    count the session returns equals the frames drawn);
/// 7. fitted: once playing, the window has the size the fit gave it, the video
///    view has the video's aspect, and a resize keeps it (checked with step 2);
/// 8. fullscreen: F and Escape, pressed on the video view, enter and leave
///    native fullscreen;
/// 9. closed while playing: teardown runs in order and the app exits.
///
/// The timed steps use one-shot timers; the session state itself arrives through
/// wakeup-driven drains, never through polling.
final class SmokeRun {
    enum Phase { case idle, playing, pausing, holding, resuming, resizing, fullscreen, finished }

    private weak var app: PlayerApp?
    private let path: String?
    private var phase = Phase.idle
    private var cancelled = false
    private var pausePosition = 0.0
    private var playingRenders = 0
    private var pausedRenders = 0
    private var resizeRenders = 0

    init(app: PlayerApp, path: String?) {
        self.app = app
        self.path = path
    }

    private var renders: Int { app?.video?.videoLayer.counts.renders ?? 0 }

    /// Every step passed.
    var finished: Bool { phase == .finished }

    func cancel() { cancelled = true }

    func start() {
        guard let path else { fail("Smoke needs a media file argument"); return }
        after(25) { [self] in
            if phase != .finished { fail("Smoke timed out in phase \(phase) after \(renders) render calls") }
        }
        // Idle: the render context exists, nothing is loaded, nothing may be drawn.
        after(0.6) { [self] in
            guard renders == 0 else { fail("\(renders) render calls while idle"); return }
            phase = .playing
            app?.play(path)
        }
    }

    func sessionChanged(_ snapshot: SessionSnapshot) {
        guard !cancelled else { return }
        let position = snapshot.timePos ?? 0
        switch phase {
        case .playing where snapshot.status == .playing && position > 0.7 && renders > 10:
            if let failure = app?.windowBehaviour?.smokeFitFailure(video: snapshot.video) { fail(failure); return }
            phase = .pausing
            playingRenders = renders
            app?.client?.dispatch(.setPaused(paused: true))
        case .pausing where snapshot.status == .paused:
            phase = .holding
            // Let the pause settle, then measure two seconds of it.
            after(0.5) { [self] in
                pausePosition = app?.client?.store.snapshot.timePos ?? 0
                pausedRenders = renders
                after(2) { [self] in checkPause() }
            }
        case .resuming where snapshot.status == .playing && position > pausePosition + 0.5 && renders > pausedRenders + 5:
            phase = .resizing
            resize()
        default:
            break
        }
    }

    private func checkPause() {
        guard let app, let client = app.client else { return }
        let snapshot = client.store.snapshot
        guard snapshot.status == .paused, abs((snapshot.timePos ?? 0) - pausePosition) < 0.12 else {
            fail("Pause did not hold playback position"); return
        }
        guard renders == pausedRenders else {
            fail("\(renders - pausedRenders) render calls in two paused seconds"); return
        }
        phase = .resuming
        client.dispatch(.setPaused(paused: false))
    }

    private func resize() {
        guard let window = app?.window else { return }
        resizeRenders = renders
        let origin = window.frame.origin
        let sizes = [NSSize(width: 800, height: 500), NSSize(width: 1100, height: 660), NSSize(width: 640, height: 420),
                     NSSize(width: 1000, height: 600), NSSize(width: 720, height: 480), NSSize(width: 960, height: 580)]
        for (index, size) in sizes.enumerated() {
            after(0.1 * Double(index + 1)) { window.setFrame(NSRect(origin: origin, size: size), display: true) }
        }
        after(1.0) { [self] in finish() }
    }

    private func finish() {
        guard let app, let client = app.client, let layer = app.video?.videoLayer, !cancelled else { return }
        let surface = layer.surface
        let use = surface.contextUse
        guard use.held == 1, use.drawnInto == [surface.context] else {
            fail("Core Animation holds \(use.held) GL contexts (\(use.copies) copied, \(use.releases) released) "
                 + "and drew into \(use.drawnInto.count) after resizing"); return
        }
        guard renders > resizeRenders else { fail("No render calls while resizing during playback"); return }
        let counts = layer.counts
        guard counts.swaps == counts.renders else {
            fail("\(counts.swaps) swaps reached libmpv for \(counts.renders) frames drawn"); return
        }
        guard let behaviour = app.windowBehaviour else { fail("The window has no window behaviour"); return }
        phase = .fullscreen
        let snapshot = client.store.snapshot
        let pixelFormat = surface.floatColor ? "float16" : "rgba8"
        let line = "OKP_SMOKE loaded=true progressed=true pause=true resume=true idle_render_calls=0 paused_render_calls=0 "
            + "render_calls=\(counts.renders) swaps=\(counts.swaps) before_pause=\(playingRenders) "
            + "before_resize=\(resizeRenders) ca_contexts_held=\(use.held) ca_context_copies=\(use.copies) "
            + "draw_contexts=\(use.drawnInto.count) pixel_format=\(pixelFormat) "
            + "volume=\(Int(snapshot.volume ?? -1)) tracks=\(snapshot.tracks.count) \(behaviour.smokeFitSummary)"
        behaviour.smokeFullscreenRoundTrip(timeout: 8) { [weak self] failure in
            guard let self, let app = self.app, !self.cancelled else { return }
            if let failure { self.fail(failure); return }
            self.phase = .finished
            print(line + " fullscreen=entered_and_left")
            if let directory = app.snapshotDirectory { app.captureAppearances(to: directory) }
            // Closing the window during playback runs the real teardown path; the app
            // reports it once teardown has returned.
            app.window.close()
        }
    }

    private func fail(_ message: String) {
        guard !cancelled else { return }
        app?.fail(message)
    }

    private func after(_ seconds: Double, _ body: @escaping () -> Void) {
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { [weak self] in
            guard let self, !self.cancelled else { return }
            body()
        }
    }
}

/// Starts the app. The `ok-player` executable target only calls this.
public enum PlayerLauncher {
    @MainActor
    public static func run() {
        let application = NSApplication.shared
        application.setActivationPolicy(.regular)
        let delegate = PlayerApp()
        application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}
