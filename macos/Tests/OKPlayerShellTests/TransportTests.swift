import XCTest
@testable import OKPlayerShell

/// A hide timer the test fires by hand. It remembers the last scheduled block
/// even after a cancel, so a test can play a timer that fires late.
final class ManualHideTimer: HideTimer {
    private(set) var delay: TimeInterval?
    private(set) var scheduled = 0
    private var fire: (() -> Void)?
    private var lastFire: (() -> Void)?

    var isPending: Bool { fire != nil }

    func schedule(after seconds: TimeInterval, _ fire: @escaping () -> Void) {
        delay = seconds
        scheduled += 1
        self.fire = fire
        lastFire = fire
    }

    func cancel() {
        delay = nil
        fire = nil
    }

    /// The pending hide falls due.
    func elapse() {
        let pending = fire
        cancel()
        pending?()
    }

    /// The last scheduled block runs although it was cancelled.
    func fireLate() { lastFire?() }
}

/// The scrubber's drag turns pointer fractions into session seeks.
final class ScrubStateTests: XCTestCase {
    func testAClickSeeksToThePointAndTheReleaseIsExact() {
        var scrub = ScrubState()
        XCTAssertEqual(scrub.begin(at: 0.25, duration: 400), .seek(seconds: 100, mode: .absolute, precision: .keyframe))
        XCTAssertTrue(scrub.isDragging)
        XCTAssertEqual(scrub.dragPosition(duration: 400), 100)
        XCTAssertEqual(scrub.end(at: 0.25, duration: 400), .seek(seconds: 100, mode: .absolute, precision: .exact))
        XCTAssertFalse(scrub.isDragging)
        XCTAssertNil(scrub.dragPosition(duration: 400), "the readout follows the playhead again")
    }

    func testADragSendsKeyframeSeeksAndSkipsTinySteps() {
        var scrub = ScrubState()
        XCTAssertEqual(scrub.begin(at: 0.5, duration: 100), .seek(seconds: 50, mode: .absolute, precision: .keyframe))
        XCTAssertNil(scrub.move(to: 0.501, duration: 100), "0.1 s from the last seek")
        XCTAssertEqual(scrub.dragPosition(duration: 100)!, 50.1, accuracy: 1e-9, "the readout still follows the pointer")
        XCTAssertEqual(scrub.move(to: 0.6, duration: 100), .seek(seconds: 60, mode: .absolute, precision: .keyframe))
        XCTAssertEqual(scrub.end(at: 0.6, duration: 100), .seek(seconds: 60, mode: .absolute, precision: .exact))
    }

    func testFractionsOffTheTrackClampToItsEnds() {
        var scrub = ScrubState()
        XCTAssertEqual(scrub.begin(at: -0.3, duration: 90), .seek(seconds: 0, mode: .absolute, precision: .keyframe))
        XCTAssertEqual(scrub.move(to: 1.7, duration: 90), .seek(seconds: 90, mode: .absolute, precision: .keyframe))
        XCTAssertEqual(scrub.end(at: .nan, duration: 90), .seek(seconds: 0, mode: .absolute, precision: .exact))
    }

    func testNothingSeeksWithoutAKnownDuration() {
        for duration in [nil, 0, -5, Double.infinity, Double.nan] as [Double?] {
            var scrub = ScrubState()
            XCTAssertNil(scrub.begin(at: 0.5, duration: duration), "\(String(describing: duration))")
            XCTAssertFalse(scrub.isDragging)
            XCTAssertNil(scrub.end(at: 0.5, duration: duration))
        }
    }

    func testAReleaseWithoutAPressSendsNothing() {
        var scrub = ScrubState()
        XCTAssertNil(scrub.end(at: 0.5, duration: 100))
    }

    func testTheTrackFractionIgnoresTheInsetEnds() {
        XCTAssertEqual(TrackGeometry.fraction(atX: 7, width: 214, inset: 7), 0)
        XCTAssertEqual(TrackGeometry.fraction(atX: 107, width: 214, inset: 7), 0.5)
        XCTAssertEqual(TrackGeometry.fraction(atX: -20, width: 214, inset: 7), 0)
        XCTAssertEqual(TrackGeometry.fraction(atX: 500, width: 214, inset: 7), 1)
        XCTAssertEqual(TrackGeometry.fraction(atX: 5, width: 10, inset: 7), 0, "no usable track")
    }
}

