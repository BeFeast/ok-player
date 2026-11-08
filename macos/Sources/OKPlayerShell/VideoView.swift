import AppKit
import COKPCore
import OpenGL
import OpenGL.GL3
import QuartzCore

/// The CGL pixel format and context the video layer draws with: one of each for
/// the life of the layer, whatever happens to the window's size or screen.
final class GLSurface {
    /// What Core Animation has done with the context so far: how often
    /// `copyCGLContext` handed it out, how many of those came back through
    /// `releaseCGLContext`, and every context it drew into. The smoke checks that
    /// resizing the window leaves exactly one context held and drawn into.
    struct ContextUse {
        var copies = 0
        var releases = 0
        var drawnInto: Set<CGLContextObj> = []
        /// Contexts Core Animation holds now.
        var held: Int { copies - releases }
    }

    let pixelFormat: CGLPixelFormatObj
    let context: CGLContextObj
    /// The colour buffer has floating-point components (16 bits each), so EDR
    /// output later is a layer flag, not a new renderer.
    let floatColor: Bool

    private let useLock = NSLock()
    private var use = ContextUse()

    private init(pixelFormat: CGLPixelFormatObj, context: CGLContextObj, floatColor: Bool) {
        self.pixelFormat = pixelFormat
        self.context = context
        self.floatColor = floatColor
    }

    deinit {
        CGLReleaseContext(context)
        CGLReleasePixelFormat(pixelFormat)
    }

    /// A 3.2 core profile surface, float-capable when the renderer offers it.
    static func make() -> GLSurface? {
        let profile: [CGLPixelFormatAttribute] = [
            kCGLPFAOpenGLProfile, CGLPixelFormatAttribute(rawValue: kCGLOGLPVersion_3_2_Core.rawValue),
            kCGLPFADoubleBuffer, kCGLPFAAllowOfflineRenderers,
        ]
        let float: [CGLPixelFormatAttribute] = [
            kCGLPFAColorFloat, kCGLPFAColorSize, CGLPixelFormatAttribute(rawValue: 64),
        ]
        let attempts: [([CGLPixelFormatAttribute], Bool)] = [
            (profile + [kCGLPFAAccelerated] + float, true),
            (profile + float, true),
            (profile + [kCGLPFAAccelerated], false),
            (profile, false),
        ]
        for (attributes, floatColor) in attempts {
            var pixelFormat: CGLPixelFormatObj?
            var count: GLint = 0
            guard CGLChoosePixelFormat(attributes + [CGLPixelFormatAttribute(rawValue: 0)], &pixelFormat, &count) == kCGLNoError,
                  let pixelFormat else { continue }
            var context: CGLContextObj?
            guard CGLCreateContext(pixelFormat, nil, &context) == kCGLNoError, let context else {
                CGLReleasePixelFormat(pixelFormat)
                continue
            }
            return GLSurface(pixelFormat: pixelFormat, context: context, floatColor: floatColor)
        }
        return nil
    }

    /// Core Animation's context use; read from any thread.
    var contextUse: ContextUse {
        useLock.lock()
        defer { useLock.unlock() }
        return use
    }

    fileprivate func noteContextUse(_ change: (inout ContextUse) -> Void) {
        useLock.lock()
        change(&use)
        useLock.unlock()
    }

    /// Run `body` with the context locked and current on this thread, as every
    /// `okp_session_render_*` call needs.
    func withCurrentContext<Value>(_ body: () -> Value) -> Value {
        CGLLockContext(context)
        CGLSetCurrentContext(context)
        defer {
            CGLSetCurrentContext(nil)
            CGLUnlockContext(context)
        }
        return body()
    }
}

/// The video layer: libmpv's OpenGL render API in a `CAOpenGLLayer` with its own
/// context and its own serial render queue.
///
/// libmpv's update callback schedules an update on the render queue; when the
/// update says a frame is due, the display link is started, and its next tick
/// draws the frame on the render queue: `display()`, `CATransaction.flush()`, then
/// `okp_session_render_report_swap`. Nothing is drawn without a due frame (or a
/// redraw the view asks for after a resize), and nothing draws on the main thread.
final class VideoLayer: CAOpenGLLayer {
    private static let renderQueueKey = DispatchSpecificKey<Bool>()

