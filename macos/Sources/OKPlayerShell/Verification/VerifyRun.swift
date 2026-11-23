import AppKit
import ScreenCaptureKit

struct VerificationOptions {
    let scenario: URL
    let fixture: URL
    let output: URL
    let profile: URL
    let appearance: String
    let controlDisablesClearance: Bool

    init?(arguments: [String]) {
        func value(_ option: String) -> String? {
            arguments.first { $0.hasPrefix(option + "=") }.map { String($0.dropFirst(option.count + 1)) }
        }
        guard let scenario = value("--verify-scenario"), let fixture = value("--verify-fixture"),
              let output = value("--verify-output"), let profile = value("--verify-profile"),
              let appearance = value("--verify-appearance") else { return nil }
        self.scenario = URL(fileURLWithPath: scenario)
        self.fixture = URL(fileURLWithPath: fixture)
        self.output = URL(fileURLWithPath: output, isDirectory: true)
        self.profile = URL(fileURLWithPath: profile, isDirectory: true)
        self.appearance = appearance
        controlDisablesClearance = arguments.contains("--verify-no-subtitle-clearance")
    }

    func config(shellCommands: [String]) -> SessionConfig {
        var config = SessionConfig.forApp(shellCommands: shellCommands)
        config.dataDir = profile.appendingPathComponent("data").path
        config.configDir = profile.appendingPathComponent("config").path
        config.cacheDir = profile.appendingPathComponent("cache").path
        return config
    }

    func write(_ result: [String: Any]) throws {
        let bytes = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
        try bytes.write(to: output.appendingPathComponent("app-result.json"), options: .atomic)
    }

    func finish(status: String, error: String) -> Never {
        try? write(["status": status, "error": error])
        fputs("Verification: \(error)\n", stderr)
        exit(2)
    }
}

struct VerificationScenario: Decodable {
    struct Step: Decodable {
        let action: Action
        let seconds: Double?
        let name: String?
        let expect: Expectation?
    }
    enum Action: String, Decodable {
        case open, pause, seek, capture
        case playUntilHidden = "play_until_hidden", pointerReveal = "pointer_reveal"
        case enterFullscreen = "enter_fullscreen", leaveFullscreen = "leave_fullscreen"
    }
    enum Expectation: String, Decodable {
        case noCue = "no_cue", clearance, baseMargin = "base_margin"
    }
    let schemaVersion: Int
    let id: String
    let steps: [Step]
}

private struct VerifyError: LocalizedError {
    let message: String
    let blocked: Bool
    init(_ message: String, blocked: Bool = false) {
        self.message = message
        self.blocked = blocked
    }
    var errorDescription: String? { message }
}

@MainActor
final class VerifyRun {
    private unowned let app: PlayerApp
    private let options: VerificationOptions
    private var observations: [[String: Any]] = []
    private var failures: [String] = []
    private var visibleCueBottom: CGFloat?
    private var hiddenCueBottom: CGFloat?
    private var compositorStatus = "not_probed"

    init(app: PlayerApp, options: VerificationOptions) {
        self.app = app
        self.options = options
    }

    private var snapshot: SessionSnapshot { app.client?.store.snapshot ?? SessionSnapshot() }
    private var swaps: Int { app.video?.videoLayer.counts.swaps ?? 0 }

