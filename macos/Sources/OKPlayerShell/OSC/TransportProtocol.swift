import Foundation

// The answer to the session's `transport` query (docs/session-abi.md, "Transport
// controls"), mirroring rust/crates/okp-session/src/transport.rs. The session
// computes every string and fraction here with the core's rules, so the controls
// only show them. Pinned by rust/crates/okp-session/fixtures/transport.

/// What the transport controls show.
struct TransportView: Codable, Equatable {
    /// `MM:SS`, or `HH:MM:SS` from the first hour.
    var elapsed: String
    /// `-MM:SS`; `--:--` for a stream without a known duration.
    var remaining: String
    var total: String
    /// The playhead (or the drag position asked for) as a fraction of the duration.
    var progress: Double
    var buffered: Double
    var chapterTicks: [Double]
    var seekable: Bool
    var volume: VolumeView
    /// The current speed as the speed control shows it, `1.00×`.
    var speed: String
    var speeds: [SpeedChoice]
    var osc: OscView
}

struct VolumeView: Codable, Equatable {
    /// Percent, 0 to `max`.
    var level: Double
    var max: Double
    var fraction: Double
    /// Where 100 % sits on the track.
    var unityFraction: Double
    var muted: Bool
    var boosted: Bool
    var readout: String
}

struct SpeedChoice: Codable, Equatable {
    var speed: Double
    var label: String
    var selected: Bool
}

/// When the controls exist and when they may hide.
struct OscView: Codable, Equatable {
    /// The controls while shown.
    var revealed: OscSurface
    /// The controls once hidden for inactivity.
    var hidden: OscSurface
    /// They hide after `hideAfterMs` without activity; only while media plays.
    var autoHide: Bool
    var hideAfterMs: UInt64
}

/// The core's projection of the controls' surface: absent without media; hidden
/// for inactivity, in place but taking no focus and no clicks.
struct OscSurface: Codable, Equatable {
    var visible: Bool
    var mapped: Bool
    var focusable: Bool
    var hitTestable: Bool
}

extension OscView {
    /// The surface for the auto-hide state.
    func surface(revealed: Bool) -> OscSurface { revealed ? self.revealed : hidden }

    /// The idle timeout in seconds.
    var hideAfter: TimeInterval { TimeInterval(hideAfterMs) / 1000 }
}
