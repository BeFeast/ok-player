import Foundation
import MediaPlayer
@testable import OKPlayerShell

/// Stand-ins for the system services the OS integration talks to, recording what the
/// shell asked of them.

final class RecordingNowPlayingCenter: NowPlayingCenter {
    struct Write {
        var info: [String: Any]?
        var playbackState: MPNowPlayingPlaybackState
    }

    private(set) var writes: [Write] = []
    var last: Write? { writes.last }

    func publish(_ info: [String: Any]?, playbackState: MPNowPlayingPlaybackState) {
        writes.append(Write(info: info, playbackState: playbackState))
    }
}

final class FakeRemoteCommandSource: RemoteCommandSource {
    private(set) var handler: ((RemoteCommand) -> MPRemoteCommandHandlerStatus)?
    private(set) var offeredSteps: [(next: Bool, previous: Bool)] = []
    private(set) var uninstalls = 0

    func install(_ handler: @escaping (RemoteCommand) -> MPRemoteCommandHandlerStatus) {
        self.handler = handler
    }

    func setPlaylistSteps(next: Bool, previous: Bool) {
        offeredSteps.append((next, previous))
    }

    func uninstall() {
        handler = nil
        uninstalls += 1
    }

    /// The command as the system would deliver it; `nil` when nothing is installed.
    func fire(_ command: RemoteCommand) -> MPRemoteCommandHandlerStatus? {
        handler?(command)
    }
}

final class FakePowerAssertions: PowerAssertions {
    private var nextID: UInt32 = 40
    private(set) var held: Set<UInt32> = []
    private(set) var created = 0
    private(set) var released = 0
    private(set) var reasons: [String] = []
    var refuses = false

    func preventDisplaySleep(reason: String) -> UInt32? {
        reasons.append(reason)
        guard !refuses else { return nil }
        nextID += 1
        created += 1
        held.insert(nextID)
        return nextID
    }

    func release(_ assertion: UInt32) {
        released += 1
        held.remove(assertion)
    }
}

final class FakeRecentDocumentList: RecentDocumentList {
    private(set) var urls: [URL] = []
    private(set) var noted: [URL] = []

    init(urls: [URL] = []) {
        self.urls = urls
    }

    func note(_ url: URL) {
        noted.append(url)
        urls.removeAll { $0 == url }
        urls.insert(url, at: 0)
    }

    func clear() {
        urls = []
    }
}

final class FakePlaylist: PlaylistNavigation {
    var position = QueuePosition(index: 1, count: 3)
    var canGoNext = true
    var canGoPrevious = true
    private(set) var steps: [PlaylistStep] = []

    func canStep(_ step: PlaylistStep) -> Bool {
        step == .next ? canGoNext : canGoPrevious
    }

    func step(_ step: PlaylistStep) -> DispatchResult {
        steps.append(step)
        return .accepted(requestId: 7)
    }
}

/// Session snapshots for the OS integration tests.
enum Snapshots {
    static func media(_ status: Status, path: String = "/Users/me/Movies/Film.mkv", kind: SourceKind = .local,
                      time: Double? = 30, duration: Double? = 120, speed: Double? = 1, video: Bool = true,
                      title: String? = nil, chapters: Int = 0, buffering: Bool = false) -> SessionSnapshot {
        var snapshot = SessionSnapshot()
        snapshot.status = status
        snapshot.source = MediaSource(kind: kind, location: path)
        snapshot.timePos = time
        snapshot.duration = duration
        snapshot.speed = speed
        snapshot.title = title
        snapshot.buffering = buffering
        snapshot.video = video ? VideoGeometry(width: 1920, height: 1080, displayAspect: 16.0 / 9.0) : nil
        snapshot.chapters = (0..<chapters).map { Chapter(index: Int64($0), time: Double($0) * 10, title: nil) }
        return snapshot
    }

    static let idle = SessionSnapshot()
}

/// Something that records the session commands it is handed and answers with `reply`.
final class SessionRecorder {
    private(set) var commands: [Command] = []
    var reply: DispatchResult = .accepted(requestId: 1)

    func dispatch(_ command: Command) -> DispatchResult {
        commands.append(command)
        return reply
    }
}
