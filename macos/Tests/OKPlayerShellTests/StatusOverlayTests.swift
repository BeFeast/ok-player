import AppKit
import XCTest
@testable import OKPlayerShell

/// Stands in for the video: the clicks that reach it, and keyboard focus, which it
/// takes like the video view.
private final class ClickTarget: NSView {
    override var acceptsFirstResponder: Bool { true }
}

/// A key view that takes focus whatever the keyboard navigation setting, where a
/// card button takes it only with keyboard navigation on.
private final class FocusableView: NSView {
    override var acceptsFirstResponder: Bool { true }
}

/// The status layer over real AppKit views, in a window that is never shown: what
/// each state draws, which clicks the card takes, what its buttons do, and the toast.
final class StatusOverlayTests: XCTestCase {
    private var window: NSWindow!
    private var root: NSView!
    private var video: ClickTarget!
    private var timer: ManualHideTimer!
    private var layer: StatusLayer!
    private var surface = StatusSurface.clear
    private var retried: [MediaSource] = []
    private var openedAnother = 0
    private var copied: [String] = []
    private var toasted: [String] = []

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
        let frame = NSRect(x: 0, y: 0, width: 800, height: 450)
        window = NSWindow(contentRect: frame, styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        root = NSView(frame: frame)
        window.contentView = root
        video = ClickTarget(frame: root.bounds)
        root.addSubview(video)
        timer = ManualHideTimer()
        layer = StatusLayer(timer: timer)
        layer.install(in: root, above: video)
        layer.overlay.focusReturn = video
        layer.query = { [unowned self] in self.surface }
        layer.onRetry = { [unowned self] source in self.retried.append(source) }
        layer.onOpenAnother = { [unowned self] in self.openedAnother += 1 }
        layer.copy = { [unowned self] text in self.copied.append(text) }
        layer.onToast = { [unowned self] text in self.toasted.append(text) }
        retried = []
        openedAnother = 0
        copied = []
        toasted = []
    }

    override func tearDown() {
        window.close()
        super.tearDown()
    }

    private func show(_ surface: StatusSurface) {
        self.surface = surface
        layer.refresh()
        root.layoutSubtreeIfNeeded()
    }

    /// What a click at `point` in the root view lands on (`hitTest` takes the
    /// point in the superview's coordinates).
    private func hit(at point: NSPoint) -> NSView? {
        root.hitTest(root.convert(point, to: root.superview))
    }

    private func hit(_ view: NSView) -> NSView? {
        hit(at: view.convert(NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: root))
    }

    func testAFailureShowsTheSessionsCardWithItsButtonsInOrderAndTakesItsClicksOnly() throws {
        show(.failed(card: StatusTests.card))
        let overlay = layer.overlay
        XCTAssertFalse(overlay.cardPanel.isHidden)
        XCTAssertTrue(overlay.progressPanel.isHidden)
        XCTAssertEqual(overlay.titleLabel.stringValue, "Playback failed")
        XCTAssertEqual(overlay.messageLabel.stringValue, "OK Player could not open this source.")
        XCTAssertEqual(overlay.buttons.map { $0.action }, [.retry, .openAnother, .copyDetails])
        XCTAssertEqual(overlay.buttons.map { $0.button.title }, ["Retry", "Open another", "Copy details"])
        // A click on the card stays on the card; anywhere else it reaches the video.
        let retry = try XCTUnwrap(overlay.button(for: .retry))
        XCTAssertTrue(hit(retry) === retry)
        XCTAssertTrue(hit(at: NSPoint(x: 10, y: 10)) === video)
    }

    func testTheCardsButtonsRetryItsSourceOpenAnotherAndCopyItsDetails() throws {
        show(.failed(card: StatusTests.card))
        let overlay = layer.overlay
        try XCTUnwrap(overlay.button(for: .retry)).performClick(nil)
        XCTAssertEqual(retried, [StatusTests.card.source])
        try XCTUnwrap(overlay.button(for: .openAnother)).performClick(nil)
        XCTAssertEqual(openedAnother, 1)
        try XCTUnwrap(overlay.button(for: .copyDetails)).performClick(nil)
        XCTAssertEqual(copied, [StatusTests.card.detail])
        XCTAssertEqual(toasted, ["Copied details"])
    }

    func testLoadingAndBufferingShowASpinnerThatClicksPassThrough() {
        show(.loading(label: "Opening film.mkv"))
        let overlay = layer.overlay
        XCTAssertFalse(overlay.progressPanel.isHidden)
        XCTAssertTrue(overlay.cardPanel.isHidden)
        XCTAssertEqual(overlay.progressLabel.stringValue, "Opening film.mkv")
        XCTAssertTrue(hit(overlay.progressPanel) === video, "a double click on the video still reaches it")
        show(.buffering(percent: 45, label: "Buffering 45%"))
        XCTAssertEqual(overlay.progressLabel.stringValue, "Buffering 45%")
        show(.clear)
        XCTAssertTrue(overlay.progressPanel.isHidden)
        XCTAssertTrue(overlay.cardPanel.isHidden)
        XCTAssertEqual(layer.display, .clear)
    }