    let surface: GLSurface
    let pacer = FramePacer()
    let renderQueue = DispatchQueue(label: "uk.oklabs.ok-player.render", qos: .userInteractive)
    /// Starts the display link; called on the main queue.
    var startLink: (() -> Void)?
    /// Reports a renderer failure; called on the main queue.
    var onError: ((String) -> Void)?

    // Render-queue state.
    private var session: OpaquePointer?
    private var drawing = false
    private var drewFrame = false
    private var renderFailed = false
    private var errorReported = false

    private let countersLock = NSLock()
    private var renders = 0
    private var swaps = 0

    init(surface: GLSurface) {
        self.surface = surface
        super.init()
        renderQueue.setSpecific(key: VideoLayer.renderQueueKey, value: true)
        isAsynchronous = false
        needsDisplayOnBoundsChange = false
    }

    /// Core Animation's copy for presentation. It shares the surface and never draws.
    override init(layer: Any) {
        guard let original = layer as? VideoLayer else { fatalError("init(layer:) needs a VideoLayer") }
        surface = original.surface
        super.init(layer: layer)
    }

    required init?(coder: NSCoder) { fatalError("Use init(surface:)") }

    /// Frames drawn and presented, counted here, and the swaps the session says
    /// reached libmpv, as `okp_session_render_report_swap` returned last; read
    /// from any thread. Each presented frame reports one swap, so the two match.
    var counts: (renders: Int, swaps: Int) {
        countersLock.lock()
        defer { countersLock.unlock() }
        return (renders, swaps)
    }

    /// Create the session's render context on the render queue. `session` must
    /// stay alive until `detach()` has returned.
    func attach(session: OpaquePointer) {
        let ctx = Unmanaged.passUnretained(self).toOpaque()
        renderQueue.async { [self] in
            var error = [CChar](repeating: 0, count: 1024)
            let created = surface.withCurrentContext {
                okp_session_render_create(session, videoLayerUpdate, ctx, &error, numericCast(error.count))
            }
            if created {
                self.session = session
            } else {
                reportError("Could not create the video renderer: \(String(cString: error))")
            }
        }
    }

    /// Destroy the render context and wait until it is gone. After this, libmpv
    /// never calls the update callback again and nothing more is drawn.
    func detach() {
        renderQueue.sync {
            guard let session else { return }
            surface.withCurrentContext { okp_session_render_destroy(session) }
            self.session = nil
        }
    }

    /// From libmpv's update callback, on one of its threads.
    fileprivate func updateRequested() {
        if pacer.with({ $0.updateRequested() }) {
            renderQueue.async { [self] in serveUpdate() }
        }
    }

    /// From a display-link tick that returned `.draw`.
    func scheduleDraw() {
        renderQueue.async { [self] in drawFrame() }
    }

    private func serveUpdate() {
        pacer.with { $0.updateStarted() }
        guard let session else { return }
        let due = surface.withCurrentContext { okp_session_render_update(session) }
        if pacer.with({ $0.updateFinished(frameDue: due) }) {
            DispatchQueue.main.async { [weak self] in self?.startLink?() }
        }
    }

    private func drawFrame() {
        defer { pacer.with { $0.drawFinished() } }
        guard let session else { return }
        drawing = true
        drewFrame = false
        renderFailed = false
        display()
        CATransaction.flush()
        drawing = false
        if renderFailed { reportError("Video rendering failed") }
        guard drewFrame else { return }
        let reported = surface.withCurrentContext { okp_session_render_report_swap(session) }
        countersLock.lock()
        renders += 1
        swaps = Int(reported)
        countersLock.unlock()
    }

    private func reportError(_ message: String) {
        guard !errorReported else { return }
        errorReported = true
        DispatchQueue.main.async { [weak self] in self?.onError?(message) }
    }

    override func copyCGLPixelFormat(forDisplayMask mask: UInt32) -> CGLPixelFormatObj {
        // Core Animation releases what it is given.
        CGLRetainPixelFormat(surface.pixelFormat)
    }

    override func copyCGLContext(forPixelFormat pf: CGLPixelFormatObj) -> CGLContextObj {
        // Core Animation hands the context back through `releaseCGLContext`.
        surface.noteContextUse { $0.copies += 1 }
        return CGLRetainContext(surface.context)
    }

