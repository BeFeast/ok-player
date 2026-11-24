import AppKit
import COKPCore
import Foundation
import OpenGL.GL3
import XCTest
@testable import OKPlayerShell

final class SubtitleRenderTests: XCTestCase {
    func testSameStemSRTIsRenderedAutomatically() throws {
        try checkSubtitles(file: "subtitle-sidecar.mp4", external: true, backingScale: 2)
    }

    func testSelectedEmbeddedSRTIsRendered() throws {
        try checkSubtitles(file: "subtitle-embedded.mkv", external: false, backingScale: 1)
    }

    func testTaggedSidecarsAppearInSubtitleMenuAndPreferredLanguageIsSelected() throws {
        let env = ProcessInfo.processInfo.environment
        guard let fixtures = env["OKP_MACOS_SUBTITLE_FIXTURES"] else {
            throw XCTSkip("The hosted macOS workflow generates the video fixture")
        }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let stem = "A Serious Man (2009) {tmdb-12573}"
        let media = root.appendingPathComponent("\(stem).mp4")
        let fixture = URL(fileURLWithPath: fixtures).appendingPathComponent("subtitle-sidecar.mp4")
        try FileManager.default.copyItem(at: fixture, to: media)
        for suffix in ["en.hi.srt", "he.srt", "ru.hi.srt"] {
            let file = root.appendingPathComponent("\(stem).\(suffix)")
            try "1\n00:00:02,000 --> 00:00:04,000\nA line\n\n".write(to: file, atomically: true, encoding: .utf8)
        }
        var config = SessionConfig.forApp()
        config.dataDir = root.appendingPathComponent("data").path
        config.configDir = root.appendingPathComponent("config").path
        config.cacheDir = root.appendingPathComponent("cache").path
        config.mpvOptions += [
            MpvOption(name: "vo", value: "null"),
            MpvOption(name: "ao", value: "null"),
            MpvOption(name: "pause", value: "yes"),
            MpvOption(name: "slang", value: "heb,en"),
        ]
        let client = try SessionClient(config: config)
        defer { client.close() }
        let result = client.dispatch(.open(source: MediaSource(kind: .local, location: media.path)))
        guard case .accepted = result else { return XCTFail("Open failed: \(result)") }
        let snapshot = try XCTUnwrap(waitFor(client) { snapshot in
            snapshot.status == .paused ? snapshot : nil
        })
        let subtitles = snapshot.tracks.filter { $0.kind == .subtitle }
        XCTAssertEqual(subtitles.map(\.lang), ["en", "heb", "ru"])
        XCTAssertTrue(subtitles.allSatisfy(\.external))
        XCTAssertEqual(subtitles.filter(\.selected).map(\.lang), ["heb"])

        _ = NSApplication.shared
        let controls = TrackControls()
        controls.query = {
            guard case .tracks(let tracks)? = client.query(.tracks) else { return nil }
            return tracks
        }
        let items = controls.menuItems(.subtitle).prefix { !$0.isSeparatorItem }
        XCTAssertEqual(items.map(\.title), [
            "Off", "English (SDH) · SRT · External", "Hebrew · SRT · External", "Russian (SDH) · SRT · External",
        ])
        XCTAssertEqual(items.filter { $0.state == .on }.map(\.title), ["Hebrew · SRT · External"])
    }

