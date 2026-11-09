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

/// Paints its bounds with one palette role.
final class PaletteSurface: NSView {
    let fill: PaletteRole

    init(fill: PaletteRole) {
        self.fill = fill
        super.init(frame: .zero)
    }
    required init?(coder: NSCoder) { fatalError("Use init(fill:)") }
    override func draw(_ dirtyRect: NSRect) {
        fill.color.setFill()
        bounds.fill()
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
    /// the core's `AppIdentity::macos` does, the session's engine defaults, and the
    /// registry commands the shell carries out (`PlayerCommands.shellCommands`).
    static func forApp(shellCommands: [String] = []) -> SessionConfig {
        let files = FileManager.default
        let bundleID = "uk.oklabs.ok-player"
        let support = files.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent(bundleID)
        let caches = files.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent(bundleID)
        return SessionConfig(app: AppIdentity(name: "OK Player", platform: .macos),
                             dataDir: support.path, configDir: support.path, cacheDir: caches.path,
                             shellCommands: shellCommands)
    }
}

final class PlayerApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow!
    var windowBehaviour: PlayerWindowController?
    var video: VideoView?
    let status = NSTextField(labelWithString: "Open a video to begin")
    /// The Open button and the status line, centred over the video while nothing plays.
    let prompt = NSStackView()
    /// The floating transport controls over the video.
    let osc = TransportOverlay()
    /// Loading, buffering and failure states over the video, and the toast.
    let statusLayer = StatusLayer()
    /// The Up Next side panel and its button on the controls.
    let upNext = UpNextController()
    /// Audio and subtitle popovers, the subtitle file panel, the delay and size keys.
    let tracks = TrackControls()
    var client: SessionClient?
    /// Where opened, dropped and Finder-opened files go (Open/).
    let openRouter = OpenRouter()
    let smoke = CommandLine.arguments.contains("--smoke-test")
    let snapshotDirectory = CommandLine.arguments.first { $0.hasPrefix("--snapshot-dir=") }
        .map { String($0.dropFirst("--snapshot-dir=".count)) }
    var smokeRun: SmokeRun?
    /// Now Playing, remote commands, display sleep, recent documents, Dock menu.
    lazy var system: SystemIntegration = SystemIntegration(
        session: { [weak self] command in
            self?.client?.dispatch(command) ?? .rejected(reason: .noActiveMedia, message: "No playback session")
        },
        open: { [weak self] path in self?.play(path) })
    /// Next and previous item for the remote commands and the Dock menu.
    lazy var playlist: SessionPlaylist = SessionPlaylist { [weak self] command in
        self?.client?.dispatch(command) ?? .rejected(reason: .noActiveMedia, message: "No playback session")
    }
    /// The idle Welcome view (Continue watching) and the History window.
    lazy var history: HistoryIntegration = HistoryIntegration(app: self)
    /// The registry commands of the menu bar and the window keys (Menu/).
    let commands = PlayerCommands()
    var menuBar: MenuBar?
    /// Key presses the window received; each one counts as activity for the transport controls.
    private(set) var keyPresses = 0
    var shuttingDown = false
    var closing = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        // One player window: no window tabs, so no tab items in View and Window.
        NSWindow.allowsAutomaticWindowTabbing = false
        let menuBar = MenuBar(commands: commands, openRecent: system.makeOpenRecentItem(),
                              history: history.makeMenuItem())
        self.menuBar = menuBar
        menuBar.install()
        let behaviour = PlayerWindowController(contentRect: NSRect(x: 0, y: 0, width: 960, height: 580))
        windowBehaviour = behaviour; window = behaviour.window
        window.title = "OK Player"; window.delegate = self; window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 420, height: 280)
        window.backgroundColor = PaletteRole.canvas.color
        let root = PaletteSurface(fill: .canvas); window.contentView = makeDropTarget(content: root)
        // The video area keeps its place in the layout even when no renderer exists.
        let surface = GLSurface.make()
        let videoArea: NSView = surface.map { VideoView(surface: $0) as NSView } ?? NSView()
        video = videoArea as? VideoView
        // The video fills the window; the transport controls float over it.
        for view in [videoArea, osc] {
            view.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(view)
            NSLayoutConstraint.activate([
                view.topAnchor.constraint(equalTo: root.topAnchor), view.bottomAnchor.constraint(equalTo: root.bottomAnchor),
                view.leadingAnchor.constraint(equalTo: root.leadingAnchor), view.trailingAnchor.constraint(equalTo: root.trailingAnchor)
            ])
        }
        let open = AccentButton(label: "Open…", target: self, action: #selector(openFile))
        status.textColor = PaletteRole.textMuted.color
        prompt.addArrangedSubview(open); prompt.addArrangedSubview(status)
        prompt.orientation = .vertical; prompt.alignment = .centerX; prompt.spacing = 10
        prompt.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(prompt)
        NSLayoutConstraint.activate([
            prompt.centerXAnchor.constraint(equalTo: root.centerXAnchor), prompt.centerYAnchor.constraint(equalTo: root.centerYAnchor),
            prompt.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 12)
        ])
        history.install(in: root, above: osc.panel)
        behaviour.install(in: root, videoArea: videoArea)
        osc.addTrailingControl(upNext.button)
        upNext.install(in: root, above: osc.panel)
        tracks.install(in: osc, over: root)
        tracks.window = window
        // The status layer's toast goes on top of everything installed so far.
        statusLayer.install(in: root, above: videoArea)
        statusLayer.onRetry = { [weak self] source in self?.play(source) }
        statusLayer.onOpenAnother = { [weak self] in self?.openFile() }
        statusLayer.onToast = { [weak self] text in self?.smokeRun?.toastShown(text) }
        // The track readouts (delay, size, a subtitle file that did not load) are toasts.
        tracks.toast = { [weak self] text in self?.statusLayer.show(toast: text) }
        // The title bar's traffic lights and title strip show and hide with the
        // transport controls, and a key press brings the controls back like a
        // pointer movement.
        osc.onRevealChange = { [weak self] revealed in self?.oscRevealChanged(revealed) }
        behaviour.oscVisibilityChanged(osc.isRevealed, animated: false)
        behaviour.window.onKeyDown = { [weak self] in self?.keyPressed() }
        window.center(); window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        guard let video else { fail("No OpenGL pixel format is available for video"); return }
        let args = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("--") }
        // The smoke opens its fixture itself, once it has checked the idle state.
        let smokePath = smoke ? openRouter.takeHeld().first?.path ?? args.first : nil
        registerPlayerCommands()
        let client: SessionClient
        do {
            // The smoke runs on a seeded history of its own (`SmokeHistory`).
            client = try SessionClient(config: smoke
                ? SmokeHistory.config(fixture: smokePath, shellCommands: commands.shellCommands)
                : .forApp(shellCommands: commands.shellCommands))
        } catch {
            fail("\(error)"); return
        }
        self.client = client
        behaviour.connect(client) { [weak self] in self?.togglePause() }
        client.onBatch = { [weak self] batch in self?.sessionChanged(batch) }
        playlist.state = client.store.playlist
        system.playlist = playlist
        system.start()
        statusLayer.query = { [weak client] in
            guard case .statusSurface(let surface)? = client?.query(.statusSurface) else { return nil }
            return surface
        }
        osc.dispatch = { [weak client] command in _ = client?.dispatch(command) }
        osc.query = { [weak client] at in
            guard case .transport(let view)? = client?.query(.transport(at: at)) else { return nil }
            return view
        }
        tracks.dispatch = { [weak client] command in
            client?.dispatch(command) ?? .rejected(reason: .noActiveMedia, message: "No playback session")
        }
        tracks.query = { [weak client] in
            guard case .tracks(let view)? = client?.query(.tracks) else { return nil }
            return view
        }
        tracks.holdControls = { [weak self] hold in
            if hold { self?.osc.autoHide.pin() } else { self?.osc.autoHide.unpin() }
        }
        osc.onPrevious = { [weak self] in _ = self?.playlist.step(.previous) }
        osc.onNext = { [weak self] in _ = self?.playlist.step(.next) }
        osc.focusReturn = video
        upNext.dispatch = { [weak client] command in
            client?.dispatch(command) ?? .rejected(reason: .noActiveMedia, message: "No playback session")
        }
        upNext.query = { [weak client] query in client?.query(query) }
        upNext.isFullscreen = { [weak behaviour] in behaviour?.window.isInFullscreen ?? false }
        upNext.panel.focusReturn = video
        statusLayer.overlay.focusReturn = video
        video.videoLayer.onError = { [weak self] message, transient in self?.fail(message, clearsOnOpen: transient) }
        if let handle = client.handle { video.videoLayer.attach(session: handle) }
        connectPlayerCommands(client)
        updateWelcome(client.store.snapshot)
        if smoke {
            connectOpenRouter(client)
            smokeRun = SmokeRun(app: self, path: smokePath)
            smokeRun?.start()
        } else if !connectOpenRouter(client), let path = args.first {
            self.open([URL(fileURLWithPath: path)])
        }
    }
    func play(_ path: String) {
        play(MediaSource(kind: .local, location: path))
    }
    /// Open `source`; `resumeFrom` `nil` leaves the start position to the session's
    /// resume rule. The status layer shows it loading, or the session's failure card.
    func play(_ source: MediaSource, resumeFrom: Double? = nil) {
        guard let client else { return }
        let result = client.dispatch(.open(source: source, resumeFrom: resumeFrom))
        statusLayer.opened(result)
        showOpening((source.location as NSString).lastPathComponent)
    }
    /// A file or folder named `name` is opening: the status layer shows it loading,
    /// in the Welcome view's and the Open prompt's place.
    func showOpening(_ name: String) {
        window.title = "\(name) — OK Player"
        history.setWelcomeShown(false)
        prompt.isHidden = true
    }
    @objc func togglePause() { client?.dispatch(.togglePause) }
    /// The transport controls showed or hid.
    func oscRevealChanged(_ revealed: Bool) {
        windowBehaviour?.oscVisibilityChanged(revealed, animated: true)
        smokeRun?.oscRevealChanged(revealed)
    }
    func keyPressed() {
        keyPresses += 1
        osc.noteActivity()
    }
    /// The session reported a batch; the store already holds the new state.
    func sessionChanged(_ batch: EventBatch) {
        guard !shuttingDown, let client else { return }
        history.sessionChanged(batch)
        commands.invalidate()
        playlist.state = client.store.playlist
        system.privateSession = client.store.privateSession
        system.update(client.store.snapshot)
        // Up Next only renders the store, and its rows send playlist commands by
        // index into the session's playlist, so it follows every batch, also while
        // a failure card is up: a jump from it does not go through play().
        upNext.sessionChanged(playlist: client.store.playlist, snapshot: client.store.snapshot)
        // An error against a track surface's own request (a subtitle file libmpv
        // cannot read) shows in the track readout, and an open that found nothing
        // leaves what plays alone and comes with the session's notice
        // (Open/PlayerOpen.swift): neither is toasted as the raw error.
        var shown = batch
        shown.events = batch.events.filter { event in
            guard case .error(let kind, _, let requestId) = event else { return true }
            if kind == .nothingPlayable { nothingPlayableFound(); return false }
            return !tracks.claimError(requestId: requestId)
        }
        smokeRun?.batchArrived(shown)
        if batch.events.contains(.shutdown) { fail("The playback engine stopped"); return }
        statusLayer.sessionChanged(shown)
        let snapshot = client.store.snapshot
        windowBehaviour?.sessionChanged(snapshot)
        video?.hasVideo = snapshot.video != nil
        osc.sessionChanged(snapshot)
        tracks.sessionChanged(snapshot)
        osc.playlistSteps = (previous: playlist.canStep(.previous), next: playlist.canStep(.next))
        prompt.isHidden = snapshot.source != nil || !statusLayer.display.isClear
        updateWelcome(snapshot)
        smokeRun?.sessionChanged(snapshot)
    }
    /// A failure card covers the video: the session's for the current source, or
    /// the shell's own. The Welcome view stays away while one is up.
    var errorShown: Bool {
        switch statusLayer.display {
        case .failure, .shellFailure: return true
        case .clear, .progress: return false
        }
    }
    /// The shell itself cannot play: no renderer, no session, the engine quit, or a
    /// failed smoke step. The status layer shows it as a card without actions; one
    /// that `clearsOnOpen` (a frame did not render) goes with the next open.
    func fail(_ message: String, clearsOnOpen: Bool = false) {
        guard !shuttingDown, statusLayer.showShellFailure(message, clearsOnOpen: clearsOnOpen) else { return }
        history.setWelcomeShown(false)
        prompt.isHidden = true
        fputs("OK Player: \(message)\n", stderr)
        if smoke { shutdown(); exit(1) }
    }
    /// Smoke evidence: renders the window content in the light and the dark appearance
    /// to PNG files. Only this window's appearance is overridden, never the system's.
    /// The OpenGL video layer is not part of an AppKit cache, so it stays empty.
    func captureAppearances(to directory: String, prefix: String = "window") {
        guard let view = window.contentView else { return }
        for (name, appearanceName) in [("light", NSAppearance.Name.aqua), ("dark", NSAppearance.Name.darkAqua)] {
            guard let appearance = NSAppearance(named: appearanceName) else { continue }
            window.appearance = appearance
            view.layoutSubtreeIfNeeded()
            guard let image = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { continue }
            appearance.performAsCurrentDrawingAppearance { view.cacheDisplay(in: view.bounds, to: image) }
            let file = URL(fileURLWithPath: directory).appendingPathComponent("\(prefix)-\(name).png")
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
        system.shutdown()
        if let client {
            client.dispatch(.close)
            video?.shutdown()
            client.close()
        }
        client = nil
        SmokeHistory.cleanUp()
    }
    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        windowBehaviour?.windowWillResize(to: frameSize) ?? frameSize
    }
    func windowWillClose(_ notification: Notification) {
        // Terminating closes the window again; the first close does the work.
        guard !closing else { return }
        closing = true
        shutdown()
        if smokeRun?.finished == true {
            if let problem = system.smokeCheckWithdrawn() { fputs("OK Player: \(problem)\n", stderr); exit(1) }
            print("OKP_SMOKE teardown=clean"); fflush(stdout)
        }
        NSApp.terminate(nil)
    }
    func applicationWillTerminate(_ notification: Notification) { shutdown() }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? { system.makeDockMenu() }
}

