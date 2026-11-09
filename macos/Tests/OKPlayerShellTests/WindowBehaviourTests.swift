import AppKit
import XCTest
@testable import OKPlayerShell

/// A timer the test fires by hand.
private final class ManualTimer: ClickTimer {
    let delay: TimeInterval
    private let body: () -> Void
    private(set) var cancelled = false

    init(delay: TimeInterval, body: @escaping () -> Void) {
        self.delay = delay
        self.body = body
    }

    func cancel() { cancelled = true }

    func fire() {
        if !cancelled { body() }
    }
}

/// The player window's decisions (#881): which keys and clicks toggle
/// fullscreen, how clicks become pause or fullscreen, what size a resize settles
/// on and what the chrome shows. PlayerWindowControllerTests drives the same
/// rules through a real window.
final class WindowBehaviourTests: XCTestCase {
    // MARK: Fullscreen command routing

    func testFTogglesFullscreenInAWindowAndInFullscreen() {
        for fullscreen in [false, true] {
            XCTAssertEqual(WindowKeys.command(forKey: "f", keyCode: 3, modifiers: [], isRepeat: false,
                                              isFullscreen: fullscreen), .toggleFullscreen)
        }
    }

    func testOnlyAPlainFirstPressOfFIsAWindowKey() {
        XCTAssertNil(WindowKeys.command(forKey: "f", keyCode: 3, modifiers: [], isRepeat: true, isFullscreen: false),
                     "holding F must not flip fullscreen back and forth")
        for modifiers: NSEvent.ModifierFlags in [.command, .shift, .option, .control] {
            XCTAssertNil(WindowKeys.command(forKey: "f", keyCode: 3, modifiers: modifiers, isRepeat: false,
                                            isFullscreen: false), "\(modifiers)")
        }
        XCTAssertNil(WindowKeys.command(forKey: "g", keyCode: 5, modifiers: [], isRepeat: false, isFullscreen: false))
        XCTAssertEqual(WindowKeys.command(forKey: "f", keyCode: 3, modifiers: [.capsLock, .function], isRepeat: false,
                                          isFullscreen: false), .toggleFullscreen, "lock and fn are not chords")
    }

    func testEscapeLeavesFullscreenAndIsNotTakenInAWindow() {
        XCTAssertEqual(WindowKeys.command(forKey: "\u{1b}", keyCode: WindowKeys.escapeKeyCode, modifiers: [],
                                          isRepeat: false, isFullscreen: true), .exitFullscreen)
        XCTAssertNil(WindowKeys.command(forKey: "\u{1b}", keyCode: WindowKeys.escapeKeyCode, modifiers: [],
                                        isRepeat: false, isFullscreen: false))
    }

    func testControlCommandFIsTheFullScreenKeyEquivalent() {
        XCTAssertEqual(WindowKeys.command(forKeyEquivalent: "f", modifiers: [.command, .control]), .toggleFullscreen)
        XCTAssertNil(WindowKeys.command(forKeyEquivalent: "f", modifiers: [.command]), "Command-F is Find")
        XCTAssertNil(WindowKeys.command(forKeyEquivalent: "f", modifiers: [.command, .control, .shift]))
        XCTAssertNil(WindowKeys.command(forKeyEquivalent: "g", modifiers: [.command, .control]))
    }

    func testLeavingFullscreenDoesNothingInAWindowAndTogglingAlwaysToggles() {
        XCTAssertTrue(WindowCommand.exitFullscreen.togglesFullscreen(isFullscreen: true))
        XCTAssertFalse(WindowCommand.exitFullscreen.togglesFullscreen(isFullscreen: false),
                       "Escape in a window must not enter fullscreen")
        XCTAssertTrue(WindowCommand.toggleFullscreen.togglesFullscreen(isFullscreen: false))
        XCTAssertTrue(WindowCommand.toggleFullscreen.togglesFullscreen(isFullscreen: true))
    }

    // MARK: Clicks on the video

