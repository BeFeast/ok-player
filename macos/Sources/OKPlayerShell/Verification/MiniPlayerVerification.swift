import AppKit

@MainActor
extension VerifyRun {
    func performMiniStep(_ step: VerificationScenario.Step) async throws {
        guard let window = app.window as? PlayerWindow, let video = app.video, let client = app.client else {
            throw VerifyError("Compact verification needs the player window, video and session", blocked: true)
        }
        let mini = app.miniPlayer
        let controls = mini.controls
        switch step.action {
        case .open, .openPortrait, .pin:
            try await performWindowStep(step)
        case .enterMini:
            let menu = app.menuBar?.menus[.window]
            guard let menu, let index = menu.items.firstIndex(where: { $0.playerCommandID == "mini-player" }) else {
                throw VerifyError("Window menu has no Mini-Player command")
            }
            let position = snapshot.timePos ?? 0
            let before = swaps
            miniOriginal = (window.frame, window.level, window.windowNumber, snapshot.source, client)
            menu.update()
            menu.performActionForItem(at: index)
            try await wait("Window menu switched to the floating Mini-Player") {
                mini.isActive && self.swaps > before
            }
            if !(snapshot.timePos.map { $0 >= position } ?? false) {
                failures.append("compact_mini:enter_playback")
            }
        case .pointerReveal:
            try miniMouse(.mouseMoved, at: miniBody(in: video))
            try await wait("Mini-Player revealed by video motion") { controls.revealed }
        case .dragMini:
            guard let area = window.screen?.visibleFrame else { throw VerifyError("No screen for compact snap") }
            let origin = window.frame.origin
            let near = CGPoint(x: area.minX + 30, y: area.maxY - window.frame.height - 30)
            let start = miniBody(in: video)
            let position = snapshot.timePos ?? 0
            try miniMouse(.leftMouseDown, at: start)
            try miniMouse(.leftMouseDragged, at: CGPoint(x: start.x + near.x - origin.x,
                                                         y: start.y + near.y - origin.y))
            try miniMouse(.leftMouseUp, at: start)
            try await Task.sleep(nanoseconds: 350_000_000)
            let frame = window.frame
            if abs(frame.minX - area.minX - 16) > 1 || abs(area.maxY - frame.maxY - 16) > 1 {
                failures.append("compact_mini:corner_snap")
            }
            if snapshot.status != .playing || (snapshot.timePos ?? -1) < position {
                failures.append("compact_mini:drag_playback")
            }
            observations.append(["name": "corner-snap", "frame_points": dimensions(frame),
                                 "work_area_points": dimensions(area)])
        case .resizeMini:
            let origin = window.frame.origin
            let start = window.convertPoint(fromScreen: CGPoint(x: origin.x + 3, y: origin.y + window.frame.height * 0.62))
            try miniMouse(.leftMouseDown, at: start)
            try miniMouse(.leftMouseDragged, at: CGPoint(x: start.x + window.frame.width - 60, y: start.y))
            try miniMouse(.leftMouseUp, at: start)
            try await wait("Mini-Player resize obeyed the short-edge floor") {
                abs(min(window.frame.width, window.frame.height) - 160) <= 1
            }
        case .wheelMini:
            guard let before = snapshot.volume, let primary = NSScreen.screens.first,
                  let raw = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1,
                                    wheel1: 1, wheel2: 0, wheel3: 0) else {
                throw VerifyError("Could not create the compact wheel input")
            }
            let screenPoint = window.convertPoint(toScreen: miniBody(in: video))
            raw.location = CGPoint(x: screenPoint.x, y: primary.frame.maxY - screenPoint.y)
            guard let event = NSEvent(cgEvent: raw) else { throw VerifyError("Could not convert compact wheel input") }
            window.sendEvent(event)
            try await wait("Compact video wheel raised the session volume") {
                abs((self.snapshot.volume ?? before) - before - 1) < 0.05
            }
        case .seekMini:
            guard let time = step.seconds, let duration = snapshot.duration else {
                throw VerifyError("Compact seek lacks its target or duration")
            }
            window.contentView?.layoutSubtreeIfNeeded()
            let seek = controls.seek
            let inset = ScrubberView.inset
            let x = inset + (seek.bounds.width - 2 * inset) * time / duration
            let point = seek.convert(CGPoint(x: x, y: seek.bounds.midY), to: nil)
            let original = window.frame
            let before = swaps
            try miniMouse(.leftMouseDown, at: point)
            try miniMouse(.leftMouseUp, at: point)
            try await wait("Mini-Player seek changed the session playhead and rendered") {
                abs((self.snapshot.timePos ?? 0) - time) < 0.25 && self.swaps > before
            }
            if window.frame != original { failures.append("compact_mini:seek_moved_window") }
        case .pause:
            try await perform(step)
            if !controls.revealed { failures.append("compact_mini_floor:paused_controls") }
        case .playTo:
            if snapshot.status == .paused { try await perform(step) }
            else { try await performWindowStep(step) }
        case .idleMini:
            try pressKey(" ", code: 49)
            // Activity leaves the controls over the video until the real timeout.
            try miniMouse(.mouseMoved, at: miniBody(in: video))
            try await wait("Mini-Player auto-hid during playback") {
                self.snapshot.status == .playing && !controls.revealed && controls.isHidden
            }
        case .accessibleMini:
            mini.accessibilityOverride = MiniPlayerAccessibility(
                reduceMotion: true, reduceTransparency: true, increaseContrast: true)
            if !controls.usesSolidMaterial || !controls.seek.highContrast ||
                !controls.expand.acceptsFirstResponder || controls.expand.accessibilityLabel() != "Expand Mini-Player" ||
                controls.seek.accessibilityRole() != .slider || controls.seek.frame.height < 20 ||
                !window.makeFirstResponder(controls.seek) {
                failures.append("compact_mini_floor:accessible_controls")
            }
        case .restoreMini:
            let previousPosition = snapshot.timePos ?? 0
            for count in 1...2 {
                let location = miniBody(in: video)
                try miniMouse(.leftMouseDown, at: location, count: count)
                try miniMouse(.leftMouseUp, at: location, count: count)
            }
            try await wait("Double-click expanded the same player") { !mini.isActive }
            try await Task.sleep(nanoseconds: 400_000_000)
            guard let original = miniOriginal else { throw VerifyError("The standard frame was not recorded") }
            if window.frame != original.frame || window.isInFullscreen || window.level != original.level ||
                window.windowNumber != original.id || snapshot.source != original.media ||
                app.client !== original.client || snapshot.status != .playing ||
                (snapshot.timePos ?? 0) < previousPosition {
                failures.append("compact_mini:restore")
            }
            observations.append(["name": "restored", "frame_points": dimensions(window.frame),
                                 "original_frame_points": dimensions(original.frame), "window_id": window.windowNumber])
        case .enterFullscreen:
            if snapshot.status == .paused { try pressKey(" ", code: 49) }
            try await perform(step)
            if mini.isActive || !window.isInFullscreen || controls.revealed ||
                snapshot.source != miniOriginal?.media || app.client !== miniOriginal?.client {
                failures.append("compact_mini_floor:fullscreen_exclusivity")
            }
            observations.append(["name": "fullscreen-exclusivity", "fullscreen": window.isInFullscreen,
                                 "mini_active": mini.isActive, "window_id": window.windowNumber])
        case .leaveFullscreen:
            try await perform(step)
            if mini.isActive || window.isInFullscreen || window.frame != miniOriginal?.frame {
                failures.append("compact_mini_floor:fullscreen_restore")
            }
        case .capture:
            guard let name = step.name, let expected = step.expect else {
                throw VerifyError("Compact capture needs its name and expectation")
            }
            try await captureMini(name, expected: expected, window: window, video: video)
        default:
            throw VerifyError("Unsupported compact verification step")
        }
    }

    private func miniBody(in video: NSView) -> NSPoint {
        video.convert(NSPoint(x: video.bounds.width * 0.25, y: video.bounds.height * 0.62), to: nil)
    }

    private func miniMouse(_ type: NSEvent.EventType, at point: NSPoint, count: Int = 1) throws {
        guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                                             timestamp: ProcessInfo.processInfo.systemUptime,
                                             windowNumber: app.window.windowNumber, context: nil, eventNumber: 0,
                                             clickCount: count, pressure: type == .leftMouseUp ? 0 : 1) else {
            throw VerifyError("Could not create compact pointer input")
        }
        app.window.sendEvent(event)
    }

    private func captureMini(_ name: String, expected: VerificationScenario.Expectation,
                             window: PlayerWindow, video: VideoView) async throws {
        try await Task.sleep(nanoseconds: 350_000_000)
        guard let root = window.contentView, let geometry = snapshot.video, let original = miniOriginal else {
            throw VerifyError("Compact capture lacks its window, source, or recorded standard geometry")
        }
        root.layoutSubtreeIfNeeded()
        let controls = app.miniPlayer.controls
        let size = video.bounds.size
        let hover = expected == .miniHover
        let hidden = expected == .miniHidden
        let width = Int((size.width * 2).rounded())
        let height = Int((size.height * 2).rounded())
        let ratio = CGFloat(geometry.width) / CGFloat(geometry.height)
        if !app.miniPlayer.isActive || !window.styleMask.isEmpty || window.level != .floating ||
            window.windowNumber != original.id || window.isOpaque || app.client !== original.client ||
            snapshot.source != original.media || !app.osc.isHidden || app.windowBehaviour?.titleStrip.isHidden != true ||
            abs(size.width - size.height * ratio) > 2 || abs(min(size.width, size.height) - (hover ? 270 : 160)) > 1 ||
            controls.revealed == hidden || controls.isHidden != hidden {
            failures.append("compact_mini:geometry_\(name)")
        }
        var frame: VideoEvidence?
        for attempt in 0..<8 {
            frame = video.videoLayer.verificationFrame(width: width, height: height)
            if let frame, hover ? frame.hasPicture : frame.videoPresent && frame.glyphPixels > 100 { break }
            if attempt < 7 { try await Task.sleep(nanoseconds: 130_000_000) }
        }
        guard let frame, hover ? frame.hasPicture : frame.videoPresent else {
            throw VerifyError("Could not capture live 2× compact video", blocked: true)
        }

        var facts: [String: Any] = ["name": name, "frame_points": dimensions(window.frame),
                                    "video_points": dimensions(video.bounds), "window_id": window.windowNumber,
                                    "controls_revealed": controls.revealed, "floating": window.level == .floating,
                                    "clearance_points": controls.subtitleClearance, "swap_count": swaps]
        if !hover {
            guard let glyphs = frame.glyphBounds, frame.glyphPixels > 100 else {
                throw VerifyError("No pixels from the selected SRT at \(name)")
            }
            let bottom = glyphs.minY / 2
            facts["subtitle_bottom_points"] = bottom
            facts["glyph_pixels"] = frame.glyphPixels
            if hidden {
                if bottom >= 49 { failures.append("compact_mini_floor:baseline") }
            } else {
                if bottom <= controls.subtitleClearance { failures.append("compact_mini_floor:clearance") }
            }
        }

        let compositor = await captureCompositor(width: width, height: height)
        guard let image = compositor ?? windowComposite(root: root, video: video, frame: frame.image),
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode the compact evidence", blocked: true)
        }
        let file = options.output.appendingPathComponent(name + ".png")
        try png.write(to: file)
        facts["capture"] = compositor == nil ? "app_composite_2x" : "window_compositor_2x"
        facts["png"] = file.lastPathComponent
        observations.append(facts)
        print("OKP_VERIFY captured \(name)")
    }
}
