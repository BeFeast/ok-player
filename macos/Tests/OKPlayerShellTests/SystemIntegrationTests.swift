import AppKit
import MediaPlayer
import XCTest
@testable import OKPlayerShell

/// The display-sleep assertion, recent documents, File > Open Recent, the Dock menu,
/// and all of them driven together from session snapshots.
final class SystemIntegrationTests: XCTestCase {
    // MARK: Display sleep

    func testTheDisplayIsKeptAwakeOnlyWhileVideoPlays() {
        let power = FakePowerAssertions()
        let guardian = DisplaySleepGuard(power: power)
        guardian.update(Snapshots.idle)
        guardian.update(Snapshots.media(.opening, video: false))
        XCTAssertEqual(power.created, 0, "nothing plays yet")

        guardian.update(Snapshots.media(.playing))
        XCTAssertEqual(power.created, 1)
        XCTAssertEqual(power.held.count, 1)
        XCTAssertEqual(power.reasons, [DisplaySleepGuard.reason])
        guardian.update(Snapshots.media(.playing, time: 31))
        guardian.update(Snapshots.media(.playing, time: 32, buffering: true))
        XCTAssertEqual(power.created, 1, "one assertion for one stretch of playback")

        guardian.update(Snapshots.media(.paused))
        XCTAssertTrue(power.held.isEmpty, "a pause lets the display sleep")
        XCTAssertNil(guardian.assertion)
        guardian.update(Snapshots.media(.playing))
        XCTAssertEqual(power.held.count, 1)
        guardian.update(Snapshots.media(.ended))
        XCTAssertTrue(power.held.isEmpty, "the end of the file lets it sleep")
        guardian.update(Snapshots.media(.playing))
        guardian.update(Snapshots.idle)
        XCTAssertTrue(power.held.isEmpty, "closing lets it sleep")
        XCTAssertEqual(power.created, 3)
        XCTAssertEqual(power.released, 3)
    }

    func testAudioNeverKeepsTheDisplayAwake() {
        let power = FakePowerAssertions()
        let guardian = DisplaySleepGuard(power: power)
        guardian.update(Snapshots.media(.playing, path: "/Users/me/Music/Song.flac", video: false))
        guardian.update(Snapshots.media(.playing, path: "/Users/me/Music/Song.mp3", video: true))
        XCTAssertEqual(power.created, 0)
        guardian.update(Snapshots.media(.playing))
        guardian.update(Snapshots.media(.playing, path: "/Users/me/Music/Song.mp3", video: false))
        XCTAssertTrue(power.held.isEmpty, "switching to an audio file releases it")
    }

    func testARefusedAssertionIsTriedAgainAtTheNextStartNotOnEveryUpdate() {
        let power = FakePowerAssertions()
        power.refuses = true
        let guardian = DisplaySleepGuard(power: power)
        for second in 0..<20 { guardian.update(Snapshots.media(.playing, time: Double(second))) }
        XCTAssertEqual(power.reasons.count, 1)
        power.refuses = false
        guardian.update(Snapshots.media(.paused))
        XCTAssertEqual(power.released, 0, "nothing was held")
        guardian.update(Snapshots.media(.playing))
        XCTAssertEqual(power.held.count, 1)
    }

    // MARK: Recent documents

    func testEachLoadedLocalFileIsNotedOnce() {
        let list = FakeRecentDocumentList()
        let recents = RecentDocuments(list: list)
        recents.update(Snapshots.media(.opening, path: "/Users/me/Movies/A.mkv"))
        XCTAssertTrue(list.noted.isEmpty, "a file is noted once it has loaded")
        recents.update(Snapshots.media(.playing, path: "/Users/me/Movies/A.mkv"))
        recents.update(Snapshots.media(.paused, path: "/Users/me/Movies/A.mkv"))
        recents.update(Snapshots.media(.playing, path: "/Users/me/Movies/A.mkv"))
        recents.update(Snapshots.media(.playing, path: "/Users/me/Movies/B.mkv"))
        XCTAssertEqual(list.noted.map(\.path), ["/Users/me/Movies/A.mkv", "/Users/me/Movies/B.mkv"])
        XCTAssertTrue(list.noted.allSatisfy(\.isFileURL))
    }