    private func checkSubtitles(file: String, external: Bool, backingScale: GLsizei) throws {
        let env = ProcessInfo.processInfo.environment
        guard let fixtures = env["OKP_MACOS_SUBTITLE_FIXTURES"] else {
            throw XCTSkip("The hosted macOS workflow generates these video fixtures")
        }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let log = try XCTUnwrap(env["OKP_MACOS_OUTPUT"]) + "/\(file).mpv.log"
        var config = SessionConfig.forApp()
        config.dataDir = root.appendingPathComponent("data").path
        config.configDir = root.appendingPathComponent("config").path
        config.cacheDir = root.appendingPathComponent("cache").path
        config.mpvOptions += [
            MpvOption(name: "log-file", value: log),
            MpvOption(name: "msg-level", value: "all=v"),
        ]
        let client = try SessionClient(config: config)
        defer { client.close() }
        let renderer = SubtitleFrameReader(surface: try XCTUnwrap(GLSurface.make()),
                                           session: try XCTUnwrap(client.handle), backingScale: backingScale)
        defer { renderer.close() }
        try renderer.start()

        let path = URL(fileURLWithPath: fixtures).appendingPathComponent(file).path
        let result = client.dispatch(.open(source: MediaSource(kind: .local, location: path)))
        guard case .accepted = result else { return XCTFail("Open failed: \(result)") }
        let loaded = try XCTUnwrap(waitFor(client) { snapshot in
            snapshot.status == .playing && snapshot.tracks.contains { $0.kind == .subtitle }
                ? snapshot : nil
        }, "Subtitle track did not load: \(log)")
        let track = try XCTUnwrap(loaded.tracks.first {
            $0.kind == .subtitle && $0.external == external && (external || $0.lang == "rus")
        })
        if external {
            XCTAssertTrue(track.selected, "The same-stem SRT should be selected on open")
        } else {
            XCTAssertFalse(track.selected, "The second embedded track must be selected by the test")
            let result = client.dispatch(.selectTrack(kind: .subtitle, id: track.id))
            guard case .accepted = result else { return XCTFail("Track selection failed: \(result)") }
        }
        XCTAssertNotNil(waitFor(client) { snapshot in
            snapshot.tracks.first { $0.kind == .subtitle && $0.id == track.id && $0.selected }
        }, "libmpv did not report the subtitle as selected: \(log)")

        _ = NSApplication.shared
        let controls = TrackControls()
        controls.query = {
            guard case .tracks(let tracks)? = client.query(.tracks) else { return nil }
            return tracks
        }
        let items = controls.menuItems(.subtitle).prefix { !$0.isSeparatorItem }
        XCTAssertEqual(items.count, external ? 2 : 3)
        XCTAssertEqual(items.first?.title, "Off")
        XCTAssertTrue(items.allSatisfy { !$0.title.isEmpty })
        let checked = items.filter { $0.state == .on }
        XCTAssertEqual(checked.count, 1)
        XCTAssertEqual((checked.first?.representedObject as? TrackActionBox)?.action,
                       .select(kind: .subtitle, id: track.id))
        if external { XCTAssertTrue(checked.first?.title.hasSuffix("External") == true) }

        let timer = ManualHideTimer()
        let osc = TransportOverlay(hideTimer: timer)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 360),
                              styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.close() }
        window.contentView = osc
        osc.subtitleSession = client
        osc.query = { at in
            guard case .transport(let transport)? = client.query(.transport(at: at)) else { return nil }
            return transport
        }
        osc.sessionChanged(client.store.snapshot)
        osc.layoutSubtreeIfNeeded()
        XCTAssertFalse(osc.panel.isHidden, "The transport controls should be visible")
        XCTAssertEqual(osc.panel.alphaValue, 1, "The transport controls should be visible")
        let panelTop = osc.panel.frame.maxY - osc.bounds.minY
        XCTAssertGreaterThan(panelTop, 20, "The panel must have its real laid-out geometry")
        let obscuredRows = Int(ceil(panelTop * CGFloat(backingScale)))
        timer.elapse()
        XCTAssertEqual(osc.panel.alphaValue, 0, "The transport controls should have auto-hidden")

        // The fixture's flat gray video stays the same before and during the cue.
        // Require a real video frame with no glyphs first, then white pixels that
        // can only come from the cue. No commands here display an mpv OSD.
        XCTAssertNotNil(waitFor(client) { _ in
            renderer.frame.flatMap { $0.brightPixels == 0 ? $0 : nil }
        }, "No blank video frame before the cue: \(log)")
        let natural = try XCTUnwrap(waitForCue(client, renderer) { $0.lowestBrightRow < obscuredRows },
                                    "The selected subtitle did not draw in the default position: \(log)")

        osc.noteActivity()
        XCTAssertEqual(osc.panel.alphaValue, 1, "Activity should reveal the controls")
        let lifted = try XCTUnwrap(waitForCue(client, renderer) { $0.lowestBrightRow >= obscuredRows },
                                   "The selected subtitle is covered by the visible transport controls: \(log)")
        timer.elapse()
        XCTAssertEqual(osc.panel.alphaValue, 0, "The transport controls should have auto-hidden again")
        let restored = try XCTUnwrap(waitForCue(client, renderer) { $0.lowestBrightRow == natural.lowestBrightRow },
                                     "The selected subtitle did not return to its default position: \(log)")
        print("Subtitle render: \(file) at \(backingScale)x, OSC top=\(obscuredRows) px, "
              + "glyph pixels=\(lifted.brightPixels), default/lifted/restored bottom="
              + "\(natural.lowestBrightRow)/\(lifted.lowestBrightRow)/\(restored.lowestBrightRow) px")
    }

    private func waitForCue(_ client: SessionClient, _ renderer: SubtitleFrameReader,
                            where matches: (SubtitleFrame) -> Bool) -> SubtitleFrame? {
        waitFor(client) { snapshot in
            guard let time = snapshot.timePos, time >= 2, time < 9,
                  let frame = renderer.frame, frame.brightPixels > 100, matches(frame) else { return nil }
            return frame
        }
    }

    private func waitFor<Value>(_ client: SessionClient,
                                _ read: (SessionSnapshot) -> Value?) -> Value? {
        let deadline = ProcessInfo.processInfo.systemUptime + 6
        repeat {
            client.drainEvents()
            if let value = read(client.store.snapshot) { return value }
            Thread.sleep(forTimeInterval: 0.01)
        } while ProcessInfo.processInfo.systemUptime < deadline
        return nil
    }
}