/// The bounded smoke the macOS workflows run (`--smoke-test`). Each step is checked
/// against what the session reported and what the video layer counted:
///
/// 1. idle: no render calls before a file is open; the Welcome view shows the seeded
///    Continue watching shelf (`SmokeHistory`);
/// 1a. failed: opening a missing file shows the session's failure card with Retry
///    and Open another, and no alert; Retry opens the file again, which fails
///    again; the fixture is then opened as Finder opens a file
///    (`application(_:open:)`), which shows the loading indicator with its name,
///    and playing media shows no status state;
/// 2. playing: the position and the render calls advance; the open went through the
///    open router (`smokeOpenFailure`) and the Welcome view is gone; Now Playing shows the file
///    as playing video, the display-sleep assertion is held and the system offers the
///    remote commands; the menu bar has its nine menus, Play/Pause from the registry
///    validates enabled and a held Space's auto-repeat toggles nothing, on the
///    focused scrubber and on the video; the pause is sent through the handler the
///    system holds for its pause command;
/// 3. paused: the position holds and the render calls stay flat for two seconds; Now
///    Playing shows the pause and the assertion is released; the resume is sent
///    through the handler for the system's play command;
/// 4. resumed: both advance again;
/// 5. resized: after repeated window resizes Core Animation holds exactly one GL
///    context and has drawn into no context but the layer's own;
/// 6. presented: every frame drawn reported a swap that reached libmpv (the swap
///    count the session returns equals the frames drawn);
/// 7. fitted: once playing, the window has the size the fit gave it, the video
///    view has the video's aspect, and a resize keeps it (checked with step 2);
/// 8. transport controls: they stay up through the paused seconds, hide after the
///    session's idle timeout once playback resumes without activity, take the
///    title bar's traffic lights with them, hand the scrubber's keyboard focus
///    back to the video as they hide, come back on activity with the session's
///    time readout, and are in the snapshots;
/// 9. fullscreen: F and Escape, pressed on the video view, enter and leave
///    native fullscreen through the shortcut table's registry commands, and each
///    reaches the controls as activity;
/// 10. tracks: `]` steps the subtitle size through the shortcut table with the
///    session's readout as a toast, and an added subtitle file becomes the selected
///    external track (checked at the end);
/// 11. Up Next: the fixture plays from a folder holding two copies of it, and the
///    menu bar's Up Next command opens the panel with both under the Now Playing
///    card, the first marked current, before the snapshots;
/// 12. toast: a volume nudge shows the session's notice as a toast in the top half
///    of the window, clear of captions, and the snapshots show it;
/// 13. closed while playing: teardown runs in order, the system offers no remote
///    command any more, and the app exits.
///
/// The timed steps use one-shot timers; the session state itself arrives through
/// wakeup-driven drains, never through polling.
final class SmokeRun {
    enum Phase {
        case idle, failing, retrying, playing, pausing, holding, resuming, resizing, hiding, titlebar, fullscreen,
             toasting, finished
    }