    override func releaseCGLContext(_ c: CGLContextObj) {
        surface.noteContextUse { $0.releases += 1 }
        // The default implementation releases the context.
        super.releaseCGLContext(c)
    }

    override func canDraw(inCGLContext ctx: CGLContextObj, pixelFormat pf: CGLPixelFormatObj,
                          forLayerTime t: CFTimeInterval, displayTime ts: UnsafePointer<CVTimeStamp>?) -> Bool {
        // Only the render queue draws. A display Core Animation or AppKit starts on
        // another thread keeps the last frame.
        DispatchQueue.getSpecific(key: VideoLayer.renderQueueKey) == true && drawing
    }

    override func draw(inCGLContext ctx: CGLContextObj, pixelFormat pf: CGLPixelFormatObj,
                       forLayerTime t: CFTimeInterval, displayTime ts: UnsafePointer<CVTimeStamp>?) {
        surface.noteContextUse { _ = $0.drawnInto.insert(ctx) }
        // Core Animation sets the viewport to the layer's size in pixels.
        var viewport = [GLint](repeating: 0, count: 4)
        glGetIntegerv(GLenum(GL_VIEWPORT), &viewport)
        if let session, viewport[2] > 0, viewport[3] > 0 {
            if okp_session_render(session, viewport[2], viewport[3]) {
                drewFrame = true
            } else {
                renderFailed = true
            }
        }
        // The default implementation flushes the context.
        super.draw(inCGLContext: ctx, pixelFormat: pf, forLayerTime: t, displayTime: ts)
    }
}

/// libmpv's render update callback. It runs on a libmpv thread and only schedules
/// work on the render queue.
private func videoLayerUpdate(_ ctx: UnsafeMutableRawPointer?) {
    guard let ctx else { return }
    Unmanaged<VideoLayer>.fromOpaque(ctx).takeUnretainedValue().updateRequested()
}

/// The video surface: a layer-hosting view over a `VideoLayer`, with the display
/// link that paces its drawing.
final class VideoView: NSView {
    let videoLayer: VideoLayer
    var onTogglePause: (() -> Void)?
    /// A primary-button release on the video, with AppKit's click count.
    var onClick: ((Int) -> Void)?
    /// The current media has video. Only then does a resize redraw the frame.
    var hasVideo = false
    private var frameLink: CADisplayLink?
    private var tornDown = false

    init(surface: GLSurface) {
        videoLayer = VideoLayer(surface: surface)
        super.init(frame: .zero)
        // Layer-hosting: the layer is set before `wantsLayer`, and AppKit never
        // asks it to draw.
        layer = videoLayer
        wantsLayer = true
        videoLayer.startLink = { [weak self] in self?.frameLink?.isPaused = false }
    }

    required init?(coder: NSCoder) { fatalError("Use init(surface:)") }

    override var acceptsFirstResponder: Bool { true }

    // Taking the press keeps the release coming back to this view.
    override func mouseDown(with event: NSEvent) {}

    override func mouseUp(with event: NSEvent) {
        onClick?(event.clickCount)
    }

    override func keyDown(with event: NSEvent) {
        if event.charactersIgnoringModifiers == " " {
            onTogglePause?()
        } else {
            super.keyDown(with: event)
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window, !tornDown else { return }
        videoLayer.contentsScale = window.backingScaleFactor
        guard frameLink == nil else { return }
        let link = displayLink(target: self, selector: #selector(frameLinkFired(_:)))
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        frameLink = link
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        videoLayer.contentsScale = window?.backingScaleFactor ?? 1
        redraw()
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        redraw()
    }

    /// Draw the current frame again on the next tick, if there is video.
    func redraw() {
        guard hasVideo, !tornDown else { return }
        if videoLayer.pacer.with({ $0.redrawRequested() }) {
            frameLink?.isPaused = false
        }
    }

    @objc private func frameLinkFired(_ link: CADisplayLink) {
        switch videoLayer.pacer.with({ $0.tick() }) {
        case .draw: videoLayer.scheduleDraw()
        case .wait: break
        case .pauseLink: link.isPaused = true
        }
    }

    /// Stop the display link and destroy the render context. Call it before the
    /// session is freed.
    func shutdown() {
        tornDown = true
        frameLink?.invalidate()
        frameLink = nil
        videoLayer.detach()
    }
}
