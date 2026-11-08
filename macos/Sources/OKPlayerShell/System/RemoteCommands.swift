import Foundation
import MediaPlayer

// Media keys (F7-F9), Control Center and AirPods reach the app as MPRemoteCommandCenter
// commands. Each becomes a session command, so the session validates it like any other;
// no event tap, so no Accessibility or Input Monitoring permission.

/// A command from the system's remote-command center.
enum RemoteCommand: Equatable {
    case play, pause, togglePlayPause, stop
    case nextTrack, previousTrack
    case changePlaybackPosition(seconds: Double)
    case skipForward(seconds: Double)
    case skipBackward(seconds: Double)
    case changePlaybackRate(rate: Double)
}

enum PlaylistStep: Equatable {
    case next, previous
}

/// Next and previous item, and the current item's place, as the OS integration needs
/// them. Implemented over the session playlist once the shell decodes it.
protocol PlaylistNavigation: AnyObject {
    var position: QueuePosition { get }
    func canStep(_ step: PlaylistStep) -> Bool
    func step(_ step: PlaylistStep) -> DispatchResult
}

/// Where a remote command goes.
enum RemoteRoute: Equatable {
    case session(Command)
    case playlist(PlaylistStep)
    /// A value the session could not use (not finite, not positive).
    case refused
}

enum RemoteCommandMapping {
    /// The skip buttons move ten seconds.
    static let skipInterval = 10.0
    /// The speeds the system may ask for are kept in the range Linux MPRIS accepts.
    static let rates: ClosedRange<Double> = 0.25...4.0
    /// The speeds offered to the system's rate menu.
    static let offeredRates: [Double] = [0.5, 0.75, 1, 1.25, 1.5, 2]

    static func route(_ command: RemoteCommand) -> RemoteRoute {
        switch command {
        case .play: return .session(.setPaused(paused: false))
        case .pause: return .session(.setPaused(paused: true))
        case .togglePlayPause: return .session(.togglePause)
        case .stop: return .session(.close)
        case .nextTrack: return .playlist(.next)
        case .previousTrack: return .playlist(.previous)
        case .changePlaybackPosition(let seconds):
            guard seconds.isFinite else { return .refused }
            return .session(.seek(seconds: max(0, seconds), mode: .absolute, precision: .exact))
        case .skipForward(let seconds):
            guard seconds.isFinite, seconds > 0 else { return .refused }
            return .session(.seek(seconds: seconds, mode: .relative, precision: .exact))
        case .skipBackward(let seconds):
            guard seconds.isFinite, seconds > 0 else { return .refused }
            return .session(.seek(seconds: -seconds, mode: .relative, precision: .exact))
        case .changePlaybackRate(let rate):
            guard rate.isFinite, rate > 0 else { return .refused }
            return .session(.setSpeed(speed: min(max(rate, rates.lowerBound), rates.upperBound)))
        }
    }

    /// The handler status for the session's answer.
    static func status(_ result: DispatchResult) -> MPRemoteCommandHandlerStatus {
        switch result {
        case .accepted, .noOp: return .success
        case .rejected(.noActiveMedia, _): return .noActionableNowPlayingItem
        case .rejected, .failed: return .commandFailed
        }
    }
}

/// Where remote commands come from: `MPRemoteCommandCenter` in the app, a fake in tests.
protocol RemoteCommandSource: AnyObject {
    /// Start delivering every supported command to `handler`.
    func install(_ handler: @escaping (RemoteCommand) -> MPRemoteCommandHandlerStatus)
    /// Offer or withdraw the next and previous item commands.
    func setPlaylistSteps(next: Bool, previous: Bool)
    /// Stop delivering commands.
    func uninstall()
}

/// Turns remote commands into session commands and keeps Next and Previous offered
/// only while the playlist has somewhere to go.
final class RemoteCommandController {
    private let source: RemoteCommandSource
    private let session: (Command) -> DispatchResult
    var playlist: PlaylistNavigation?
    private var installed = false
    private var offered: (next: Bool, previous: Bool)?

