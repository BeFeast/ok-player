import Foundation

// The transport controls' behaviour as plain types, so XCTest covers it without
// AppKit: which trailing label shows, what a scrubber drag sends, how scrolling
// becomes volume steps, and when the controls hide. Times, volume rules and the
// controls' surface come from the session (TransportProtocol.swift); these types
// only turn input into commands and keep the timer.

/// Which trailing label the time readout shows; a click on it switches.
enum TrailingTime: Equatable {
    case remaining, total

    var toggled: TrailingTime { self == .remaining ? .total : .remaining }

    func text(in view: TransportView) -> String { self == .remaining ? view.remaining : view.total }

    /// What a click on the label does next.
    var toolTip: String { self == .remaining ? "Show total time" : "Show remaining time" }
}

/// Where along a horizontal track the pointer is.
enum TrackGeometry {
    /// The fraction of a track `width` points wide, inset by `inset` on both ends,
    /// under `x`; clamped to 0...1.
    static func fraction(atX x: Double, width: Double, inset: Double) -> Double {
        let usable = width - 2 * inset
        guard usable > 0 else { return 0 }
        return Swift.min(Swift.max((x - inset) / usable, 0), 1)
    }
}

/// A scrubber drag: keyframe seeks while the pointer moves, one exact seek where it
/// is released, and the drag position for the readout in between.
struct ScrubState {
    /// Live seeks closer together than this are skipped; the release is exact anyway.
    static let minimumLiveStep = 0.2

    /// Where the drag is, as a fraction; nil when nothing is being dragged.
    private(set) var dragFraction: Double?
    private var lastLiveTarget: Double?

    var isDragging: Bool { dragFraction != nil }

    /// The drag position in seconds, for the session's readout at that position.
    func dragPosition(duration: Double?) -> Double? {
        guard let dragFraction, let duration = Self.seekable(duration) else { return nil }
        return dragFraction * duration
    }

    /// The pointer went down on the track. Nil when the media cannot be sought.
    mutating func begin(at fraction: Double, duration: Double?) -> Command? {
        cancel()
        return track(fraction, duration: duration)
    }

    /// The pointer moved during the drag. Nothing once the drag has ended.
    mutating func move(to fraction: Double, duration: Double?) -> Command? {
        guard isDragging else { return nil }
        return track(fraction, duration: duration)
    }

    /// The pointer was released: an exact seek to where it is.
    mutating func end(at fraction: Double, duration: Double?) -> Command? {
        defer { cancel() }
        guard isDragging, let duration = Self.seekable(duration) else { return nil }
        return .seek(seconds: Self.clamped(fraction) * duration, mode: .absolute, precision: .exact)
    }

    /// The drag is over without a release, because the media can no longer be
    /// sought (it ended under the pointer): no seek, and no drag position left
    /// behind for the next file.
    mutating func cancel() {
        dragFraction = nil
        lastLiveTarget = nil
    }

    private mutating func track(_ fraction: Double, duration: Double?) -> Command? {
        guard let duration = Self.seekable(duration) else {
            dragFraction = nil
            return nil
        }
        let fraction = Self.clamped(fraction)
        dragFraction = fraction
        let target = fraction * duration
        if let last = lastLiveTarget, abs(target - last) < Self.minimumLiveStep { return nil }
        lastLiveTarget = target
        return .seek(seconds: target, mode: .absolute, precision: .keyframe)
    }

    private static func seekable(_ duration: Double?) -> Double? {
        guard let duration, duration.isFinite, duration > 0 else { return nil }
        return duration
    }

    private static func clamped(_ fraction: Double) -> Double {
        fraction.isFinite ? Swift.min(Swift.max(fraction, 0), 1) : 0
    }
}

/// One scroll event, as the volume control reads it.
struct ScrollInput: Equatable {
    enum Phase: Equatable { case none, began, changed, ended }

    /// Vertical delta in the device's own direction: positive is the wheel turned
    /// away from the user, or two fingers moved up, whatever the natural-scrolling
    /// setting. That direction raises the volume.
    var deltaY: Double
    /// A trackpad or Magic Mouse (points), not a notched wheel (lines).
    var precise: Bool
    var phase: Phase = .none
    /// Inertia after the fingers left the trackpad.
    var momentum: Bool = false
}

/// Turns scrolling into volume steps without running away: a wheel notch is one
/// step however fast the wheel spins, trackpad points add up to steps, and the
/// inertia after lifting the fingers changes nothing.
struct VolumeScrollAccumulator {
    /// Trackpad points per step.
    static let pointsPerStep = 10.0
    /// The most steps one trackpad event can make.
    static let maxStepsPerEvent = 3
    /// Volume percent per step; the session clamps the result to its range.
    static let percentPerStep = 1.0

