import AppKit
import XCTest
@testable import OKPlayerShell

/// Stands in for the video: takes keyboard focus as `VideoView` does.
private final class FocusTarget: NSView {
    override var acceptsFirstResponder: Bool { true }
}

/// The overlay over real AppKit views, in a window that is never shown: where
/// keyboard focus goes, how a press ends when the file ends under it, and what
/// the mute button asks the session for. Keys on the focused scrubber are the
/// player window's (`ShortcutWindowTests`).
final class TransportOverlayTests: XCTestCase {
    private var timer: ManualHideTimer!
    private var overlay: TransportOverlay!
    private var video: FocusTarget!
    private var window: NSWindow!
    private var dispatched: [Command] = []
    /// Every drag position the overlay asked the session's readout for.
    private var asked: [Double?] = []
    private var seekable = true
    private var reply: TransportView!

    override func setUpWithError() throws {
        try super.setUpWithError()
        _ = NSApplication.shared
        let decoded = try GoldenFixtures.decode(QueryReply.self, "transport/query-reply-transport.json")
        guard case .transport(let view) = decoded.result else { return XCTFail("not a transport reply") }
        reply = view
        timer = ManualHideTimer()
        overlay = TransportOverlay(hideTimer: timer)
        video = FocusTarget()
        let frame = NSRect(x: 0, y: 0, width: 640, height: 360)
        window = NSWindow(contentRect: frame, styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        let root = NSView(frame: frame)
        window.contentView = root
        for view in [video!, overlay!] as [NSView] {
            view.frame = root.bounds
            root.addSubview(view)
        }
        overlay.focusReturn = video
        dispatched = []
        asked = []
        seekable = true
        overlay.dispatch = { [unowned self] command in self.dispatched.append(command) }
        overlay.query = { [unowned self] at in
            self.asked.append(at)
            var view = self.reply!
            view.seekable = self.seekable
            return view
        }
    }

    override func tearDown() {
        overlay.removeFromSuperview()
        window.close()
        super.tearDown()
    }

    private func snapshot(_ status: Status, duration: Double?) -> SessionSnapshot {
        var snapshot = SessionSnapshot()
        snapshot.status = status
        snapshot.source = MediaSource(kind: .local, location: "/Users/example/Movies/film.mkv")
        snapshot.duration = duration
        snapshot.timePos = duration.map { $0 / 4 }
        return snapshot
    }

    /// The scrubber has keyboard focus, as after a click on it.
    private func focusScrubber() {
        overlay.sessionChanged(snapshot(.playing, duration: 4000))
        XCTAssertTrue(window.makeFirstResponder(overlay.scrubber), "the shown scrubber takes focus")
        XCTAssertTrue(window.firstResponder === overlay.scrubber)
    }

    /// A pointer event over the middle of the scrubber, wherever layout put it.
    private func pointer(_ type: NSEvent.EventType) throws -> NSEvent {
        window.layoutIfNeeded()
        let bar = overlay.scrubber
        let point = bar.convert(NSPoint(x: bar.bounds.midX, y: bar.bounds.midY), to: nil)
        return try XCTUnwrap(NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: 0,
                                                windowNumber: window.windowNumber, context: nil, eventNumber: 0,
                                                clickCount: 1, pressure: 1))
    }

    func testControlsThatHideGiveTheScrubbersFocusBackToTheVideo() {
        focusScrubber()
        timer.elapse()
        XCTAssertFalse(overlay.isRevealed)
        XCTAssertTrue(window.firstResponder === video, "Left/Right would seek through hidden controls")
    }

    func testAScrubberThatCanNoLongerSeekGivesItsFocusBack() {
        focusScrubber()
        seekable = false
        overlay.sessionChanged(snapshot(.ended, duration: nil))
        XCTAssertFalse(overlay.scrubber.isEnabled)
        XCTAssertTrue(window.firstResponder === video)
    }