    private weak var app: PlayerApp?
    private let path: String?
    private var phase = Phase.idle
    private var cancelled = false
    private var pausePosition = 0.0
    private var playingRenders = 0
    private var pausedRenders = 0
    private var resizeRenders = 0
    /// `load_failed` errors while the missing file was opened and retried.
    private var loadFailures = 0
    /// The summary of the status steps for the `OKP_SMOKE` line.
    private var statusSummary = ""
    /// The folder with the two copies of the fixture the smoke plays.
    private var folder: URL?
    /// The copy of the fixture the smoke opens.
    private var opened: String?

    init(app: PlayerApp, path: String?) {
        self.app = app
        self.path = path
    }

    private var renders: Int { app?.video?.videoLayer.counts.renders ?? 0 }

    /// Every step passed.
    var finished: Bool { phase == .finished }

    func cancel() {
        cancelled = true
        if let folder { try? FileManager.default.removeItem(at: folder) }
        folder = nil
    }

    func start() {
        guard let fixture = path else { fail("Smoke needs a media file argument"); return }
        // Two copies of the fixture in a folder of their own: the session lists the
        // folder as a two-item playlist for Up Next.
        let first: String
        do {
            first = try makeTwoItemFolder(from: fixture)
            opened = first
        } catch {
            fail("Could not copy the fixture into a folder of two: \(error)"); return
        }
        after(30) { [self] in
            if phase != .finished { fail("Smoke timed out in phase \(phase) after \(renders) render calls") }
        }
        // Idle: the render context exists, nothing is loaded, nothing may be drawn.
        after(0.6) { [self] in
            guard renders == 0 else { fail("\(renders) render calls while idle"); return }
            // The Welcome view lists the seeded history while nothing plays.
            if let problem = app?.history.smokeWelcomeFailure(fixture: fixture, snapshotDirectory: app?.snapshotDirectory) {
                fail(problem); return
            }
            // A file in a folder that does not exist, so no folder listing follows.
            phase = .failing
            app?.play(NSTemporaryDirectory() + "okp-smoke-missing-\(getpid())/missing.mp4")
        }
    }

