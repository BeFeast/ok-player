import Foundation

/// What the status layer shows over the video.
enum StatusDisplay: Equatable {
    case clear
    /// A spinner with a line under it: a source opening, or a cache stall.
    case progress(String)
    /// The session's failure card for the current source.
    case failure(FailureCard)
    /// A failure of the shell itself (no renderer, no session, the engine quit):
    /// a card with the message and no actions, since nothing can be retried.
    case shellFailure(String)

    /// Whether anything covers the video.
    var isClear: Bool { self == .clear }
}

/// Turns what the session reports into what the status layer shows. The session
/// decides the surface and words every notice; the shell only picks which of its
/// reports become a toast.
enum StatusRules {
    /// The title of a shell failure card.
    static let shellFailureTitle = "Could not play video"

    /// The display for the session's surface. A shell failure wins: once the shell
    /// itself has failed, nothing the session reports can be acted on.
    static func display(_ surface: StatusSurface, shellFailure: String?) -> StatusDisplay {
        if let shellFailure { return .shellFailure(shellFailure) }
        switch surface {
        case .clear: return .clear
        case .loading(let label), .buffering(_, let label): return .progress(label)
        case .failed(let card): return .failure(card)
        }
    }

    /// The toasts a drained batch asks for, oldest first: every notice, and every
    /// error but a `load_failed` one, which the failure card or a notice already
    /// shows (docs/session-abi.md, "Status surface and notices").
    static func toasts(in batch: EventBatch) -> [String] {
        batch.events.compactMap { event -> String? in
            switch event {
            case .notice(let text): return text
            case .error(let kind, let message, _) where kind != .loadFailed: return message
            default: return nil
            }
        }
    }
}