private struct SubtitleFrame {
    let brightPixels: Int
    let lowestBrightRow: Int
}

/// Uses the same CGL format and session render ABI as VideoLayer, with an offscreen
/// framebuffer so window chrome and desktop contents cannot affect the pixels.
private final class SubtitleFrameReader {
    private let width: GLsizei
    private let height: GLsizei
    private let surface: GLSurface
    private let queue = DispatchQueue(label: "subtitle-render-test")
    private var session: OpaquePointer?
    private var framebuffer: GLuint = 0
    private var color: GLuint = 0
    private var pixels: [UInt8]
    private var latestFrame: SubtitleFrame?

    init(surface: GLSurface, session: OpaquePointer, backingScale: GLsizei) {
        self.surface = surface
        self.session = session
        width = 640 * backingScale
        height = 360 * backingScale
        pixels = [UInt8](repeating: 0, count: Int(width * height * 4))
    }

    func start() throws {
        let failure: String? = queue.sync {
            surface.withCurrentContext {
                glGenFramebuffers(1, &framebuffer)
                glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
                glGenRenderbuffers(1, &color)
                glBindRenderbuffer(GLenum(GL_RENDERBUFFER), color)
                glRenderbufferStorage(GLenum(GL_RENDERBUFFER),
                                      GLenum(surface.floatColor ? GL_RGBA16F : GL_RGBA8), width, height)
                glFramebufferRenderbuffer(GLenum(GL_FRAMEBUFFER), GLenum(GL_COLOR_ATTACHMENT0),
                                          GLenum(GL_RENDERBUFFER), color)
                guard glCheckFramebufferStatus(GLenum(GL_FRAMEBUFFER)) == GLenum(GL_FRAMEBUFFER_COMPLETE) else {
                    return "Could not allocate subtitle test framebuffer"
                }

                var error = [CChar](repeating: 0, count: 1024)
                let context = Unmanaged.passUnretained(self).toOpaque()
                guard okp_session_render_create(session, subtitleFrameReady, context, &error, numericCast(error.count)) else {
                    return String(cString: error)
                }
                return nil
            }
        }
        if let failure { throw SessionClientError(description: failure) }
    }

    func close() {
        queue.sync {
            surface.withCurrentContext {
                okp_session_render_destroy(session)
                session = nil
                glDeleteRenderbuffers(1, &color)
                glDeleteFramebuffers(1, &framebuffer)
            }
        }
    }

    var frame: SubtitleFrame? {
        queue.sync { latestFrame }
    }

    fileprivate func update() {
        queue.async { [self] in
            guard let session else { return }
            surface.withCurrentContext {
                guard okp_session_render_update(session) else { return }
                glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
                guard okp_session_render(session, width, height) else { return }
                glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
                pixels.withUnsafeMutableBytes { buffer in
                    glReadPixels(0, 0, width, height, GLenum(GL_RGBA), GLenum(GL_UNSIGNED_BYTE), buffer.baseAddress)
                }
                _ = okp_session_render_report_swap(session)
                // Ignore pre-playback clears. The gray video is visible at the top
                // left throughout the fixture (far from the subtitle).
                guard (12...100).contains(pixels[0]) else { return }
                var bright = 0
                var lowestBrightRow = -1
                // glReadPixels starts at the displayed bottom, with the same flip as VideoLayer.
                for i in stride(from: 0, to: pixels.count, by: 4) {
                    if pixels[i] > 180 && pixels[i + 1] > 180 && pixels[i + 2] > 180 {
                        if bright == 0 { lowestBrightRow = i / (Int(width) * 4) }
                        bright += 1
                    }
                }
                latestFrame = SubtitleFrame(brightPixels: bright, lowestBrightRow: lowestBrightRow)
            }
        }
    }
}

private func subtitleFrameReady(_ context: UnsafeMutableRawPointer?) {
    guard let context else { return }
    Unmanaged<SubtitleFrameReader>.fromOpaque(context).takeUnretainedValue().update()
}
