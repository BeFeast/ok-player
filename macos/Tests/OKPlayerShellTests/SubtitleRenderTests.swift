import COKPCore
import Foundation
import OpenGL.GL3
import XCTest
@testable import OKPlayerShell

final class SubtitleRenderTests: XCTestCase {
    func testSameStemSRTIsRenderedAutomatically() throws {
        try checkSubtitles(file: "subtitle-sidecar.mp4", external: true)
    }

    func testSelectedEmbeddedSRTIsRendered() throws {
        try checkSubtitles(file: "subtitle-embedded.mkv", external: false)
    }

    private func checkSubtitles(file: String, external: Bool) throws {
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
                                           session: try XCTUnwrap(client.handle))
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

        // The fixture's flat gray video stays the same before and during the cue.
        // Require a real video frame with no glyphs first, then white pixels that
        // can only come from the cue. No commands here display an mpv OSD.
        let blank = waitFor(client) { _ in renderer.frame(hasGlyphs: false) }
        XCTAssertNotNil(blank, "No blank video frame before the cue: \(log)")
        let cue = try XCTUnwrap(waitFor(client) { snapshot in
            guard let time = snapshot.timePos, time >= 2, time < 9 else { return nil as Int? }
            return renderer.frame(hasGlyphs: true)
        }, "The selected subtitle drew no glyphs into the OpenGL framebuffer: \(log)")
        print("Subtitle render: \(file) had \(cue) bright pixels during the cue")
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

/// Uses the same CGL format and session render ABI as VideoLayer, with an offscreen
/// framebuffer so window chrome and desktop contents cannot affect the pixels.
private final class SubtitleFrameReader {
    private let width: GLsizei = 640
    private let height: GLsizei = 360
    private let surface: GLSurface
    private let queue = DispatchQueue(label: "subtitle-render-test")
    private var session: OpaquePointer?
    private var framebuffer: GLuint = 0
    private var color: GLuint = 0
    private var latestBrightPixels: Int?

    init(surface: GLSurface, session: OpaquePointer) {
        self.surface = surface
        self.session = session
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

    func frame(hasGlyphs: Bool) -> Int? {
        queue.sync {
            guard let count = latestBrightPixels else { return nil }
            return (hasGlyphs ? count > 100 : count == 0) ? count : nil
        }
    }

    fileprivate func update() {
        queue.async { [self] in
            guard let session else { return }
            surface.withCurrentContext {
                guard okp_session_render_update(session) else { return }
                glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
                guard okp_session_render(session, width, height) else { return }
                glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
                var pixels = [UInt8](repeating: 0, count: Int(width * height * 4))
                pixels.withUnsafeMutableBytes { buffer in
                    glReadPixels(0, 0, width, height, GLenum(GL_RGBA), GLenum(GL_UNSIGNED_BYTE), buffer.baseAddress)
                }
                _ = okp_session_render_report_swap(session)
                // Ignore pre-playback clears. The gray video is visible at the top
                // left throughout the fixture (far from the subtitle).
                guard (12...100).contains(pixels[0]) else { return }
                latestBrightPixels = stride(from: 0, to: pixels.count, by: 4)
                    .count { pixels[$0] > 180 && pixels[$0 + 1] > 180 && pixels[$0 + 2] > 180 }
            }
        }
    }
}

private func subtitleFrameReady(_ context: UnsafeMutableRawPointer?) {
    guard let context else { return }
    Unmanaged<SubtitleFrameReader>.fromOpaque(context).takeUnretainedValue().update()
}
