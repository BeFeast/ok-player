import Foundation

/// Next and previous item over the session playlist (docs/session-abi.md, Playlist).
/// The session steps in play order and wraps at both ends whatever the repeat mode,
/// so both steps go somewhere exactly when the playlist has two items or more.
final class SessionPlaylist: PlaylistNavigation {
    /// The playlist as of the last drained batch (`SessionStore.playlist`).
    var state = PlaylistState()
    private let session: (Command) -> DispatchResult

    init(session: @escaping (Command) -> DispatchResult) {
        self.session = session
    }

    /// The current item's place among the items; a lone file, or no playlist yet,
    /// is item 1 of 1.
    var position: QueuePosition {
        guard let index = state.currentIndex, state.items.indices.contains(index) else { return .single }
        return QueuePosition(index: index, count: state.items.count)
    }

    func canStep(_ step: PlaylistStep) -> Bool {
        state.items.count >= 2
    }

    func step(_ step: PlaylistStep) -> DispatchResult {
        session(.playlist(action: step == .next ? .next : .previous))
    }
}
