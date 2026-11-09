import Foundation
import MediaPlayer
import UniformTypeIdentifiers

// What macOS shows in Control Center's Now Playing, derived from the session snapshot
// alone. Elapsed time and rate let the system move the position forward on its own,
// so the info is written when the state changes or the position jumps (a seek), not
// on every time update.

/// Whether the media counts as video or audio for Now Playing and display sleep.
enum MediaKind: Equatable {
    case video, audio

    var nowPlayingType: MPNowPlayingInfoMediaType {
        switch self {
        case .video: return .video
        case .audio: return .audio
        }
    }

    /// A file whose type is audio. Embedded cover art reaches libmpv as a video
    /// frame, so the file type decides before the video geometry does.
    static func isAudioFile(_ location: String) -> Bool {
        let fileExtension = (location as NSString).pathExtension
        guard !fileExtension.isEmpty, let type = UTType(filenameExtension: fileExtension) else { return false }
        return type.conforms(to: .audio)
    }
}

extension SessionSnapshot {
    var mediaKind: MediaKind {
        if let source, MediaKind.isAudioFile(source.location) { return .audio }
        return video == nil ? .audio : .video
    }

    /// A file is open or opening. An ended or idle session has nothing to control.
    var hasActiveMedia: Bool {
        source != nil && (status == .opening || status == .playing || status == .paused)
    }
}

/// The current item's place in the playlist. Without a playlist the open file is
/// the only item.
struct QueuePosition: Equatable {
    var index: Int
    var count: Int

    static let single = QueuePosition(index: 0, count: 1)
}

/// Everything Now Playing shows, in the system's terms.
struct NowPlayingState: Equatable {
    var title: String
    /// `nil` when the media has no known length.
    var duration: Double?
    var elapsed: Double
    /// How fast the position moves now: the speed while playing, 0 while paused,
    /// opening or buffering.
    var rate: Double
    /// The speed playback runs at when it runs.
    var defaultRate: Double
    var mediaKind: MediaKind
    var playback: MPNowPlayingPlaybackState
    var queue: QueuePosition
    var chapterCount: Int

    /// The state for a snapshot, or `nil` when no media is open.
    init?(snapshot: SessionSnapshot, queue: QueuePosition = .single) {
        guard snapshot.hasActiveMedia, let source = snapshot.source else { return nil }
        title = NowPlayingState.displayTitle(snapshot.title, source: source)
        let length = snapshot.duration.flatMap { value -> Double? in value.isFinite && value > 0 ? value : nil }
        duration = length
        var position = snapshot.timePos.flatMap { value -> Double? in value.isFinite ? max(0, value) : nil } ?? 0
        if let length { position = min(position, length) }
        elapsed = position
        let speed = snapshot.speed.flatMap { value -> Double? in value.isFinite && value > 0 ? value : nil } ?? 1
        defaultRate = speed
        switch snapshot.status {
        case .playing:
            playback = .playing
            rate = snapshot.buffering ? 0 : speed
        case .paused:
            playback = .paused
            rate = 0
        default:
            // Opening: the item is about to play, but its position does not move yet.
            playback = .playing
            rate = 0
        }
        mediaKind = snapshot.mediaKind
        self.queue = queue
        chapterCount = snapshot.chapters.count
    }

    /// The media title libmpv reports, else the file or link name.
    static func displayTitle(_ mediaTitle: String?, source: MediaSource) -> String {
        if let mediaTitle = mediaTitle?.trimmingCharacters(in: .whitespacesAndNewlines), !mediaTitle.isEmpty {
            return mediaTitle
        }
        let name: String
        switch source.kind {
        case .local: name = (source.location as NSString).lastPathComponent
        case .url: name = URL(string: source.location)?.lastPathComponent ?? ""
        }
        if !name.isEmpty, name != "/" { return name }
        return source.location.isEmpty ? "OK Player" : source.location
    }

    /// The position the system shows `seconds` after this state was published.
    func extrapolatedElapsed(after seconds: Double) -> Double {
        let position = elapsed + rate * max(0, seconds)
        return duration.map { min(position, $0) } ?? position
    }

    /// The same state apart from the position.
    func sameApartFromPosition(_ other: NowPlayingState) -> Bool {
        var moved = other
        moved.elapsed = elapsed
        return moved == self
    }
}

enum NowPlayingInfo {
    /// The `nowPlayingInfo` dictionary for a state.
    static func dictionary(_ state: NowPlayingState, artwork: MPMediaItemArtwork?) -> [String: Any] {
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: state.title,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: state.elapsed,
            MPNowPlayingInfoPropertyPlaybackRate: state.rate,
            MPNowPlayingInfoPropertyDefaultPlaybackRate: state.defaultRate,
            MPNowPlayingInfoPropertyMediaType: state.mediaKind.nowPlayingType.rawValue,
            MPNowPlayingInfoPropertyPlaybackQueueIndex: state.queue.index,
            MPNowPlayingInfoPropertyPlaybackQueueCount: state.queue.count,
            MPNowPlayingInfoPropertyChapterCount: state.chapterCount,
        ]
        if let duration = state.duration { info[MPMediaItemPropertyPlaybackDuration] = duration }
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        return info
    }
}

/// Where Now Playing info goes: the system's info center in the app, a recorder in
/// tests.
protocol NowPlayingCenter: AnyObject {
    /// Replace the info and the playback state together. `nil` info clears it.
    func publish(_ info: [String: Any]?, playbackState: MPNowPlayingPlaybackState)
}

/// `MPNowPlayingInfoCenter`. Setting `playbackState` is what makes the app the Now
/// Playing app on macOS.
final class SystemNowPlayingCenter: NowPlayingCenter {
    private let center = MPNowPlayingInfoCenter.default()

    func publish(_ info: [String: Any]?, playbackState: MPNowPlayingPlaybackState) {
        center.nowPlayingInfo = info
        center.playbackState = playbackState
    }

    /// What the system holds now, for the smoke.
    var current: (info: [String: Any]?, playbackState: MPNowPlayingPlaybackState) {
        (center.nowPlayingInfo, center.playbackState)
    }
}

/// Writes Now Playing when the state changes, and when the position moved further
/// than the system's own extrapolation by more than `seekTolerance` (a seek, or a
/// stall the session did not report).
final class NowPlayingPublisher {
    /// Linux sends MPRIS `Seeked` from the same distance.
    static let seekTolerance = 0.75

    private let center: NowPlayingCenter
    private let artwork: MPMediaItemArtwork?
    private let clock: () -> TimeInterval
    private(set) var published: NowPlayingState?
    private var publishedAt: TimeInterval = 0
    /// Nothing is published, so there is nothing to clear.
    private var cleared = true

    init(center: NowPlayingCenter, artwork: MPMediaItemArtwork?,
         clock: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        self.center = center
        self.artwork = artwork
        self.clock = clock
    }

    func update(_ state: NowPlayingState?) {
        guard let state else {
            clear()
            return
        }
        let now = clock()
        if let published, published.sameApartFromPosition(state) {
            let expected = published.extrapolatedElapsed(after: now - publishedAt)
            guard abs(state.elapsed - expected) > NowPlayingPublisher.seekTolerance else { return }
        }
        center.publish(NowPlayingInfo.dictionary(state, artwork: artwork), playbackState: state.playback)
        published = state
        publishedAt = now
        cleared = false
    }

    /// Remove the info; the app stops being the Now Playing app.
    func clear() {
        guard !cleared else { return }
        center.publish(nil, playbackState: .stopped)
        published = nil
        cleared = true
    }
}