    /// The app hands over every drained batch. Errors other than the missing
    /// file's load failures fail the smoke.
    func batchArrived(_ batch: EventBatch) {
        guard !cancelled else { return }
        for case .error(let kind, let message, _) in batch.events {
            if kind == .loadFailed && (phase == .failing || phase == .retrying) {
                loadFailures += 1
            } else {
                fail("The session reported an error while \(phase): \(message)")
            }
        }
    }

    /// The failure card is up for the missing file, with its actions and no alert.
    private func failureCardProblem() -> String? {
        guard let app else { return "No app" }
        guard case .failure(let card) = app.statusLayer.display else {
            return "No failure card for the missing file (\(app.statusLayer.display))"
        }
        let overlay = app.statusLayer.overlay
        guard !overlay.cardPanel.isHidden, overlay.titleLabel.stringValue == card.title, !card.title.isEmpty else {
            return "The failure card is not shown"
        }
        guard overlay.button(for: .retry) != nil, overlay.button(for: .openAnother) != nil else {
            return "The failure card has no Retry or Open another"
        }
        guard app.window.attachedSheet == nil else { return "An alert sheet showed with the failure card" }
        guard app.prompt.isHidden else { return "The Open prompt showed over the failure card" }
        return nil
    }

    /// Retry reopened the missing file and it failed again: open the fixture, which
    /// shows the loading indicator with its name.
    private func playFixture() {
        guard let app, let opened else { return }
        if let directory = app.snapshotDirectory { app.captureAppearances(to: directory, prefix: "status") }
        phase = .playing
        // Opened the way Finder's Open With opens a file.
        app.application(NSApplication.shared, open: [URL(fileURLWithPath: opened)])
        let name = (opened as NSString).lastPathComponent
        guard case .progress(let line) = app.statusLayer.display, line.contains(name),
              !app.statusLayer.overlay.progressPanel.isHidden else {
            fail("No loading indicator naming \(name) (\(app.statusLayer.display))"); return
        }
        statusSummary = "status_card=shown_and_retried load_failures=\(loadFailures) loading=shown"
    }