    init(source: RemoteCommandSource, session: @escaping (Command) -> DispatchResult) {
        self.source = source
        self.session = session
    }

    func start() {
        guard !installed else { return }
        installed = true
        source.install { [weak self] command in self?.handle(command) ?? .commandFailed }
        refresh()
    }

    func handle(_ command: RemoteCommand) -> MPRemoteCommandHandlerStatus {
        switch RemoteCommandMapping.route(command) {
        case .session(let sessionCommand):
            return RemoteCommandMapping.status(session(sessionCommand))
        case .playlist(let step):
            guard let playlist, playlist.canStep(step) else { return .noSuchContent }
            return RemoteCommandMapping.status(playlist.step(step))
        case .refused:
            return .commandFailed
        }
    }

    /// Offer Next and Previous per the playlist; writes only when that changes.
    func refresh() {
        guard installed else { return }
        let next = playlist?.canStep(.next) ?? false
        let previous = playlist?.canStep(.previous) ?? false
        if let offered, offered.next == next, offered.previous == previous { return }
        offered = (next, previous)
        source.setPlaylistSteps(next: next, previous: previous)
    }

    func stop() {
        guard installed else { return }
        installed = false
        offered = nil
        source.uninstall()
    }
}

/// `MPRemoteCommandCenter`. Handlers run on the main thread.
final class SystemRemoteCommandSource: RemoteCommandSource {
    private let center = MPRemoteCommandCenter.shared()
    private var targets: [(command: MPRemoteCommand, target: Any)] = []

    func install(_ handler: @escaping (RemoteCommand) -> MPRemoteCommandHandlerStatus) {
        uninstall()
        let onMain: (RemoteCommand) -> MPRemoteCommandHandlerStatus = { command in
            if Thread.isMainThread { return handler(command) }
            return DispatchQueue.main.sync { handler(command) }
        }
        add(center.playCommand) { _ in onMain(.play) }
        add(center.pauseCommand) { _ in onMain(.pause) }
        add(center.togglePlayPauseCommand) { _ in onMain(.togglePlayPause) }
        add(center.stopCommand) { _ in onMain(.stop) }
        add(center.nextTrackCommand) { _ in onMain(.nextTrack) }
        add(center.previousTrackCommand) { _ in onMain(.previousTrack) }
        let interval = [NSNumber(value: RemoteCommandMapping.skipInterval)]
        center.skipForwardCommand.preferredIntervals = interval
        center.skipBackwardCommand.preferredIntervals = interval
        add(center.skipForwardCommand) { event in
            let seconds = (event as? MPSkipIntervalCommandEvent)?.interval ?? RemoteCommandMapping.skipInterval
            return onMain(.skipForward(seconds: seconds))
        }
        add(center.skipBackwardCommand) { event in
            let seconds = (event as? MPSkipIntervalCommandEvent)?.interval ?? RemoteCommandMapping.skipInterval
            return onMain(.skipBackward(seconds: seconds))
        }
        add(center.changePlaybackPositionCommand) { event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            return onMain(.changePlaybackPosition(seconds: event.positionTime))
        }
        center.changePlaybackRateCommand.supportedPlaybackRates = RemoteCommandMapping.offeredRates.map { NSNumber(value: $0) }
        add(center.changePlaybackRateCommand) { event in
            guard let event = event as? MPChangePlaybackRateCommandEvent else { return .commandFailed }
            return onMain(.changePlaybackRate(rate: Double(event.playbackRate)))
        }
    }

    private func add(_ command: MPRemoteCommand,
                     _ handler: @escaping (MPRemoteCommandEvent) -> MPRemoteCommandHandlerStatus) {
        command.isEnabled = true
        targets.append((command, command.addTarget(handler: handler)))
    }

    func setPlaylistSteps(next: Bool, previous: Bool) {
        center.nextTrackCommand.isEnabled = next
        center.previousTrackCommand.isEnabled = previous
    }

    func uninstall() {
        for (command, target) in targets {
            command.removeTarget(target)
            command.isEnabled = false
        }
        targets = []
    }
}