    func run() async {
        var result: [String: Any] = [
            "appearance": options.appearance, "pid": getpid(),
            "control": options.controlDisablesClearance ? "subtitle_clearance_disabled" : "none",
        ]
        var code: Int32 = 0
        do {
            let scenario = try SessionJSON.makeDecoder().decode(VerificationScenario.self, from: Data(contentsOf: options.scenario))
            guard scenario.schemaVersion == 1, scenario.id == "subtitle-lift" else {
                throw VerifyError("Unsupported scenario or schema")
            }
            try await wait("mapped AppKit window", blocked: true) {
                self.app.window.isVisible && self.app.window.windowNumber > 0 && self.app.window.screen != nil
            }
            let appearance: NSAppearance.Name = options.appearance == "dark" ? .darkAqua : .aqua
            app.window.appearance = NSAppearance(named: appearance)
            guard app.window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == appearance else {
                throw VerifyError("Requested appearance is unavailable", blocked: true)
            }
            guard let windowServerRows = CGWindowListCopyWindowInfo(.optionIncludingWindow,
                                                                   CGWindowID(app.window.windowNumber)) as? [[String: Any]],
                  windowServerRows.contains(where: { ($0[kCGWindowOwnerPID as String] as? Int32) == getpid() }) else {
                throw VerifyError("WindowServer did not list the expected window/PID", blocked: true)
            }
            result["window_id"] = app.window.windowNumber
            result["source_sha"] = Bundle.main.object(forInfoDictionaryKey: "OKPSourceSHA")
            result["revision"] = Bundle.main.object(forInfoDictionaryKey: "OKPSourceRevision")
            result["window_backing_scale"] = app.window.backingScaleFactor
            compositorStatus = CGPreflightScreenCaptureAccess() ? "available" : "permission_unavailable"
            for step in scenario.steps {
                try await perform(step)
            }
            result["status"] = failures.isEmpty ? "passed" : "failed"
            code = failures.isEmpty ? 0 : 1
        } catch {
            result["error"] = error.localizedDescription
            result["status"] = (error as? VerifyError)?.blocked == true ? "blocked_environment" : "error"
            code = 2
        }
        result["checkpoints"] = observations
        result["assertion_failures"] = failures
        result["compositor"] = compositorStatus
        do {
            try options.write(result)
        } catch {
            fputs("Verification could not write its result: \(error)\n", stderr)
            code = 2
        }
        app.shutdown()
        exit(code)
    }

