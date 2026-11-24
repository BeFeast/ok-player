import AppKit

@MainActor
extension VerifyRun {
    private var context: CommandPopover { app.commandPopover }

    func performCommandStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .open, .sizeWindow:
            try await performWindowStep(step)
        case .checkContextIdle:
            try checkContextIdle()
        case .checkContextSearch:
            try checkContextSearch()
        case .checkContextEmpty:
            try clearContextSearch(toEmptyResult: true)
        case .checkContextPages:
            try checkContextPages()
        case .checkContextCorners:
            try await checkContextCorners()
        case .checkPlaybackKeys:
            try await checkPlaybackKeys()
        case .checkQueueKeys:
            try await checkQueueKeys()
        case .checkEscapePriority:
            try await checkEscapePriority()
        case .checkTextPriority:
            try await checkTextPriority()
        case .capture:
            guard let name = step.name, let expected = step.expect else {
                throw VerifyError("Command capture needs a name and expected state")
            }
            try await captureCommands(name, expected: expected)
        default:
            throw VerifyError("Unsupported command action \(step.action.rawValue)")
        }
    }

    private func checkContextIdle() throws {
        guard let video = app.video, app.history.showsWelcome, snapshot.status == .idle else {
            throw VerifyError("The clean Welcome surface was not shown")
        }
        // Anchor just beside the centered Welcome controls.
        let point = video.convert(CGPoint(x: max(12, video.bounds.midX - 176), y: video.bounds.midY), to: nil)
        try clickCommandSurface(at: point, right: true)
        guard context.isShown else { throw VerifyError("Right-click did not open commands on Welcome") }
        let titles = context.entries.map { entry -> String in
            switch entry {
            case .command(let row): return row.label
            case .page(_, let title): return title
            case .group(let label): return label
            case .separator: return "-"
            }
        }
        if titles != ["Play / Pause", "Subtitles", "Audio", "Chapters", "Screenshot", "Fullscreen", "-",
                      "Video", "Playback", "Window", "Tools / Advanced", "-", "Close Media", "Settings..."] {
            failures.append("context:curated_order")
        }
        if !context.isSearching || contextRow("play-pause")?.enabled != false {
            failures.append("context:idle_focus_and_availability")
        }
        print("OKP_VERIFY Welcome context used curated ordering and live availability")
    }

    private func checkContextSearch() throws {
        guard context.isSearching, let editor = app.window.firstResponder as? NSTextView else {
            throw VerifyError("The AppKit search field did not take focus")
        }
        let keys: [(String, UInt16)] = [("s", 1), ("c", 8), ("r", 15), ("e", 14), ("e", 14), ("n", 45)]
        for (letter, code) in keys {
            try pressKey(letter, code: code, focusVideo: false)
        }
        if context.searchText != "screen" || contextRow("save-frame") == nil {
            failures.append("context:search_label")
        }

        editor.selectAll(nil)
        editor.insertText("⌘O", replacementRange: editor.selectedRange())
        if let open = contextRow("open-file") {
            if !open.available || !open.enabled || open.shortcut?.macOSHint != "⌘O" {
                failures.append("context:native_hint")
            }
        } else {
            failures.append("context:search_shortcut")
        }
        editor.selectAll(nil)
        editor.insertText("window", replacementRange: editor.selectedRange())
        if !context.entries.contains(.group(label: "View & window")) || contextRow("fit-window-to-media") == nil {
            failures.append("context:grouped_search")
        }
    }

    private func clearContextSearch(toEmptyResult: Bool) throws {
        guard let editor = app.window.firstResponder as? NSTextView else {
            throw VerifyError("The command field editor was lost")
        }
        editor.selectAll(nil)
        try pressKey("\u{7f}", code: 51, focusVideo: false)
        if !context.searchText.isEmpty || context.entries.count != 14 { failures.append("context:clear_search") }
        if toEmptyResult {
            editor.insertText("no command 93827", replacementRange: editor.selectedRange())
            if !context.showsEmptyResult || !context.entries.isEmpty { failures.append("context:empty_search") }
        }
    }

    private func checkContextPages() throws {
        try clearContextSearch(toEmptyResult: false)
        try pressKey("\u{f701}", code: 125, focusVideo: false)
        try pressKey("\r", code: 36, focusVideo: false)
        if context.currentPage != .video || contextRow("aspect-auto") == nil { failures.append("context:video_page") }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        if !context.isShown || context.currentPage != .topLevel { failures.append("context:escape_to_root") }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        if context.isShown { failures.append("context:escape_dismiss") }
        try pressKey("p", code: 35, modifiers: [.shift, .command], throughApplication: true)
        if !context.isShown || !context.isSearching { failures.append("context:keyboard_invocation") }
        try pressKey("p", code: 35, focusVideo: false, modifiers: [.shift, .command], repeat: true)
        if !context.isShown { failures.append("context:held_invocation") }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
    }

    private func checkContextCorners() async throws {
        guard let window = app.window, let video = app.video, let display = window.screen else {
            throw VerifyError("Corner verification needs a displayed video window")
        }
        let work = display.visibleFrame
        let width = min(work.width, 620)
        let height = min(work.height, 400)
        for upperRight in [true, false] {
            window.setFrame(CGRect(x: upperRight ? work.maxX - width : work.minX,
                                   y: upperRight ? work.maxY - height : work.minY,
                                   width: width, height: height), display: true)
            window.contentView?.layoutSubtreeIfNeeded()
            let point = video.convert(CGPoint(x: upperRight ? video.bounds.maxX - 12 : video.bounds.minX + 12,
                                              y: upperRight ? video.bounds.maxY - 12 : video.bounds.minY + 12), to: nil)
            try clickCommandSurface(at: point, right: true)
            guard context.isShown else { throw VerifyError("Right-click did not open commands at a video corner") }
            if !commandsAreContained(in: work) { failures.append("context:work_area_clamp") }
            if contextRow("play-pause")?.enabled != true { failures.append("context:playing_availability") }
            let outside = video.convert(CGPoint(x: upperRight ? video.bounds.minX + 25 : video.bounds.maxX - 25,
                                                y: video.bounds.midY), to: nil)
            try clickCommandSurface(at: outside)
            try clickCommandSurface(at: outside, count: 2)
            try await Task.sleep(nanoseconds: UInt64((NSEvent.doubleClickInterval + 0.10) * 1_000_000_000))
            if context.isShown || snapshot.status != .playing || app.windowBehaviour?.window.isInFullscreen == true {
                failures.append("context:dismiss_consumes_click")
            }
        }
        window.setFrame(CGRect(x: work.midX - width / 2, y: work.midY - height / 2,
                               width: min(960, work.width), height: min(620, work.height)).fitted(inside: work),
                        display: true)
        window.center()
        try pressKey("p", code: 35, modifiers: [.shift, .command])
        if !context.isShown || !context.isSearching { failures.append("context:command_chord") }
    }

    private func checkPlaybackKeys() async throws {
        try pressKey(" ", code: 49)
        try await wait("Space paused the video") { self.snapshot.status == .paused }
        try pressKey(" ", code: 49, repeat: true)
        if app.commands.lastInvocation?.outcome != .heldRepeat { failures.append("keyboard:held_space") }
        try pressKey("k", code: 40)
        try await wait("K resumed the video") { self.snapshot.status == .playing }

        app.osc.playButton.performClick(nil)
        try await wait("OSC Play/Pause paused the same session") { self.snapshot.status == .paused }
        guard let menu = app.menuBar?.menus[.playback],
              let toggle = menu.items.firstIndex(where: { $0.playerCommandID == "play-pause" }) else {
            throw VerifyError("Playback menu is missing the shared Play/Pause command")
        }
        menu.update()
        menu.performActionForItem(at: toggle)
        try await wait("menu Play/Pause resumed the same session") { self.snapshot.status == .playing }

        app.osc.noteActivity()
        guard app.window.makeFirstResponder(app.osc.scrubber) else { throw VerifyError("Seekbar did not take focus") }
        try pressKey(" ", code: 49, focusVideo: false)
        try await wait("Space paused playback with seekbar focus") { self.snapshot.status == .paused }
        try seek(12)
        try await expectPosition(12, label: "Seekbar input reached 12 seconds")
        guard app.window.makeFirstResponder(app.osc.scrubber) else { throw VerifyError("Seekbar lost focus") }
        try pressKey("\u{f703}", code: 124, focusVideo: false)
        try await expectPosition(17, label: "Right stepped playback with seekbar focus")
        try pressKey("\u{f703}", code: 124, focusVideo: false, repeat: true)
        try await expectPosition(22, label: "Held Right repeated the seek")
        try pressKey("\u{f702}", code: 123, focusVideo: false)
        try await expectPosition(17, label: "Left stepped back")

        guard let before = snapshot.volume else { throw VerifyError("Engine volume was not observed") }
        try pressKey("\u{f701}", code: 125)
        try await wait("Down nudged volume") { abs((self.snapshot.volume ?? before) - (before - 5)) < 0.1 }
        try pressKey("\u{f700}", code: 126)
        try await wait("Up restored volume") { abs((self.snapshot.volume ?? -1) - before) < 0.1 }
        let muted = app.osc.transport?.volume.muted
        try pressKey("m", code: 46)
        try await wait("M toggled mute") { self.app.osc.transport?.volume.muted != muted }
        try pressKey("m", code: 46, repeat: true)
        if app.commands.lastInvocation?.outcome != .heldRepeat { failures.append("keyboard:held_mute") }
        try pressKey("m", code: 46)
        try await wait("a second M restored mute state") { self.app.osc.transport?.volume.muted == muted }

        let optionalKeys: [(String, UInt16, String)] =
            [("j", 38, "go-to-time"), ("l", 37, "ab-loop"), ("c", 8, "save-frame")]
        for (key, code, id) in optionalKeys {
            guard app.commands.keys.match(KeyPress(key: key)) == .run(id),
                  let row = app.commands.row(id) else {
                failures.append("keyboard:registered_\(id)")
                continue
            }
            if !row.available {
                try pressKey(key, code: code)
                if app.commands.lastInvocation != .init(id: id, outcome: .unavailable) || snapshot.status != .paused {
                    failures.append("keyboard:unavailable_\(id)")
                }
            }
        }
        try pressKey("k", code: 40)
        try await wait("keyboard playback checkpoint resumed") { self.snapshot.status == .playing }
    }

    private func checkQueueKeys() async throws {
        guard let client = app.client else { throw VerifyError("Queue requires a live session") }
        let second = options.profile.appendingPathComponent("second.mp4")
        try FileManager.default.copyItem(at: options.fixture, to: second)
        let source = MediaSource(kind: .local, location: second.path)
        _ = client.dispatch(.playlist(action: .queue(sources: [source], mode: .append)))
        try await wait("session queued a second clip") { client.store.playlist.items.count == 2 }

        try pressKey("\u{f72d}", code: 121)
        try await wait("Page Down played the next item") {
            client.store.playlist.currentIndex == 1 &&
                self.snapshot.source == source && self.snapshot.status == .playing
        }
        try pressKey("\u{f72c}", code: 116)
        try await wait("Page Up played the previous item") {
            client.store.playlist.currentIndex == 0 &&
                self.snapshot.source?.location == self.options.fixture.path && self.snapshot.status == .playing
        }

        try pressKey("k", code: 40, modifiers: [.command, .shift])
        try await wait("the chapter shortcut opened Up Next") { self.app.upNext.isShown }
        let table = app.upNext.panel.tableView
        guard app.window.makeFirstResponder(table), table.numberOfRows > 1 else {
            throw VerifyError("The queue could not take focus with two rows")
        }
        table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        try pressKey("\u{f701}", code: 125, focusVideo: false)
        if table.selectedRow != 1 { failures.append("keyboard:queue_navigation") }
        try pressKey("\u{f72d}", code: 121, focusVideo: false)
        if client.store.playlist.currentIndex != 0 { failures.append("keyboard:queue_page_priority") }
        try pressKey(" ", code: 49, focusVideo: false)
        try await wait("Space reached playback from the queue") { self.snapshot.status == .paused }
        try pressKey("k", code: 40, focusVideo: false)
        try await wait("K reached playback from the queue") { self.snapshot.status == .playing }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        if app.upNext.isShown { failures.append("keyboard:queue_escape") }

        let opensBefore = app.commands.performedCounts["open-file", default: 0]
        try pressKey("o", code: 31, modifiers: .command, throughApplication: true)
        try await wait("Cmd+O opened the native file panel") { self.app.window.attachedSheet is NSOpenPanel }
        let opensAfterPress = app.commands.performedCounts["open-file", default: 0]
        if opensAfterPress != opensBefore + 1 { failures.append("keyboard:open_dispatch") }
        try pressKey("o", code: 31, focusVideo: false, modifiers: .command, repeat: true, throughApplication: true)
        if app.commands.performedCounts["open-file", default: 0] != opensAfterPress {
            failures.append("keyboard:held_open_sheet")
        }
        (app.window.attachedSheet as? NSOpenPanel)?.cancel(nil)
        try await wait("Open panel cancelled without changing media") {
            self.app.window.attachedSheet == nil && client.store.playlist.currentIndex == 0
        }
        try pressKey("o", code: 31, modifiers: .command, repeat: true, throughApplication: true)
        if app.window.attachedSheet != nil || app.commands.performedCounts["open-file", default: 0] != opensAfterPress {
            failures.append("keyboard:held_open")
        }
    }

    private func checkEscapePriority() async throws {
        guard let controller = app.windowBehaviour else { throw VerifyError("Fullscreen controller is missing") }
        var fullscreenChanged: Bool?
        controller.onFullscreenChanged = { fullscreenChanged = $0 }
        defer { controller.onFullscreenChanged = nil }
        try pressKey("f", code: 3)
        try await wait("F entered native fullscreen") { fullscreenChanged == true }
        try pressKey("f", code: 3, repeat: true)
        if app.commands.lastInvocation?.outcome != .heldRepeat { failures.append("keyboard:held_fullscreen") }
        try pressKey("k", code: 40, modifiers: [.command, .shift])
        try await wait("Up Next shown in fullscreen") { self.app.upNext.isShown }
        try pressKey("p", code: 35, modifiers: [.command, .shift])
        try await wait("command popover above fullscreen Up Next") { self.context.isShown }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        try pressKey("\u{1b}", code: 53, focusVideo: false, repeat: true)
        if context.isShown || !app.upNext.isShown || app.windowBehaviour?.window.isInFullscreen != true {
            failures.append("keyboard:escape_top_surface")
        }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        if app.upNext.isShown || app.windowBehaviour?.window.isInFullscreen != true {
            failures.append("keyboard:escape_queue")
        }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        try await wait("Escape left fullscreen after closing the surfaces") { fullscreenChanged == false }
    }

    private func checkTextPriority() async throws {
        guard let root = app.window.contentView else { throw VerifyError("Missing text focus host") }
        let field = NSTextField(string: "")
        field.frame = CGRect(x: 48, y: 115, width: 200, height: 28)
        root.addSubview(field)
        defer {
            app.window.makeFirstResponder(app.video)
            field.removeFromSuperview()
        }
        guard app.window.makeFirstResponder(field), let editor = field.currentEditor() as? NSTextView else {
            throw VerifyError("Test text field did not take focus")
        }
        let previous = app.commands.lastInvocation
        let status = snapshot.status
        try pressKey("k", code: 40, focusVideo: false)
        try await Task.sleep(nanoseconds: 150_000_000)
        if editor.string != "k" || app.commands.lastInvocation != previous || snapshot.status != status {
            failures.append("keyboard:text_priority")
        }
        if !options.negativeControl {
            let keys: [(String, UInt16)] = [(" ", 49), ("j", 38), ("l", 37), ("m", 46), ("f", 3)]
            for (letter, code) in keys {
                try pressKey(letter, code: code, focusVideo: false)
            }
            try pressKey("\u{f702}", code: 123, focusVideo: false)
            try pressKey("c", code: 8, focusVideo: false)
            if editor.string != "k jlmcf" || app.commands.lastInvocation != previous {
                failures.append("keyboard:text_navigation")
            }
        }
    }

    private func expectPosition(_ target: Double, label: String) async throws {
        try await wait(label) { abs((self.snapshot.timePos ?? -1) - target) < 0.1 }
    }

    private func contextRow(_ id: String) -> RegistryCommand? {
        context.entries.compactMap { entry -> RegistryCommand? in
            guard case .command(let row) = entry, row.id == id else { return nil }
            return row
        }.first
    }

    private func commandsAreContained(in workArea: CGRect) -> Bool {
        guard let root = app.window.contentView else { return false }
        let frame = context.frameInWindow
        return root.convert(root.bounds, to: nil).contains(frame) && workArea.contains(app.window.convertToScreen(frame))
    }

    func clickCommandSurface(at point: CGPoint, right: Bool = false, count: Int = 1) throws {
        let types: [NSEvent.EventType] = right ? [.rightMouseDown, .rightMouseUp] : [.leftMouseDown, .leftMouseUp]
        for type in types {
            guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                                                timestamp: ProcessInfo.processInfo.systemUptime,
                                                windowNumber: app.window.windowNumber, context: nil,
                                                eventNumber: 0, clickCount: count,
                                                pressure: type == types[0] ? 1 : 0) else {
                throw VerifyError("Could not construct an AppKit context click")
            }
            app.window.sendEvent(event)
        }
    }

    private func captureCommands(_ name: String, expected: VerificationScenario.Expectation) async throws {
        try await Task.sleep(nanoseconds: 250_000_000)
        guard let window = app.window, let root = window.contentView, let video = app.video else {
            throw VerifyError("The command capture window disappeared")
        }
        root.layoutSubtreeIfNeeded()
        let wantsContext = expected != .keyboardPlayback
        let wantsVideo = expected == .contextVideo || expected == .keyboardPlayback
        let visible = context.isShown
        if visible != wantsContext || (wantsContext && !context.isSearching)
            || (expected == .contextEmpty && !context.showsEmptyResult)
            || (expected == .contextResults && context.searchText != "window")
            || (wantsVideo ? snapshot.status != .playing : snapshot.status != .idle) {
            failures.append("command_capture:\(name)")
        }

        let compositor = await captureCompositor(width: Int((root.bounds.width * 2).rounded()),
                                                 height: Int((root.bounds.height * 2).rounded()))
        let image: CGImage
        if let compositor {
            image = compositor
        } else {
            var frame: CGImage?
            if wantsVideo {
                let evidence = video.videoLayer.verificationFrame(width: Int((video.bounds.width * 2).rounded()),
                                                                  height: Int((video.bounds.height * 2).rounded()))
                guard let evidence, evidence.hasPicture else {
                    throw VerifyError("Real video was unavailable at \(name)", blocked: true)
                }
                frame = evidence.image
            }
            guard let composite = windowComposite(root: root, video: video, frame: frame) else {
                throw VerifyError("Could not compose the 2× command screenshot", blocked: true)
            }
            image = composite
        }
        guard let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode the 2× command screenshot", blocked: true)
        }
        let file = options.output.appendingPathComponent(name + ".png")
        try png.write(to: file)
        var facts: [String: Any] = [
            "name": name, "png": file.lastPathComponent,
            "capture": compositor == nil ? "app_composite_2x" : "window_compositor_2x",
            "context_visible": visible, "search_focused": context.isSearching, "playback": snapshot.status.rawValue,
            "window_frame_points": dimensions(window.frame), "render_scale": 2, "swap_count": swaps,
            "png_pixels": [image.width, image.height],
        ]
        if visible {
            facts["context_frame_points"] = dimensions(context.frameInWindow)
            facts["search"] = context.searchText
        }
        observations.append(facts)
        print("OKP_VERIFY captured \(name)")
    }
}
