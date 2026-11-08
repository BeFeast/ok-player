import MediaPlayer
import XCTest
@testable import OKPlayerShell

/// Media keys, Control Center and AirPods: every remote command becomes a session
/// command, and the session's answer becomes the handler status.
final class RemoteCommandTests: XCTestCase {
    private let source = FakeRemoteCommandSource()
    private let session = SessionRecorder()
    private lazy var controller = RemoteCommandController(source: source, session: { [unowned self] in self.session.dispatch($0) })

    func testEveryRemoteCommandBecomesTheMatchingSessionCommand() throws {
        controller.start()
        let cases: [(RemoteCommand, Command)] = [
            (.play, .setPaused(paused: false)),
            (.pause, .setPaused(paused: true)),
            (.togglePlayPause, .togglePause),
            (.stop, .close),
            (.changePlaybackPosition(seconds: 42.5), .seek(seconds: 42.5, mode: .absolute, precision: .exact)),
            (.changePlaybackPosition(seconds: -3), .seek(seconds: 0, mode: .absolute, precision: .exact)),
            (.skipForward(seconds: 10), .seek(seconds: 10, mode: .relative, precision: .exact)),
            (.skipBackward(seconds: 10), .seek(seconds: -10, mode: .relative, precision: .exact)),
            (.changePlaybackRate(rate: 1.5), .setSpeed(speed: 1.5)),
        ]
        for (remote, _) in cases {
            XCTAssertEqual(try XCTUnwrap(source.fire(remote), "no handler installed"), .success, "\(remote)")
        }
        XCTAssertEqual(session.commands, cases.map { $0.1 })
    }

    func testRatesStayInTheSupportedRangeAndNonsenseNeverReachesTheSession() {
        controller.start()
        _ = source.fire(.changePlaybackRate(rate: 16))
        _ = source.fire(.changePlaybackRate(rate: 0.1))
        XCTAssertEqual(session.commands, [.setSpeed(speed: 4), .setSpeed(speed: 0.25)])
        for nonsense: RemoteCommand in [.changePlaybackRate(rate: 0), .changePlaybackRate(rate: .nan),
                                        .changePlaybackPosition(seconds: .infinity), .skipForward(seconds: 0),
                                        .skipBackward(seconds: -.infinity)] {
            XCTAssertEqual(source.fire(nonsense), .commandFailed, "\(nonsense)")
        }
        XCTAssertEqual(session.commands.count, 2)
    }

    func testTheSessionsAnswerBecomesTheHandlerStatus() {
        controller.start()
        let answers: [(DispatchResult, MPRemoteCommandHandlerStatus)] = [
            (.accepted(requestId: 3), .success),
            (.noOp, .success),
            (.rejected(reason: .noActiveMedia, message: "nothing is open"), .noActionableNowPlayingItem),
            (.rejected(reason: .notFinite, message: "not finite"), .commandFailed),
            (.failed(requestId: 4, message: "libmpv refused"), .commandFailed),
        ]
        for (reply, status) in answers {
            session.reply = reply
            XCTAssertEqual(source.fire(.togglePlayPause), status, "\(reply)")
        }
    }

    func testNextAndPreviousStepThePlaylistAndAreOfferedOnlyWhenItCanStep() {
        controller.start()
        XCTAssertEqual(source.offeredSteps.last?.next, false, "without a playlist there is nothing to step to")
        XCTAssertEqual(source.offeredSteps.last?.previous, false)
        XCTAssertEqual(source.fire(.nextTrack), .noSuchContent)

        let playlist = FakePlaylist()
        playlist.canGoPrevious = false
        controller.playlist = playlist
        controller.refresh()
        XCTAssertEqual(source.offeredSteps.last?.next, true)
        XCTAssertEqual(source.offeredSteps.last?.previous, false)
        XCTAssertEqual(source.fire(.nextTrack), .success)
        XCTAssertEqual(source.fire(.previousTrack), .noSuchContent)
        XCTAssertEqual(playlist.steps, [.next])
        XCTAssertTrue(session.commands.isEmpty, "playlist steps are not playback commands")

        let writes = source.offeredSteps.count
        controller.refresh()
        XCTAssertEqual(source.offeredSteps.count, writes, "an unchanged offer is not written again")
    }

    func testStoppingRemovesTheHandler() {
        controller.start()
        controller.start()
        controller.stop()
        XCTAssertNil(source.fire(.play))
        XCTAssertEqual(source.uninstalls, 1)
        controller.stop()
        XCTAssertEqual(source.uninstalls, 1)
    }
}
