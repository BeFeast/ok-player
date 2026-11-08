import AppKit
import COKPCore
import OpenGL.GL3

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

final class VideoView: NSOpenGLView {
    var session: OpaquePointer?
    var rendererReady = false
    var renderCount = 0
    var onError: ((String) -> Void)?

    init() {
        let attributes: [NSOpenGLPixelFormatAttribute] = [
            UInt32(NSOpenGLPFAOpenGLProfile), UInt32(NSOpenGLProfileVersion3_2Core),
            UInt32(NSOpenGLPFAColorSize), 24,
            UInt32(NSOpenGLPFAAlphaSize), 8,
            UInt32(NSOpenGLPFADoubleBuffer), 0
        ]
        super.init(frame: .zero, pixelFormat: NSOpenGLPixelFormat(attributes: attributes)!)!
        wantsBestResolutionOpenGLSurface = true
    }
    required init?(coder: NSCoder) { fatalError("Use init()") }
    override var acceptsFirstResponder: Bool { true }
    override func keyDown(with event: NSEvent) {
        if event.charactersIgnoringModifiers == " ", let session {
            _ = okp_live_session_toggle_pause(session)
        } else { super.keyDown(with: event) }
    }
    func renderFrame() {
        guard let session, let context = openGLContext else { return }
        context.makeCurrentContext()
        if !rendererReady {
            var interval: GLint = 1
            context.setValues(&interval, for: .swapInterval)
            guard okp_mac_result_ok(okp_live_session_create_render_context(session)) else {
                onError?("Could not create the video renderer")
                return
            }
            rendererReady = true
        }
        let pixels = convertToBacking(bounds)
        guard pixels.width > 0, pixels.height > 0 else { return }
        glBindFramebuffer(GLenum(GL_FRAMEBUFFER), 0)
        if okp_mac_result_ok(okp_live_session_render(session, Int32(pixels.width), Int32(pixels.height))) {
            context.flushBuffer()
            renderCount += 1
        } else { onError?("Video rendering failed") }
    }
    override func reshape() { super.reshape(); openGLContext?.update() }
    func shutdown() {
        if let session {
            openGLContext?.makeCurrentContext()
            okp_live_session_destroy_render_context(session)
        }
        rendererReady = false
        session = nil
    }
}