/// Scrolling becomes volume steps without running away.
final class VolumeScrollTests: XCTestCase {
    private func wheel(_ delta: Double) -> ScrollInput { ScrollInput(deltaY: delta, precise: false) }
    private func pad(_ delta: Double, _ phase: ScrollInput.Phase = .changed) -> ScrollInput {
        ScrollInput(deltaY: delta, precise: true, phase: phase)
    }

    func testAWheelNotchIsOneStepHoweverFastTheWheelSpins() {
        var scroll = VolumeScrollAccumulator()
        XCTAssertEqual(scroll.steps(for: wheel(1)), 1)
        XCTAssertEqual(scroll.steps(for: wheel(9.7)), 1, "an accelerated notch")
        XCTAssertEqual(scroll.steps(for: wheel(-4)), -1)
        XCTAssertEqual(scroll.steps(for: wheel(0)), 0)
    }

    func testTrackpadPointsAddUpToSteps() {
        var scroll = VolumeScrollAccumulator()
        var steps = scroll.steps(for: pad(0, .began))
        for _ in 0..<10 { steps += scroll.steps(for: pad(3)) }
        XCTAssertEqual(steps, 3, "30 points at 10 points a step")
        XCTAssertEqual(scroll.residue, 0)
        XCTAssertEqual(scroll.steps(for: pad(-25)), -2)
    }

    func testInertiaAfterLiftingTheFingersChangesNothing() {
        var scroll = VolumeScrollAccumulator()
        XCTAssertEqual(scroll.steps(for: pad(8)), 0)
        for delta in [40.0, 25, 12, 4] {
            XCTAssertEqual(scroll.steps(for: ScrollInput(deltaY: delta, precise: true, phase: .none, momentum: true)), 0)
        }
        XCTAssertEqual(scroll.residue, 0, "inertia also drops the points short of a step")
    }

    func testANewGestureStartsWithoutTheLastOnesLeftover() {
        var scroll = VolumeScrollAccumulator()
        XCTAssertEqual(scroll.steps(for: pad(7)), 0)
        XCTAssertEqual(scroll.steps(for: pad(0, .ended)), 0)
        XCTAssertEqual(scroll.steps(for: pad(0, .began)), 0)
        XCTAssertEqual(scroll.steps(for: pad(5)), 0, "7 + 5 would have been a step")
    }

    func testReversingDirectionCancelsTheLeftoverFirst() {
        var scroll = VolumeScrollAccumulator()
        XCTAssertEqual(scroll.steps(for: pad(8)), 0)
        XCTAssertEqual(scroll.steps(for: pad(-8)), 0)
        XCTAssertEqual(scroll.steps(for: pad(-10)), -1)
    }

    func testOneTrackpadEventMakesAtMostThreeSteps() {
        var scroll = VolumeScrollAccumulator()
        XCTAssertEqual(scroll.steps(for: pad(200)), VolumeScrollAccumulator.maxStepsPerEvent)
        XCTAssertEqual(scroll.residue, 0)
        XCTAssertEqual(scroll.steps(for: pad(-200)), -VolumeScrollAccumulator.maxStepsPerEvent)
    }

    func testNonFiniteDeltasAreIgnored() {
        var scroll = VolumeScrollAccumulator()
        XCTAssertEqual(scroll.steps(for: pad(.nan)), 0)
        XCTAssertEqual(scroll.steps(for: wheel(.infinity)), 0)
    }

    func testStepsBecomeVolumeNudgesForTheSession() {
        XCTAssertNil(VolumeScrollAccumulator.command(steps: 0))
        XCTAssertEqual(VolumeScrollAccumulator.command(steps: -3), .nudgeVolume(delta: -3))
        XCTAssertEqual(VolumeScrollAccumulator.command(steps: 2), .nudgeVolume(delta: 2))
    }

    func testTheVolumeTrackMapsToTheSessionsRange() {
        XCTAssertEqual(VolumeTrack.level(atFraction: 1, maximum: 130), 130)
        XCTAssertEqual(VolumeTrack.level(atFraction: 100.0 / 130.0, maximum: 130), 100, accuracy: 1e-9)
        XCTAssertEqual(VolumeTrack.level(atFraction: -0.5, maximum: 130), 0)
        XCTAssertEqual(VolumeTrack.level(atFraction: 2, maximum: 130), 130)
    }
}

