import AppKit

@MainActor
extension VerifyRun {
    func performOpenStep(_ step: VerificationScenario.Step) async throws {
        let firstRun = app.history.firstRun
        let button = firstRun.openButton
        let directory = options.fixture.deletingLastPathComponent()
        let second = directory.appendingPathComponent("second.mp4")
        let folder = directory.appendingPathComponent("Episodes", isDirectory: true)
        let episodes = ["Episode 2.mp4", "Episode 10.mp4"].map { folder.appendingPathComponent($0) }
        let subtitle = directory.appendingPathComponent("external.he.srt")
        switch step.action {
        case .sizeWindow, .closeMedia:
            try await performWindowStep(step)
        case .tabWelcome:
            try await wait("first-run Welcome is mapped and key", blocked: true) {
                self.app.window.isKeyWindow && self.app.history.showsWelcome
            }
            app.window.contentView?.layoutSubtreeIfNeeded()
            let controlsVisible = !app.osc.panel.isHiddenOrHasHiddenAncestor && app.osc.panel.alphaValue > 0.01
            let role = button.accessibilityRole()?.rawValue ?? "missing"
            let label = button.accessibilityLabel() ?? "missing"
            guard snapshot.source == nil, app.history.welcome.entries.isEmpty,
                  firstRun.mark.image?.isValid == true, !firstRun.isHiddenOrHasHiddenAncestor,
                  !button.bounds.isEmpty, role == NSAccessibility.Role.button.rawValue,
                  label == "Open File", !controlsVisible else {
                throw VerifyError("Invalid first-run surface (AX role \(role), label \(label), "
                    + "controls visible \(controlsVisible), mark available \(firstRun.mark.image?.isValid == true))")
            }
            _ = app.window.makeFirstResponder(app.video)
            app.window.recalculateKeyViewLoop()
            // AppKit can put title controls ahead of Open in the keyboard loop.
            for _ in 0..<12 {
                if button.showsKeyFocus { break }
                try sendWelcomeKey("\t", code: 48)
            }
            if !button.showsKeyFocus { failures.append("welcome_first_run:focus") }
        case .cancelWelcomeOpen:
            if button.showsKeyFocus {
                try sendWelcomeKey(" ", code: 49)
            } else if options.negativeControl {
                button.performClick(nil)
            }
            try await cancelPicker(files: true, directories: false)
            guard snapshot.source == nil, app.history.showsWelcome, !firstRun.isHidden,
                  app.openRouter.lastOutcome == nil else {
                throw VerifyError("Cancel from the Welcome sheet changed the idle player")
            }
        case .pickerFiles, .pickerFolder, .pickerPlaylist:
            let chooseFolder = step.action == .pickerFolder
            if chooseFolder { app.openFolder() } else { app.openFile() }
            try await cancelPicker(files: !chooseFolder, directories: chooseFolder)
            let selected: [URL]
            let expected: [URL]
            switch step.action {
            case .pickerFolder:
                selected = [folder]
                expected = episodes
            case .pickerPlaylist:
                selected = [directory.appendingPathComponent("ordered.m3u")]
                expected = [second, options.fixture]
            default:
                selected = [options.fixture, second]
                expected = selected
            }
            // The remote NSOpenPanel service does not expose row selection to
            // this process. Drive the same accepted-selection boundary separately.
            OpenPanelConfiguration.submit(.OK, urls: selected) { _ = app.open($0) }
            try await expectQueue(expected)
        case .finderOpen:
            let claims = DocumentTypeCheck.claims(Bundle.main.infoDictionary ?? [:])
            guard case .openFormats(let formats)? = app.client?.query(.openFormats),
                  DocumentTypeCheck.failures(formats, claims: claims).isEmpty else {
                throw VerifyError("The packaged app does not declare the system document types that it opens")
            }
            // Ask Launch Services from a separate process, as Finder does. Keep
            // the running app's main thread available to dispatch Open Documents.
            let arguments = ["-a", Bundle.main.bundleURL.path, options.fixture.path]
            let status = try await Task.detached {
                let open = Process()
                open.executableURL = URL(fileURLWithPath: "/usr/bin/open")
                open.arguments = arguments
                try open.run()
                open.waitUntilExit()
                return open.terminationStatus
            }.value
            guard status == 0 else { throw VerifyError("Launch Services Open With exited with status \(status)") }
            // Opening a single file picks up the other playable file beside it.
            try await expectQueue([options.fixture, second])
        case .cancelPlaybackOpen:
            guard let source = snapshot.source, let queue = app.client?.store.playlist.items else {
                throw VerifyError("No active media or session to preserve when the picker closes")
            }
            let outcome = app.openRouter.lastOutcome
            app.openFile()
            try await cancelPicker(files: true, directories: false)
            OpenPanelConfiguration.submit(.OK, urls: []) { _ = app.open($0) }
            guard snapshot.source == source, app.client?.store.playlist.items == queue,
                  app.openRouter.lastOutcome == outcome else {
                throw VerifyError("Cancel or empty picker input replaced active playback")
            }
        case .badDrop:
            let active = snapshot.source != nil
            let payload = active ? directory.appendingPathComponent("notes.txt") : subtitle
            let (target, sender) = try dropDestination([payload])
            let outcome = app.openRouter.lastOutcome
            guard target.draggingEntered(sender).isEmpty, target.draggingUpdated(sender).isEmpty,
                  !target.prepareForDragOperation(sender), target.highlight.isHidden, !button.dragActive,
                  app.openRouter.lastOutcome == outcome,
                  !app.statusLayer.toastView.isHiddenOrHasHiddenAncestor,
                  app.statusLayer.toastView.label.stringValue == OpenNotice.nothingToOpen.text else {
                throw VerifyError("Unsupported drop highlighted, entered the router, or lacked feedback")
            }
            target.draggingExited(sender)
        case .dragEnter:
            let (target, sender) = try dropDestination([second])
            guard target.draggingEntered(sender) == .copy, target.draggingUpdated(sender) == .copy,
                  target.prepareForDragOperation(sender), target.highlight.isHidden else {
                throw VerifyError("Valid idle drag did not select the Welcome target")
            }
            sender.draggingSourceOperationMask = .move
            guard target.draggingUpdated(sender).isEmpty, !button.dragActive else {
                throw VerifyError("The drop highlight persisted after the source stopped offering Copy")
            }
            sender.draggingSourceOperationMask = .copy
            guard target.draggingUpdated(sender) == .copy else {
                throw VerifyError("The supported drag did not restore Copy")
            }
            if !button.dragActive { failures.append("open_drag_drop:welcome-accent") }
        case .dragExit:
            let (target, sender) = try dropDestination([second])
            target.draggingExited(sender)
            guard !button.dragActive, target.highlight.isHidden, snapshot.source == nil else {
                throw VerifyError("A cancelled drag retained its highlight or opened media")
            }
        case .dropMixed:
            let (target, sender) = try dropDestination([second, folder, options.fixture])
            try performDrop(target, sender: sender)
            try await expectQueue([second] + episodes + [options.fixture])
            guard app.upNext.show() else { throw VerifyError("The open queue did not reach Up Next") }
        case .dropSubtitle:
            app.upNext.hide()
            let source = snapshot.source
            let existing = Set(snapshot.tracks.filter { $0.kind == .subtitle && $0.external }.map(\.id))
            let (target, sender) = try dropDestination([subtitle])
            try performDrop(target, sender: sender)
            try await wait("one external subtitle added to current playback") {
                let tracks = self.snapshot.tracks.filter { $0.kind == .subtitle && $0.external && !existing.contains($0.id) }
                return self.snapshot.source == source && source != nil && tracks.count == 1 && tracks[0].selected
            }
            guard case .subtitlesAdded(let replies)? = app.openRouter.lastOutcome, replies.count == 1 else {
                throw VerifyError("The subtitle drop did not dispatch exactly once")
            }
        case .capture:
            guard let name = step.name, step.expect == .openEvidence else {
                throw VerifyError("Invalid open checkpoint")
            }
            try await captureOpen(name)
        default:
            throw VerifyError("Unsupported open action \(step.action.rawValue)")
        }
    }