    /// Trackpad points short of the next step.
    private(set) var residue = 0.0

    mutating func steps(for input: ScrollInput) -> Int {
        guard !input.momentum else {
            residue = 0
            return 0
        }
        guard input.deltaY.isFinite else { return 0 }
        guard input.precise else {
            residue = 0
            return input.deltaY > 0 ? 1 : (input.deltaY < 0 ? -1 : 0)
        }
        if input.phase == .began { residue = 0 }
        residue += input.deltaY
        var whole = (residue / Self.pointsPerStep).rounded(.towardZero)
        residue -= whole * Self.pointsPerStep
        let limit = Double(Self.maxStepsPerEvent)
        if abs(whole) > limit {
            whole = whole < 0 ? -limit : limit
            residue = 0
        }
        if input.phase == .ended { residue = 0 }
        return Int(whole)
    }

    /// The command for `steps`, or nil for none.
    static func command(steps: Int) -> Command? {
        steps == 0 ? nil : .nudgeVolume(delta: Double(steps) * percentPerStep)
    }
}

/// The volume slider's track: 0 at the left end, the session's maximum at the right.
enum VolumeTrack {
    /// The level a click or drag at `fraction` of the track sets.
    static func level(atFraction fraction: Double, maximum: Double) -> Double {
        guard fraction.isFinite, maximum.isFinite else { return 0 }
        return Swift.min(Swift.max(fraction, 0), 1) * maximum
    }
}

/// Schedules one pending hide at a time.
protocol HideTimer: AnyObject {
    /// Replace any pending hide with one that fires after `seconds`.
    func schedule(after seconds: TimeInterval, _ fire: @escaping () -> Void)
    func cancel()
}

/// The app's timer: a cancellable block on the main queue.
final class MainQueueHideTimer: HideTimer {
    private var pending: DispatchWorkItem?

    func schedule(after seconds: TimeInterval, _ fire: @escaping () -> Void) {
        cancel()
        let item = DispatchWorkItem(block: fire)
        pending = item
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: item)
    }

    func cancel() {
        pending?.cancel()
        pending = nil
    }
}

/// When the transport controls show, as on Linux: activity shows them; while the
/// session allows auto-hide (media playing) and nothing pins them (the pointer over
/// the controls, an open menu, a drag), they hide once the session's idle timeout
/// passes without activity. Paused or loading media keeps them up.
final class AutoHide {
    private(set) var revealed = true
    private(set) var allowed = false
    private(set) var pins = 0
    private var hideAfter: TimeInterval = 2.5
    private var hideScheduled = false
    private let timer: HideTimer
    /// Called with the new value whenever `revealed` changes.
    var onChange: ((Bool) -> Void)?

    init(timer: HideTimer = MainQueueHideTimer()) {
        self.timer = timer
    }

    /// The session's policy, from each transport view. Repeating it changes nothing.
    func update(allowed: Bool, hideAfter: TimeInterval) {
        self.hideAfter = hideAfter
        let wasAllowed = self.allowed
        self.allowed = allowed
        if allowed && pins == 0 {
            if !wasAllowed || (revealed && !hideScheduled) { scheduleHide() }
        } else {
            showPersistently()
        }
    }

    /// The pointer moved, a key was pressed, the wheel turned.
    func activity() {
        set(revealed: true)
        if allowed && pins == 0 { scheduleHide() }
    }

    func pin() {
        pins += 1
        showPersistently()
    }

    func unpin() {
        pins = Swift.max(0, pins - 1)
        if allowed && pins == 0 { scheduleHide() }
    }

    /// Drop every pin: the controls went away, so nothing over them can still hold them.
    func unpinAll() {
        guard pins > 0 else { return }
        pins = 0
        if allowed { scheduleHide() }
    }

    private func showPersistently() {
        cancelHide()
        set(revealed: true)
    }

    private func scheduleHide() {
        guard revealed else { return }
        cancelHide()
        hideScheduled = true
        timer.schedule(after: hideAfter) { [weak self] in self?.hideFired() }
    }

    private func cancelHide() {
        timer.cancel()
        hideScheduled = false
    }

    private func hideFired() {
        hideScheduled = false
        guard allowed, pins == 0 else { return }
        set(revealed: false)
    }

    private func set(revealed value: Bool) {
        guard revealed != value else { return }
        revealed = value
        onChange?(value)
    }
}