/// When the transport controls show and hide.
final class AutoHideTests: XCTestCase {
    private var timer: ManualHideTimer!
    private var autoHide: AutoHide!
    private var changes: [Bool] = []

    override func setUp() {
        super.setUp()
        timer = ManualHideTimer()
        autoHide = AutoHide(timer: timer)
        changes = []
        autoHide.onChange = { [unowned self] in self.changes.append($0) }
    }

    func testTheControlsHideAfterTheSessionsTimeoutOnlyWhilePlaying() {
        autoHide.update(allowed: false, hideAfter: 2.5)
        XCTAssertTrue(autoHide.revealed)
        XCTAssertFalse(timer.isPending, "loading or paused media keeps the controls up")

        autoHide.update(allowed: true, hideAfter: 2.5)
        XCTAssertEqual(timer.delay, 2.5, "the session's idle timeout")
        timer.elapse()
        XCTAssertFalse(autoHide.revealed)
        XCTAssertEqual(changes, [false])
    }

    func testPausingBringsTheControlsBackAndKeepsThemUp() {
        autoHide.update(allowed: true, hideAfter: 2.5)
        timer.elapse()
        autoHide.update(allowed: false, hideAfter: 2.5)
        XCTAssertTrue(autoHide.revealed)
        XCTAssertFalse(timer.isPending)
        XCTAssertEqual(changes, [false, true])
    }

    func testActivityShowsTheControlsAndRestartsTheTimer() {
        autoHide.update(allowed: true, hideAfter: 2.5)
        timer.elapse()
        autoHide.activity()
        XCTAssertTrue(autoHide.revealed)
        XCTAssertTrue(timer.isPending)
        let scheduled = timer.scheduled
        autoHide.activity()
        XCTAssertEqual(timer.scheduled, scheduled + 1, "each movement starts the timeout again")
        timer.elapse()
        XCTAssertFalse(autoHide.revealed)
    }

    func testRepeatingThePolicyDoesNotPostponeTheHide() {
        autoHide.update(allowed: true, hideAfter: 2.5)
        for _ in 0..<5 { autoHide.update(allowed: true, hideAfter: 2.5) }
        XCTAssertEqual(timer.scheduled, 1, "every session batch repeats the policy")
        timer.elapse()
        autoHide.update(allowed: true, hideAfter: 2.5)
        XCTAssertFalse(autoHide.revealed, "a later batch does not bring hidden controls back")
    }

    func testPinnedControlsStayUpUntilEveryPinIsReleased() {
        autoHide.update(allowed: true, hideAfter: 2.5)
        autoHide.pin()
        autoHide.pin()
        XCTAssertFalse(timer.isPending)
        autoHide.unpin()
        XCTAssertFalse(timer.isPending, "one pin is still held")
        autoHide.unpin()
        XCTAssertTrue(timer.isPending)
        timer.elapse()
        XCTAssertFalse(autoHide.revealed)
        autoHide.unpin()
        XCTAssertEqual(autoHide.pins, 0, "an extra release does not go negative")
    }

    func testALateTimerDoesNotHidePinnedOrPausedControls() {
        autoHide.update(allowed: true, hideAfter: 2.5)
        autoHide.pin()
        timer.fireLate()
        XCTAssertTrue(autoHide.revealed)
        autoHide.unpin()
        autoHide.update(allowed: false, hideAfter: 2.5)
        timer.fireLate()
        XCTAssertTrue(autoHide.revealed)
        XCTAssertEqual(changes, [])
    }

    func testControlsThatWentAwayReleaseTheirPins() {
        autoHide.update(allowed: true, hideAfter: 2.5)
        autoHide.pin()
        autoHide.unpinAll()
        XCTAssertEqual(autoHide.pins, 0)
        timer.elapse()
        XCTAssertFalse(autoHide.revealed)
    }
}

/// The time readout's trailing label.
final class TrailingTimeTests: XCTestCase {
    func testAClickSwitchesBetweenRemainingAndTotal() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "transport/query-reply-transport.json")
        guard case .transport(let view) = reply.result else { return XCTFail("not a transport reply") }
        var trailing = TrailingTime.remaining
        XCTAssertEqual(trailing.text(in: view), "-50:00")
        trailing = trailing.toggled
        XCTAssertEqual(trailing.text(in: view), "01:06:40")
        XCTAssertEqual(trailing.toggled, .remaining)
    }
}