    private func sendWelcomeKey(_ text: String, code: UInt16) throws {
        func keyEvent(_ type: NSEvent.EventType) throws -> NSEvent {
            guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [],
                                               timestamp: ProcessInfo.processInfo.systemUptime,
                                               windowNumber: app.window.windowNumber, context: nil, characters: text,
                                               charactersIgnoringModifiers: text, isARepeat: false, keyCode: code) else {
                throw VerifyError("Cannot construct a Welcome keyboard event")
            }
            return event
        }
        if text == " " { NSApp.postEvent(try keyEvent(.keyUp), atStart: false) }
        app.window.sendEvent(try keyEvent(.keyDown))
    }

    private func cancelPicker(files: Bool, directories: Bool) async throws {
        try await wait("native Open sheet attached") { self.app.window.attachedSheet != nil }
        guard let panel = app.window.attachedSheet as? NSOpenPanel else {
            throw VerifyError("The attached picker is not an NSOpenPanel")
        }
        if panel.canChooseFiles != files || panel.canChooseDirectories != directories || !panel.allowsMultipleSelection {
            failures.append(directories ? "open_files_folders:folder-filter" : "open_files_folders:file-filter")
        }
        panel.cancel(nil)
        try await wait("native Open sheet cancelled") { self.app.window.attachedSheet == nil }
    }

    private func expectQueue(_ urls: [URL]) async throws {
        // Finder delivers resolved file URLs. The panel and the test fixture can
        // still use the /var symlink to the same files under /private/var.
        func resolved(_ path: String) -> String { URL(fileURLWithPath: path).resolvingSymlinksInPath().path }
        let paths = urls.map { $0.resolvingSymlinksInPath().path }
        do {
            try await wait("ordered queue playing: \(urls.map(\.lastPathComponent).joined(separator: ", "))") {
                let queue = self.app.client?.store.playlist
                return queue?.items.map { resolved($0.source.location) } == paths && queue?.currentIndex == 0
                    && self.snapshot.status == .playing && self.snapshot.source.map { resolved($0.location) } == paths.first
                    && self.snapshot.video != nil
            }
        } catch {
            let titles = app.client?.store.playlist.items.map(\.title) ?? []
            throw VerifyError("\(error.localizedDescription); playlist \(titles), status \(snapshot.status.rawValue)")
        }
    }

    private func dropDestination(_ urls: [URL]) throws -> (FileDropView, VerificationDrag) {
        guard let target = app.dropTarget,
              let plane: NSView = app.history.showsWelcome ? app.history.firstRun.openButton : app.video else {
            throw VerifyError("No live AppKit drop plane")
        }
        target.layoutSubtreeIfNeeded()
        let point = CGPoint(x: plane.bounds.midX, y: plane.bounds.midY)
        var hit = target.hitTest(plane.convert(point, to: target.superview))
        while let view = hit, !view.registeredDraggedTypes.contains(.fileURL) { hit = view.superview }
        guard hit === target else { throw VerifyError("The visible drop hit does not reach the registered destination") }
        return (target, try VerificationDrag(window: app.window, location: plane.convert(point, to: nil), urls: urls))
    }

    private func performDrop(_ target: FileDropView, sender: VerificationDrag) throws {
        guard target.draggingEntered(sender) == .copy, target.prepareForDragOperation(sender),
              target.performDragOperation(sender), target.highlight.isHidden,
              !app.history.firstRun.openButton.dragActive else {
            throw VerifyError("The registered destination rejected a valid drop or retained its highlight")
        }
        target.draggingEnded(sender)
    }

    private func captureOpen(_ name: String) async throws {
        try await Task.sleep(nanoseconds: 250_000_000)
        guard let root = app.window.contentView, let video = app.video else {
            throw VerifyError("Open evidence lost the window or video plane")
        }
        root.layoutSubtreeIfNeeded()
        var frame: VideoEvidence?
        if snapshot.video != nil {
            let width = Int((video.bounds.width * 2).rounded())
            let height = Int((video.bounds.height * 2).rounded())
            for attempt in 0..<8 {
                frame = video.videoLayer.verificationFrame(width: width, height: height)
                if frame?.hasPicture == true { break }
                if attempt < 7 { try await Task.sleep(nanoseconds: 130_000_000) }
            }
            guard frame?.hasPicture == true else {
                throw VerifyError("Open scenario did not render playback", blocked: true)
            }
        }
        guard let image = windowComposite(root: root, video: video, frame: frame?.image),
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not capture the open checkpoint", blocked: true)
        }
        let file = name + ".png"
        try png.write(to: options.output.appendingPathComponent(file))
        let button = app.history.firstRun.openButton
        observations.append([
            "name": name, "png": file, "render_scale": 2, "capture": "app_composite_2x",
            "window_frame_points": dimensions(app.window.frame),
            "open_surface_points": dimensions(button.convert(button.bounds, to: root)),
            "welcome": app.history.showsWelcome, "open_focused": button.showsKeyFocus,
            "drop_active": button.dragActive, "playback": snapshot.status.rawValue,
            "queue": app.client?.store.playlist.items.map(\.title) ?? [],
        ])
    }
}