    private func perform(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .open:
            let before = swaps
            app.application(NSApp, open: [options.fixture])
            try await wait("fixture playback, external SRT selection and frame swap") {
                self.snapshot.source?.location == self.options.fixture.path &&
                self.snapshot.status == .playing && self.selectedExternal != nil && self.swaps > before
            }
        case .pause:
            try pressKey(" ", code: 49)
            try await wait("pause via window responder") { self.snapshot.status == .paused }
        case .seek:
            guard let seconds = step.seconds else { throw VerifyError("Seek lacks a timestamp") }
            let before = swaps
            try seek(seconds)
            try await wait("seek to \(seconds) s and fresh swap") {
                abs((self.snapshot.timePos ?? -1) - seconds) < 0.08 && self.swaps > before
            }
        case .playUntilHidden:
            try pressKey(" ", code: 49)
            try await wait("playing with OSC auto-hidden") {
                self.snapshot.status == .playing && !self.app.osc.isRevealed
            }
        case .pointerReveal:
            let window = app.window!
            guard let video = app.video,
                  let event = NSEvent.mouseEvent(with: .mouseMoved, location: video.convert(
                    CGPoint(x: video.bounds.midX, y: video.bounds.midY), to: nil),
                    modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0) else {
                throw VerifyError("Cannot construct an AppKit pointer event")
            }
            // Deliver to the same tracking-area responder that receives a real window mouse move.
            app.osc.mouseMoved(with: event)
            try await wait("OSC revealed by pointer tracking") { self.app.osc.isRevealed }
        case .enterFullscreen, .leaveFullscreen:
            let entering = step.action == .enterFullscreen
            guard let behaviour = app.windowBehaviour else {
                throw VerifyError("The native fullscreen controller is unavailable", blocked: true)
            }
            var completed = false
            let before = swaps
            behaviour.onFullscreenChanged = { completed = $0 == entering }
            defer { behaviour.onFullscreenChanged = nil }
            try pressKey(entering ? "f" : "\u{1b}", code: entering ? 3 : 53)
            try await wait("native fullscreen \(entering ? "entered" : "exited") and frame swapped", blocked: true) {
                completed && self.swaps > before
            }
        case .capture:
            guard let name = step.name, let expectation = step.expect else {
                throw VerifyError("Capture lacks its name or expectation")
            }
            try await capture(name, expecting: expectation)
        }
    }

    private var selectedExternal: Track? {
        snapshot.tracks.first { $0.kind == .subtitle && $0.external && $0.selected }
    }

    private func wait(_ step: String, blocked: Bool = false, condition: () -> Bool) async throws {
        let limit = ProcessInfo.processInfo.systemUptime + 8
        while !condition() {
            if ProcessInfo.processInfo.systemUptime >= limit { throw VerifyError("Timed out: \(step)", blocked: blocked) }
            try await Task.sleep(nanoseconds: 30_000_000)
        }
        print("OKP_VERIFY \(step)")
    }

    private func pressKey(_ characters: String, code: UInt16) throws {
        let window = app.window!
        guard window.makeFirstResponder(app.video),
              let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil,
                                           characters: characters, charactersIgnoringModifiers: characters,
                                           isARepeat: false, keyCode: code) else {
            throw VerifyError("Cannot deliver a key event to the video responder")
        }
        window.sendEvent(event)
    }

    private func seek(_ timestamp: Double) throws {
        let window = app.window!
        window.contentView?.layoutSubtreeIfNeeded()
        let slider = app.osc.scrubber
        guard slider.isEnabled, slider.accessibilityRole() == .slider, let duration = snapshot.duration else {
            throw VerifyError("Seek AX slider is unavailable")
        }
        let inset = ScrubberView.inset
        let x = inset + (slider.bounds.width - 2 * inset) * timestamp / duration
        let location = slider.convert(CGPoint(x: x, y: slider.bounds.midY), to: nil)
        for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(with: type, location: location, modifierFlags: [],
                                                timestamp: ProcessInfo.processInfo.systemUptime,
                                                windowNumber: window.windowNumber, context: nil, eventNumber: 0,
                                                clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0) else {
                throw VerifyError("Cannot construct an AppKit seek event")
            }
            window.sendEvent(event)
        }
    }

    private func dimensions(_ rect: CGRect) -> [String: Double] {
        ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]
    }

    private func capture(_ name: String, expecting expectation: VerificationScenario.Expectation) async throws {
        // Let AppKit finish presentation and the live video layer swap before reading the FBO.
        try await Task.sleep(nanoseconds: 330_000_000)
        guard let root = app.window.contentView, let video = app.video, let track = selectedExternal else {
            throw VerifyError("The selected external SRT or video view disappeared")
        }
        root.layoutSubtreeIfNeeded()
        let panel = app.osc.panel
        let rect = panel.convert(panel.bounds, to: video)
        let panelHit = app.osc.hitTest(panel.convert(CGPoint(x: panel.bounds.midX, y: panel.bounds.midY),
                                                         to: app.osc.superview)) != nil
        let visible = !panel.isHidden && panel.alphaValue > 0
        let expectsVisible = expectation != .baseMargin
        guard visible == expectsVisible, panelHit == expectsVisible, rect.height > 0 else {
            throw VerifyError("OSC visibility/hit testing disagreed with checkpoint \(name)")
        }

        let frameSwaps = swaps
        let frameWidth = Int((video.bounds.width * 2).rounded())
        let frameHeight = Int((video.bounds.height * 2).rounded())
        guard frameWidth > 0, frameHeight > 0,
              let frame = video.videoLayer.verificationFrame(width: frameWidth, height: frameHeight),
              frame.videoPresent else {
            throw VerifyError("Genuine 2× libmpv capture is unavailable at \(name)", blocked: true)
        }
        let time = snapshot.timePos ?? -1
        let file = options.output.appendingPathComponent(name + ".png")
        let captureMode: String
        let composite: CGImage
        if let compositor = await captureCompositor(width: frameWidth, height: frameHeight) {
            captureMode = "window_compositor_2x"
            composite = compositor
        } else {
            captureMode = "app_composite_2x"
            var chrome: [NSView] = []
            if visible {
                if let title = app.windowBehaviour?.titleStrip { chrome.append(title) }
                chrome.append(panel)
                for button: NSWindow.ButtonType in [.closeButton, .miniaturizeButton, .zoomButton] {
                    if let view = app.window.standardWindowButton(button) { chrome.append(view) }
                }
            }
            guard let image = appComposite(frame.image, over: video, chrome: chrome) else {
                throw VerifyError("Could not compose the 2× video and actual AppKit chrome", blocked: true)
            }
            composite = image
        }
        guard let png = NSBitmapImageRep(cgImage: composite).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode 2× PNG", blocked: true)
        }
        try png.write(to: file)

        var checkpoint: [String: Any] = [
            "name": name, "time_seconds": time, "playback": snapshot.status.rawValue,
            "fullscreen": app.window.styleMask.contains(.fullScreen),
            "track_id": track.id, "track_external": track.external, "swap_count": frameSwaps,
            "osc_visible": visible, "osc_hit_testable": panelHit, "osc_rect_points": dimensions(rect),
            "seek_ax_role": app.osc.scrubber.accessibilityRole()?.rawValue ?? "",
            "seek_ax_label": app.osc.scrubber.accessibilityLabel() ?? "",
            "window_frame_points": dimensions(app.window.frame),
            "video_bounds_points": dimensions(video.bounds), "glyph_pixels": frame.glyphPixels,
            "render_scale": 2, "framebuffer_pixels": [frameWidth, frameHeight],
            "capture": captureMode, "color_space": "sRGB", "png": file.lastPathComponent,
        ]
        if let glyphs = frame.glyphBounds {
            checkpoint["glyph_bounds_pixels_bottom_origin"] = dimensions(glyphs)
        }
        observations.append(checkpoint)
        let glyphs = frame.glyphBounds
        switch expectation {
        case .noCue:
            guard glyphs == nil else { throw VerifyError("Fixture already has glyphs before its first cue") }
        case .clearance, .baseMargin:
            guard let glyphs, frame.glyphPixels > 100, glyphs.minY > 0,
                  glyphs.maxY < CGFloat(frameHeight) else {
                throw VerifyError("Known SRT cue was not painted inside the video at \(name)")
            }
            let bottom = glyphs.minY / 2
            if expectation == .clearance {
                if bottom < rect.maxY + 1 { failures.append("subtitle_clearance:\(name)") }
                if name.hasSuffix("second-cue") { visibleCueBottom = bottom }
                if name.hasSuffix("revealed-osc"), let hiddenCueBottom, bottom <= hiddenCueBottom + 1 {
                    failures.append("subtitle_reveal_did_not_lift")
                }
            } else {
                hiddenCueBottom = bottom
                guard let visibleCueBottom else { throw VerifyError("Missing visible checkpoint for the second cue") }
                if bottom >= visibleCueBottom - 1 || bottom >= rect.maxY {
                    failures.append("subtitle_base_margin_not_restored")
                }
            }
        }
        print("OKP_VERIFY captured \(name) via \(captureMode)")
    }

    private func captureCompositor(width: Int, height: Int) async -> CGImage? {
        guard compositorStatus == "available" || compositorStatus == "captured" else { return nil }
        guard app.window.backingScaleFactor == 2 else {
            compositorStatus = "window_backing_is_not_2x"
            return nil
        }
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            guard let own = content.windows.first(where: { $0.windowID == CGWindowID(app.window.windowNumber) }) else {
                compositorStatus = "window_unavailable"
                return nil
            }
            let config = SCStreamConfiguration()
            config.width = width
            config.height = height
            config.showsCursor = false
            config.ignoreShadowsSingleWindow = true
            let image = try await SCScreenshotManager.captureImage(contentFilter: SCContentFilter(desktopIndependentWindow: own),
                                                                  configuration: config)
            guard image.width == width && image.height == height else {
                compositorStatus = "unexpected_dimensions"
                return nil
            }
            compositorStatus = "captured"
            return image
        } catch {
            compositorStatus = "capture_failed"
            return nil
        }
    }

    private func appComposite(_ frame: CGImage, over video: VideoView, chrome: [NSView]) -> CGImage? {
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: frame.width, height: frame.height,
                                      bitsPerComponent: 8, bytesPerRow: frame.width * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.draw(frame, in: CGRect(x: 0, y: 0, width: frame.width, height: frame.height))
        context.scaleBy(x: 2, y: 2)
        for view in chrome where !view.isHidden && view.alphaValue > 0 && !view.bounds.isEmpty {
            let bounds = view.bounds
            guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(ceil(bounds.width * 2)),
                                               pixelsHigh: Int(ceil(bounds.height * 2)), bitsPerSample: 8,
                                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                               colorSpaceName: .deviceRGB,
                                               bytesPerRow: 0, bitsPerPixel: 0) else { return nil }
            bitmap.size = bounds.size
            view.effectiveAppearance.performAsCurrentDrawingAppearance { view.cacheDisplay(in: bounds, to: bitmap) }
            guard let image = bitmap.cgImage else { return nil }
            context.draw(image, in: view.convert(bounds, to: video))
        }
        return context.makeImage()
    }
}
