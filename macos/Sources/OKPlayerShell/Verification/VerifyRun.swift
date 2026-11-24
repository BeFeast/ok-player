import AppKit
import ScreenCaptureKit

struct VerificationOptions {
    let scenario: URL
    let fixture: URL
    let output: URL
    let profile: URL
    let appearance: String
    let negativeControl: Bool
    let httpFixture: URL?
    var scenarioID: String { scenario.deletingPathExtension().lastPathComponent }
    var control: String {
        guard negativeControl else { return "none" }
        return switch scenarioID {
        case "subtitle-lift": "subtitle_clearance_disabled"
        case "window-chrome": "title_identity_hidden"
        case "window-fullscreen": "video_fullscreen_disabled"
        case "window-fit": "initial_fit_disabled"
        case "window-float": "floating_level_disabled"
        case "osc-layout": "slot_collapse_disabled"
        case "osc-idle-and-pins": "panel_pin_disabled"
        case "osc-volume-speed": "volume_wheel_disabled"
        case "osc-status": "copy_details_disabled"
        default: "unsupported_control"
        }
    }
    var controlDisablesClearance: Bool { control == "subtitle_clearance_disabled" }

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
        httpFixture = value("--verify-http-fixture").flatMap { URL(string: $0) }
        negativeControl = arguments.contains("--verify-negative-control")
    }

    func config(shellCommands: [String]) -> SessionConfig {
        var config = SessionConfig.forApp(shellCommands: shellCommands)
        config.dataDir = profile.appendingPathComponent("data").path
        config.configDir = profile.appendingPathComponent("config").path
        config.cacheDir = profile.appendingPathComponent("cache").path
        if scenarioID == "osc-status" {
            config.mpvOptions.append(MpvOption(name: "network-timeout", value: "5"))
        }
        if let spec = try? readScenario(), let languages = spec.preferredSubtitleLanguages {
            config.mpvOptions.append(MpvOption(name: "slang", value: languages))
        }
        return config
    }

    func readScenario() throws -> VerificationScenario {
        try SessionJSON.makeDecoder().decode(VerificationScenario.self, from: Data(contentsOf: scenario))
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
        let subtitleLabels: [String]?
        let selectedSubtitle: String?
        let width: Double?
    }
    enum Action: String, Decodable {
        case open, pause, seek, capture
        case playTo = "play_to", playUntilHidden = "play_until_hidden", pointerReveal = "pointer_reveal"
        case enterFullscreen = "enter_fullscreen", leaveFullscreen = "leave_fullscreen"
        case checkSubtitleMenu = "check_subtitle_menu"
        case sizeWindow = "size_window", closeMedia = "close_media", doubleClick = "double_click"
        case smallWorkArea = "small_work_area", openPortrait = "open_portrait", resizeEdges = "resize_edges"
        case pin, unpin, overlapWindow = "overlap_window"
        case sizeOSC = "size_osc", checkMore = "check_more", checkSeek = "check_seek"
        case checkIdle = "check_idle", checkPins = "check_pins"
        case showVolume = "show_volume", checkWheel = "check_wheel", checkSpeedMute = "check_speed_mute"
        case openStall = "open_stall", releaseStall = "release_stall", openFailure = "open_failure"
        case checkFailureActions = "check_failure_actions", checkStale = "check_stale"
    }
    enum Expectation: String, Decodable {
        case noCue = "no_cue", clearance, baseMargin = "base_margin"
        case welcome, playing, fullscreen, restored, landscape, portrait, idle, floating, normal, repinned
        case oscRow = "osc_row", oscHidden = "osc_hidden", oscPinned = "osc_pinned"
        case volume, speed, loading, failure, recovered
    }
    let schemaVersion: Int
    let id: String
    let preferredSubtitleLanguages: String?
    let steps: [Step]
}

struct VerifyError: LocalizedError {
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
    unowned let app: PlayerApp
    let options: VerificationOptions
    var observations: [[String: Any]] = []
    var failures: [String] = []
    var initialFrame: CGRect?
    var preFullscreenFrame: CGRect?
    var preFullscreenTime: Double?
    var fullscreenEntries = 0
    var testWindow: NSWindow?
    private var visibleCueBottom: CGFloat?
    private var hiddenCueBottom: CGFloat?
    private var compositorStatus = "not_probed"
    private var subtitleMenu: [String: [String]]?

    init(app: PlayerApp, options: VerificationOptions) {
        self.app = app
        self.options = options
    }

    var snapshot: SessionSnapshot { app.client?.store.snapshot ?? SessionSnapshot() }
    var swaps: Int { app.video?.videoLayer.counts.swaps ?? 0 }

