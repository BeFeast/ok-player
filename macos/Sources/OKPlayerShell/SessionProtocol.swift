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
    /// A watch-history action, answered with `DispatchResult.history`.
    case history(action: HistoryAction)
    /// Open, step through and edit the playlist.
    case playlist(action: PlaylistAction)
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
    /// The answer to a `history` command.
    case history(result: HistoryActionResult)
    /// Carried out by the session itself (a playlist edit, or an open whose
    /// folders are being read); nothing reached the engine yet. Events report the
    /// effect.
    case applied
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
    /// A watch-history query, answered with `QueryResult.history`.
    case history(query: HistoryQuery)
    /// The playlist as it is now.
    case playlist
}

struct QueryEnvelope: Codable, Equatable {
    var schemaVersion = SessionJSON.schemaVersion
    var query: Query
}

enum QueryResult: Codable, Equatable {
    case snapshot(snapshot: SessionSnapshot)
    case commandRegistry(commands: [RegistryCommand])
    case shortcutTable(shortcuts: [ShortcutRow])
    case history(result: HistoryQueryResult)
    case playlist(playlist: PlaylistState)
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

/// One entry of a drained batch: at most one `snapshotChanged`, first; then at
/// most one `playlistChanged`; errors in the order they happened; `shutdown` last.
enum Event: Codable, Equatable {
    case snapshotChanged(changes: [SnapshotChange])
    /// The playlist changed: its items, current item, play modes or scanning.
    case playlistChanged(playlist: PlaylistState)
    case error(kind: ErrorKind, message: String, requestId: UInt64?)
    case shutdown
    case history(event: HistoryEvent)
}

extension Event {
    /// Whether `name` (an `events` entry's key, after the decoder's snake case
    /// conversion) is an event this shell knows.
    static func isKnown(_ name: String) -> Bool {
        CodingKeys(stringValue: name) != nil
    }
}

struct EventBatch: Codable, Equatable {
    var schemaVersion: Int
    var events: [Event]

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, events
    }

    init(schemaVersion: Int, events: [Event]) {
        self.schemaVersion = schemaVersion
        self.events = events
    }

    /// Decodes the entries one by one and drops an event this shell does not know,
    /// keeping the rest of the batch (docs/session-abi.md, "Compatibility within a
    /// version"). A known event that does not decode still fails the batch.
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        schemaVersion = try container.decode(Int.self, forKey: .schemaVersion)
        var entries = try container.nestedUnkeyedContainer(forKey: .events)
        var events: [Event] = []
        while !entries.isAtEnd {
            if let event = try entries.decode(EventEntry.self).event {
                events.append(event)
            }
        }
        self.events = events
    }
}

/// One entry of a batch's `events`: the event, or `nil` for one this shell does
/// not know.
private struct EventEntry: Decodable {
    let event: Event?

    init(from decoder: Decoder) throws {
        let names = try decoder.container(keyedBy: EntryName.self).allKeys
        guard names.contains(where: { Event.isKnown($0.stringValue) }) else {
            event = nil
            return
        }
        event = try Event(from: decoder)
    }

    private struct EntryName: CodingKey {
        let stringValue: String
        var intValue: Int? { nil }
        init(stringValue: String) { self.stringValue = stringValue }
        init?(intValue: Int) { return nil }
    }
}

// MARK: - Watch history (rust/crates/okp-session/src/history/wire.rs)
//
// Resume, progress and per-file memory need no command: the session does them as
// media opens, plays and changes. These are the shell's ways in: the private
// session, removing an entry, and the two lists it renders.

enum HistoryAction: Codable, Equatable {
    /// While the private session is on, nothing new is recorded and nothing
    /// remembered is applied to an open.
    case setPrivate(enabled: Bool)
    /// Forget one entry; the media itself is untouched.
    case remove(source: MediaSource)
}

enum HistoryActionResult: Codable, Equatable {
    case done
    /// There was nothing to change.
    case unchanged
    /// The history file could not be written; nothing changed.
    case failed(message: String)
}

