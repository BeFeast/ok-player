import AppKit

@MainActor
extension VerifyRun {
    func performMusicStep(_ step: VerificationScenario.Step) async throws {
        guard let window = app.window, let client = app.client, let video = app.video else {
            throw VerifyError("Compact Music verification needs the player window and session", blocked: true)
        }
        let music = app.music
        let controls = music.view
        let extras = options.fixture.deletingLastPathComponent().appendingPathComponent("extras")
        let plain = extras.appendingPathComponent("music-no-art.mp3")
        switch step.action {
        case .open:
            initialFrame = window.frame
            app.application(NSApp, open: [options.fixture])
            try await wait("the covered audio file is playing") {
                self.snapshot.status == .playing && self.snapshot.audioMetadata != nil && self.snapshot.timePos != nil
            }
            if !music.isActive {
                failures.append("compact_music:auto_entry")
                try toggleMusicFromMenu()
            }
            try await wait("artwork for the active track arrived") { controls.artwork.hasImage }
            if snapshot.video != nil || window.frame == initialFrame || !app.osc.isHidden ||
                controls.title.stringValue != "Blue Hour" || controls.detail.stringValue != "OK Player · Compact Sessions" {
                failures.append("compact_music:artwork_and_metadata")
            }
        case .musicPause:
            if !controls.controlsShown { try revealMusicControls(in: window) }
            controls.play.performClick(nil)
            try await wait("the compact Play/Pause button paused playback") {
                self.snapshot.status == .paused && controls.controlsShown
            }
        case .musicIdle:
            controls.play.performClick(nil)
            try await wait("the compact transport hid during playback") {
                self.snapshot.status == .playing && !controls.controlsShown
            }
            if controls.artwork.isHidden || controls.title.isHidden || controls.title.stringValue.isEmpty {
                failures.append("compact_music:idle_artwork")
            }
        case .musicSeek:
            guard let time = step.seconds, let duration = snapshot.duration else {
                throw VerifyError("Missing Music seek target")
            }
            window.contentView?.layoutSubtreeIfNeeded()
            let seek = controls.seek
            let x = ScrubberView.inset + (seek.bounds.width - 2 * ScrubberView.inset) * time / duration
            let point = seek.convert(CGPoint(x: x, y: seek.bounds.midY), to: nil)
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                try sendMusicMouse(type, at: point, in: window)
            }
            try await wait("the compact timeline moved the playhead") {
                abs((self.snapshot.timePos ?? -1) - time) < 0.15
            }
        case .musicQueue:
            controls.queue.performClick(nil)
            app.upNext.panel.upNextButton.performClick(nil)
            if !app.upNext.isShown || !controls.queueShown || app.upNext.panel.superview !== controls.queueHost {
                failures.append("compact_music:queue_panel")
            }
            app.upNext.chooseFiles = { _, selection in selection([plain]) }
            defer { app.upNext.chooseFiles = nil }
            app.commands.run("play-next")
            try await wait("Play Next inserted the second audio fixture") {
                client.store.playlist.items.count == 2 && self.app.upNext.panel.list.rows.count == 2
            }
            if !controls.next.isEnabled { failures.append("compact_music:transport_next") }
        case .musicNext:
            controls.next.performClick(nil)
            try await wait("the compact Next button played the queued audio fixture") {
                self.snapshot.source?.location == plain.path && self.snapshot.audioMetadata != nil &&
                    self.snapshot.status == .playing
            }
            if !music.isActive {
                // Continue after recording the deliberately disabled auto-entry.
                try toggleMusicFromMenu()
                if options.control.isEmpty { failures.append("compact_music:queue_kept_music") }
            }
            if controls.artwork.hasImage || controls.title.stringValue == "Blue Hour" {
                failures.append("compact_music:stale_artwork")
            }
            app.upNext.hide()
            controls.play.performClick(nil)
            try await wait("queued audio paused") { self.snapshot.status == .paused }
        case .musicRoundTrip, .musicVideoRoundTrip:
            let hasVideo = step.action == .musicVideoRoundTrip
            if hasVideo {
                try pressKey(" ", code: 49)
                try await wait("video paused before manual Music entry") { self.snapshot.status == .paused }
            }
            let original = (window.windowNumber, snapshot.source, snapshot.timePos ?? 0)
            if !music.isActive { try toggleMusicFromMenu() }
            guard music.isActive else { throw VerifyError("The Window command did not open Music") }
            if hasVideo {
                if controls.switchToVideo.isHidden { failures.append("compact_music:switch_affordance") }
                controls.switchToVideo.performClick(nil)
            } else {
                try toggleMusicFromMenu()
            }
            if music.isActive || video.isHidden || app.osc.isHidden {
                failures.append("compact_music:standard_presentation")
            }
            try toggleMusicFromMenu()
            if !music.isActive || snapshot.source != original.1 || app.client !== client ||
                window.windowNumber != original.0 || snapshot.status != .paused ||
                abs((snapshot.timePos ?? 0) - original.2) > 0.15 {
                failures.append("compact_music:round_trip")
            }
        case .openMusicVideo:
            let fallback = extras.appendingPathComponent("music-video.mp4")
            let before = swaps
            app.application(NSApp, open: [fallback])
            try await wait("new video restored the standard presenter and rendered") {
                self.snapshot.source?.location == fallback.path && self.snapshot.video != nil &&
                    !music.isActive && !video.isHidden && self.swaps > before
            }
        case .capture:
            guard let name = step.name, let expected = step.expect else {
                throw VerifyError("Missing Music checkpoint")
            }
            try await captureMusic(name, expected: expected, window: window, video: video)
        case .closeMedia:
            try await performWindowStep(step)
            if music.isActive || window.frame != initialFrame { failures.append("compact_music:restore_idle") }
        default:
            throw VerifyError("Unsupported Music step \(step.action.rawValue)")
        }
    }

    private func toggleMusicFromMenu() throws {
        guard let menu = app.menuBar?.menus[.window],
              let index = menu.items.firstIndex(where: { $0.playerCommandID == CompactMusic.commandID }) else {
            throw VerifyError("Window menu has no Compact Music command")
        }
        menu.update()
        guard menu.items[index].isEnabled else { throw VerifyError("The Compact Music command is disabled") }
        menu.performActionForItem(at: index)
    }

    private func revealMusicControls(in window: NSWindow) throws {
        let artwork = app.music.view.artwork
        let point = artwork.convert(CGPoint(x: artwork.bounds.midX, y: artwork.bounds.midY), to: nil)
        try sendMusicMouse(.mouseMoved, at: point, in: window)
    }

    private func sendMusicMouse(_ type: NSEvent.EventType, at point: NSPoint, in window: NSWindow) throws {
        guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                                            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                            context: nil, eventNumber: 0, clickCount: 1,
                                            pressure: type == .leftMouseDown ? 1 : 0) else {
            throw VerifyError("Cannot create compact Music mouse input")
        }
        window.sendEvent(event)
    }

    private func captureMusic(_ name: String, expected: VerificationScenario.Expectation,
                              window: NSWindow, video: VideoView) async throws {
        try await Task.sleep(nanoseconds: 330_000_000)
        guard let root = window.contentView else { throw VerifyError("Music capture lost its window", blocked: true) }
        root.layoutSubtreeIfNeeded()
        let music = app.music
        let view = music.view
        let isVideo = expected == .musicVideo
        if music.isActive == isVideo || view.isHidden != isVideo ||
            (!isVideo && view.controlsShown == (expected == .musicHidden)) ||
            (!isVideo && view.artwork.hasImage != (expected != .musicPlaceholder)) ||
            (!isVideo && view.queueShown != (expected == .musicQueue)) {
            failures.append("compact_music:checkpoint_\(name)")
        }

        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        var frame: CGImage?
        if isVideo {
            guard let evidence = video.videoLayer.verificationFrame(
                width: Int((video.bounds.width * 2).rounded()), height: Int((video.bounds.height * 2).rounded())),
                  evidence.hasPicture else {
                throw VerifyError("Could not capture live video fallback", blocked: true)
            }
            frame = evidence.image
        }
        let compositor = await captureCompositor(width: width, height: height)
        guard let image = compositor ?? windowComposite(root: root, video: video, frame: frame),
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode Music evidence", blocked: true)
        }
        let file = options.output.appendingPathComponent(name + ".png")
        try png.write(to: file)
        observations.append([
            "name": name, "png": file.lastPathComponent, "frame_points": dimensions(window.frame),
            "capture": compositor == nil ? "app_composite_2x" : "window_compositor_2x",
            "artwork": view.artwork.hasImage, "title": view.title.stringValue, "controls_revealed": view.controlsShown,
        ])
        print("OKP_VERIFY captured \(name)")
    }
}
