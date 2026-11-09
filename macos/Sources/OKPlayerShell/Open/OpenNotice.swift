import AppKit

/// What the window says when an open plays nothing, as a toast (Status/). Playback
/// goes on. An open whose folders and playlist files held nothing playable is the
/// session's own notice.
enum OpenNotice: Equatable {
    /// The session's plan opens nothing: no media file, folder or playlist file,
    /// and no subtitle file to load onto playing media.
    case nothingToOpen
    var text: String {
        switch self {
        case .nothingToOpen: return "Nothing to play in the selection"
        }
    }

    /// The notice for a router outcome that played nothing; `nil` for one that
    /// opened something, was held for the launch or failed (a failure is shown as
    /// one).
    init?(_ outcome: OpenOutcome) {
        switch outcome {
        case .nothing, .opened(_, .noOp, _): self = .nothingToOpen
        case .opened, .subtitlesAdded, .held: return nil
        }
    }

    /// The window title while `source` is loaded, as an open sets it.
    static func windowTitle(loaded source: MediaSource?) -> String {
        guard let source else { return "OK Player" }
        return "\((source.location as NSString).lastPathComponent) — OK Player"
    }
}
