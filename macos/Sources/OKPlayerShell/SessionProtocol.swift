import Foundation

// The JSON documents the Rust session exchanges with the shell (docs/session-abi.md).
//
// Every type mirrors one in rust/crates/okp-session/src/protocol.rs. Rust tags
// commands, results and events externally with snake_case names and writes every
// variant as an object, even an empty one. Swift's synthesised `Codable` for enums
// with labelled associated values produces the same shape once keys go through the
// snake_case strategies of `SessionJSON`. The golden documents in
// rust/crates/okp-session/fixtures pin it on both sides.

enum SessionJSON {
    /// The `schema_version` of every envelope this shell writes and reads.
    static let schemaVersion = 1

    static func makeDecoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }

    static func makeEncoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
}

enum Platform: String, Codable, CaseIterable {
    case linux, macos
}

struct AppIdentity: Codable, Equatable {
    var name: String
    var platform: Platform
}

struct MpvOption: Codable, Equatable {
    var name: String
    var value: String
}

/// What the shell passes when it creates a session.
struct SessionConfig: Codable, Equatable {
    var schemaVersion = SessionJSON.schemaVersion
    var app: AppIdentity
    var dataDir: String
    var configDir: String
    var cacheDir: String
    var hwdec: String?
    var mpvOptions: [MpvOption] = []
}

enum SourceKind: String, Codable, CaseIterable {
    case local, url
}

struct MediaSource: Codable, Equatable {
    var kind: SourceKind
    var location: String
}

enum SeekMode: String, Codable, CaseIterable {
    case absolute, relative
}

enum SeekPrecision: String, Codable, CaseIterable {
    case exact, keyframe
}

enum TrackKind: String, Codable, CaseIterable {
    case audio, subtitle
}

/// An intent from the shell. The session validates it before libmpv sees it.
enum Command: Codable, Equatable {
    case open(source: MediaSource, resumeFrom: Double? = nil, audioTrack: Int64? = nil, subtitleTrack: Int64? = nil)
    case close
    case setPaused(paused: Bool)
    case togglePause
    case seek(seconds: Double, mode: SeekMode, precision: SeekPrecision)
    case setVolume(volume: Double)
    case setMute(muted: Bool)
    case setSpeed(speed: Double)
    /// `id: nil` turns that kind of track off.
    case selectTrack(kind: TrackKind, id: Int64?)
    case setSubtitleDelay(seconds: Double)
    /// Change the volume by `delta` percent; the session clamps it (OSC/TransportProtocol.swift).
    case nudgeVolume(delta: Double)
    /// Mute as the core does: a level of 0 that the next toggle takes back to the
    /// last level above it (docs/session-abi.md, "Transport controls").
    case toggleMute
}

struct CommandEnvelope: Codable, Equatable {
    var schemaVersion = SessionJSON.schemaVersion
    var command: Command
}

enum RejectReason: String, Codable, CaseIterable {
    case noActiveMedia = "no_active_media"
    case notFinite = "not_finite"
    case malformed
    case unsupportedSchemaVersion = "unsupported_schema_version"
    case invalidArgument = "invalid_argument"
}

enum DispatchResult: Codable, Equatable {
    case accepted(requestId: UInt64)
    case noOp
    case rejected(reason: RejectReason, message: String)
    case failed(requestId: UInt64, message: String)
}

struct DispatchReply: Codable, Equatable {
    var schemaVersion: Int
    var result: DispatchResult
}

enum Query: Codable, Equatable {
    case snapshot
    /// What the transport controls show; with `at`, the readout at that position.
    case transport(at: Double? = nil)
}

struct QueryEnvelope: Codable, Equatable {
    var schemaVersion = SessionJSON.schemaVersion
    var query: Query
}

enum QueryResult: Codable, Equatable {
    case snapshot(snapshot: SessionSnapshot)
    case transport(transport: TransportView)
    case rejected(reason: RejectReason, message: String)
}

struct QueryReply: Codable, Equatable {
    var schemaVersion: Int
    var result: QueryResult
}

enum Status: String, Codable, CaseIterable {
    case idle, opening, playing, paused, ended
}

enum EndReason: String, Codable, CaseIterable {
    case eof, stopped, quit, error, redirect
}

enum ErrorKind: String, Codable, CaseIterable {
    case loadFailed = "load_failed"
    case commandFailed = "command_failed"
    case engine
}

struct Track: Codable, Equatable {
    var id: Int64
    var kind: TrackKind
    var selected: Bool
    var external: Bool
    var `default`: Bool
    var title: String?
    var lang: String?
}

struct Chapter: Codable, Equatable {
    var index: Int64
    var time: Double
    var title: String?
}

struct VideoGeometry: Codable, Equatable {
    var width: Int32
    var height: Int32
    var displayAspect: Double
}

/// The full state the shell renders from.
struct SessionSnapshot: Codable, Equatable {
    var status = Status.idle
    var source: MediaSource?
    var timePos: Double?
    var duration: Double?
    var volume: Double?
    var mute = false
    var speed: Double?
    var subtitleDelay = 0.0
    var tracks: [Track] = []
    var chapters: [Chapter] = []
    var endReason: EndReason?
    var title: String?
    var video: VideoGeometry?
    var buffering = false
    var cacheAhead: Double?

    /// Apply one change of a `snapshot_changed` event, as the Rust
    /// `SessionSnapshot::apply` does.
    mutating func apply(_ change: SnapshotChange) {
        switch change {
        case .status(let value): status = value
        case .source(let value): source = value
        case .timePos(let value): timePos = value
        case .duration(let value): duration = value
        case .volume(let value): volume = value
        case .mute(let value): mute = value
        case .speed(let value): speed = value
        case .subtitleDelay(let value): subtitleDelay = value
        case .tracks(let value): tracks = value
        case .chapters(let value): chapters = value
        case .endReason(let value): endReason = value
        case .title(let value): title = value
        case .video(let value): video = value
        case .buffering(let value): buffering = value
        case .cacheAhead(let value): cacheAhead = value
        }
    }
}

/// One changed snapshot field and its new value.
enum SnapshotChange: Codable, Equatable {
    case status(value: Status)
    case source(value: MediaSource?)
    case timePos(value: Double?)
    case duration(value: Double?)
    case volume(value: Double?)
    case mute(value: Bool)
    case speed(value: Double?)
    case subtitleDelay(value: Double)
    case tracks(value: [Track])
    case chapters(value: [Chapter])
    case endReason(value: EndReason?)
    case title(value: String?)
    case video(value: VideoGeometry?)
    case buffering(value: Bool)
    case cacheAhead(value: Double?)
}

/// One entry of a drained batch: at most one `snapshotChanged`, first; errors in
/// the order they happened; `shutdown` last.
enum Event: Codable, Equatable {
    case snapshotChanged(changes: [SnapshotChange])
    case error(kind: ErrorKind, message: String, requestId: UInt64?)
    case shutdown
}

struct EventBatch: Codable, Equatable {
    var schemaVersion: Int
    var events: [Event]
}
