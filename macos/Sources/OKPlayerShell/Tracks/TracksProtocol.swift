import Foundation

// The answer to the session's `tracks` query (docs/session-abi.md, "Tracks and
// subtitles"), mirroring rust/crates/okp-session/src/tracks.rs. The session labels
// every track with the core's rules and formats both readouts, so the menus only
// show them. Pinned by rust/crates/okp-session/fixtures/tracks.

/// What the audio and subtitle menus show.
struct TracksView: Codable, Equatable {
    /// The audio tracks in libmpv's order; empty without loaded media.
    var audio: [TrackChoice]
    /// Subtitles Off (`id == nil`) first, then the subtitle tracks.
    var subtitles: [TrackChoice]
    var subtitleDelay: SubtitleValue
    var subtitleScale: SubtitleValue
    /// Media is playing or paused: a file can be added and the delay and size changed.
    var adjustable: Bool
}

/// One row of a track menu.
struct TrackChoice: Codable, Equatable {
    /// What `Command.selectTrack` takes; `nil` turns subtitles off.
    var id: Int64?
    /// The core's descriptor: name, language, channels, codec, source.
    var label: String
    var selected: Bool
}

/// A subtitle adjustment and its readout.
struct SubtitleValue: Codable, Equatable {
    /// Seconds for the delay, a factor (1 = 100 %) for the size.
    var value: Double
    /// `+250 ms`; `110%`.
    var label: String
}
