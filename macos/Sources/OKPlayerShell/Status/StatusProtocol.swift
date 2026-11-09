import Foundation

// The status documents (docs/session-abi.md, "Status surface and notices"), mirroring
// rust/crates/okp-session/src/status.rs. The golden documents in
// rust/crates/okp-session/fixtures/status pin the shape on both sides.

/// What covers the video, as the session's `status_surface` query answers it.
enum StatusSurface: Codable, Equatable {
    /// Nothing: the video, or the idle screen, shows.
    case clear
    /// A source is opening; `label` names it (`Opening film.mkv`).
    case loading(label: String)
    /// Loaded media waits for libmpv's cache; `percent` is how far it has refilled
    /// towards resuming, when libmpv reports it.
    case buffering(percent: Int?, label: String)
    /// The current source did not play.
    case failed(card: FailureCard)
}

/// A failed load, classified and worded by the core.
struct FailureCard: Codable, Equatable {
    var kind: FailureKind
    var title: String
    var message: String
    /// What Copy details copies: the source and a short reason.
    var detail: String
    /// What Retry opens again.
    var source: MediaSource
    /// The card's buttons, left to right.
    var actions: [FailureActionView]
}

enum FailureKind: String, Codable, CaseIterable {
    case missingCodec = "missing_codec"
    case renderer, application
}

enum FailureAction: String, Codable, CaseIterable {
    case retry
    case openAnother = "open_another"
    case copyDetails = "copy_details"
}

/// One button of the failure card.
struct FailureActionView: Codable, Equatable {
    var action: FailureAction
    var label: String
}
