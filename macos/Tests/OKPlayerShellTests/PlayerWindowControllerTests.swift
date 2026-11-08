import AppKit
import XCTest
@testable import OKPlayerShell

/// A player window that is in fullscreen when the test flags it, so routing can
/// be checked without moving a window to a new Space.
private final class FlaggedWindow: PlayerWindow {
    var flagged = false
    override var isInFullscreen: Bool { flagged }
}

/// The session's window rules as the test answers them: the fits it hands out
/// once each, and core's click rules.
private final class StandInRules: WindowRules {
    struct FitRequest {
        var minSize: CGSize
        var chrome: CGSize
    }

    var fits: [CGRect] = []
    private(set) var fitRequests: [FitRequest] = []

    func initialWindowFit(backingScale: CGFloat, visibleFrame: CGRect, minSize: CGSize, chrome: CGSize) -> CGRect? {
        fitRequests.append(FitRequest(minSize: minSize, chrome: chrome))
        return fits.isEmpty ? nil : fits.removeFirst()
    }

    func videoClickIntent(clickCount: Int) -> VideoClickIntent { WindowBehaviourTests.coreRules(clickCount) }
}

/// A held single click the test releases by hand, at most once.
private final class HeldClick: ClickTimer {
    private var body: (() -> Void)?

    init(body: @escaping () -> Void) { self.body = body }

    func cancel() { body = nil }

    func fire() {
        let body = self.body
        self.body = nil
        body?()
    }
}

/// The window behaviour of #881 on a real `PlayerWindow` and video view, laid
/// out like the app's (the control bar 48 points under the video): keys pressed
/// on the video, clicks on it, the session's fit, the resize rule and the
/// chrome that follows the OSC.
final class PlayerWindowControllerTests: XCTestCase {
    private var window: FlaggedWindow!
    private var controller: PlayerWindowController!
    private var video: VideoView!
    private var rules: StandInRules!
    private var held: [HeldClick] = []
    private var fullscreenToggles = 0
    private var pauses = 0

    private let wide = VideoGeometry(width: 640, height: 360, displayAspect: 16 / 9)