    func testAPrivateSessionAddsNoRecentDocuments() {
        let list = FakeRecentDocumentList()
        let recents = RecentDocuments(list: list)
        recents.privateSession = true
        recents.update(Snapshots.media(.playing, path: "/Users/me/Movies/Secret.mkv"))
        recents.privateSession = false
        recents.update(Snapshots.media(.playing, path: "/Users/me/Movies/Secret.mkv"))
        XCTAssertTrue(list.noted.isEmpty, "a file opened privately stays out after the session stops being private")
        recents.update(Snapshots.media(.playing, path: "/Users/me/Movies/Public.mkv"))
        XCTAssertEqual(list.noted.map(\.lastPathComponent), ["Public.mkv"])
    }

    func testLinksAndFilesThatNeverLoadedAreNotNoted() {
        let list = FakeRecentDocumentList()
        let recents = RecentDocuments(list: list)
        recents.update(Snapshots.media(.playing, path: "https://example.com/film.mp4", kind: .url))
        recents.update(Snapshots.media(.opening, path: "/Users/me/Movies/Broken.mkv"))
        recents.update(Snapshots.media(.ended, path: "/Users/me/Movies/Broken.mkv"))
        XCTAssertTrue(list.noted.isEmpty)
    }

    func testOpenRecentListsTheFilesAndNamesTheFolderWhereNamesRepeat() {
        let urls = [URL(fileURLWithPath: "/Users/me/Movies/Show/S01/Episode.mkv"),
                    URL(fileURLWithPath: "/Users/me/Movies/Film.mkv"),
                    URL(fileURLWithPath: "/Users/me/Movies/Show/S02/Episode.mkv")]
        XCTAssertEqual(OpenRecentMenuModel.entries(urls).map(\.title),
                       ["Episode.mkv — S01", "Film.mkv", "Episode.mkv — S02"])
    }

    func testOpenRecentOpensAnEntryAndClearsTheList() throws {
        let film = URL(fileURLWithPath: "/Users/me/Movies/Film.mkv")
        let list = FakeRecentDocumentList(urls: [film])
        var opened: [URL] = []
        let openRecent = OpenRecentMenu(list: list) { opened.append($0) }
        let menu = try XCTUnwrap(openRecent.item.submenu)
        openRecent.menuNeedsUpdate(menu)
        XCTAssertEqual(menu.items.map(\.isSeparatorItem), [false, true, false])
        XCTAssertEqual(menu.items.filter { !$0.isSeparatorItem }.map(\.title), ["Film.mkv", "Clear Menu"])
        try choose(menu.items[0])
        XCTAssertEqual(opened, [film])

        try choose(menu.items[2])
        XCTAssertTrue(list.urls.isEmpty)
        openRecent.menuNeedsUpdate(menu)
        XCTAssertEqual(menu.items.map(\.title), ["Clear Menu"])
        XCTAssertNil(menu.items[0].action, "an empty list has nothing to clear")
    }

    // MARK: Dock menu

    func testTheDockMenuFollowsTheSessionState() {
        let playing = DockMenuModel.entries(Snapshots.media(.playing), canNext: true, canPrevious: false)
        XCTAssertEqual(playing, [
            DockMenuEntry(title: "Pause", action: .togglePause, enabled: true),
            DockMenuEntry(title: "Next", action: .step(.next), enabled: true),
            DockMenuEntry(title: "Previous", action: .step(.previous), enabled: false),
        ])
        XCTAssertEqual(DockMenuModel.entries(Snapshots.media(.paused), canNext: false, canPrevious: false).first,
                       DockMenuEntry(title: "Play", action: .togglePause, enabled: true))
        XCTAssertEqual(DockMenuModel.entries(Snapshots.idle, canNext: false, canPrevious: false).first?.enabled, false)
        XCTAssertEqual(DockMenuModel.entries(Snapshots.media(.opening), canNext: false, canPrevious: false).first?.enabled, false,
                       "the session refuses a pause toggle while a file opens")
    }

    // MARK: Together

    private struct Rig {
        let center: RecordingNowPlayingCenter
        let remote: FakeRemoteCommandSource
        let power: FakePowerAssertions
        let recents: FakeRecentDocumentList
        let session: SessionRecorder
        let system: SystemIntegration