    func testAShellFailureShowsACardWithoutActionsOverAnySurface() {
        show(.loading(label: "Opening film.mkv"))
        layer.showShellFailure("The playback engine stopped")
        let overlay = layer.overlay
        XCTAssertFalse(overlay.cardPanel.isHidden)
        XCTAssertEqual(overlay.titleLabel.stringValue, StatusRules.shellFailureTitle)
        XCTAssertEqual(overlay.messageLabel.stringValue, "The playback engine stopped")
        XCTAssertTrue(overlay.buttons.isEmpty)
        // Later surfaces, opens and failures do not replace it.
        show(.clear)
        layer.opened(.accepted(requestId: 7))
        XCTAssertFalse(layer.showShellFailure("Video rendering failed", clearsOnOpen: true))
        XCTAssertEqual(layer.display, .shellFailure("The playback engine stopped"))
    }

    func testARendererFailureStaysUntilTheNextAcceptedOpenThenTheSessionsSurfaceShows() {
        show(.loading(label: "Opening film.mkv"))
        XCTAssertTrue(layer.showShellFailure("Video rendering failed", clearsOnOpen: true))
        let overlay = layer.overlay
        root.layoutSubtreeIfNeeded()
        XCTAssertEqual(layer.display, .shellFailure("Video rendering failed"))
        XCTAssertTrue(overlay.buttons.isEmpty)
        let cardCenter = overlay.cardPanel.convert(
            NSPoint(x: overlay.cardPanel.bounds.midX, y: overlay.cardPanel.bounds.midY), to: root)
        // Playback goes on under it until an open.
        show(.clear)
        XCTAssertEqual(layer.display, .shellFailure("Video rendering failed"))
        // An open the session rejects leaves it and toasts the reason.
        layer.opened(.rejected(reason: .unavailable, message: "The engine is not available"))
        XCTAssertEqual(layer.display, .shellFailure("Video rendering failed"))
        XCTAssertEqual(toasted, ["The engine is not available"])
        // The next accepted open shows the session's surface for the new source.
        surface = .loading(label: "Opening next.mkv")
        layer.opened(.accepted(requestId: 8))
        root.layoutSubtreeIfNeeded()
        XCTAssertEqual(layer.display, .progress("Opening next.mkv"))
        XCTAssertTrue(overlay.cardPanel.isHidden)
        XCTAssertTrue(hit(at: cardCenter) === video, "clicks on the video reach it again")
        show(.failed(card: StatusTests.card))
        XCTAssertEqual(layer.display, .failure(StatusTests.card))
        // An engine that quits after a renderer failure stays for good.
        XCTAssertTrue(layer.showShellFailure("Video rendering failed", clearsOnOpen: true))
        XCTAssertTrue(layer.showShellFailure("The playback engine stopped"))
        layer.opened(.accepted(requestId: 9))
        XCTAssertEqual(layer.display, .shellFailure("The playback engine stopped"))
    }

    /// Puts keyboard focus on the card: on its Retry button when the host has
    /// keyboard navigation on, else on a key view placed in the card.
    private func focusTheCard() throws -> NSView {
        let overlay = layer.overlay
        let retry = try XCTUnwrap(overlay.button(for: .retry))
        if window.makeFirstResponder(retry) { return retry }
        let inCard = FocusableView(frame: NSRect(x: 0, y: 0, width: 4, height: 4))
        overlay.cardPanel.addSubview(inCard)
        XCTAssertTrue(window.makeFirstResponder(inCard))
        return inCard
    }

    func testFocusOnTheCardGoesBackToTheVideoWhenTheCardHidesOrItsButtonsChange() throws {
        show(.failed(card: StatusTests.card))
        let holder = try focusTheCard()
        XCTAssertTrue(window.firstResponder === holder)
        // Retry or Open another moves the card on to loading, which hides it.
        show(.loading(label: "Opening film.mkv"))
        XCTAssertTrue(window.firstResponder === video, "Space would not reach the video")
        // A second failure's card replaces the buttons.
        show(.failed(card: StatusTests.card))
        _ = try focusTheCard()
        var next = StatusTests.card
        next.kind = .missingCodec
        next.title = "Missing codec"
        show(.failed(card: next))
        XCTAssertTrue(window.firstResponder === video)
        // Focus outside the card stays where it is.
        let elsewhere = FocusableView(frame: NSRect(x: 0, y: 0, width: 4, height: 4))
        root.addSubview(elsewhere)
        XCTAssertTrue(window.makeFirstResponder(elsewhere))
        show(.clear)
        XCTAssertTrue(window.firstResponder === elsewhere)
    }

    func testABatchsNoticesShowAsAToastNearTheTopThatTakesNoClicks() {
        layer.sessionChanged(EventBatch(schemaVersion: 1, events: [
            .notice(text: "Volume 95%"),
            .error(kind: .loadFailed, message: "loading failed", requestId: nil),
            .notice(text: "Volume 90%"),
        ]))
        root.layoutSubtreeIfNeeded()
        let toast = layer.toastView
        XCTAssertEqual(toasted, ["Volume 95%", "Volume 90%"])
        XCTAssertFalse(toast.isHidden)
        XCTAssertEqual(toast.label.stringValue, "Volume 90%")
        XCTAssertGreaterThan(toast.convert(toast.bounds, to: root).minY, root.bounds.midY, "clear of captions")
        XCTAssertTrue(hit(toast) === video)
        timer.elapse()
        XCTAssertTrue(toast.isHidden)
        // Any part of the shell can show one the same way.
        layer.show(toast: "Speed 1.25×")
        XCTAssertEqual(toast.label.stringValue, "Speed 1.25×")
    }
}
