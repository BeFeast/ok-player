import XCTest
@testable import OKPlayerShell

/// The video layer's pacing: libmpv's callback leads to an update on the render
/// thread, a due frame to one draw on the next display-link tick, and no due frame
/// to no draw at all, which is what keeps a paused or idle player from rendering.
final class FramePacingTests: XCTestCase {
    func testCallbacksBeforeTheUpdateRunsShareOneUpdate() {
        var pacing = FramePacing()
        XCTAssertTrue(pacing.updateRequested())
        XCTAssertFalse(pacing.updateRequested())
        XCTAssertFalse(pacing.updateRequested())
        pacing.updateStarted()
        XCTAssertTrue(pacing.updateRequested(), "a callback during the update schedules the next one")
    }

    func testAnUpdateWithoutADueFrameNeitherStartsTheLinkNorDraws() {
        var pacing = FramePacing()
        _ = pacing.updateRequested()
        pacing.updateStarted()
        XCTAssertFalse(pacing.updateFinished(frameDue: false))
        XCTAssertFalse(pacing.linkRunning)
        for _ in 0..<(FramePacing.idleTicksBeforePause * 3) {
            XCTAssertNotEqual(pacing.tick(), .draw)
        }
    }

    func testADueFrameStartsTheLinkAndIsDrawnOnceOnTheNextTick() {
        var pacing = FramePacing()
        XCTAssertTrue(pacing.updateFinished(frameDue: true), "the first due frame starts the link")
        XCTAssertEqual(pacing.tick(), .draw)
        pacing.drawFinished()
        XCTAssertEqual(pacing.tick(), .wait, "one due frame is drawn once")
        XCTAssertFalse(pacing.updateFinished(frameDue: true), "the link is already running")
        XCTAssertEqual(pacing.tick(), .draw)
    }

    func testAFrameDueDuringADrawWaitsForThatDrawToFinish() {
        var pacing = FramePacing()
        _ = pacing.updateFinished(frameDue: true)
        XCTAssertEqual(pacing.tick(), .draw)
        _ = pacing.updateFinished(frameDue: true)
        XCTAssertEqual(pacing.tick(), .wait, "never two draws in flight")
        XCTAssertEqual(pacing.tick(), .wait)
        pacing.drawFinished()
        XCTAssertEqual(pacing.tick(), .draw)
    }

    func testTheLinkPausesAfterIdleTicksAndTheNextFrameStartsItAgain() {
        var pacing = FramePacing()
        _ = pacing.updateFinished(frameDue: true)
        XCTAssertEqual(pacing.tick(), .draw)
        pacing.drawFinished()
        for _ in 1..<FramePacing.idleTicksBeforePause {
            XCTAssertEqual(pacing.tick(), .wait)
        }
        XCTAssertEqual(pacing.tick(), .pauseLink)
        XCTAssertFalse(pacing.linkRunning)
        XCTAssertTrue(pacing.updateFinished(frameDue: true), "a paused link is started again")
        XCTAssertEqual(pacing.tick(), .draw)
    }

    func testARedrawDrawsWithoutAFrameFromLibmpv() {
        var pacing = FramePacing()
        XCTAssertTrue(pacing.redrawRequested())
        XCTAssertEqual(pacing.tick(), .draw)
    }
}