        init() {
            let center = RecordingNowPlayingCenter()
            let remote = FakeRemoteCommandSource()
            let power = FakePowerAssertions()
            let recents = FakeRecentDocumentList()
            let session = SessionRecorder()
            self.center = center
            self.remote = remote
            self.power = power
            self.recents = recents
            self.session = session
            system = SystemIntegration(session: { session.dispatch($0) }, open: { _ in },
                                       nowPlayingCenter: center, remoteSource: remote, power: power,
                                       recentList: recents, artwork: nil, clock: { 0 })
        }
    }

    func testOneSnapshotDrivesNowPlayingDisplaySleepAndRecents() throws {
        let rig = Rig()
        rig.system.start()
        rig.system.update(Snapshots.media(.playing, title: "Film"))
        XCTAssertEqual(rig.center.last?.playbackState, .playing)
        XCTAssertEqual(rig.center.last?.info?[MPMediaItemPropertyTitle] as? String, "Film")
        XCTAssertEqual(rig.power.held.count, 1)
        XCTAssertEqual(rig.recents.noted.map(\.lastPathComponent), ["Film.mkv"])

        XCTAssertEqual(rig.remote.fire(.skipForward(seconds: 10)), .success)
        XCTAssertEqual(rig.session.commands, [.seek(seconds: 10, mode: .relative, precision: .exact)])

        rig.system.update(Snapshots.media(.paused, title: "Film"))
        XCTAssertEqual(rig.center.last?.playbackState, .paused)
        XCTAssertTrue(rig.power.held.isEmpty)

        rig.system.shutdown()
        XCTAssertNil(rig.center.last?.info)
        XCTAssertEqual(rig.center.last?.playbackState, .stopped)
        XCTAssertNil(rig.remote.fire(.play), "remote commands stop with the app")
    }

    func testShutdownWhilePlayingReleasesTheAssertion() {
        let rig = Rig()
        rig.system.update(Snapshots.media(.playing))
        rig.system.shutdown()
        XCTAssertTrue(rig.power.held.isEmpty)
    }

    func testThePrivateSessionKeepsFilesOutOfRecents() {
        let rig = Rig()
        rig.system.privateSession = true
        rig.system.update(Snapshots.media(.playing))
        XCTAssertTrue(rig.recents.noted.isEmpty)
        XCTAssertEqual(rig.center.writes.count, 1, "Now Playing still shows the file")
    }

    func testThePlaylistFeedsTheQueuePositionNextPreviousAndTheDockMenu() throws {
        let rig = Rig()
        rig.system.start()
        let playlist = FakePlaylist()
        rig.system.playlist = playlist
        XCTAssertEqual(rig.remote.offeredSteps.last?.next, true)
        rig.system.update(Snapshots.media(.playing))
        XCTAssertEqual(rig.center.last?.info?[MPNowPlayingInfoPropertyPlaybackQueueIndex] as? Int, 1)
        XCTAssertEqual(rig.center.last?.info?[MPNowPlayingInfoPropertyPlaybackQueueCount] as? Int, 3)

        let menu = rig.system.makeDockMenu()
        XCTAssertEqual(menu.items.map(\.title), ["Pause", "Next", "Previous"])
        XCTAssertEqual(menu.items.map(\.isEnabled), [true, true, true])
        try choose(menu.items[0])
        try choose(menu.items[2])
        XCTAssertEqual(rig.session.commands, [.togglePause])
        XCTAssertEqual(playlist.steps, [.previous])
    }

    func testWithoutAPlaylistTheDockMenuOffersNoSteps() {
        let rig = Rig()
        rig.system.update(Snapshots.media(.paused))
        let menu = rig.system.makeDockMenu()
        XCTAssertEqual(menu.items.map(\.title), ["Play", "Next", "Previous"])
        XCTAssertEqual(menu.items.map(\.isEnabled), [true, false, false])
    }

    /// Send the item's action to its target, as AppKit does when it is chosen.
    private func choose(_ item: NSMenuItem, file: StaticString = #filePath, line: UInt = #line) throws {
        let target = try XCTUnwrap(item.target as? NSObject, "\(item.title) has no target", file: file, line: line)
        let action = try XCTUnwrap(item.action, "\(item.title) has no action", file: file, line: line)
        _ = target.perform(action, with: item)
    }
}