    func testTheFileEndingUnderAPressEndsTheDragAndReleasesItsHold() throws {
        overlay.sessionChanged(snapshot(.playing, duration: 4000))
        overlay.scrubber.mouseDown(with: try pointer(.leftMouseDown))
        XCTAssertEqual(overlay.autoHide.pins, 1, "the drag holds the controls up")
        guard case .seek(seconds: _, mode: .absolute, precision: .keyframe)? = dispatched.last else {
            return XCTFail("the press did not seek: \(dispatched)")
        }

        // The file ends under the held pointer: libmpv unloads it, so there is no
        // duration and nothing to seek.
        seekable = false
        overlay.sessionChanged(snapshot(.ended, duration: nil))
        XCTAssertFalse(overlay.scrubber.isEnabled)
        XCTAssertEqual(overlay.autoHide.pins, 0, "the ended drag no longer holds the controls")

        // The pointer is over the bar when the press is let go, which holds the
        // controls on its own; the late release must not take that hold away.
        overlay.autoHide.pin()
        overlay.scrubber.mouseUp(with: try pointer(.leftMouseUp))
        XCTAssertFalse(overlay.scrubber.isPressed, "the release reached the disabled bar")
        XCTAssertEqual(overlay.autoHide.pins, 1)
        XCTAssertEqual(dispatched.count, 1, "nothing to seek in a file that is gone")
        overlay.autoHide.unpin()

        // The next file shows its own playhead, not the old drag position.
        seekable = true
        asked = []
        overlay.sessionChanged(snapshot(.playing, duration: 600))
        XCTAssertEqual(asked, [nil])
    }

    func testPreviousAndNextFollowThePlaylistAndRunTheirActions() {
        var steps: [String] = []
        overlay.sessionChanged(snapshot(.playing, duration: 600))
        XCTAssertFalse(overlay.previousButton.isEnabled, "no action, no playlist")
        XCTAssertFalse(overlay.nextButton.isEnabled)
        overlay.onPrevious = { steps.append("previous") }
        overlay.onNext = { steps.append("next") }
        XCTAssertFalse(overlay.previousButton.isEnabled, "a lone item has nothing to step to")
        XCTAssertFalse(overlay.nextButton.isEnabled)

        overlay.playlistSteps = (previous: true, next: true)
        XCTAssertTrue(overlay.previousButton.isEnabled)
        XCTAssertTrue(overlay.nextButton.isEnabled)
        overlay.nextButton.performClick(nil)
        overlay.previousButton.performClick(nil)
        XCTAssertEqual(steps, ["next", "previous"])
        XCTAssertTrue(dispatched.isEmpty, "stepping is the playlist's, not a playback command")

        overlay.playlistSteps = (previous: false, next: false)
        XCTAssertFalse(overlay.previousButton.isEnabled)
        XCTAssertFalse(overlay.nextButton.isEnabled)
        overlay.playlistSteps = (previous: true, next: true)
        overlay.onNext = nil
        XCTAssertTrue(overlay.previousButton.isEnabled)
        XCTAssertFalse(overlay.nextButton.isEnabled, "no action, no button")
    }

    func testTheControlsNeverHoldTheWindowToTheirOwnWidth() {
        // A window keeps the size it is given at the window-size-stay-put priority;
        // anything stronger inside it wins, so the content stands in for a window here.
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 2000, height: 1000))
        let content = NSView()
        let controls = TransportOverlay(hideTimer: ManualHideTimer())
        for view in [content, controls] { view.translatesAutoresizingMaskIntoConstraints = false }
        container.addSubview(content)
        content.addSubview(controls)
        let width = content.widthAnchor.constraint(equalToConstant: 1200)
        let height = content.heightAnchor.constraint(equalToConstant: 600)
        for size in [width, height] { size.priority = .windowSizeStayPut }
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            content.topAnchor.constraint(equalTo: container.topAnchor),
            width, height,
            controls.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            controls.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            controls.topAnchor.constraint(equalTo: content.topAnchor),
            controls.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])
        container.layoutSubtreeIfNeeded()
        XCTAssertEqual(content.frame.width, 1200, accuracy: 0.5, "the window keeps the width it was given")
        XCTAssertEqual(controls.panel.frame.width, 760, accuracy: 0.5, "the bar stops at its widest")

        width.constant = 700
        container.layoutSubtreeIfNeeded()
        XCTAssertEqual(content.frame.width, 700, accuracy: 0.5)
        XCTAssertEqual(controls.panel.frame.width, 668, accuracy: 0.5, "a narrower window keeps 16 points each side")
    }

    func testTheMuteButtonAsksTheSessionToToggleMute() {
        overlay.sessionChanged(snapshot(.playing, duration: 4000))
        overlay.muteButton.performClick(nil)
        XCTAssertEqual(dispatched, [.toggleMute])
    }
}