    /// The status layer showed a toast.
    func toastShown(_ text: String) {
        guard phase == .toasting, let app, !cancelled else { return }
        let toast = app.statusLayer.toastView
        guard text.hasPrefix("Volume"), !toast.isHidden, toast.label.stringValue == text else {
            fail("The volume nudge toasted \"\(text)\""); return
        }
        // Captions sit at the bottom of the video; the toast stays in the top half.
        guard let root = app.window.contentView,
              toast.convert(toast.bounds, to: root).minY > root.bounds.midY else {
            fail("The toast is not in the top half of the window"); return
        }
        phase = .finished
        print("OKP_SMOKE \(statusSummary) toast=\(text.replacingOccurrences(of: " ", with: "_"))")
        if let directory = app.snapshotDirectory { app.captureAppearances(to: directory) }
        // Closing the window during playback runs the real teardown path; the app
        // reports it once teardown has returned.
        app.window.close()
    }

    func sessionChanged(_ snapshot: SessionSnapshot) {
        guard !cancelled else { return }
        let position = snapshot.timePos ?? 0
        switch phase {
        case .failing where loadFailures >= 1:
            if let problem = failureCardProblem() { fail(problem); return }
            phase = .retrying
            app?.statusLayer.overlay.button(for: .retry)?.performClick(nil)
        case .retrying where loadFailures >= 2:
            if let problem = failureCardProblem() { fail(problem); return }
            playFixture()
        case .playing where snapshot.status == .playing && position > 0.7 && renders > 10:
            if let display = app?.statusLayer.display, !display.isClear {
                fail("The status layer shows \(display) during playback"); return
            }
            if let failure = app?.windowBehaviour?.smokeFitFailure(video: snapshot.video) { fail(failure); return }
            if app?.history.showsWelcome == true { fail("The Welcome view stayed up during playback"); return }
            if let opened, let failure = app?.smokeOpenFailure(fixture: opened) { fail(failure); return }
            if let failure = app?.smokeMenuFailure() { fail(failure); return }
            phase = .pausing
            playingRenders = renders
            // Now Playing and the display-sleep assertion, then pause as a media key would.
            if let problem = app?.system.smokeCheckPlayingThenPause() { fail(problem) }
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
        // Playing before the pause and paused since: longer than the idle timeout,
        // so controls that hid while paused would be gone by now.
        guard app.osc.isRevealed, !app.osc.panel.isHidden else {
            fail("The transport controls hid while paused"); return
        }
        phase = .resuming
        if let problem = app.system.smokeCheckPausedThenResume() { fail(problem) }
    }

    /// Copy `fixture` twice into a new temporary folder; returns the first copy.
    private func makeTwoItemFolder(from fixture: String) throws -> String {
        let files = FileManager.default
        let source = URL(fileURLWithPath: fixture)
        let folder = files.temporaryDirectory.appendingPathComponent("okp-smoke-\(UUID().uuidString)")
        try files.createDirectory(at: folder, withIntermediateDirectories: true)
        self.folder = folder
        let stem = source.deletingPathExtension().lastPathComponent, ext = source.pathExtension
        let copies = (1...2).map { folder.appendingPathComponent("\(stem) \($0)").appendingPathExtension(ext) }
        for copy in copies { try files.copyItem(at: source, to: copy) }
        return copies[0].path
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
        guard let app, app.client != nil, let layer = app.video?.videoLayer, !cancelled else { return }
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
        if let problem = app.tracks.smokeStart(window: app.window) { fail(problem); return }
        phase = .hiding
        // Playing without pointer activity: the controls hide once the idle timeout
        // has passed. The scrubber holds keyboard focus first, as after a click;
        // hidden, it has to give it back to the video.
        app.osc.noteActivity()
        guard app.window.makeFirstResponder(app.osc.scrubber) else {
            fail("The scrubber did not take keyboard focus during playback"); return
        }
        let hideAfter = app.osc.transport?.osc.hideAfter ?? 2.5
        after(hideAfter + 1.5) { [self] in
            if phase == .hiding { fail("The transport controls did not hide during playback") }
        }
    }

    /// The app forwards every show and hide of the transport controls.
    func oscRevealChanged(_ revealed: Bool) {
        guard !revealed, phase == .hiding, !cancelled, let app else { return }
        phase = .titlebar
        guard app.window.firstResponder === app.video else {
            fail("The hidden scrubber kept keyboard focus"); return
        }
        // The title bar's traffic lights fade out with the controls and are hidden
        // once the fade has finished.
        after(0.6) { [self] in controlsHid() }
    }

    private func controlsHid() {
        guard phase == .titlebar, let app, let behaviour = app.windowBehaviour, !cancelled else { return }
        let lights = [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
            .compactMap { app.window.standardWindowButton($0) }
        guard !lights.isEmpty, lights.allSatisfy(\.isHidden) else {
            fail("The traffic lights did not hide with the transport controls"); return
        }
        app.osc.noteActivity()
        guard lights.allSatisfy({ !$0.isHidden }) else {
            fail("The traffic lights did not come back with the transport controls"); return
        }
        let elapsed = app.osc.elapsedLabel.stringValue, trailing = app.osc.trailingLabel.stringValue
        guard app.osc.isRevealed, !app.osc.panel.isHidden, elapsed.contains(":"), elapsed != "00:00" else {
            fail("The transport controls did not come back with the time (\(elapsed))"); return
        }
        // Previous and next follow the session playlist: the fixture's folder decides
        // whether there is anything to step to.
        let items = app.playlist.state.items.count
        let steps = app.osc.nextButton.isEnabled
        guard steps == (items >= 2), app.osc.previousButton.isEnabled == steps else {
            fail("Previous/next are \(steps ? "enabled" : "disabled") with \(items) playlist items"); return
        }
        phase = .fullscreen
        let pressesBefore = app.keyPresses
        let performedBefore = app.commands.performedCounts
        behaviour.smokeFullscreenRoundTrip(timeout: 8) { [weak self] failure in
            guard let self, let app = self.app, let client = app.client, let layer = app.video?.videoLayer,
                  !self.cancelled else { return }
            if let failure { self.fail(failure); return }
            if let problem = app.tracks.smokeFinish() { self.fail(problem); return }
            // F and Escape ran the registry's fullscreen commands through the
            // shortcut table, and the window reported each key press to the
            // transport controls as activity.
            if let failure = app.commands.smokeFullscreenKeysFailure(since: performedBefore) {
                self.fail(failure); return
            }
            let presses = app.keyPresses - pressesBefore
            guard presses >= 2 else {
                self.fail("\(presses) of the F and Escape presses reached the transport controls"); return
            }
            // Up Next, opened through the menu bar's registry command, lists the
            // folder's two copies.
            guard app.commands.run(UpNextController.commandID) == .performed, app.upNext.isShown else {
                self.fail("The menu bar's Up Next command did not open the panel"); return
            }
            if let failure = app.upNext.smokeShowFailure(playlist: app.playlist.state) {
                self.fail(failure); return
            }
            let counts = layer.counts, use = layer.surface.contextUse
            let snapshot = client.store.snapshot
            let pixelFormat = layer.surface.floatColor ? "float16" : "rgba8"
            print("OKP_SMOKE loaded=true progressed=true pause=true resume=true idle_render_calls=0 paused_render_calls=0 "
                  + "render_calls=\(counts.renders) swaps=\(counts.swaps) before_pause=\(self.playingRenders) "
                  + "before_resize=\(self.resizeRenders) ca_contexts_held=\(use.held) ca_context_copies=\(use.copies) "
                  + "draw_contexts=\(use.drawnInto.count) pixel_format=\(pixelFormat) "
                  + "volume=\(Int(snapshot.volume ?? -1)) tracks=\(snapshot.tracks.count) \(behaviour.smokeFitSummary) "
                  + "fullscreen=entered_and_left osc_paused_shown=true osc_auto_hidden=true osc_focus_returned=true "
                  + "osc_titlebar_faded=true osc_key_activity=\(presses) osc_elapsed=\(elapsed) osc_trailing=\(trailing) "
                  + "playlist_items=\(items) osc_steps=\(steps ? "enabled" : "disabled") \(app.tracks.smokeSummary) "
                  + "\(app.upNext.smokeSummary)")
            // One toast before the snapshots: the session confirms a volume nudge.
            self.phase = .toasting
            _ = client.dispatch(.nudgeVolume(delta: -5))
            self.after(3) { [weak self] in
                guard let self, self.phase == .toasting else { return }
                self.fail("The volume nudge showed no toast")
            }
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
        if let status = DocumentTypeCheck.runIfRequested(CommandLine.arguments) { exit(status) }
        let application = NSApplication.shared
        application.setActivationPolicy(.regular)
        let delegate = PlayerApp()
        application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}
