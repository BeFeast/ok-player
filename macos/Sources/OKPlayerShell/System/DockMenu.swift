import Foundation

/// One item of the Dock menu (`applicationDockMenu`). Recent files appear in the Dock
/// menu through the system's recent documents, above these items.
struct DockMenuEntry: Equatable {
    enum Action: Equatable {
        case togglePause
        case step(PlaylistStep)
    }

    var title: String
    var action: Action
    var enabled: Bool
}

enum DockMenuModel {
    static func entries(_ snapshot: SessionSnapshot, canNext: Bool, canPrevious: Bool) -> [DockMenuEntry] {
        let running = snapshot.status == .playing || snapshot.status == .opening
        // The session accepts a pause toggle only while a file is playing or paused.
        let toggles = snapshot.source != nil && (snapshot.status == .playing || snapshot.status == .paused)
        return [
            DockMenuEntry(title: running ? "Pause" : "Play", action: .togglePause, enabled: toggles),
            DockMenuEntry(title: "Next", action: .step(.next), enabled: canNext),
            DockMenuEntry(title: "Previous", action: .step(.previous), enabled: canPrevious),
        ]
    }
}
