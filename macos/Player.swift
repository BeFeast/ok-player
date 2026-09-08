import AppKit
import OpenGL.GL3

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
    let pause = NSButton(title: "Pause", target: nil, action: nil)
    var session: OpaquePointer?
    var timer: Timer?
    var pendingPath: String?
    let smoke = CommandLine.arguments.contains("--smoke-test")
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
        let root = NSView(); window.contentView = root
        video.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(video)
        let open = NSButton(title: "Open…", target: self, action: #selector(openFile))
        pause.target = self; pause.action = #selector(togglePause)
        let controls = NSStackView(views: [open, pause, status])
        controls.orientation = .horizontal; controls.spacing = 14
        controls.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(controls)
        NSLayoutConstraint.activate([
            video.topAnchor.constraint(equalTo: root.topAnchor), video.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            video.trailingAnchor.constraint(equalTo: root.trailingAnchor), video.bottomAnchor.constraint(equalTo: controls.topAnchor, constant: -8),
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
        status.stringValue = "Opening…"
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
        pause.title = paused ? "Play" : "Pause"
        if snapshot.time_pos_known {
            status.stringValue = String(format: "%.0f / %.0f s", snapshot.time_pos, snapshot.duration)
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
            shutdown(); print("OKP_SMOKE teardown=clean"); fflush(stdout); NSApp.terminate(nil)
        }
    }
    func fail(_ message: String) {
        guard !shuttingDown, !errorShown else { return }
        errorShown = true
        status.stringValue = message
        fputs("OK Player: \(message)\n", stderr)
        if smoke { shutdown(); exit(1) }
        let alert = NSAlert(); alert.messageText = "Could not play video"; alert.informativeText = message
        alert.beginSheetModal(for: window)
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
let application = NSApplication.shared
application.setActivationPolicy(.regular)
let delegate = PlayerApp()
application.delegate = delegate
application.run()
