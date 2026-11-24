import AppKit

@MainActor
extension VerifyRun {
    func performWindowStep(_ step: VerificationScenario.Step) async throws {
        guard let controller = app.windowBehaviour, let video = app.video,
              let root = app.window.contentView, let screen = app.window.screen else {
            throw VerifyError("The window, display or player render plane is unavailable", blocked: true)
        }
        switch step.action {
        case .sizeWindow, .smallWorkArea:
            let area = screen.visibleFrame
            let small = step.action == .smallWorkArea
            let permitted = CGSize(width: min(area.width, small ? 750 : 1280),
                                   height: min(area.height, small ? 676 : 800))
            let workArea = WindowRect(inside: CGRect(
                x: area.midX - permitted.width / 2, y: area.midY - permitted.height / 2,
                width: permitted.width, height: permitted.height)).cgRect
            if small { controller.verificationWorkArea = workArea }
            let idle = small ? CGRect(x: workArea.minX + 20, y: workArea.minY + 24,
                                      width: min(520, workArea.width), height: min(410, workArea.height)) : workArea
            app.window.setFrame(idle, display: true)
            initialFrame = app.window.frame
        case .open, .openPortrait:
            let url = step.action == .open ? options.fixture
                : options.fixture.deletingLastPathComponent().appendingPathComponent("portrait.mp4")
            let before = swaps
            app.application(NSApp, open: [url])
            try await wait("opened \(url.lastPathComponent) via AppKit and swapped") {
                self.snapshot.source?.location == url.path && self.snapshot.status == .playing &&
                    self.snapshot.video != nil && self.swaps > before
            }
        case .playTo:
            guard let seconds = step.seconds else { throw VerifyError("Missing playback timestamp") }
            try await wait("playback reached the reference checkpoint at \(seconds) s") {
                self.snapshot.status == .playing && (self.snapshot.timePos ?? -1) >= seconds
            }
        case .doubleClick:
            preFullscreenFrame = app.window.frame
            preFullscreenTime = snapshot.timePos
            controller.onFullscreenChanged = { [weak self] entered in if entered { self?.fullscreenEntries += 1 } }
            let location = video.convert(NSPoint(x: video.bounds.midX, y: video.bounds.midY), to: nil)
            let before = swaps
            for count in [1, 2] {
                for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                    guard let event = NSEvent.mouseEvent(with: type, location: location, modifierFlags: [],
                                                        timestamp: ProcessInfo.processInfo.systemUptime,
                                                        windowNumber: app.window.windowNumber, context: nil,
                                                        eventNumber: 0, clickCount: count,
                                                        pressure: type == .leftMouseDown ? 1 : 0) else {
                        throw VerifyError("Could not construct a video double-click")
                    }
                    app.window.sendEvent(event)
                }
            }
            // Leave time for the native Space animation and the pending single click.
            let deadline = ProcessInfo.processInfo.systemUptime + 5
            while (fullscreenEntries == 0 || swaps == before) && ProcessInfo.processInfo.systemUptime < deadline {
                try await Task.sleep(nanoseconds: 50_000_000)
            }
            if controller.isPinned && (fullscreenEntries != 1 || controller.window.level != .normal) {
                failures.append("window_float:fullscreen_level")
            }
        case .leaveFullscreen:
            if controller.window.isInFullscreen { try await perform(step) }
        case .pointerReveal, .pause:
            try await perform(step)
        case .closeMedia:
            guard let menu = app.menuBar?.menus[.file], let index = menu.items.firstIndex(of: app.closeMediaItem) else {
                throw VerifyError("File > Close Media is unavailable")
            }
            menu.update()
            guard app.closeMediaItem.isEnabled else { throw VerifyError("File > Close Media is disabled") }
            menu.performActionForItem(at: index)
            try await wait("restored idle screen via Close Media") { self.snapshot.source == nil && self.app.history.showsWelcome }
        case .resizeEdges:
            guard let area = controller.verificationWorkArea else { throw VerifyError("No small work area") }
            let original = app.window.frame
            // AppKit provides the proposed size to this delegate for all four edges.
            for (index, change) in [CGSize(width: 80, height: 0), CGSize(width: -50, height: 0),
                                    CGSize(width: 0, height: 115), CGSize(width: 0, height: -90)].enumerated() {
                let proposal = CGSize(width: original.width + change.width, height: original.height + change.height)
                let size = app.windowWillResize(app.window, to: proposal)
                var rect = CGRect(origin: original.origin, size: size)
                if index == 0 { rect.origin.x = original.maxX - size.width }
                if index == 3 { rect.origin.y = original.maxY - size.height }
                app.window.setFrame(rect, display: true)
                app.windowDidEndLiveResize(Notification(name: NSWindow.didEndLiveResizeNotification, object: app.window))
                root.layoutSubtreeIfNeeded()
                if !area.contains(app.window.frame) || !videoFitsAspect(video, controller: controller) {
                    failures.append("window_fit:edge_\(index)")
                }
            }
        case .pin, .unpin:
            let button = controller.pinButton
            let expected = step.action == .pin
            guard viewVisible(button), button.isEnabled, button.window === app.window,
                  button.bounds.width >= 22, button.bounds.height >= 20,
                  !(button.accessibilityLabel() ?? "").isEmpty else {
                throw VerifyError("Always on Top has no live, accessible title bar button")
            }
            button.performClick(nil)
            if controller.isPinned != expected || (button.state == .on) != expected ||
                (app.pinItem.state == .on) != expected { failures.append("window_float:pin_state") }
        case .overlapWindow:
            let overlap = testWindow ?? NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: false)
            overlap.isReleasedWhenClosed = false
            let frame = app.window.frame
            overlap.setFrame(frame.insetBy(dx: frame.width / 4, dy: frame.height / 4), display: true)
            overlap.level = .normal
            overlap.orderFrontRegardless()
            testWindow = overlap
            try await wait("WindowServer mapped the overlapping normal-level window", blocked: true) {
                self.frontWindowIndex(overlap) != nil && self.frontWindowIndex(self.app.window) != nil
            }
        case .capture:
            guard let name = step.name, let expected = step.expect else { throw VerifyError("Window checkpoint is missing fields") }
            try await captureWindow(name, expecting: expected, root: root, video: video, controller: controller)
        default:
            throw VerifyError("Unsupported window scenario action \(step.action.rawValue)")
        }
    }

    private func captureWindow(_ name: String, expecting expected: VerificationScenario.Expectation,
                               root: NSView, video: VideoView, controller: PlayerWindowController) async throws {
        try await Task.sleep(nanoseconds: 320_000_000)
        root.layoutSubtreeIfNeeded()
        let window = controller.window
        guard let screen = window.screen else {
            throw VerifyError("The player lost its display at \(name)", blocked: true)
        }
        let strip = controller.titleStrip
        let titleRect = strip.label.convert(strip.label.bounds, to: root)
        let videoRect = video.convert(video.bounds, to: root)
        let hasVideo = snapshot.video != nil
        let chromeVisible = viewVisible(strip.identity)
        var facts: [String: Any] = [
            "name": name, "fullscreen": window.isInFullscreen, "fullscreen_entries": fullscreenEntries,
            "window_frame_points": dimensions(window.frame), "video_plane_points": dimensions(videoRect),
            "title_frame_points": dimensions(titleRect), "title": strip.title, "title_visible": chromeVisible,
            "pin_visible": viewVisible(controller.pinButton), "pinned": controller.isPinned,
            "window_level": window.level.rawValue, "playback": snapshot.status.rawValue,
            "time_seconds": snapshot.timePos ?? -1, "swap_count": swaps, "render_scale": 2,
        ]
        switch expected {
        case .welcome, .playing:
            let lights = controller.trafficLights
            let lightsRight = lights.map { $0.convert($0.bounds, to: root).maxX }.max() ?? 0
            let pin = controller.pinButton.convert(controller.pinButton.bounds, to: root)
            let visiblePin = viewVisible(controller.pinButton) && controller.pinButton.isEnabled
            let mediaTitle = snapshot.title ?? options.fixture.lastPathComponent
            let span = videoRect.minX == root.bounds.minX && videoRect.maxX == root.bounds.maxX &&
                videoRect.minY == root.bounds.minY && abs(videoRect.maxY - (root.bounds.maxY - TitleStripView.height)) < 1
            let onScreen = screen.visibleFrame.contains(window.frame)
            if !onScreen || !chromeVisible || lights.count != 3 || !lights.allSatisfy(viewVisible) || !visiblePin ||
                titleRect.minX <= lightsRight || titleRect.minX >= 125 || titleRect.maxX > pin.minX + 1 ||
                strip.hasMedia != hasVideo || strip.title != (expected == .welcome ? "OK Player" : mediaTitle) ||
                !span || (expected == .welcome ? !app.history.showsWelcome : !hasVideo) {
                failures.append("window_chrome:\(name)")
            }
            facts["reference_viewport_points"] = [1280, 800]
            facts["native_lights"] = lights.count
            facts["pin_frame_points"] = dimensions(pin)
            facts["playback_strip"] = strip.hasMedia
        case .fullscreen, .restored:
            let fullscreen = expected == .fullscreen
            let screenFrame = screen.frame
            let fillsScreen = sameFrame(window.frame, screenFrame) && sameFrame(videoRect, root.bounds)
            let restored = preFullscreenFrame.map { sameFrame(window.frame, $0) } ?? false
            let progressing = snapshot.status == .playing && (snapshot.timePos ?? 0) > (preFullscreenTime ?? 0)
            let oscVisible = viewVisible(app.osc.panel)
            if window.isInFullscreen != fullscreen || fullscreenEntries != 1 || !progressing ||
                (fullscreen ? (!fillsScreen || chromeVisible || viewVisible(controller.pinButton) || !oscVisible) : !restored) {
                failures.append("window_fullscreen:\(name)")
            }
            facts["screen_frame_points"] = dimensions(screenFrame)
            facts["normal_frame_restored"] = restored
            facts["osc_visible"] = oscVisible
        case .landscape, .portrait:
            guard let area = controller.verificationWorkArea, let geometry = snapshot.video else {
                throw VerifyError("Fit checkpoint lacks a work area or video geometry")
            }
            let fixtureMatches = expected == .landscape ? geometry.width >= 3840 : geometry.height > geometry.width
            let fitted = controller.lastFit != nil && area.contains(window.frame) && videoFitsAspect(video, controller: controller)
            if !fitted || !fixtureMatches || !chromeVisible { failures.append("window_fit:\(name)") }
            facts["work_area_points"] = dimensions(area)
            facts["minimum_frame_points"] = [window.minSize.width, window.minSize.height]
            if let requested = controller.lastFit { facts["fitted_content_points"] = dimensions(requested) }
            facts["source_video_pixels"] = [geometry.width, geometry.height]
        case .idle:
            if !app.history.showsWelcome || !(initialFrame.map { sameFrame(window.frame, $0) } ?? false) ||
                !chromeVisible { failures.append("window_fit:\(name)") }
        case .floating, .normal, .repinned:
            let pinned = expected != .normal
            guard let other = testWindow, let otherZ = frontWindowIndex(other), let ownZ = frontWindowIndex(window) else {
                throw VerifyError("WindowServer ordering unavailable", blocked: true)
            }
            if (expected == .repinned && (window.isInFullscreen || fullscreenEntries != 1)) ||
                controller.isPinned != pinned || window.level != (pinned ? .floating : .normal) ||
                (pinned ? ownZ >= otherZ : ownZ <= otherZ) {
                failures.append("window_float:\(name)")
            }
            facts["player_stack_index"] = ownZ
            facts["overlap_stack_index"] = otherZ
        default:
            throw VerifyError("Unsupported window expectation")
        }
        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        let image: CGImage
        if let compositor = await captureCompositor(width: width, height: height) {
            image = compositor
            facts["capture"] = "window_compositor_2x"
        } else {
            var frame: CGImage?
            if hasVideo {
                let drawn = video.videoLayer.verificationFrame(width: Int((videoRect.width * 2).rounded()),
                                                               height: Int((videoRect.height * 2).rounded()))
                guard let drawn, drawn.hasPicture else {
                    throw VerifyError("No live 2× libmpv frame at \(name)", blocked: true)
                }
                frame = drawn.image
            }
            guard let composite = windowComposite(root: root, video: video, frame: frame) else {
                throw VerifyError("Could not composite live AppKit planes at \(name)", blocked: true)
            }
            image = composite
            facts["capture"] = "app_composite_2x"
        }
        let file = options.output.appendingPathComponent(name + ".png")
        guard let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode window PNG", blocked: true)
        }
        try png.write(to: file)
        facts["png"] = file.lastPathComponent
        facts["png_pixels"] = [image.width, image.height]
        observations.append(facts)
        print("OKP_VERIFY captured \(name)")
    }

    private func viewVisible(_ view: NSView) -> Bool {
        !view.isHiddenOrHasHiddenAncestor && view.alphaValue > 0.99 && !view.bounds.isEmpty
    }

    private func videoFitsAspect(_ video: VideoView, controller: PlayerWindowController) -> Bool {
        guard let geometry = snapshot.video, geometry.height > 0 else { return false }
        let ratio = CGFloat(geometry.width) / CGFloat(geometry.height)
        let size = video.bounds.size
        if abs(size.width - size.height * ratio) <= 2 { return true }
        // A viewport at the minimum control width can legitimately pillarbox a portrait.
        return ratio < 1 && abs(controller.window.frame.width - controller.window.minSize.width) <= 1
            && size.width > size.height * ratio
    }

    private func sameFrame(_ lhs: CGRect, _ rhs: CGRect) -> Bool {
        abs(lhs.minX - rhs.minX) <= 1 && abs(lhs.minY - rhs.minY) <= 1 &&
            abs(lhs.width - rhs.width) <= 1 && abs(lhs.height - rhs.height) <= 1
    }

    private func frontWindowIndex(_ window: NSWindow) -> Int? {
        let rows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]]
        return rows?.firstIndex { ($0[kCGWindowNumber as String] as? Int) == window.windowNumber }
    }
}