    func run() async {
        var result: [String: Any] = [
            "appearance": options.appearance, "pid": getpid(),
            "control": options.control,
        ]
        var code: Int32 = 0
        do {
            let scenario = try options.readScenario()
            guard scenario.schemaVersion == 1, scenario.id == options.scenarioID else {
                throw VerifyError("Unsupported scenario or schema")
            }
            try await wait("mapped AppKit window registered with WindowServer", blocked: true) {
                guard let window = self.app.window, window.isVisible, window.windowNumber > 0, window.screen != nil,
                      let rows = CGWindowListCopyWindowInfo(.optionIncludingWindow,
                                                            CGWindowID(window.windowNumber)) as? [[String: Any]] else { return false }
                return rows.contains { ($0[kCGWindowOwnerPID as String] as? Int32) == getpid() }
            }
            guard let window = app.window else { throw VerifyError("The mapped player window disappeared", blocked: true) }
            let appearance: NSAppearance.Name = options.appearance == "dark" ? .darkAqua : .aqua
            window.appearance = NSAppearance(named: appearance)
            guard window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == appearance else {
                throw VerifyError("Requested appearance is unavailable", blocked: true)
            }
            result["window_id"] = window.windowNumber
            result["source_sha"] = Bundle.main.object(forInfoDictionaryKey: "OKPSourceSHA")
            result["revision"] = Bundle.main.object(forInfoDictionaryKey: "OKPSourceRevision")
            result["window_backing_scale"] = window.backingScaleFactor
            compositorStatus = CGPreflightScreenCaptureAccess() ? "available" : "permission_unavailable"
            for step in scenario.steps {
                if scenario.id.hasPrefix("window-") { try await performWindowStep(step) }
                else if scenario.id.hasPrefix("osc-") { try await performTransportStep(step) }
                else { try await perform(step) }
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
        result["subtitle_menu"] = subtitleMenu
        do {
            try options.write(result)
        } catch {
            fputs("Verification could not write its result: \(error)\n", stderr)
            code = 2
        }
        testWindow?.close()
        testWindow = nil
        app.shutdown()
        exit(code)
    }

    func perform(_ step: VerificationScenario.Step) async throws {
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
        case .playTo:
            guard let seconds = step.seconds else { throw VerifyError("Playback step lacks a timestamp") }
            try pressKey(" ", code: 49)
            try await wait("playing through subtitle packet at \(seconds) s") {
                self.snapshot.status == .playing && (self.snapshot.timePos ?? -1) >= seconds
            }
        case .playUntilHidden:
            try pressKey(" ", code: 49)
            try await wait("playing with OSC auto-hidden") {
                self.snapshot.status == .playing && !self.app.osc.isRevealed
            }
        case .pointerReveal:
            guard let window = app.window, let video = app.video,
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
        case .checkSubtitleMenu:
            guard let expectedLabels = step.subtitleLabels, let expectedSelected = step.selectedSubtitle else {
                throw VerifyError("Subtitle menu check lacks labels or selection")
            }
            let items = app.tracks.menuItems(.subtitle).prefix { !$0.isSeparatorItem }
            let labels = items.map(\.title)
            let selected = items.filter { $0.state == .on }.map(\.title)
            subtitleMenu = ["labels": labels, "selected": selected]
            if labels != expectedLabels || selected != [expectedSelected] {
                failures.append("subtitle_menu")
            }
        default:
            throw VerifyError("Unsupported subtitle step")
        }
    }

    private var selectedExternal: Track? {
        snapshot.tracks.first { $0.kind == .subtitle && $0.external && $0.selected }
    }

    func wait(_ step: String, blocked: Bool = false, condition: () -> Bool) async throws {
        let limit = ProcessInfo.processInfo.systemUptime + 8
        while !condition() {
            if ProcessInfo.processInfo.systemUptime >= limit { throw VerifyError("Timed out: \(step)", blocked: blocked) }
            try await Task.sleep(nanoseconds: 30_000_000)
        }
        print("OKP_VERIFY \(step)")
    }

    func pressKey(_ characters: String, code: UInt16) throws {
        guard let window = app.window, window.makeFirstResponder(app.video),
              let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil,
                                           characters: characters, charactersIgnoringModifiers: characters,
                                           isARepeat: false, keyCode: code) else {
            throw VerifyError("Cannot deliver a key event to the video responder")
        }
        window.sendEvent(event)
    }

    func seek(_ timestamp: Double) throws {
        guard let window = app.window else { throw VerifyError("The seek window disappeared", blocked: true) }
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

    func dimensions(_ rect: CGRect) -> [String: Double] {
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
        guard frameWidth > 0, frameHeight > 0 else {
            throw VerifyError("The 2× framebuffer has no area", blocked: true)
        }
        var observed: VideoEvidence?
        // A seek can swap video before libmpv has updated the subtitle plane.
        // Retry missing glyphs, independent of their clearance relative to the OSC.
        for attempt in 0..<8 {
            observed = video.videoLayer.verificationFrame(width: frameWidth, height: frameHeight)
            if let observed, observed.videoPresent, expectation == .noCue || observed.glyphPixels > 100 { break }
            if attempt < 7 { try await Task.sleep(nanoseconds: 130_000_000) }
        }
        guard let frame = observed, frame.videoPresent else {
            throw VerifyError("Genuine 2× libmpv capture is unavailable at \(name)", blocked: true)
        }
        let time = snapshot.timePos ?? -1
        let file = options.output.appendingPathComponent(name + ".png")
        let captureMode: String
        let composite: CGImage
        let windowWidth = Int((root.bounds.width * 2).rounded())
        let windowHeight = Int((root.bounds.height * 2).rounded())
        if let compositor = await captureCompositor(width: windowWidth, height: windowHeight) {
            captureMode = "window_compositor_2x"
            composite = compositor
        } else {
            captureMode = "app_composite_2x"
            guard let image = windowComposite(root: root, video: video, frame: frame.image) else {
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
            "capture": captureMode, "color_space": (composite.colorSpace?.name as String?) ?? "unknown",
            "png": file.lastPathComponent,
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
        default:
            throw VerifyError("Unsupported subtitle checkpoint")
        }
        print("OKP_VERIFY captured \(name) via \(captureMode)")
    }

    func captureCompositor(width: Int, height: Int) async -> CGImage? {
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

}
