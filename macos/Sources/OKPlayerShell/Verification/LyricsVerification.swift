import AppKit

@MainActor
extension VerifyRun {
    func performLyricsStep(_ step: VerificationScenario.Step) async throws {
        guard let window = app.window, let client = app.client, let video = app.video else {
            throw VerifyError("Local Lyrics verification needs the player window and session", blocked: true)
        }
        let music = app.music
        let controls = music.view
        let lyrics = app.lyrics
        let panel = lyrics.panel
        let extras = options.fixture.deletingLastPathComponent().appendingPathComponent("extras")
        let plain = extras.appendingPathComponent("lyrics-plain.mp3")
        let malformed = extras.appendingPathComponent("lyrics-malformed.mp3")
        let missing = extras.appendingPathComponent("lyrics-missing.mp3")

        switch step.action {
        case .open:
            initialFrame = window.frame
            app.application(NSApp, open: [options.fixture])
            try await wait("synced audio track is playing in Compact Music") {
                self.snapshot.status == .playing && music.isActive &&
                    music.audioMetadata != nil && self.snapshot.timePos != nil
            }
            controls.lyrics.performClick(nil)
            try await wait("local synced LRC sidecar loaded into Lyrics view") {
                lyrics.isShown && controls.lyricsShown && panel.displayedState == .synced &&
                    panel.syncedRowButtons.count == 18 && panel.displayedSource?.location == self.options.fixture.path
            }
            if panel.syncedRowButtons.contains(where: { $0.line.text.contains("ignored malformed") }) ||
                !panel.syncedRowButtons[8].line.isGap || panel.syncedRowButtons[8].line.text != "♪" {
                recordLyricsFailure("lyrics_sidecar:lrc_parsing")
            }
            // Wait for real playback to cross past line 1 (1.0s) so live timing is observed.
            try await wait("playback advanced past the second synced line") {
                self.snapshot.status == .playing && (self.snapshot.timePos ?? 0) >= 1.25
            }
            if lyrics.syncsActiveLine {
                try await wait("live playback highlighted an active lyric line") {
                    guard let active = panel.displayedActiveIndex else { return false }
                    return active >= 1 && panel.syncedRowButtons[active].isActiveLine
                }
            } else if panel.displayedActiveIndex != nil {
                recordLyricsFailure("lyrics_sidecar:active_highlight")
            }

        case .lyricsAdvance:
            guard let target = step.seconds else { throw VerifyError("Missing lyrics_advance timestamp") }
            if snapshot.status == .playing {
                controls.play.performClick(nil)
                try await wait("paused before seeking to later synced lyric line") { self.snapshot.status == .paused }
            }
            client.dispatch(.seek(seconds: target, mode: .absolute, precision: .exact))
            try await wait("playhead reached later lyric line") {
                abs((self.snapshot.timePos ?? -1) - target) < 0.35
            }
            window.contentView?.layoutSubtreeIfNeeded()
            if lyrics.syncsActiveLine {
                try await wait("active line highlighted and scrolled down into viewport") {
                    panel.displayedActiveIndex == 13 &&
                        panel.syncedRowButtons[13].isActiveLine &&
                        panel.scrollOffsetY > 0 &&
                        panel.isRowVisibleInViewport(13)
                }
            }
            if panel.displayedActiveIndex != 13 || !panel.syncedRowButtons[13].isActiveLine ||
                panel.scrollOffsetY <= 0 || !panel.isRowVisibleInViewport(13) {
                recordLyricsFailure("lyrics_sidecar:active_highlight")
            }
            lyricsDeepScrollOffset = panel.scrollOffsetY

        case .lyricsSeekBack:
            guard let target = step.seconds, let duration = snapshot.duration else {
                throw VerifyError("Missing lyrics_seek_back timestamp")
            }
            window.contentView?.layoutSubtreeIfNeeded()
            let seek = controls.seek
            let x = ScrubberView.inset + (seek.bounds.width - 2 * ScrubberView.inset) * target / duration
            let point = seek.convert(CGPoint(x: x, y: seek.bounds.midY), to: nil)
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                try sendLyricsMouse(type, at: point, in: window)
            }
            try await wait("backward seek landed near earlier lyric line") {
                abs((self.snapshot.timePos ?? -1) - target) < 0.35
            }
            if lyrics.syncsActiveLine {
                try await wait("backward seek moved highlight and scroll back up") {
                    panel.displayedActiveIndex == 2 &&
                        panel.syncedRowButtons[2].isActiveLine &&
                        panel.scrollOffsetY < self.lyricsDeepScrollOffset &&
                        panel.isRowVisibleInViewport(2)
                }
            }
            if panel.displayedActiveIndex != 2 || !panel.syncedRowButtons[2].isActiveLine ||
                panel.scrollOffsetY >= lyricsDeepScrollOffset || !panel.isRowVisibleInViewport(2) {
                recordLyricsFailure("lyrics_sidecar:active_highlight")
            }

        case .lyricsClickRow:
            guard panel.syncedRowButtons.indices.contains(10) else {
                throw VerifyError("Expected synced row 10 to click")
            }
            let expectedTime = panel.syncedRowButtons[10].line.timeSeconds
            panel.syncedRowButtons[10].performClick(nil)
            try await wait("clicking timed lyric row sought playback") {
                abs((self.snapshot.timePos ?? -1) - expectedTime) < 0.35
            }
            if lyrics.syncsActiveLine {
                try await wait("clicked lyric row became active and visible") {
                    panel.displayedActiveIndex == 10 &&
                        panel.syncedRowButtons[10].isActiveLine &&
                        panel.isRowVisibleInViewport(10)
                }
            }
            if panel.displayedActiveIndex != 10 || !panel.syncedRowButtons[10].isActiveLine ||
                !panel.isRowVisibleInViewport(10) {
                recordLyricsFailure("lyrics_sidecar:active_highlight")
            }

        case .lyricsSwitchPlain:
            app.application(NSApp, open: [plain])
            try await wait("plain LRC track loaded without stale synced lyrics") {
                self.snapshot.source?.location == plain.path &&
                    panel.displayedSource?.location == plain.path &&
                    panel.displayedState == .plain &&
                    (self.snapshot.status == .playing || self.snapshot.status == .paused)
            }
            if snapshot.status == .playing {
                controls.play.performClick(nil)
                try await wait("plain LRC track paused") { self.snapshot.status == .paused }
            }
            let labels = panel.plainRowLabels.map(\.stringValue)
            if panel.headerText != "LYRICS · NOT SYNCED" || !panel.syncedRowButtons.isEmpty ||
                panel.displayedActiveIndex != nil || labels.count != 6 ||
                labels.first != "Wander down the boulevard" ||
                labels.contains(where: { $0.contains("City lights") || $0.contains("radio on low") }) {
                recordLyricsFailure("lyrics_sidecar:plain_and_no_stale")
            }

        case .lyricsSwitchMalformed:
            app.application(NSApp, open: [malformed])
            try await wait("malformed LRC track resolved to honest invalid fallback") {
                self.snapshot.source?.location == malformed.path &&
                    panel.displayedSource?.location == malformed.path &&
                    panel.displayedState == .invalid
            }
            if !panel.syncedRowButtons.isEmpty || !panel.plainRowLabels.isEmpty ||
                panel.fallbackText != "Local lyrics file could not be read" {
                recordLyricsFailure("lyrics_sidecar:malformed_fallback")
            }

        case .lyricsSwitchMissing:
            app.application(NSApp, open: [missing])
            try await wait("missing LRC track resolved to honest missing fallback") {
                self.snapshot.source?.location == missing.path &&
                    panel.displayedSource?.location == missing.path &&
                    panel.displayedState == .missing &&
                    (self.snapshot.status == .playing || self.snapshot.status == .paused)
            }
            if snapshot.status == .playing {
                controls.play.performClick(nil)
                try await wait("missing LRC track paused") { self.snapshot.status == .paused }
            }
            if !panel.syncedRowButtons.isEmpty || !panel.plainRowLabels.isEmpty ||
                panel.fallbackText != "No lyrics found for this track" {
                recordLyricsFailure("lyrics_sidecar:missing_fallback")
            }

        case .capture:
            guard let name = step.name, let expected = step.expect else {
                throw VerifyError("Missing Lyrics checkpoint")
            }
            try await captureLyrics(name, expected: expected, window: window, video: video)

        case .closeMedia:
            try await performWindowStep(step)
            if lyrics.isShown || !panel.isHidden || panel.displayedState != .unavailable ||
                !panel.syncedRowButtons.isEmpty || !panel.plainRowLabels.isEmpty {
                recordLyricsFailure("lyrics_sidecar:close_cleanup")
            }

        default:
            throw VerifyError("Unsupported Lyrics step \(step.action.rawValue)")
        }
    }

    private func recordLyricsFailure(_ code: String) {
        if !failures.contains(code) { failures.append(code) }
    }

    private func sendLyricsMouse(_ type: NSEvent.EventType, at point: NSPoint, in window: NSWindow) throws {
        guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [],
                                             timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                             context: nil, eventNumber: 0, clickCount: 1,
                                             pressure: type == .leftMouseDown ? 1 : 0) else {
            throw VerifyError("Cannot create Lyrics mouse input")
        }
        window.sendEvent(event)
    }

    private func captureLyrics(_ name: String, expected: VerificationScenario.Expectation,
                               window: NSWindow, video: VideoView) async throws {
        try await Task.sleep(nanoseconds: 330_000_000)
        guard let root = window.contentView else { throw VerifyError("Lyrics capture lost its window", blocked: true) }
        root.layoutSubtreeIfNeeded()
        let panel = app.lyrics.panel
        let expectedState: LyricsState = switch expected {
        case .lyricsSynced: .synced
        case .lyricsPlain: .plain
        case .lyricsMissing: .missing
        default: .unavailable
        }
        if !app.music.isActive || !app.lyrics.isShown || panel.isHidden || panel.displayedState != expectedState {
            recordLyricsFailure("lyrics_sidecar:checkpoint_\(name)")
        }

        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        let compositor = await captureCompositor(width: width, height: height)
        guard let image = compositor ?? windowComposite(root: root, video: video, frame: nil),
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not encode Lyrics evidence", blocked: true)
        }
        let file = options.output.appendingPathComponent(name + ".png")
        try png.write(to: file)
        var observation: [String: Any] = [
            "name": name, "png": file.lastPathComponent, "frame_points": dimensions(window.frame),
            "capture": compositor == nil ? "app_composite_2x" : "window_compositor_2x",
            "lyrics_state": panel.displayedState.rawValue,
            "lyrics_header": panel.headerText,
            "scroll_offset_y": Double(panel.scrollOffsetY),
            "synced_rows": panel.syncedRowButtons.count,
            "plain_rows": panel.plainRowLabels.count,
        ]
        if let active = panel.displayedActiveIndex {
            observation["active_index"] = active
        }
        observations.append(observation)
        print("OKP_VERIFY captured \(name)")
    }
}
