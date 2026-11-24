import AppKit

@MainActor
extension VerifyRun {
    func performWelcomeReturnStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .sizeWindow, .closeMedia:
            try await performWindowStep(step)
        case .open:
            try await openWelcomeFixture()
        case .waitForEOF:
            try await wait("natural EOF with no successor") {
                self.app.history.showsWelcome || self.snapshot.endReason == .eof
            }
        case .closeMediaShortcut:
            try pressKey("w", code: 13, modifiers: .command, throughApplication: true)
            try await wait("restored Welcome via Command-W") {
                self.snapshot.source == nil && self.app.history.showsWelcome
            }
        case .advancePlaylist:
            let next = options.fixture.deletingLastPathComponent().appendingPathComponent("next/second.mp4")
            guard app.open([options.fixture, next]) else { throw VerifyError("Could not open the two-item queue") }
            try await wait("EOF advances to the next item without closing playback") {
                self.snapshot.status == .playing && self.snapshot.source?.location == next.path
                    && !self.app.history.showsWelcome
            }
        case .openAnother:
            let invalid = options.fixture.deletingLastPathComponent().appendingPathComponent("broken/invalid.mp4")
            app.play(invalid.path)
            try await wait("invalid media shows the playback error") { self.app.errorShown }
            guard !app.history.showsWelcome, let action = app.statusLayer.overlay.button(for: .openAnother),
                  action.isEnabled else { throw VerifyError("Open Another is unavailable on the playback error") }
            action.performClick(nil)
            try await wait("Open Another presents the native picker") { self.app.window.attachedSheet is NSOpenPanel }
            (app.window.attachedSheet as? NSOpenPanel)?.cancel(nil)
            try await wait("Open Another picker dismissed") { self.app.window.attachedSheet == nil }
            guard app.errorShown, !app.history.showsWelcome else {
                throw VerifyError("Cancelling Open Another did not preserve the failure card")
            }
            try await openWelcomeFixture(fromPicker: true)
        case .capture:
            guard let name = step.name else { throw VerifyError("Missing Welcome checkpoint name") }
            try await captureWelcomeReturn(name)
        default:
            throw VerifyError("Unsupported Welcome return step")
        }
    }

    private func openWelcomeFixture(fromPicker: Bool = false) async throws {
        let before = swaps
        if fromPicker {
            // NSOpenPanel's remote service cannot select a row from this process.
            OpenPanelConfiguration.submit(.OK, urls: [options.fixture]) { _ = app.open($0) }
        } else {
            app.application(NSApp, open: [options.fixture])
        }
        try await wait("the single fixture plays and swaps a frame") {
            self.snapshot.status == .playing && self.snapshot.source?.location == self.options.fixture.path
                && self.snapshot.video != nil && self.swaps > before
        }
        guard let video = app.video,
              let frame = video.videoLayer.verificationFrame(width: 640, height: 288),
              let pixel = NSBitmapImageRep(cgImage: frame.image).colorAt(x: 320, y: 144),
              isFixtureColor(pixel) else {
            throw VerifyError("The live GL renderer did not produce the magenta fixture", blocked: true)
        }
        welcomeReturnFrame = frame.image
    }

    private func captureWelcomeReturn(_ name: String) async throws {
        try await Task.sleep(nanoseconds: 330_000_000)
        guard let root = app.window.contentView, let video = app.video else {
            throw VerifyError("The Welcome window or video plane disappeared")
        }
        root.layoutSubtreeIfNeeded()
        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        let compositor = await captureCompositor(width: width, height: height)
        let videoVisible = !video.isHiddenOrHasHiddenAncestor && !video.videoLayer.isHidden
        // AppKit cannot cache a CAOpenGLLayer. When the WindowServer cannot
        // supply a capture, composite the last live GL frame if that plane is
        // still visible. A fresh render on idle would miss the retained frame.
        let retainedFrame = videoVisible ? welcomeReturnFrame : nil
        guard let image = compositor ?? windowComposite(root: root, video: video, frame: retainedFrame) else {
            throw VerifyError("Cannot capture the Welcome window", blocked: true)
        }
        let bitmap = NSBitmapImageRep(cgImage: image)
        let quietPoints = [0.25, 0.45, 0.65, 0.82, 0.965].flatMap { y in
            [24, width - 25].map { x in (x, Int(Double(height) * y)) }
        }
        var canvasMatches = 0
        var fixtureSamples = 0
        guard let canvas = welcomeCanvas(root, width: width, height: height) else {
            throw VerifyError("Cannot sample the themed canvas", blocked: true)
        }
        let reference = NSBitmapImageRep(cgImage: canvas)
        for (x, y) in quietPoints {
            guard let actual = bitmap.colorAt(x: x, y: y), let expected = reference.colorAt(x: x, y: y) else { continue }
            if isFixtureColor(actual) { fixtureSamples += 1 }
            if colorDistance(actual, expected) < 0.12 { canvasMatches += 1 }
        }
        let host = app.history.welcomeView
        let hostRect = host.convert(host.bounds, to: root)
        let heading = headingPixels(bitmap, host: hostRect, appearance: host.effectiveAppearance)
        if !app.history.showsWelcome || snapshot.source != nil || app.history.welcome.entries.isEmpty {
            failures.append("welcome_return:state:\(name)")
        }
        if fixtureSamples != 0 || canvasMatches != quietPoints.count {
            failures.append("welcome_return:canvas:\(name)")
        }
        let chromeBottom = app.windowBehaviour?.trafficLights.map {
            CGFloat(height) - $0.convert($0.bounds, to: root).minY * 2
        }.max() ?? CGFloat(height)
        if heading.map({ $0.minY > chromeBottom + 2 && hostRect.maxY <= root.bounds.maxY }) != true {
            failures.append("welcome_return:heading:\(name)")
        }
        let file = name + ".png"
        guard let png = bitmap.representation(using: .png, properties: [:]) else {
            throw VerifyError("Cannot encode Welcome PNG", blocked: true)
        }
        try png.write(to: options.output.appendingPathComponent(file))
        var facts: [String: Any] = [
            "name": name, "png": file, "render_scale": 2, "playback": snapshot.status.rawValue,
            "welcome": app.history.showsWelcome, "video_layer_visible": videoVisible,
            "color_fixture_samples": fixtureSamples, "canvas_samples": canvasMatches,
            "canvas_samples_expected": quietPoints.count, "welcome_frame_points": dimensions(hostRect),
            "capture": compositor == nil ? "app_composite_2x" : "window_compositor_2x",
        ]
        if let heading { facts["heading_pixels_top_origin"] = dimensions(heading) }
        observations.append(facts)
    }

    private func welcomeCanvas(_ root: NSView, width: Int, height: Int) -> CGImage? {
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                                      bytesPerRow: width * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.scaleBy(x: 2, y: 2)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
        PlayerCanvas.drawCanvas(in: root.bounds, appearance: root.effectiveAppearance)
        NSGraphicsContext.restoreGraphicsState()
        return context.makeImage()
    }

    private func headingPixels(_ bitmap: NSBitmapImageRep, host: CGRect, appearance: NSAppearance) -> CGRect? {
        let x = Int(max(24, (host.width - 680) / 2) * 2)
        let y = bitmap.pixelsHigh - Int(host.maxY * 2)
        // On the controlled wide viewport the 30pt heading occupies this box,
        // above the smaller subtitle. Require complete ink, away from its edges.
        let region = CGRect(x: x - 4, y: y + 52, width: 590, height: 74)
        guard region.minX >= 0, region.maxX <= CGFloat(bitmap.pixelsWide),
              region.minY >= 0, region.maxY <= CGFloat(bitmap.pixelsHigh) else { return nil }
        let ink = appearance.performAsCurrentDrawingAppearance { PaletteRole.text.color.usingColorSpace(.sRGB) }
        guard let ink else { return nil }
        var painted = CGRect.null
        for row in stride(from: Int(region.minY), to: Int(region.maxY), by: 2) {
            for column in stride(from: Int(region.minX), to: Int(region.maxX), by: 2) {
                if let color = bitmap.colorAt(x: column, y: row), colorDistance(color, ink) < 0.14 {
                    painted = painted.union(CGRect(x: column, y: row, width: 1, height: 1))
                }
            }
        }
        guard painted.width >= 440, painted.height >= 38, region.insetBy(dx: 2, dy: 2).contains(painted) else {
            return nil
        }
        return painted
    }

    private func isFixtureColor(_ color: NSColor) -> Bool {
        guard let rgb = color.usingColorSpace(.sRGB) else { return false }
        return min(rgb.redComponent, rgb.blueComponent) > 0.6 && rgb.greenComponent < 0.35
    }

    private func colorDistance(_ lhs: NSColor, _ rhs: NSColor) -> CGFloat {
        guard let a = lhs.usingColorSpace(.sRGB), let b = rhs.usingColorSpace(.sRGB) else { return 1 }
        return max(abs(a.redComponent - b.redComponent), abs(a.greenComponent - b.greenComponent),
                   abs(a.blueComponent - b.blueComponent))
    }
}
