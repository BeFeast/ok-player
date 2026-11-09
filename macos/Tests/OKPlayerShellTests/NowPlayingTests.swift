import AppKit
import MediaPlayer
import XCTest
@testable import OKPlayerShell

/// Now Playing comes from the session snapshot alone: what it shows for each state,
/// and when it is written.
final class NowPlayingTests: XCTestCase {
    private func info(_ snapshot: SessionSnapshot, queue: QueuePosition = .single,
                      file: StaticString = #filePath, line: UInt = #line) throws -> (NowPlayingState, [String: Any]) {
        let state = try XCTUnwrap(NowPlayingState(snapshot: snapshot, queue: queue), file: file, line: line)
        return (state, NowPlayingInfo.dictionary(state, artwork: nil))
    }

    func testAPlayingVideoFillsEveryNowPlayingField() throws {
        let snapshot = Snapshots.media(.playing, time: 30, duration: 120, speed: 1.5, title: "Night Train", chapters: 4)
        let (state, info) = try self.info(snapshot, queue: QueuePosition(index: 2, count: 5))
        XCTAssertEqual(state.playback, .playing)
        XCTAssertEqual(info[MPMediaItemPropertyTitle] as? String, "Night Train")
        XCTAssertEqual(info[MPMediaItemPropertyPlaybackDuration] as? Double, 120)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyElapsedPlaybackTime] as? Double, 30)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyPlaybackRate] as? Double, 1.5)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyDefaultPlaybackRate] as? Double, 1.5)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyMediaType] as? UInt, MPNowPlayingInfoMediaType.video.rawValue)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyPlaybackQueueIndex] as? Int, 2)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyPlaybackQueueCount] as? Int, 5)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyChapterCount] as? Int, 4)
        XCTAssertNil(info[MPMediaItemPropertyArtwork], "no artwork was given")
    }

    func testWithoutAPlaylistTheOpenFileIsTheOnlyItem() throws {
        let (_, info) = try self.info(Snapshots.media(.playing))
        XCTAssertEqual(info[MPNowPlayingInfoPropertyPlaybackQueueIndex] as? Int, 0)
        XCTAssertEqual(info[MPNowPlayingInfoPropertyPlaybackQueueCount] as? Int, 1)
    }

    func testThePositionStandsStillWhilePausedOpeningOrBuffering() throws {
        let paused = try info(Snapshots.media(.paused, speed: 1.25))
        XCTAssertEqual(paused.0.playback, .paused)
        XCTAssertEqual(paused.1[MPNowPlayingInfoPropertyPlaybackRate] as? Double, 0)
        XCTAssertEqual(paused.1[MPNowPlayingInfoPropertyDefaultPlaybackRate] as? Double, 1.25)

        let buffering = try info(Snapshots.media(.playing, buffering: true))
        XCTAssertEqual(buffering.0.playback, .playing)
        XCTAssertEqual(buffering.1[MPNowPlayingInfoPropertyPlaybackRate] as? Double, 0)

        let opening = try info(Snapshots.media(.opening, time: nil, duration: nil, video: false))
        XCTAssertEqual(opening.0.playback, .playing)
        XCTAssertEqual(opening.1[MPNowPlayingInfoPropertyPlaybackRate] as? Double, 0)
        XCTAssertEqual(opening.1[MPNowPlayingInfoPropertyElapsedPlaybackTime] as? Double, 0)
        XCTAssertNil(opening.1[MPMediaItemPropertyPlaybackDuration], "an unknown length is left out")
    }

    func testTheElapsedTimeStaysWithinTheMedia() throws {
        XCTAssertEqual(try info(Snapshots.media(.playing, time: 130, duration: 120)).0.elapsed, 120)
        XCTAssertEqual(try info(Snapshots.media(.playing, time: -2, duration: 120)).0.elapsed, 0)
        XCTAssertEqual(try info(Snapshots.media(.playing, time: .nan, duration: .infinity)).0.elapsed, 0)
        XCTAssertNil(try info(Snapshots.media(.playing, duration: .infinity)).0.duration)
    }

    func testTheTitleFallsBackToTheFileOrLinkName() throws {
        XCTAssertEqual(try info(Snapshots.media(.playing, title: nil)).0.title, "Film.mkv")
        XCTAssertEqual(try info(Snapshots.media(.playing, title: "  \n")).0.title, "Film.mkv")
        let link = Snapshots.media(.playing, path: "https://example.com/live/stream.m3u8", kind: .url, title: nil)
        XCTAssertEqual(try info(link).0.title, "stream.m3u8")
    }

    func testAudioFilesAreAudioEvenWithCoverArtAndVideoNeedsAPicture() throws {
        let song = Snapshots.media(.playing, path: "/Users/me/Music/Song.mp3", video: true)
        XCTAssertEqual(try info(song).1[MPNowPlayingInfoPropertyMediaType] as? UInt, MPNowPlayingInfoMediaType.audio.rawValue,
                       "embedded cover art reaches libmpv as a video frame")
        let soundtrack = Snapshots.media(.playing, path: "/Users/me/Movies/Soundtrack.mkv", video: false)
        XCTAssertEqual(soundtrack.mediaKind, .audio)
        XCTAssertEqual(Snapshots.media(.playing, path: "/Users/me/Movies/Film.mp4", video: true).mediaKind, .video)
    }

    func testNoMediaMeansNoNowPlaying() {
        XCTAssertNil(NowPlayingState(snapshot: Snapshots.idle))
        XCTAssertNil(NowPlayingState(snapshot: Snapshots.media(.ended)))
        var noSource = Snapshots.media(.playing)
        noSource.source = nil
        XCTAssertNil(NowPlayingState(snapshot: noSource))
    }

    func testArtworkGoesOutWithTheInfo() throws {
        let image = NSImage(size: NSSize(width: 8, height: 8))
        let artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
        let state = try XCTUnwrap(NowPlayingState(snapshot: Snapshots.media(.playing)))
        XCTAssertTrue(NowPlayingInfo.dictionary(state, artwork: artwork)[MPMediaItemPropertyArtwork] as? MPMediaItemArtwork === artwork)
    }

    // MARK: When it is written

    private var now: TimeInterval = 1000
    private lazy var center = RecordingNowPlayingCenter()
    private lazy var publisher = NowPlayingPublisher(center: center, artwork: nil, clock: { [unowned self] in self.now })

    private func publish(_ snapshot: SessionSnapshot, at time: TimeInterval) {
        now = time
        publisher.update(NowPlayingState(snapshot: snapshot))
    }

    func testTimeUpdatesAreLeftToTheSystemButSeeksAreWritten() {
        publish(Snapshots.media(.playing, time: 10), at: 1000)
        XCTAssertEqual(center.writes.count, 1)
        // Ten updates a second while the position moves as the system expects.
        for step in 1...50 {
            publish(Snapshots.media(.playing, time: 10 + Double(step) * 0.1), at: 1000 + Double(step) * 0.1)
        }
        XCTAssertEqual(center.writes.count, 1, "elapsed time and rate let the system move the position itself")
        publish(Snapshots.media(.playing, time: 60), at: 1005.1)
        XCTAssertEqual(center.writes.count, 2, "a seek forward is written")
        XCTAssertEqual(center.last?.info?[MPNowPlayingInfoPropertyElapsedPlaybackTime] as? Double, 60)
        publish(Snapshots.media(.playing, time: 55.2), at: 1005.2)
        XCTAssertEqual(center.writes.count, 3, "so is a seek back")
    }

    func testAPositionNearTheExtrapolationIsNotASeek() {
        publish(Snapshots.media(.playing, time: 10, speed: 2), at: 1000)
        publish(Snapshots.media(.playing, time: 14 + NowPlayingPublisher.seekTolerance * 0.9, speed: 2), at: 1002)
        XCTAssertEqual(center.writes.count, 1, "at speed 2 the system expects 14 s after two seconds")
        publish(Snapshots.media(.playing, time: 14 + NowPlayingPublisher.seekTolerance * 1.1, speed: 2), at: 1002)
        XCTAssertEqual(center.writes.count, 2)
    }

    func testStateChangesAreWrittenAndPausedUpdatesAreNot() {
        publish(Snapshots.media(.playing, time: 10), at: 1000)
        publish(Snapshots.media(.paused, time: 11), at: 1001)
        XCTAssertEqual(center.writes.count, 2)
        XCTAssertEqual(center.last?.playbackState, .paused)
        publish(Snapshots.media(.paused, time: 11), at: 1004)
        XCTAssertEqual(center.writes.count, 2, "a paused position does not move")
        publish(Snapshots.media(.paused, time: 11, speed: 1.5), at: 1005)
        XCTAssertEqual(center.writes.count, 3, "a speed change is written")
        publish(Snapshots.media(.playing, path: "/Users/me/Movies/Next.mkv", time: 0, title: "Next"), at: 1006)
        XCTAssertEqual(center.writes.count, 4)
        XCTAssertEqual(center.last?.info?[MPMediaItemPropertyTitle] as? String, "Next")
        XCTAssertEqual(center.last?.playbackState, .playing)
    }

    func testClosingClearsNowPlayingOnce() {
        publish(Snapshots.idle, at: 999)
        XCTAssertTrue(center.writes.isEmpty, "nothing was published, so nothing is cleared")
        publish(Snapshots.media(.playing), at: 1000)
        publish(Snapshots.idle, at: 1001)
        publish(Snapshots.idle, at: 1002)
        XCTAssertEqual(center.writes.count, 2)
        XCTAssertNil(center.last?.info)
        XCTAssertEqual(center.last?.playbackState, .stopped)
        publish(Snapshots.media(.playing), at: 1003)
        XCTAssertEqual(center.writes.count, 3, "the same file opened again is published again")
    }
}
