import XCTest
@testable import OKPlayerShell

/// Which session reports become status states and toasts, and how long a toast stays.
final class StatusTests: XCTestCase {
    static let card = FailureCard(
        kind: .application, title: "Playback failed", message: "OK Player could not open this source.",
        detail: "OK Player could not open the media.\nPath: /Movies/missing.mkv\nReason: libmpv error -13",
        source: MediaSource(kind: .local, location: "/Movies/missing.mkv"),
        actions: [FailureActionView(action: .retry, label: "Retry"),
                  FailureActionView(action: .openAnother, label: "Open another"),
                  FailureActionView(action: .copyDetails, label: "Copy details")])

    func testEverySurfaceHasItsDisplayAndAShellFailureWins() {
        XCTAssertEqual(StatusRules.display(.clear, shellFailure: nil), .clear)
        XCTAssertEqual(StatusRules.display(.loading(label: "Opening film.mkv"), shellFailure: nil),
                       .progress("Opening film.mkv"))
        XCTAssertEqual(StatusRules.display(.buffering(percent: 45, label: "Buffering 45%"), shellFailure: nil),
                       .progress("Buffering 45%"))
        XCTAssertEqual(StatusRules.display(.failed(card: Self.card), shellFailure: nil), .failure(Self.card))
        XCTAssertEqual(StatusRules.display(.failed(card: Self.card), shellFailure: "The playback engine stopped"),
                       .shellFailure("The playback engine stopped"))
    }

    func testNoticesAndErrorsWithoutACardBecomeToastsInOrder() {
        let batch = EventBatch(schemaVersion: 1, events: [
            .snapshotChanged(changes: [.volume(value: 95)]),
            .notice(text: "Volume 95%"),
            // The failure card shows this one.
            .error(kind: .loadFailed, message: "loading failed", requestId: nil),
            .error(kind: .commandFailed, message: "error running command", requestId: 4),
            .history(event: .resumed(position: 12)),
            .notice(text: "Resumed at 00:12"),
        ])
        XCTAssertEqual(StatusRules.toasts(in: batch), ["Volume 95%", "error running command", "Resumed at 00:12"])
        XCTAssertEqual(StatusRules.toasts(in: EventBatch(schemaVersion: 1, events: [])), [])
    }

    func testAToastStaysForItsDwellAndANewerOneReplacesItAndRestartsTheTime() {
        let timer = ManualHideTimer()
        let toast = Toast(timer: timer)
        var shown: [String?] = []
        toast.onChange = { shown.append($0) }

        toast.show("Volume 95%")
        XCTAssertEqual(toast.text, "Volume 95%")
        XCTAssertEqual(timer.delay, Toast.dwell)
        toast.show("Volume 90%")
        XCTAssertEqual(toast.text, "Volume 90%")
        XCTAssertEqual(timer.scheduled, 2, "the newer toast starts its own time")
        timer.elapse()
        XCTAssertNil(toast.text)
        XCTAssertEqual(shown, ["Volume 95%", "Volume 90%", nil])
        // Dismissing again changes nothing.
        toast.dismiss()
        XCTAssertEqual(shown.count, 3)
    }

    func testAToastIsOneLineAndAnEmptyOneIsNotShown() {
        let timer = ManualHideTimer()
        let toast = Toast(timer: timer)
        toast.show("Couldn't open\n/screens/one\ntwo.png\t(2)")
        XCTAssertEqual(toast.text, "Couldn't open /screens/one two.png (2)")
        toast.show("Plays next\u{2028}after this")
        XCTAssertEqual(toast.text, "Plays next after this")
        // A joiner inside an emoji is not a line break.
        XCTAssertEqual(Toast.oneLine("Up next: 👩‍💻.mkv"), "Up next: 👩‍💻.mkv")
        toast.dismiss()
        toast.show(" \n ")
        XCTAssertNil(toast.text)
        XCTAssertFalse(timer.isPending)
    }
}
