import Foundation

// The answers to the session's `open_formats` and `open_paths` queries
// (docs/session-abi.md, "Opening files"), mirroring
// rust/crates/okp-session/src/open_paths.rs. The session applies okp-core's
// `media_formats` rules, so the shell keeps no extension lists of its own.
// Pinned by rust/crates/okp-session/fixtures/open.

/// The file types the Open panel offers, by kind: lowercase extensions without
/// the leading dot.
struct OpenFormats: Codable, Equatable {
    var video: [String]
    var audio: [String]
    var subtitle: [String]
    /// M3U and M3U8.
    var playlist: [String]
}

/// What opening a set of local paths does.
enum OpenPlan: Codable, Equatable {
    /// Replace the playlist with these sources through the playlist's `open`:
    /// media files, folders and M3U files, in the given order.
    case open(sources: [MediaSource])
    /// Only subtitle files, while media is loaded: load them onto that media.
    case addSubtitles(paths: [String])
    /// No paths, or only subtitle files with no media loaded.
    case nothing
}