enum HistoryQuery: Codable, Equatable {
    /// The newest entries for the welcome screen's Continue watching shelf.
    /// `utcOffsetSeconds` is the shell's current offset from UTC, for the labels.
    case welcomeShelf(limit: UInt32, utcOffsetSeconds: Int32)
    /// Every entry for the History page, newest first and grouped by day, keeping
    /// those whose title, folder or location contains `search` (empty keeps all).
    case page(search: String = "", utcOffsetSeconds: Int32)
}

enum HistoryState: String, Codable, CaseIterable {
    case finished, progress, barely
}

enum HistoryBucket: String, Codable, CaseIterable {
    case today, yesterday
    case earlierThisWeek = "earlier_this_week"
    case earlier
}

/// One remembered source, ready to render.
struct HistoryEntry: Codable, Equatable {
    /// What to open to play it again.
    var source: MediaSource
    var title: String
    /// The folder breadcrumb, or the location itself when it has no folder.
    var location: String
    /// Seconds; 0 once finished.
    var position: Double
    /// Seconds; 0 when unknown.
    var duration: Double
    /// Watched fraction, 0 to 1.
    var progress: Double
    var state: HistoryState
    var stateLabel: String
    var whenLabel: String
    var updatedAtUnix: Int64
    /// Where opening it starts, or `nil` for the start.
    var resumeFrom: Double?
}

/// One day group of the History page.
struct HistorySection: Codable, Equatable {
    var bucket: HistoryBucket
    var header: String
    var entries: [HistoryEntry]
}

enum HistoryQueryResult: Codable, Equatable {
    case welcomeShelf(privateSession: Bool, entries: [HistoryEntry])
    /// `readFailed`: the history file could not be read at start, so only what was
    /// recorded since is listed; `unreadableCopy` is where that file was moved.
    case page(privateSession: Bool, readFailed: Bool, unreadableCopy: String?, summary: String, endCap: String,
              sections: [HistorySection])
}

enum HistoryEvent: Codable, Equatable {
    /// The media that just loaded started from its remembered position.
    case resumed(position: Double)
    /// What the history queries answer has changed; query again.
    case changed
    case privateSession(enabled: Bool)
    /// The history file could not be written; reported once until a save succeeds.
    case saveFailed(message: String)
}

// MARK: - Playlist (rust/crates/okp-session/src/playlist/mod.rs)

/// What happens when an item ends.
enum RepeatMode: String, Codable, CaseIterable {
    case off, one, all
}

/// Where queued sources go.
enum QueueMode: String, Codable, CaseIterable {
    case append
    case playNext = "play_next"
}

/// A playlist command. Indexes count from 0 in the order of `PlaylistState.items`.
enum PlaylistAction: Codable, Equatable {
    /// Files, folders, M3U playlists or URLs, replacing the playlist.
    case open(sources: [MediaSource])
    case next
    case previous
    case jump(index: Int)
    case queue(sources: [MediaSource], mode: QueueMode)
    /// Move the item at `index` to play right after the current one.
    case playNext(index: Int)
    /// Move the item at `from` to `to` (a position after its removal).
    case move(from: Int, to: Int)
    /// Never the current item or the last one.
    case remove(index: Int)
    /// Remove every item except the current one.
    case clearQueue
    case setRepeat(mode: RepeatMode)
    /// A `seed` makes the order reproducible.
    case setShuffle(enabled: Bool, seed: UInt64? = nil)
}

/// One item of the playlist.
struct PlaylistEntry: Codable, Equatable {
    var source: MediaSource
    /// A short label: the file name, or the URL's last segment.
    var title: String
}

struct PlaylistState: Codable, Equatable {
    var items: [PlaylistEntry] = []
    var currentIndex: Int?
    var `repeat` = RepeatMode.off
    var shuffle = false
    /// A folder or playlist file is being read.
    var scanning = false
}
