import Foundation

/// When the video layer serves libmpv and when it draws, as plain state.
///
/// libmpv's update callback asks for an update on the render thread. The update
/// serves libmpv (advanced control may be waiting for it) and says whether a new
/// frame is due; only then is the display link started. A display-link tick with a
/// due frame draws it; ticks without one let the link pause again. So a paused or
/// idle player, whose libmpv announces no frames, draws nothing and stops ticking.
///
/// The update never waits for the main thread: libmpv can be blocked on the render
/// thread while the main thread waits for libmpv, so only drawing is paced by the
/// display link.
struct FramePacing {
    /// Ticks without a due frame before the display link is paused.
    static let idleTicksBeforePause = 8

    enum Tick: Equatable {
        /// Draw now, on the render thread.
        case draw
        /// Nothing to draw on this tick.
        case wait
        /// Nothing to draw for a while: pause the display link.
        case pauseLink
    }

    private(set) var updateScheduled = false
    private(set) var frameDue = false
    private(set) var drawInFlight = false
    private(set) var linkRunning = false
    private var idleTicks = 0

    /// libmpv's update callback fired. True when an update has to be scheduled on
    /// the render thread; callbacks before it runs share that update.
    mutating func updateRequested() -> Bool {
        guard !updateScheduled else { return false }
        updateScheduled = true
        return true
    }

    /// The render thread starts serving the update. A callback from now on
    /// schedules another one.
    mutating func updateStarted() {
        updateScheduled = false
    }

    /// The update said whether a new frame is due. True when the display link has
    /// to be started.
    mutating func updateFinished(frameDue due: Bool) -> Bool {
        guard due else { return false }
        frameDue = true
        return startLink()
    }

    /// The shell wants the current frame drawn again, at a new size for instance.
    /// True when the display link has to be started.
    mutating func redrawRequested() -> Bool {
        frameDue = true
        return startLink()
    }

    /// A display-link tick.
    mutating func tick() -> Tick {
        if frameDue && !drawInFlight {
            frameDue = false
            drawInFlight = true
            idleTicks = 0
            return .draw
        }
        if frameDue || drawInFlight {
            idleTicks = 0
            return .wait
        }
        idleTicks += 1
        guard idleTicks >= FramePacing.idleTicksBeforePause else { return .wait }
        idleTicks = 0
        linkRunning = false
        return .pauseLink
    }

    /// The render thread finished a draw the last `.draw` tick started.
    mutating func drawFinished() {
        drawInFlight = false
    }

    private mutating func startLink() -> Bool {
        guard !linkRunning else { return false }
        linkRunning = true
        idleTicks = 0
        return true
    }
}

/// `FramePacing` shared between libmpv's callback thread, the render queue and the
/// main thread.
final class FramePacer {
    private let lock = NSLock()
    private var pacing = FramePacing()

    func with<Value>(_ body: (inout FramePacing) -> Value) -> Value {
        lock.lock()
        defer { lock.unlock() }
        return body(&pacing)
    }
}
