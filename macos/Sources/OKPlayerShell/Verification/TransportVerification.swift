import AppKit

@MainActor
extension VerifyRun {
    private var osc: TransportOverlay { app.osc }

    func performTransportStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .open:
            try await performWindowStep(step)
        case .pause, .pointerReveal:
            try await perform(step)
        case .sizeOSC:
            guard let width = step.width, let screen = app.window.screen else {
                throw VerifyError("OSC resize needs a width and display")
            }
            let area = screen.visibleFrame
            let height = min(680, area.height)
            let fit = min(width, area.width)
            app.window.setFrame(NSRect(x: area.midX - fit / 2, y: area.midY - height / 2,
                                       width: fit, height: height), display: true)
            app.window.contentView?.layoutSubtreeIfNeeded()
        case .checkSeek:
            osc.trailingLabel.performClick(nil)
            if osc.trailingLabel.title != osc.transport?.total { failures.append("osc_layout:total_time") }
            osc.trailingLabel.performClick(nil)
            let before = swaps
            try seek(2.5)
            try await wait("seek responder reached 2.5 seconds and swapped") {
                abs((self.snapshot.timePos ?? -1) - 2.5) < 0.08 && self.swaps > before
            }
        case .checkMore:
            let menu = try trackMenu(open: { self.osc.moreButton.performClick(nil) })
            let projected = osc.collapsed.map(\.id)
            let offered = menu.items.compactMap { $0.representedObject as? String }
            if projected.isEmpty || offered != projected || Set(offered).count != offered.count {
                failures.append("osc_layout:more")
            }
        case .checkIdle:
            try await wait("uncovered playing video reached its idle deadline") { !self.osc.isRevealed }
            if !osc.cursorHidden { failures.append("osc_idle_and_pins:cursor") }
        case .checkPins:
            try await checkPins()
        case .showVolume:
            try movePointer(over: osc.muteButton)
            try await wait("speaker hover opened the volume capsule") { !self.osc.volumeCapsule.isHidden }
        case .setUnity:
            try await typeVolume("99.5", expecting: 99.5)
            try await typeVolume("100", expecting: 100)
        case .checkWheel:
            try await checkWheel()
        case .checkSpeedMute:
            try await checkSpeedMute()
        case .openStall:
            app.play(try source("stall.mp4"))
            try await wait("HTTP source shows a loading surface") {
                if case .progress = self.app.statusLayer.display { return true }
                return false
            }
        case .releaseStall:
            try await tellFixture("release-stall")
            try await wait("released stream is playing with video and clear status") {
                self.snapshot.status == .playing && self.snapshot.video != nil &&
                (self.snapshot.timePos ?? 0) > 0.3 && self.app.statusLayer.display.isClear
            }
        case .openBuffer:
            app.play(try source("buffer.mp4"))
            try await wait("partially delivered video played and entered buffering") {
                if case .buffering = self.app.statusLayer.surface {
                    return (self.snapshot.timePos ?? 0) > 0.3
                }
                return false
            }
        case .releaseBuffer:
            let stalled = snapshot.timePos ?? 0
            try await tellFixture("release-buffer")
            try await wait("playback progressed after the HTTP body resumed") {
                self.app.statusLayer.display.isClear && (self.snapshot.timePos ?? 0) > stalled + 0.3
            }
        case .openFailure:
            try await openFailure()
        case .checkFailureActions:
            try await checkFailureActions()
        case .checkStale:
            try await checkStale()
        case .capture:
            guard let name = step.name, let expected = step.expect else {
                throw VerifyError("Transport capture needs a name and expected surface")
            }
            try await captureTransport(name, expected)
        default:
            throw VerifyError("Unsupported transport action \(step.action.rawValue)")
        }
    }

    private func movePointer(over view: NSView) throws {
        let point = view.convert(NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil)
        guard let event = NSEvent.mouseEvent(with: .mouseMoved, location: point, modifierFlags: [],
                                            timestamp: ProcessInfo.processInfo.systemUptime,
                                            windowNumber: app.window.windowNumber, context: nil,
                                            eventNumber: 0, clickCount: 0, pressure: 0) else {
            throw VerifyError("AppKit could not construct a pointer event")
        }
        osc.mouseMoved(with: event)
    }

    /// Observe the real NSMenuDelegate open and close callbacks during native tracking.
    private func trackMenu(delay: TimeInterval = 0.15, inspect: (() -> Void)? = nil,
                           open: () throws -> Void) throws -> NSMenu {
        var observed: NSMenu?
        osc.onMenuOpen = { menu in
            observed = menu
            let timer = Timer(timeInterval: delay, repeats: false) { _ in
                MainActor.assumeIsolated {
                    inspect?()
                    menu.cancelTracking()
                }
            }
            RunLoop.current.add(timer, forMode: .eventTracking)
            RunLoop.current.add(timer, forMode: .default)
        }
        defer { osc.onMenuOpen = nil }
        try open()
        guard let observed else { throw VerifyError("The AppKit menu did not enter tracking") }
        return observed
    }

    private func checkPins() async throws {
        guard let video = app.video else { throw VerifyError("Missing video responder") }
        try pressKey(" ", code: 49)
        try await wait("keyboard revealed the OSC and paused playback") {
            self.snapshot.status == .paused && self.osc.isRevealed
        }
        let idleWait: UInt64 = 2_850_000_000
        try await Task.sleep(nanoseconds: idleWait)
        if !osc.isRevealed || osc.cursorHidden { failures.append("osc_idle_and_pins:pause") }
        try pressKey(" ", code: 49)
        try await wait("keyboard resumed playback") { self.snapshot.status == .playing }
        app.upNext.button.performClick(nil)
        try await wait("Up Next is open") { self.app.upNext.isShown }
        try await Task.sleep(nanoseconds: idleWait)
        if !osc.isRevealed { failures.append("osc_idle_and_pins:panel") }
        app.upNext.button.performClick(nil)
        try await wait("closed panel restored the idle timeout") { !self.osc.isRevealed }

        try movePointer(over: video)
        app.tracks.subtitleButton.performClick(nil)
        try await wait("track popover is open") { self.app.tracks.isPopoverShown }
        try await Task.sleep(nanoseconds: idleWait)
        if !osc.isRevealed { failures.append("osc_idle_and_pins:popover") }
        app.tracks.dismissPopover()
        try await wait("closed popover restored the deadline") { !self.osc.isRevealed }

        let point = video.convert(NSPoint(x: video.bounds.midX, y: video.bounds.midY), to: nil)
        try clickCommandSurface(at: point, right: true)
        guard app.commandPopover.isShown else { throw VerifyError("AppKit did not open the playback command surface") }
        try await Task.sleep(nanoseconds: idleWait)
        if !osc.isRevealed { failures.append("osc_idle_and_pins:context") }
        try pressKey("\u{1b}", code: 53, focusVideo: false)
        try await wait("closed playback command surface") { !self.app.commandPopover.isShown }
        try movePointer(over: video)
        app.upNext.button.performClick(nil)
        try await wait("reopened Up Next for the pinned checkpoint") { self.app.upNext.isShown }
    }

    private func typeVolume(_ text: String, expecting level: Double) async throws {
        try movePointer(over: osc.muteButton)
        let field = osc.volumeReadout
        field.selectText(nil)
        guard let editor = field.currentEditor() as? NSTextView else {
            throw VerifyError("Exact volume field did not enter editing")
        }
        editor.insertText(text, replacementRange: editor.selectedRange())
        try pressKey("\r", code: 36, focusVideo: false)
        try await wait("observed typed volume \(text)%") {
            abs((self.snapshot.volume ?? -1) - level) < 0.05
        }
    }

    private func checkWheel() async throws {
        guard let video = app.video, let before = snapshot.volume,
              let screen = NSScreen.screens.first else { throw VerifyError("Wheel needs observed volume and a screen") }
        let local = video.convert(NSPoint(x: video.bounds.midX, y: video.bounds.midY), to: nil)
        let screenPoint = app.window.convertPoint(toScreen: local)
        guard let raw = CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1,
                                wheel1: 1, wheel2: 0, wheel3: 0) else {
            throw VerifyError("Could not create a wheel input")
        }
        raw.location = CGPoint(x: screenPoint.x, y: screen.frame.maxY - screenPoint.y)
        guard let event = NSEvent(cgEvent: raw) else { throw VerifyError("Could not convert wheel input to AppKit") }
        app.window.sendEvent(event)
        if options.negativeControl {
            try await Task.sleep(nanoseconds: 300_000_000)
        } else {
            try await wait("wheel raised observed volume from \(before)") {
                abs((self.snapshot.volume ?? before) - before - 1) < 0.05
            }
        }
        if abs((snapshot.volume ?? before) - before - 1) >= 0.05 { failures.append("osc_volume_speed:wheel") }
    }

    private func checkSpeedMute() async throws {
        guard let previous = snapshot.volume else { throw VerifyError("Volume was not observed") }
        osc.muteButton.performClick(nil)
        try await wait("speaker muted playback") { self.osc.transport?.volume.muted == true }
        osc.muteButton.performClick(nil)
        try await wait("unmute restored the previous level") {
            self.osc.transport?.volume.muted == false && abs((self.snapshot.volume ?? 0) - previous) < 0.05
        }
        let menu = try trackMenu(open: { self.osc.speedButton.performClick(nil) })
        guard let choice = menu.items.firstIndex(where: { ($0.representedObject as? Double) == 1.5 }) else {
            throw VerifyError("Speed menu lacks its 1.50× choice")
        }
        menu.performActionForItem(at: choice)
        try await wait("selected speed is observed by the engine and is the newest OSD") {
            self.snapshot.speed == 1.5 && self.osc.speedButton.value == "1.50×" &&
            self.app.statusLayer.toastView.label.stringValue == "Speed 1.50×" &&
            !self.app.statusLayer.toastView.isHidden
        }
    }

    private func source(_ route: String) throws -> MediaSource {
        guard let fixture = options.httpFixture else { throw VerifyError("Missing loopback HTTP fixture") }
        return MediaSource(kind: .url, location: fixture.appendingPathComponent(route).absoluteString)
    }

    private func tellFixture(_ command: String) async throws {
        guard let base = options.httpFixture else { throw VerifyError("Missing loopback HTTP fixture") }
        let request = URLRequest(url: base.appendingPathComponent(command), timeoutInterval: 4)
        let (_, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 204 else {
            throw VerifyError("Fixture did not acknowledge \(command)")
        }
    }

    private func openFailure() async throws {
        let failed = try source("failure.mp4")
        app.play(failed)
        try await wait("loopback HTTP failure belongs to the current source") {
            self.failureCard?.source == failed && !self.app.statusLayer.overlay.cardPanel.isHidden
        }
    }

    private var failureCard: FailureCard? {
        if case .failure(let card) = app.statusLayer.display { return card }
        return nil
    }

    private func checkFailureActions() async throws {
        let overlay = app.statusLayer.overlay
        guard let card = failureCard, let copy = overlay.button(for: .copyDetails),
              let retry = overlay.button(for: .retry), let another = overlay.button(for: .openAnother) else {
            throw VerifyError("Failure card lacks one of its actions")
        }
        let clipboard = NSPasteboard.general
        clipboard.clearContents()
        clipboard.setString("unchanged", forType: .string)
        copy.performClick(nil)
        if clipboard.string(forType: .string) != card.detail {
            failures.append("osc_status:copy_details")
        }
        if !app.statusLayer.toastView.isHidden { failures.append("osc_status:multiple_surfaces") }
        another.performClick(nil)
        try await wait("Open another presented the native file chooser") { self.app.window.attachedSheet != nil }
        guard let sheet = app.window.attachedSheet else { throw VerifyError("File chooser closed unexpectedly") }
        app.window.endSheet(sheet, returnCode: .cancel)
        try await tellFixture("allow-retry")
        retry.performClick(nil)
        try await wait("Retry loaded the same HTTP source and cleared the failure") {
            self.snapshot.source == card.source && self.snapshot.status == .playing &&
            (self.snapshot.timePos ?? 0) > 0.3 && self.app.statusLayer.display.isClear
        }
    }

    private func checkStale() async throws {
        app.play(try source("stale.mp4"))
        try await wait("second stalled source is opening") {
            if case .progress = self.app.statusLayer.display { return true }
            return false
        }
        app.application(NSApp, open: [options.fixture])
        try await tellFixture("release-stale")
        try await wait("late HTTP failure did not cover the replacement source") {
            self.snapshot.source?.location == self.options.fixture.path && self.snapshot.status == .playing &&
            (self.snapshot.timePos ?? 0) > 0.3 && self.app.statusLayer.display.isClear
        }
    }

    private func captureTransport(_ name: String, _ expected: VerificationScenario.Expectation) async throws {
        try await Task.sleep(nanoseconds: 230_000_000)
        guard let root = app.window.contentView, let video = app.video else {
            throw VerifyError("The video plane disappeared", blocked: true)
        }
        root.layoutSubtreeIfNeeded()
        let panel = osc.panel
        let panelRect = panel.convert(panel.bounds, to: root)
        let live = osc.slots.filter { !$0.view.isHidden }
        let hit = osc.hitTest(panel.convert(NSPoint(x: panel.bounds.midX, y: panel.bounds.midY),
                                            to: osc.superview)) != nil
        let visible = !panel.isHiddenOrHasHiddenAncestor && panel.alphaValue > 0
        let progress = !app.statusLayer.overlay.progressPanel.isHidden
        let failed = !app.statusLayer.overlay.cardPanel.isHidden
        let toast = !app.statusLayer.toastView.isHidden
        if [progress, failed, toast].filter({ $0 }).count > 1 { failures.append("osc_status:multiple_surfaces") }
        switch expected {
        case .oscRow:
            let row = live.allSatisfy { panel.bounds.contains($0.view.frame) &&
                abs($0.view.frame.midY - osc.scrubber.frame.midY) < 0.5 }
            let nonoverlap = zip(live, live.dropFirst()).allSatisfy { $0.0.view.frame.maxX <= $0.1.view.frame.minX }
            if !visible || !hit || !row || !nonoverlap || live.last?.id != "overflow" ||
                osc.scrubber.frame.minX <= osc.elapsedLabel.frame.maxX ||
                osc.scrubber.frame.maxX >= osc.trailingLabel.frame.minX ||
                (name == "default" ? !osc.collapsed.isEmpty : osc.collapsed.isEmpty) {
                failures.append("osc_layout:\(name)")
            }
        case .oscHidden:
            if visible || hit || !osc.cursorHidden { failures.append("osc_idle_and_pins:\(name)") }
        case .oscPinned:
            if !visible || !hit || osc.cursorHidden || !app.upNext.isShown {
                failures.append("osc_idle_and_pins:\(name)")
            }
        case .volume:
            let capsule = osc.volumeCapsule.convert(osc.volumeCapsule.bounds, to: osc)
            if !visible || osc.volumeCapsule.isHidden || capsule.minY <= panel.frame.maxY ||
                !osc.bounds.contains(capsule) || osc.volumeSlider.accessibilityRole() != .slider ||
                abs((osc.transport?.volume.unityFraction ?? 0) - 100.0 / 130.0) > 0.001 ||
                (name == "unity" && osc.transport?.volume.level != 100) ||
                (name == "boost" && osc.transport?.volume.boosted != true && !options.negativeControl) {
                failures.append("osc_volume_speed:\(name)")
            }
        case .speed:
            if snapshot.speed != 1.5 || !toast { failures.append("osc_volume_speed:\(name)") }
        case .loading:
            if !progress || failed || !app.statusLayer.overlay.progressLabel.stringValue.contains("Opening") {
                failures.append("osc_status:\(name)")
            }
        case .buffering:
            if !progress || failed || !app.statusLayer.overlay.progressLabel.stringValue.contains("Buffering") {
                failures.append("osc_status:\(name)")
            }
        case .failure:
            if !failed || progress || failureCard?.detail.isEmpty != false ||
                !app.statusLayer.overlay.sourceLabel.stringValue.contains("failure.mp4") {
                failures.append("osc_status:\(name)")
            }
        case .recovered:
            if progress || failed || snapshot.status != .playing { failures.append("osc_status:\(name)") }
        default:
            throw VerifyError("Unsupported transport checkpoint")
        }

        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        let image: CGImage
        let mode: String
        if let compositor = await captureCompositor(width: width, height: height) {
            image = compositor
            mode = "window_compositor_2x"
        } else {
            var frame: CGImage?
            if snapshot.video != nil && (snapshot.status == .playing || snapshot.status == .paused) {
                let drawn = video.videoLayer.verificationFrame(width: Int((video.bounds.width * 2).rounded()),
                                                               height: Int((video.bounds.height * 2).rounded()))
                guard let drawn, drawn.hasPicture else {
                    throw VerifyError("No live 2× libmpv frame at \(name)", blocked: true)
                }
                frame = drawn.image
            }
            guard let composite = windowComposite(root: root, video: video, frame: frame) else {
                throw VerifyError("No 2× AppKit composition at \(name)", blocked: true)
            }
            image = composite
            mode = "app_composite_2x"
        }
        guard let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode \(name) at 2×", blocked: true)
        }
        let file = options.output.appendingPathComponent(name + ".png")
        try data.write(to: file)
        observations.append([
            "name": name, "png": file.lastPathComponent, "capture": mode, "render_scale": 2,
            "window_bounds_points": dimensions(root.bounds), "video_bounds_points": dimensions(video.bounds),
            "osc_rect_points": dimensions(panelRect), "osc_visible": visible, "osc_hit_testable": hit,
            "cursor_hidden": osc.cursorHidden, "pin_count": osc.autoHide.pins,
            "slots": live.map { ["id": $0.id, "rect": dimensions($0.view.frame)] },
            "collapsed": osc.collapsed.map(\.id), "playback": snapshot.status.rawValue,
            "time_seconds": snapshot.timePos ?? -1, "volume": snapshot.volume ?? -1,
            "speed": snapshot.speed ?? -1, "toast": toast ? app.statusLayer.toastView.label.stringValue : "",
        ])
        print("OKP_VERIFY captured \(name) via \(mode)")
    }
}