    /// An arbiter with core's click rules and hand-fired timers.
    private func makeArbiter(classify: @escaping (Int) -> VideoClickIntent = WindowBehaviourTests.coreRules)
        -> (VideoClickArbiter, timers: () -> [ManualTimer], pauses: () -> Int, fullscreens: () -> Int) {
        var timers: [ManualTimer] = []
        var pauses = 0
        var fullscreens = 0
        let arbiter = VideoClickArbiter(interval: { 0.35 }, classify: classify) { delay, body in
            let timer = ManualTimer(delay: delay, body: body)
            timers.append(timer)
            return timer
        }
        arbiter.onTogglePause = { pauses += 1 }
        arbiter.onToggleFullscreen = { fullscreens += 1 }
        return (arbiter, { timers }, { pauses }, { fullscreens })
    }

    /// okp-core `video_click::release_intent`, which the app reaches through the
    /// session (WindowSessionQueryTests checks the real answers).
    static func coreRules(_ count: Int) -> VideoClickIntent {
        switch count {
        case 1: return .schedulePlayPause
        case 2: return .cancelPlayPauseAndToggleFullscreen
        default: return .ignore
        }
    }

    func testASingleClickTogglesPauseOnlyAfterTheDoubleClickInterval() {
        let (arbiter, timers, pauses, fullscreens) = makeArbiter()
        arbiter.released(clickCount: 1)
        XCTAssertEqual(pauses(), 0, "the pause waits for a possible second click")
        XCTAssertTrue(arbiter.pausePending)
        XCTAssertEqual(timers().map(\.delay), [0.35])
        timers()[0].fire()
        XCTAssertEqual(pauses(), 1)
        XCTAssertEqual(fullscreens(), 0)
        XCTAssertFalse(arbiter.pausePending)
    }

    func testADoubleClickTogglesFullscreenWithoutAStrayPause() {
        let (arbiter, timers, pauses, fullscreens) = makeArbiter()
        arbiter.released(clickCount: 1)
        arbiter.released(clickCount: 2)
        XCTAssertEqual(fullscreens(), 1)
        XCTAssertTrue(timers()[0].cancelled, "the first click's pause is dropped")
        for timer in timers() { timer.fire() }
        XCTAssertEqual(pauses(), 0)
        XCTAssertFalse(arbiter.pausePending)
    }

    func testATripleClickTogglesFullscreenOnce() {
        let (arbiter, timers, pauses, fullscreens) = makeArbiter()
        for count in 1...3 { arbiter.released(clickCount: count) }
        for timer in timers() { timer.fire() }
        XCTAssertEqual(fullscreens(), 1)
        XCTAssertEqual(pauses(), 0)
    }

    func testSeparateSingleClicksEachTogglePause() {
        let (arbiter, timers, pauses, _) = makeArbiter()
        arbiter.released(clickCount: 1)
        timers()[0].fire()
        arbiter.released(clickCount: 1)
        timers()[1].fire()
        XCTAssertEqual(pauses(), 2)
    }

    func testTheClassifierDecidesWhatAClickMeans() {
        // A session that cannot answer classifies every click as ignore.
        let (arbiter, timers, pauses, fullscreens) = makeArbiter(classify: { _ in .ignore })
        arbiter.released(clickCount: 1)
        arbiter.released(clickCount: 2)
        XCTAssertTrue(timers().isEmpty)
        XCTAssertEqual(pauses(), 0)
        XCTAssertEqual(fullscreens(), 0)
    }

    // MARK: Resize aspect

    private let bar = CGSize(width: 0, height: 48)
    private let minimum = CGSize(width: 420, height: 280)
    private let wide = CGSize(width: 640, height: 360)

    /// How far the video area (`size` less `chrome`) is from the aspect of `video`, in points of width.
    private func aspectError(_ size: CGSize, video: CGSize, chrome: CGSize) -> CGFloat {
        abs((size.width - chrome.width) - (size.height - chrome.height) * video.width / video.height)
    }

    func testAResizeKeepsTheVideoAreaAboveTheControlBarAtTheVideosAspect() {
        let current = CGSize(width: 640, height: 408)
        // A drag that is mostly sideways: the width leads.
        let wider = VideoAreaAspect.constrain(proposed: CGSize(width: 1000, height: 500), current: current,
                                              video: wide, chrome: bar, minimum: minimum)
        XCTAssertEqual(wider.width, 1000)
        XCTAssertLessThanOrEqual(aspectError(wider, video: wide, chrome: bar), 1, "\(wider)")
        // A drag on the bottom edge: the height leads and the width follows.
        let taller = VideoAreaAspect.constrain(proposed: CGSize(width: 640, height: 600), current: current,
                                               video: wide, chrome: bar, minimum: minimum)
        XCTAssertEqual(taller.height, 600)
        XCTAssertLessThanOrEqual(aspectError(taller, video: wide, chrome: bar), 1, "\(taller)")
        XCTAssertGreaterThan(taller.width, 640)
    }

