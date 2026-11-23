import AppKit
import COKPCore
import OpenGL.GL3

/// Evidence from the live app's libmpv render context, before window chrome is composited.
/// The fixture has a uniform gray video plane and white subtitle glyphs.
struct VideoEvidence {
    let image: CGImage
    let videoPresent: Bool
    let glyphBounds: CGRect?
    let glyphPixels: Int

    init?(width: Int, height: Int, rgba: [UInt8]) {
        // The render target is sized in pixels. glReadPixels reports rows bottom-up.
        var topDown = Data()
        topDown.reserveCapacity(rgba.count)
        var minX = width, minY = height, maxX = -1, maxY = -1, count = 0
        let stride = width * 4
        for y in (0..<height).reversed() {
            let start = y * stride
            topDown.append(contentsOf: rgba[start..<(start + stride)])
            for x in 0..<width {
                let i = start + x * 4
                if rgba[i] > 180 && rgba[i + 1] > 180 && rgba[i + 2] > 180 {
                    minX = min(minX, x); maxX = max(maxX, x)
                    minY = min(minY, y); maxY = max(maxY, y)
                    count += 1
                }
            }
        }
        guard let provider = CGDataProvider(data: topDown as CFData),
              let space = CGColorSpace(name: CGColorSpace.sRGB),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                                  bytesPerRow: stride, space: space,
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                  provider: provider, decode: nil, shouldInterpolate: false,
                                  intent: .defaultIntent) else { return nil }
        self.image = image
        videoPresent = [0, 1, 2].allSatisfy { (12...100).contains(rgba[stride * (height / 2) + (width / 8) * 4 + $0]) }
        glyphBounds = count > 0 ? CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1) : nil
        glyphPixels = count
    }
}

extension GLSurface {
    /// Render a fresh 2× frame with the same session and GL context the window is using.
    /// Called only by the opt-in verifier, on the production render queue between draws.
    func verificationFrame(session: OpaquePointer, width: GLsizei, height: GLsizei) -> VideoEvidence? {
        withCurrentContext {
            var framebuffer: GLuint = 0, color: GLuint = 0
            var previousDraw: GLint = 0, previousRead: GLint = 0, previousColor: GLint = 0
            var previousViewport = [GLint](repeating: 0, count: 4)
            glGetIntegerv(GLenum(GL_DRAW_FRAMEBUFFER_BINDING), &previousDraw)
            glGetIntegerv(GLenum(GL_READ_FRAMEBUFFER_BINDING), &previousRead)
            glGetIntegerv(GLenum(GL_RENDERBUFFER_BINDING), &previousColor)
            glGetIntegerv(GLenum(GL_VIEWPORT), &previousViewport)
            glGenFramebuffers(1, &framebuffer)
            glGenRenderbuffers(1, &color)
            defer {
                glBindFramebuffer(GLenum(GL_DRAW_FRAMEBUFFER), GLuint(bitPattern: previousDraw))
                glBindFramebuffer(GLenum(GL_READ_FRAMEBUFFER), GLuint(bitPattern: previousRead))
                glBindRenderbuffer(GLenum(GL_RENDERBUFFER), GLuint(bitPattern: previousColor))
                glViewport(previousViewport[0], previousViewport[1], previousViewport[2], previousViewport[3])
                glDeleteRenderbuffers(1, &color)
                glDeleteFramebuffers(1, &framebuffer)
            }
            glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
            glBindRenderbuffer(GLenum(GL_RENDERBUFFER), color)
            glRenderbufferStorage(GLenum(GL_RENDERBUFFER), GLenum(GL_RGBA8), width, height)
            glFramebufferRenderbuffer(GLenum(GL_FRAMEBUFFER), GLenum(GL_COLOR_ATTACHMENT0),
                                      GLenum(GL_RENDERBUFFER), color)
            guard glCheckFramebufferStatus(GLenum(GL_FRAMEBUFFER)) == GLenum(GL_FRAMEBUFFER_COMPLETE),
                  okp_session_render(session, width, height) else { return nil }
            glBindFramebuffer(GLenum(GL_FRAMEBUFFER), framebuffer)
            var rgba = [UInt8](repeating: 0, count: Int(width) * Int(height) * 4)
            rgba.withUnsafeMutableBytes {
                glReadPixels(0, 0, width, height, GLenum(GL_RGBA), GLenum(GL_UNSIGNED_BYTE), $0.baseAddress)
            }
            guard glGetError() == GLenum(GL_NO_ERROR) else { return nil }
            return VideoEvidence(width: Int(width), height: Int(height), rgba: rgba)
        }
    }
}
