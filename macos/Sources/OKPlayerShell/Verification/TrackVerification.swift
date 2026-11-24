import AppKit

@MainActor
extension VerifyRun {
    func performTrackStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .open:
            try await performWindowStep(step)
        case .pause, .seek, .pointerReveal:
            try await perform(step)
        case .sizeOSC:
            try await performTransportStep(step)
        case .checkAudioSwitcher:
            try await checkAudioSwitcher()
        case .checkSubtitleSwitcher:
            try await checkSubtitleSwitcher()
        case .checkTrackPin:
            try await checkTrackPin()
            if options.scenarioID == "audio-switcher" { try await checkNoAudio() }
        case .capture:
            guard let name = step.name, step.expect == .trackPanel else {
                throw VerifyError("Track capture lacks a name or panel expectation")
            }
            try await captureTrackPanel(name)
        default:
            throw VerifyError("Unsupported track step \(step.action.rawValue)")
        }
    }

    private var tracks: TrackControls { app.tracks }

    private func openTracks(_ kind: TrackKind) throws {
        if tracks.panelKind != kind {
            let button = kind == .audio ? tracks.audioButton : tracks.subtitleButton
            guard button.isEnabled, !button.isHiddenOrHasHiddenAncestor, app.osc.isRevealed else {
                throw VerifyError("The \(kind.rawValue) OSC button is unavailable")
            }
            button.performClick(nil)
        }
        app.window.contentView?.layoutSubtreeIfNeeded()
        guard tracks.panelKind == kind else { throw VerifyError("The \(kind.rawValue) panel did not open") }
    }

    private func clickTrack(_ action: TrackMenuAction) throws {
        let controls = (tracks.panel.choiceButtons as [NSButton]) + tracks.panel.adjustmentButtons
        guard let button = controls.first(where: {
            ($0 as? TrackRowButton)?.trackAction == action || ($0 as? TrackStepButton)?.trackAction == action
        }), button.isEnabled else {
            throw VerifyError("The panel has no enabled control for \(action)")
        }
        button.performClick(nil)
    }

    private func checkSelection(_ kind: TrackKind, id: Int64?, contains text: String) throws {
        let selectedRows = tracks.panel.choiceButtons.filter {
            ($0.accessibilityValue() as? NSNumber)?.boolValue == true
        }
        let action = TrackMenuAction.select(kind: kind, id: id)
        if selectedRows.map(\.trackAction) != [action] || selectedRows.first?.title.contains(text) != true {
            failures.append("track_switcher:selected_\(text)")
        }
    }

    private func selectTrack(_ kind: TrackKind, id: Int64?, label: String) async throws {
        let action = TrackMenuAction.select(kind: kind, id: id)
        try openTracks(kind)
        try clickTrack(action)
        if tracks.isPopoverShown { failures.append("track_switcher:selection_dismissal") }
        try await wait("engine selected \(label)") {
            self.snapshot.tracks.first { $0.kind == kind && $0.selected }?.id == id
        }
        try openTracks(kind)
        try checkSelection(kind, id: id, contains: label)
    }

    private func checkAudioSwitcher() async throws {
        try await wait("both named audio tracks are available") {
            self.snapshot.tracks.filter { $0.kind == .audio && $0.title != nil }.count == 2
        }
        let rows = snapshot.tracks.filter { $0.kind == .audio }
        guard let low = rows.first(where: { $0.title == "Low 300Hz" }),
              let high = rows.first(where: { $0.title == "High 900Hz" }) else {
            throw VerifyError("The reference audio streams are missing")
        }
        try openTracks(.audio)
        try checkSelection(.audio, id: low.id, contains: "Low 300Hz")
        try await selectTrack(.audio, id: high.id, label: "High 900Hz")
        try await selectTrack(.audio, id: low.id, label: "Low 300Hz")
        trackChecks["audio_engine_ids_by_frequency_hz"] = ["300": low.id, "900": high.id]
    }

    private func checkNoAudio() async throws {
        let fixture = options.fixture.deletingLastPathComponent().appendingPathComponent("video-only.mp4")
        let before = swaps
        app.application(NSApp, open: [fixture])
        try await wait("video-only fixture playing") {
            self.snapshot.source?.location == fixture.path && self.snapshot.status == .playing &&
                self.snapshot.video != nil && self.swaps > before
        }
        try pressKey(" ", code: 49)
        try await wait("video-only fixture paused") { self.snapshot.status == .paused }
        try openTracks(.audio)
        if snapshot.tracks.contains(where: { $0.kind == .audio }) ||
            !tracks.panel.showsEmptyAudio || !tracks.panel.choiceButtons.isEmpty {
            failures.append("track_switcher:no_audio")
        }
    }

    private func checkSubtitleSwitcher() async throws {
        try await wait("both embedded subtitle tracks are available") {
            self.snapshot.tracks.filter { $0.kind == .subtitle && !$0.external }.count == 2
        }
        let subtitles = snapshot.tracks.filter { $0.kind == .subtitle && !$0.external }
        guard let en = subtitles.first(where: { $0.lang == "eng" }),
              let ru = subtitles.first(where: { $0.lang == "rus" }) else {
            throw VerifyError("The reference subtitle languages are missing")
        }
        try await selectTrack(.subtitle, id: en.id, label: "eng")
        let enCue = try await measureCue(wide: true)
        try await selectTrack(.subtitle, id: ru.id, label: "rus")
        let ruCue = try await measureCue(wide: false)
        try await selectTrack(.subtitle, id: nil, label: "Off")
        let off = try await measureCue(wide: nil)

        let external = options.fixture.deletingLastPathComponent().appendingPathComponent("added-external.srt")
        // Replace the system file chooser's response, while using the live Add button and session command.
        let present = tracks.presentFilePanel
        defer { tracks.presentFilePanel = present }
        var presented = false
        tracks.presentFilePanel = { window, chosen in
            presented = window === self.app.window
            chosen(external)
        }
        try clickTrack(.addSubtitle)
        if !presented || tracks.isPopoverShown { failures.append("track_switcher:add_panel") }
        try await wait("added external subtitle selected by the engine") {
            self.snapshot.tracks.contains { $0.kind == .subtitle && $0.external && $0.selected }
        }
        let added = snapshot.tracks.first { $0.kind == .subtitle && $0.external && $0.selected }
        try openTracks(.subtitle)
        try checkSelection(.subtitle, id: added?.id, contains: "External")
        let externalCue = try await measureCue(wide: true)
        trackChecks["subtitle_glyph_width_px"] = ["en": enCue, "ru": ruCue, "off": off, "external": externalCue]
        try await adjustSubtitles()
        try await selectTrack(.subtitle, id: en.id, label: "eng")
    }

    private func adjustSubtitles() async throws {
        for (action, delay, size) in [
            (TrackMenuAction.invoke(id: TrackMenus.delayForward), 0.05, 1.0),
            (.invoke(id: TrackMenus.sizeUp), 0.05, 1.1),
            (.invoke(id: TrackMenus.delayBack), 0.0, 1.1),
            (.invoke(id: TrackMenus.sizeDown), 0.0, 1.0),
            (.invoke(id: TrackMenus.delayBack), -0.05, 1.0),
            (.invoke(id: TrackMenus.sizeDown), -0.05, 0.9),
            (.resetDelay, 0.0, 0.9),
            (.resetSize, 0.0, 1.0),
        ] {
            try clickTrack(action)
            try await wait("engine reported subtitle delay \(delay) and size \(size)") {
                guard let view = self.tracks.query?(), let reportedDelay = view.subtitleDelay.reported,
                      let reportedSize = view.subtitleScale.reported else { return false }
                return abs(reportedDelay - delay) < 0.001 && abs(reportedSize - size) < 0.001
            }
            let ms = Int((delay * 1000).rounded())
            let expected = [(ms > 0 ? "+" : "") + "\(ms) ms", "\(Int((size * 100).rounded()))%"]
            if !tracks.isPopoverShown || tracks.panel.adjustmentValues != expected {
                failures.append("track_switcher:readout_\(ms)_\(size)")
            }
        }
        trackChecks["reset_readouts"] = tracks.panel.adjustmentValues
    }

    private func measureCue(wide: Bool?) async throws -> Int {
        var width = 0
        var matched = false
        // Track selection is reported before libmpv has rendered the new cue.
        for _ in 0..<8 {
            try await Task.sleep(nanoseconds: 120_000_000)
            let frame = try trackFrame()
            width = Int(frame.glyphBounds?.width ?? 0)
            let span = Double(width) / Double(frame.image.width)
            matched = wide.map { $0 ? span > 0.12 : (0.01...0.10).contains(span) } ?? (width == 0)
            if matched { break }
        }
        if !matched { failures.append("track_switcher:cue_width_\(wide.map(String.init) ?? "off")") }
        return width
    }

    private func checkTrackPin() async throws {
        guard let kind = tracks.panelKind else { throw VerifyError("Pin check has no track surface") }
        try pressKey(" ", code: 49)
        try await wait("playback resumed with the panel open") { self.snapshot.status == .playing }
        let time = snapshot.timePos ?? 0
        try await wait("playback crossed the OSC idle deadline") { (self.snapshot.timePos ?? 0) - time >= 2.9 }
        let pinned = app.osc.isRevealed && tracks.isPopoverShown
        if !pinned { failures.append("track_switcher:pin") }

        try pressKey("\u{1b}", code: WindowKeys.escapeKeyCode)
        if tracks.isPopoverShown { failures.append("track_switcher:escape") }
        try await wait("closing the track panel restored the OSC timeout") { !self.app.osc.isRevealed }

        app.osc.noteActivity()
        app.window.contentView?.layoutSubtreeIfNeeded()
        app.upNext.button.performClick(nil)
        guard app.upNext.isShown else { throw VerifyError("The underlying Up Next panel did not open") }
        try openTracks(kind)
        let other: TrackKind = kind == .audio ? .subtitle : .audio
        try openTracks(other)
        try pressKey("\u{1b}", code: WindowKeys.escapeKeyCode)
        if tracks.isPopoverShown || !app.upNext.isShown { failures.append("track_switcher:top_surface") }
        try pressKey("\u{1b}", code: WindowKeys.escapeKeyCode)
        if app.upNext.isShown { failures.append("track_switcher:next_escape") }
        try openTracks(kind)
        try clickOutsideTrackPanel()
        if tracks.isPopoverShown { failures.append("track_switcher:outside") }
        trackChecks["osc_pinned_across_idle"] = pinned
    }

    private func clickOutsideTrackPanel() throws {
        guard let video = app.video, let window = app.window else { throw VerifyError("Missing video window") }
        let location = video.convert(NSPoint(x: 40, y: video.bounds.midY), to: nil)
        for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(with: type, location: location, modifierFlags: [],
                                                timestamp: ProcessInfo.processInfo.systemUptime,
                                                windowNumber: window.windowNumber, context: nil, eventNumber: 0,
                                                clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0) else {
                throw VerifyError("Cannot construct an AppKit outside click")
            }
            window.sendEvent(event)
        }
    }

    private func trackFrame() throws -> VideoEvidence {
        guard let video = app.video, let frame = video.videoLayer.verificationFrame(
            width: Int((video.bounds.width * 2).rounded()), height: Int((video.bounds.height * 2).rounded())
        ), frame.hasPicture else { throw VerifyError("No actual 2× track fixture frame", blocked: true) }
        return frame
    }

    private func captureTrackPanel(_ name: String) async throws {
        try await Task.sleep(nanoseconds: 300_000_000)
        guard let window = app.window, let root = window.contentView, let video = app.video,
              let kind = tracks.panelKind else { throw VerifyError("The track panel or video is missing") }
        root.layoutSubtreeIfNeeded()
        let panel = tracks.panel
        let rect = panel.convert(panel.bounds, to: root)
        let bar = app.osc.panel.convert(app.osc.panel.bounds, to: root)
        let light = panel.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .aqua
        let anchor = kind == .audio ? tracks.audioButton : tracks.subtitleButton
        let anchorCenter = anchor.convert(NSPoint(x: anchor.bounds.midX, y: anchor.bounds.midY), to: root)
        let drawn = try trackFrame()
        if !root.bounds.contains(rect) || !rect.width.isFinite || rect.height <= 0 ||
            abs(rect.minY - bar.maxY - 4) > 0.5 || !app.osc.isRevealed || !light ||
            !(rect.minX...rect.maxX).contains(anchorCenter.x) {
            failures.append("track_switcher:geometry_\(name)")
        }
        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        let capture: (image: CGImage, mode: String)
        if let compositor = await captureCompositor(width: width, height: height) {
            capture = (compositor, "window_compositor_2x")
        } else if let composite = windowComposite(root: root, video: video, frame: drawn.image) {
            capture = (composite, "app_composite_2x")
        } else {
            throw VerifyError("No 2× track panel composite", blocked: true)
        }
        guard let bytes = NSBitmapImageRep(cgImage: capture.image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Cannot encode track panel PNG", blocked: true)
        }
        let file = options.output.appendingPathComponent(name + ".png")
        try bytes.write(to: file)
        observations.append([
            "name": name, "png": file.lastPathComponent, "capture": capture.mode, "render_scale": 2,
            "window_bounds_points": dimensions(root.bounds), "panel_bounds_points": dimensions(rect),
            "osc_rect_points": dimensions(bar), "panel_light": light, "kind": kind.rawValue,
            "time_seconds": snapshot.timePos ?? -1, "glyph_width_pixels": drawn.glyphBounds?.width ?? 0,
            "rows": panel.choiceButtons.map(\.title), "adjustments": panel.adjustmentValues,
        ])
        print("OKP_VERIFY captured \(name) via \(capture.mode)")
    }
}