    func testTheWholeContentKeepsTheAspectWhenTheWindowHasNoControlsOutsideTheVideo() {
        let size = VideoAreaAspect.constrain(proposed: CGSize(width: 1200, height: 500),
                                             current: CGSize(width: 640, height: 360), video: wide, chrome: .zero,
                                             minimum: minimum)
        XCTAssertEqual(size, CGSize(width: 1200, height: 675))
    }

    func testAResizeNeverGoesBelowTheMinimumAndKeepsTheAspectThere() {
        let small = VideoAreaAspect.constrain(proposed: CGSize(width: 300, height: 200),
                                              current: CGSize(width: 640, height: 408), video: wide, chrome: bar,
                                              minimum: minimum)
        XCTAssertGreaterThanOrEqual(small.width, minimum.width)
        XCTAssertGreaterThanOrEqual(small.height, minimum.height)
        XCTAssertLessThanOrEqual(aspectError(small, video: wide, chrome: bar), 1, "\(small)")
        let portrait = CGSize(width: 1080, height: 1920)
        let narrow = VideoAreaAspect.constrain(proposed: CGSize(width: 420, height: 300),
                                               current: CGSize(width: 500, height: 937), video: portrait, chrome: bar,
                                               minimum: minimum)
        XCTAssertGreaterThanOrEqual(narrow.width, minimum.width)
        XCTAssertGreaterThanOrEqual(narrow.height, minimum.height)
        XCTAssertLessThanOrEqual(aspectError(narrow, video: portrait, chrome: bar), 1, "\(narrow)")
    }

    func testOnlyAVideoWithASizeLocksTheAspect() {
        XCTAssertEqual(VideoAreaAspect.lock(for: VideoGeometry(width: 1920, height: 800, displayAspect: 2.4)),
                       CGSize(width: 1920, height: 800))
        XCTAssertNil(VideoAreaAspect.lock(for: nil))
        XCTAssertNil(VideoAreaAspect.lock(for: VideoGeometry(width: 0, height: 720, displayAspect: 0)))
        XCTAssertEqual(VideoAreaAspect.constrain(proposed: CGSize(width: 777, height: 333),
                                                 current: CGSize(width: 640, height: 408), video: .zero, chrome: bar,
                                                 minimum: minimum),
                       CGSize(width: 777, height: 333), "without a video size a resize is free")
    }

    // MARK: Screen areas

    func testAScreenAreaIsPassedAsTheWholePointsInsideIt() {
        XCTAssertEqual(WindowRect(inside: CGRect(x: 0.5, y: 24.3, width: 1511, height: 920.4)),
                       WindowRect(x: 1, y: 25, width: 1510, height: 919))
        XCTAssertEqual(WindowRect(inside: CGRect(x: -1920, y: 70, width: 1920, height: 1010)),
                       WindowRect(x: -1920, y: 70, width: 1920, height: 1010))
        XCTAssertEqual(WindowRect(x: 45, y: 73, width: 1421, height: 799).cgRect,
                       CGRect(x: 45, y: 73, width: 1421, height: 799))
    }

    // MARK: Chrome

    func testTheTrafficLightsAndTheTitleFollowTheOSCInAWindow() {
        var chrome = WindowChromeState()
        XCTAssertTrue(chrome.trafficLightsVisible)
        XCTAssertTrue(chrome.titleStripVisible)
        chrome.oscVisible = false
        XCTAssertFalse(chrome.trafficLightsVisible)
        XCTAssertFalse(chrome.titleStripVisible)
    }

    func testFullscreenLeavesTheTitleBarToTheSystem() {
        let chrome = WindowChromeState(oscVisible: false, fullscreen: true)
        XCTAssertTrue(chrome.trafficLightsVisible, "the system shows them with its title bar")
        XCTAssertFalse(chrome.titleStripVisible)
        XCTAssertFalse(WindowChromeState(oscVisible: true, fullscreen: true).titleStripVisible)
    }
}
