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
    /// The registry commands this shell carries out itself (dialogs, window and
    /// clipboard actions). A command neither the session nor the shell carries out
    /// is reported unavailable.
    var shellCommands: [String] = []
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
    /// Run a command of the registry (`Query.commandRegistry`) by its id.
    case invokeCommand(id: String)
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
    case unknownCommand = "unknown_command"
    case unavailable
}

enum DispatchResult: Codable, Equatable {
    case accepted(requestId: UInt64)
    case noOp
    case rejected(reason: RejectReason, message: String)
    case failed(requestId: UInt64, message: String)
    /// An invoked registry command the shell carries out itself; nothing reached
    /// the engine.
    case perform
}

struct DispatchReply: Codable, Equatable {
    var schemaVersion: Int
    var result: DispatchResult
}

enum Query: Codable, Equatable {
    case snapshot
    /// Every registry command, resolved against the session state and the window
    /// state only the shell knows. Query it when a menu opens.
    case commandRegistry(fullscreen: Bool = false, miniPlayer: Bool = false)
    /// Every bindable action with its chords on the config's platform.
    case shortcutTable
}

struct QueryEnvelope: Codable, Equatable {
    var schemaVersion = SessionJSON.schemaVersion
    var query: Query
}

enum QueryResult: Codable, Equatable {
    case snapshot(snapshot: SessionSnapshot)
    case commandRegistry(commands: [RegistryCommand])
    case shortcutTable(shortcuts: [ShortcutRow])
    case rejected(reason: RejectReason, message: String)
}

struct QueryReply: Codable, Equatable {
    var schemaVersion: Int
    var result: QueryResult
}

enum CommandGroup: String, Codable, CaseIterable {
    case playbackNavigation = "playback_navigation"
    case tracksSubtitles = "tracks_subtitles"
    case viewWindow = "view_window"
    case mediaFile = "media_file"
    case tools, settings
}

/// A place in the player command menu: its first level or one of its pages.
enum CommandMenu: String, Codable, CaseIterable {
    case topLevel = "top_level"
    case video, playback, window
    case toolsAdvanced = "tools_advanced"
}

struct ShortcutModifiers: Codable, Equatable {
    var ctrl = false
    var alt = false
    var shift = false
    /// The Command key.
    var cmd = false
}

/// One key plus modifiers.
struct ShortcutChord: Codable, Equatable {
    /// The canonical key name (`space`, `comma`, `Left`, `o`).
    var key: String
    var modifiers: ShortcutModifiers
    /// The display text (`Shift+Cmd+O`).
    var label: String
}

/// One registry command, resolved against the state at the time of the query.
struct RegistryCommand: Codable, Equatable {
    /// What `Command.invokeCommand` takes.
    var id: String
    var group: CommandGroup
    /// `nil` for a keyboard-only command.
    var menu: CommandMenu?
    var label: String
    /// Whether the session or this shell can run it at all; hide it when not.
    var available: Bool
    var enabled: Bool
    var checked: Bool
    /// The primary chord bound to it on this platform.
    var shortcut: ShortcutChord?
}

/// One bindable action of the shortcut table.
struct ShortcutRow: Codable, Equatable {
    var action: String
    /// The registry id to invoke.
    var command: String
    var primary: ShortcutChord
    var secondary: ShortcutChord?
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