/// The hosted run supplies the pasteboard and destination location. AppKit's
/// WindowServer pointer and cross-application drag remain an operator check.
@MainActor
private final class VerificationDrag: NSObject, NSDraggingInfo {
    weak var draggingDestinationWindow: NSWindow?
    let draggingLocation: NSPoint
    let draggingPasteboard: NSPasteboard
    var draggingSourceOperationMask: NSDragOperation = .copy
    let draggedImageLocation: NSPoint = .zero
    let draggedImage: NSImage? = nil
    let draggingSource: Any? = nil
    let draggingSequenceNumber = 1
    var draggingFormation: NSDraggingFormation = .none
    var animatesToDestination = false
    var numberOfValidItemsForDrop = 0
    let springLoadingHighlight: NSSpringLoadingHighlight = .none

    init(window: NSWindow, location: NSPoint, urls: [URL]) throws {
        draggingDestinationWindow = window
        draggingLocation = location
        draggingPasteboard = NSPasteboard(name: .init("okp-verify-drop-\(UUID())"))
        super.init()
        guard draggingPasteboard.writeObjects(urls.map { $0 as NSURL }) else {
            throw VerifyError("Cannot write a file drop payload to the pasteboard")
        }
    }

    func slideDraggedImage(to screenPoint: NSPoint) {}
    func resetSpringLoading() {}
    func enumerateDraggingItems(options: NSDraggingItemEnumerationOptions, for view: NSView?,
                                classes: [AnyClass], searchOptions: [NSPasteboard.ReadingOptionKey: Any],
                                using block: @escaping (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) {}
}