    override func setUpWithError() throws {
        _ = NSApplication.shared
        window = FlaggedWindow(contentRect: NSRect(x: 0, y: 0, width: 960, height: 580), styleMask: PlayerWindow.style,
                               backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 420, height: 280)
        controller = PlayerWindowController(window: window)
        controller.toggleFullscreen = { [unowned self] in self.fullscreenToggles += 1 }
        let surface = try XCTUnwrap(GLSurface.make(), "no OpenGL pixel format for the video view")
        video = VideoView(surface: surface)
        let root = NSView()
        window.contentView = root
        video.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(video)
        NSLayoutConstraint.activate([
            video.topAnchor.constraint(equalTo: root.topAnchor),
            video.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            video.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            video.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -48),
        ])
        controller.install(in: root, videoArea: video)
        rules = StandInRules()
        controller.connect(rules, togglePause: { [unowned self] in self.pauses += 1 }) { [unowned self] _, body in
            let click = HeldClick(body: body)
            self.held.append(click)
            return click
        }
    }

    override func tearDown() {
        video?.shutdown()
        window?.close()
        controller = nil
        window = nil
        video = nil
    }

    private func key(_ characters: String, keyCode: UInt16, modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
        try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0,
                                       windowNumber: window.windowNumber, context: nil, characters: characters,
                                       charactersIgnoringModifiers: characters, isARepeat: false, keyCode: keyCode))
    }

    private func release(clickCount: Int) throws {
        let event = try XCTUnwrap(NSEvent.mouseEvent(with: .leftMouseUp, location: NSPoint(x: 100, y: 200),
                                                     modifierFlags: [], timestamp: 0,
                                                     windowNumber: window.windowNumber, context: nil, eventNumber: 0,
                                                     clickCount: clickCount, pressure: 0))
        video.mouseUp(with: event)
    }

    private func playing(_ video: VideoGeometry?) -> SessionSnapshot {
        SessionSnapshot(status: video == nil ? .opening : .playing, title: "clip.mp4", video: video)
    }

    private var contentFrame: CGRect { window.contentRect(forFrameRect: window.frame) }

    // MARK: Fullscreen command routing

    func testFPressedOnTheVideoTogglesFullscreenInAWindowAndInFullscreen() throws {
        XCTAssertTrue(window.makeFirstResponder(video))
        window.sendEvent(try key("f", keyCode: 3))
        XCTAssertEqual(fullscreenToggles, 1)
        window.flagged = true
        window.sendEvent(try key("f", keyCode: 3))
        XCTAssertEqual(fullscreenToggles, 2, "F leaves fullscreen too")
    }

    func testEscapePressedOnTheVideoLeavesFullscreenAndNeverEntersIt() throws {
        XCTAssertTrue(window.makeFirstResponder(video))
        window.sendEvent(try key("\u{1b}", keyCode: WindowKeys.escapeKeyCode))
        XCTAssertEqual(fullscreenToggles, 0, "Escape in a window must not enter fullscreen")
        window.flagged = true
        window.sendEvent(try key("\u{1b}", keyCode: WindowKeys.escapeKeyCode))
        XCTAssertEqual(fullscreenToggles, 1)
    }

    func testControlCommandFIsTheWindowsFullScreenKeyEquivalent() throws {
        XCTAssertTrue(window.makeFirstResponder(video))
        XCTAssertTrue(window.performKeyEquivalent(with: try key("f", keyCode: 3, modifiers: [.command, .control])))
        XCTAssertEqual(fullscreenToggles, 1)
        XCTAssertFalse(window.performKeyEquivalent(with: try key("f", keyCode: 3, modifiers: [.command])),
                       "Command-F is Find")
        XCTAssertEqual(fullscreenToggles, 1)
    }

    // MARK: Clicks on the video

    func testASingleClickPausesAfterTheIntervalAndADoubleClickOnlyTogglesFullscreen() throws {
        try release(clickCount: 1)
        XCTAssertEqual(pauses, 0, "the pause waits for a possible second click")
        XCTAssertEqual(held.count, 1)
        held[0].fire()
        XCTAssertEqual(pauses, 1)

        try release(clickCount: 1)
        try release(clickCount: 2)
        XCTAssertEqual(fullscreenToggles, 1)
        for click in held { click.fire() }
        XCTAssertEqual(pauses, 1, "the double click's first click does not pause")
    }

    // MARK: Fit and aspect

    func testTheWindowTakesTheFitTheSessionOwesWithTheControlBarsRoom() {
        let fit = CGRect(x: 200, y: 150, width: 640, height: 408)
        rules.fits = [fit]
        controller.sessionChanged(playing(nil))
        XCTAssertTrue(rules.fitRequests.isEmpty, "nothing is asked before the video size is known")

        controller.sessionChanged(playing(wide))
        XCTAssertEqual(rules.fitRequests.count, 1)
        XCTAssertEqual(rules.fitRequests.first?.chrome, CGSize(width: 0, height: 48), "the bar under the video")
        XCTAssertEqual(rules.fitRequests.first?.minSize,
                       window.contentRect(forFrameRect: NSRect(origin: .zero, size: window.minSize)).size)
        XCTAssertEqual(contentFrame, fit)
        XCTAssertEqual(controller.lastFit, fit)
        window.contentView?.layoutSubtreeIfNeeded()
        XCTAssertEqual(video.bounds.size, CGSize(width: 640, height: 360), "the video view has the video's size")
        XCTAssertEqual(controller.titleStrip.title, "clip.mp4")

        // Every batch with video asks; the session has no second fit for the source.
        controller.sessionChanged(playing(wide))
        XCTAssertEqual(rules.fitRequests.count, 2)
        XCTAssertEqual(contentFrame, fit)
    }

    func testAFitOwedInFullscreenIsSpentWithoutResizing() {
        window.flagged = true
        let before = window.frame
        rules.fits = [CGRect(x: 200, y: 150, width: 640, height: 408)]
        controller.sessionChanged(playing(wide))
        XCTAssertEqual(rules.fitRequests.count, 1)
        XCTAssertTrue(rules.fits.isEmpty, "the fit is taken")
        XCTAssertEqual(window.frame, before)
        XCTAssertNil(controller.lastFit)
    }

    func testAResizeKeepsTheVideoViewAtTheVideosAspectWhileVideoShows() {
        rules.fits = [CGRect(x: 200, y: 150, width: 640, height: 408)]
        controller.sessionChanged(playing(wide))
        for proposed in [NSSize(width: 1000, height: 500), NSSize(width: 700, height: 650)] {
            let size = controller.windowWillResize(to: proposed)
            window.setFrame(NSRect(origin: window.frame.origin, size: size), display: false)
            window.contentView?.layoutSubtreeIfNeeded()
            XCTAssertEqual(video.bounds.width, video.bounds.height * 16 / 9, accuracy: 1, "\(video.bounds.size)")
        }

        window.flagged = true
        XCTAssertEqual(controller.windowWillResize(to: NSSize(width: 777, height: 333)),
                       NSSize(width: 777, height: 333), "fullscreen sizes are the system's")
        window.flagged = false
        controller.sessionChanged(playing(nil))
        XCTAssertEqual(controller.windowWillResize(to: NSSize(width: 777, height: 333)),
                       NSSize(width: 777, height: 333), "without video a resize is free")
    }

    // MARK: Chrome

    private var trafficLights: [NSButton] {
        [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton]
            .compactMap { window.standardWindowButton($0) }
    }

    func testTrafficLightsAndTitleHiddenWithTheOSCAreHiddenNotOnlyTransparent() {
        XCTAssertEqual(trafficLights.count, 3)
        XCTAssertFalse(trafficLights.contains { $0.isHidden })
        controller.oscVisibilityChanged(false, animated: false)
        for button in trafficLights {
            XCTAssertTrue(button.isHidden, "a transparent button would still take the click")
            XCTAssertEqual(button.alphaValue, 0)
        }
        XCTAssertTrue(controller.titleStrip.isHidden)

        controller.oscVisibilityChanged(true, animated: false)
        for button in trafficLights {
            XCTAssertFalse(button.isHidden)
            XCTAssertEqual(button.alphaValue, 1)
        }
        XCTAssertFalse(controller.titleStrip.isHidden)
    }

    func testAFadeOutHidesTheLightsWhenItEndsUnlessTheOSCCameBack() throws {
        let close = try XCTUnwrap(window.standardWindowButton(.closeButton))
        controller.oscVisibilityChanged(false, animated: true)
        wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in close.isHidden }, object: nil)],
             timeout: 3)

        controller.oscVisibilityChanged(true, animated: true)
        XCTAssertFalse(close.isHidden, "a fade-in unhides first")
        controller.oscVisibilityChanged(false, animated: true)
        controller.oscVisibilityChanged(true, animated: true)
        let settled = expectation(description: "both fades have ended")
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { settled.fulfill() }
        wait(for: [settled], timeout: 3)
        XCTAssertFalse(close.isHidden, "the fade-out that ended after the OSC came back leaves the lights")
        XCTAssertFalse(controller.titleStrip.isHidden)
    }
}