final class PlayerApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    let video = VideoView()
    let status = NSTextField(labelWithString: "Open a video to begin")
    let pause = AccentButton(label: "Pause", target: nil, action: nil)
    let bar = PaletteSurface(fill: .surface, separator: .borderSubtle)
    var statusRole = PaletteRole.textMuted
    var session: OpaquePointer?
    var timer: Timer?
    var pendingPath: String?
    let smoke = CommandLine.arguments.contains("--smoke-test")
    let snapshotDirectory = CommandLine.arguments.first { $0.hasPrefix("--snapshot-dir=") }
        .map { String($0.dropFirst("--snapshot-dir=".count)) }
    var smokeStarted = Date()
    var smokePhase = 0
    var pausePosition = 0.0
    var phaseStarted = Date()
    var loaded = false
    var shuttingDown = false
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
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 580),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "OK Player"; window.delegate = self; window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 420, height: 280)
        window.backgroundColor = PaletteRole.canvas.color
        let root = PaletteSurface(fill: .canvas); window.contentView = root
        video.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(video)
        bar.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(bar)
        let open = AccentButton(label: "Open…", target: self, action: #selector(openFile))
        pause.target = self; pause.action = #selector(togglePause)
        status.textColor = statusRole.color
        let controls = NSStackView(views: [open, pause, status])
        controls.orientation = .horizontal; controls.spacing = 14
        controls.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(controls)
        NSLayoutConstraint.activate([
            video.topAnchor.constraint(equalTo: root.topAnchor), video.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            video.trailingAnchor.constraint(equalTo: root.trailingAnchor), video.bottomAnchor.constraint(equalTo: controls.topAnchor, constant: -8),
            bar.topAnchor.constraint(equalTo: video.bottomAnchor), bar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: root.trailingAnchor), bar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            controls.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            controls.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -12),
            controls.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -10), controls.heightAnchor.constraint(equalToConstant: 30)
        ])
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        var error = [CChar](repeating: 0, count: 2048)
        session = okp_live_session_new(&error, numericCast(error.count))
        guard let session else { fail(String(cString: error)); return }
        video.session = session
        video.onError = { [weak self] message in self?.fail(message) }
        video.renderFrame()
        timer = Timer(timeInterval: 1.0 / 60.0, target: self, selector: #selector(tick), userInfo: nil, repeats: true)
        RunLoop.main.add(timer!, forMode: .common)
        smokeStarted = Date()
        let args = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("--") }
        if let path = pendingPath ?? args.first { play(path) }
    }
    func application(_ sender: NSApplication, openFile filename: String) -> Bool {
        if session != nil { play(filename) } else { pendingPath = filename }
        return true
    }
    @objc func openFile() {
        let panel = NSOpenPanel(); panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        panel.beginSheetModal(for: window) { [weak self] response in
            if response == .OK, let path = panel.url?.path { self?.play(path) }
        }
    }
    func play(_ path: String) {
        guard let session else { return }
        errorShown = false
        let result = path.withCString { okp_live_session_open_file(session, $0) }
        if !okp_mac_result_ok(result.result) { fail(lastError()); return }
        window.title = "\((path as NSString).lastPathComponent) — OK Player"
        bar.progress = 0
        showStatus("Opening…", .textMuted)
    }
    @objc func togglePause() { if let session { _ = okp_live_session_toggle_pause(session) } }
    func lastError() -> String {
        var bytes = [CChar](repeating: 0, count: 4096)
        _ = okp_live_session_last_error(session, &bytes, numericCast(bytes.count))
        return String(cString: bytes)
    }
    @objc func tick() {
        guard !shuttingDown, !errorShown, let session else { return }
        video.renderFrame()
        guard !errorShown else { return }
        let snapshot = okp_live_session_poll(session)
        if snapshot.error { fail(lastError()); return }
        loaded = loaded || snapshot.loaded
        let paused = okp_mac_is_paused(snapshot.status)
        let transport = paused ? "Play" : "Pause"
        if pause.title != transport { pause.title = transport }
        if snapshot.time_pos_known {
            showStatus(String(format: "%.0f / %.0f s", snapshot.time_pos, snapshot.duration), .text)
            bar.progress = snapshot.duration > 0 ? max(0, snapshot.time_pos / snapshot.duration) : 0
        }
        guard smoke else { return }
        if Date().timeIntervalSince(smokeStarted) > 20 { fail("Smoke playback timed out"); return }
        if smokePhase == 0 && loaded && snapshot.time_pos > 0.7 && video.renderCount > 10 {
            _ = okp_live_session_set_paused(session, true); smokePhase = 1; phaseStarted = Date()
        } else if smokePhase == 1 && paused && Date().timeIntervalSince(phaseStarted) > 0.3 {
            pausePosition = snapshot.time_pos; smokePhase = 2; phaseStarted = Date()
        } else if smokePhase == 2 && Date().timeIntervalSince(phaseStarted) > 0.5 {
            guard paused && abs(snapshot.time_pos - pausePosition) < 0.12 else { fail("Pause did not hold playback position"); return }
            _ = okp_live_session_set_paused(session, false); smokePhase = 3
        } else if smokePhase == 3 && !paused && snapshot.time_pos > pausePosition + 0.5 {
            print("OKP_SMOKE loaded=true progressed=true pause=true resume=true render_calls=\(video.renderCount)")
            if let snapshotDirectory { captureAppearances(to: snapshotDirectory) }
            shutdown(); print("OKP_SMOKE teardown=clean"); fflush(stdout); NSApp.terminate(nil)
        }
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
    /// The OpenGL video surface is not part of an AppKit cache, so it stays empty.
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
    func shutdown() {
        guard !shuttingDown else { return }; shuttingDown = true
        timer?.invalidate(); timer = nil
        if let session {
            _ = okp_live_session_close(session); video.shutdown(); okp_live_session_free(session)
        }
        session = nil
    }
    func windowWillClose(_ notification: Notification) { shutdown(); NSApp.terminate(nil) }
    func applicationWillTerminate(_ notification: Notification) { shutdown() }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
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
