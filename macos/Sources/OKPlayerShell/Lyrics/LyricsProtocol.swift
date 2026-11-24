import Foundation

/// Resolution state of local sidecar lyrics for the current audio track
/// (`rust/crates/okp-session/src/lyrics.rs`).
enum LyricsState: String, Codable, CaseIterable {
    case unavailable
    case loading
    case synced
    case plain
    case missing
    case invalid
}

/// One renderable lyric line projected by the shared Rust session.
struct LyricsLineView: Codable, Equatable {
    var timeSeconds: Double
    var text: String
    var isGap: Bool
}

/// The local sidecar lyrics view returned by `Query.lyrics`.
struct LyricsSnapshot: Codable, Equatable {
    var source: MediaSource?
    var generation: UInt64
    var header: String
    var state: LyricsState
    var message: String?
    var lines: [LyricsLineView]
    var activeIndex: Int?

    static let empty = LyricsSnapshot(
        source: nil, generation: 0, header: "LYRICS", state: .unavailable,
        message: nil, lines: [], activeIndex: nil)
}
