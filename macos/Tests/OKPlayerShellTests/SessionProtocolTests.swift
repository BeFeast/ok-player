import Foundation
import XCTest
@testable import OKPlayerShell

/// The Rust session's golden documents (rust/crates/okp-session/fixtures), read and
/// written by the Swift models. Rust's own tests pin the same files, so a protocol
/// change on either side fails here until both agree.
enum GoldenFixtures {
    static let directory = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // OKPlayerShellTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // macos
        .deletingLastPathComponent()  // repository root
        .appendingPathComponent("rust/crates/okp-session/fixtures")

    static func data(_ name: String) throws -> Data {
        try Data(contentsOf: directory.appendingPathComponent(name))
    }

    static func decode<Value: Decodable>(_ type: Value.Type, _ name: String) throws -> Value {
        try SessionJSON.makeDecoder().decode(type, from: data(name))
    }

    /// The fixture as a JSON object without its `null` members: Swift leaves a nil
    /// optional out, which Rust reads the same way.
    static func object(_ name: String) throws -> NSDictionary {
        try XCTUnwrap(strippingNulls(JSONSerialization.jsonObject(with: data(name))) as? NSDictionary)
    }

    static func encodedObject<Value: Encodable>(_ value: Value) throws -> NSDictionary {
        let data = try SessionJSON.makeEncoder().encode(value)
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? NSDictionary)
    }

    private static func strippingNulls(_ value: Any) -> Any {
        if let dictionary = value as? [String: Any] {
            return dictionary.filter { !($0.value is NSNull) }.mapValues(strippingNulls)
        }
        if let array = value as? [Any] { return array.map(strippingNulls) }
        return value
    }
}

final class SessionProtocolTests: XCTestCase {
    static let commands: [String: Command] = [
        "command-close.json": .close,
        "command-invoke-command.json": .invokeCommand(id: "mute"),
        "command-open-local.json": .open(source: MediaSource(kind: .local, location: "/Users/example/Movies/film.mkv"),
                                         resumeFrom: 754.5, audioTrack: 2, subtitleTrack: 3),
        "command-open-url.json": .open(source: MediaSource(kind: .url, location: "https://example.com/stream.m3u8")),
        "command-seek-exact.json": .seek(seconds: 90.25, mode: .absolute, precision: .exact),
        "command-seek.json": .seek(seconds: -5, mode: .relative, precision: .keyframe),
        "command-select-track-off.json": .selectTrack(kind: .audio, id: nil),
        "command-select-track.json": .selectTrack(kind: .subtitle, id: 2),
        "command-set-mute.json": .setMute(muted: true),
        "command-set-paused.json": .setPaused(paused: true),
        "command-set-speed.json": .setSpeed(speed: 1.5),
        "command-set-subtitle-delay.json": .setSubtitleDelay(seconds: -0.25),
        "command-set-volume.json": .setVolume(volume: 85),
        "command-toggle-pause.json": .togglePause,
    ]

    static let replies: [String: DispatchResult] = [
        "reply-accepted.json": .accepted(requestId: 7),
        "reply-failed.json": .failed(requestId: 8, message: "error running command"),
        "reply-no-op.json": .noOp,
        "reply-perform.json": .perform,
        "reply-rejected.json": .rejected(reason: .noActiveMedia, message: "the command needs loaded media"),
    ]

    func testEveryFixtureIsCoveredHere() throws {
        let names = try FileManager.default.contentsOfDirectory(atPath: GoldenFixtures.directory.path)
            .filter { $0.hasSuffix(".json") }
        let covered = Set(Self.commands.keys).union(Self.replies.keys).union([
            "config.json", "enums.json", "events-batch.json", "events-empty.json",
            "query-snapshot.json", "query-reply-snapshot.json", "query-reply-rejected.json",
            "query-command-registry.json", "query-reply-command-registry.json",
            "query-shortcut-table.json", "query-reply-shortcut-table.json",
        ])
        XCTAssertFalse(names.isEmpty, "no fixtures found in \(GoldenFixtures.directory.path)")
        XCTAssertEqual(Set(names), covered, "a fixture was added or removed without a Swift test")
    }

    func testEveryCommandFixtureDecodesAndEncodesBackToTheSameDocument() throws {
        for (name, command) in Self.commands {
            let envelope = CommandEnvelope(command: command)
            XCTAssertEqual(try GoldenFixtures.decode(CommandEnvelope.self, name), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(name), name)
        }
    }

    func testEveryReplyFixtureDecodes() throws {
        for (name, result) in Self.replies {
            let reply = try GoldenFixtures.decode(DispatchReply.self, name)
            XCTAssertEqual(reply.schemaVersion, SessionJSON.schemaVersion, name)
            XCTAssertEqual(reply.result, result, name)
        }
    }

    func testTheSnapshotQueryRoundTrips() throws {
        let query = QueryEnvelope(query: .snapshot)
        XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, "query-snapshot.json"), query)
        XCTAssertEqual(try GoldenFixtures.encodedObject(query), try GoldenFixtures.object("query-snapshot.json"))
    }

    func testTheRegistryQueriesRoundTrip() throws {
        let queries: [String: Query] = [
            "query-command-registry.json": .commandRegistry(fullscreen: true, miniPlayer: false),
            "query-shortcut-table.json": .shortcutTable,
        ]
        for (name, query) in queries {
            let envelope = QueryEnvelope(query: query)
            XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, name), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(name), name)
        }
    }

    func testTheCommandRegistryReplyDecodesEveryRow() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-command-registry.json")
        guard case .commandRegistry(let commands) = reply.result else {
            return XCTFail("not a registry reply: \(reply.result)")
        }
        XCTAssertEqual(commands.count, 65)
        let none = ShortcutModifiers()
        XCTAssertEqual(commands.first, RegistryCommand(
            id: "play-pause", group: .playbackNavigation, menu: .topLevel, label: "Play / Pause",
            available: true, enabled: true, checked: false,
            shortcut: ShortcutChord(key: "space", modifiers: none, label: "Space")))
        XCTAssertEqual(commands.first { $0.id == "fullscreen" }, RegistryCommand(
            id: "fullscreen", group: .viewWindow, menu: .topLevel, label: "Exit fullscreen",
            available: true, enabled: true, checked: true,
            shortcut: ShortcutChord(key: "f", modifiers: none, label: "F")))
        XCTAssertEqual(commands.first { $0.id == "open-url" }, RegistryCommand(
            id: "open-url", group: .mediaFile, menu: .toolsAdvanced, label: "Open URL...",
            available: false, enabled: true, checked: false,
            shortcut: ShortcutChord(key: "o", modifiers: ShortcutModifiers(shift: true, cmd: true), label: "Shift+Cmd+O")))
        XCTAssertNil(try XCTUnwrap(commands.first { $0.id == "seek-back" }).menu, "a keyboard-only command has no menu")
        XCTAssertNil(try XCTUnwrap(commands.first { $0.id == "zoom-out" }).shortcut)
        // Written back, the rows are the fixture: no field was dropped on the way in.
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object("query-reply-command-registry.json"))
    }

    func testTheShortcutTableReplyDecodesEveryRow() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-shortcut-table.json")
        guard case .shortcutTable(let shortcuts) = reply.result else {
            return XCTFail("not a shortcut table reply: \(reply.result)")
        }
        XCTAssertEqual(shortcuts.count, 28)
        XCTAssertEqual(shortcuts.first { $0.action == "save-screenshot" }, ShortcutRow(
            action: "save-screenshot", command: "save-frame",
            primary: ShortcutChord(key: "c", modifiers: ShortcutModifiers(), label: "C"), secondary: nil))
        XCTAssertEqual(shortcuts.first { $0.action == "open-settings" }?.primary,
                       ShortcutChord(key: "comma", modifiers: ShortcutModifiers(cmd: true), label: "Cmd+,"))
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object("query-reply-shortcut-table.json"))
    }

    func testTheSnapshotReplyDecodesEveryField() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-snapshot.json")
        let expected = SessionSnapshot(
            status: .playing,
            source: MediaSource(kind: .local, location: "/Users/example/Movies/film.mkv"),
            timePos: 12.5, duration: 5400, volume: 100, mute: false, speed: 1, subtitleDelay: 0,
            tracks: [
                Track(id: 1, kind: .audio, selected: true, external: false, default: true, title: "Stereo", lang: "eng"),
                Track(id: 1, kind: .subtitle, selected: false, external: true, default: false, title: nil, lang: nil),
            ],
            chapters: [Chapter(index: 0, time: 0, title: "Opening"), Chapter(index: 1, time: 300, title: nil)],
            endReason: nil, title: "film.mkv",
            video: VideoGeometry(width: 1920, height: 800, displayAspect: 2.4),
            buffering: false, cacheAhead: 20)
        XCTAssertEqual(reply.result, .snapshot(snapshot: expected))
    }

    func testARejectedQueryDecodes() throws {
        let reply = try GoldenFixtures.decode(QueryReply.self, "query-reply-rejected.json")
        XCTAssertEqual(reply.result, .rejected(reason: .unsupportedSchemaVersion,
                                               message: "schema_version 2 is not supported; this session speaks 1"))
    }

    func testAnEventBatchAppliesEveryChangeAndKeepsErrorsInOrder() throws {
        let batch = try GoldenFixtures.decode(EventBatch.self, "events-batch.json")
        XCTAssertEqual(batch.events.count, 4)
        XCTAssertEqual(batch.events[1], .error(kind: .loadFailed, message: "loading failed", requestId: nil))
        XCTAssertEqual(batch.events[2], .error(kind: .commandFailed, message: "property unavailable", requestId: 4))
        XCTAssertEqual(batch.events[3], .shutdown)

        let store = SessionStore()
        store.replace(with: SessionSnapshot(timePos: 30, video: VideoGeometry(width: 640, height: 360, displayAspect: 16 / 9)))
        store.apply(batch)
        let snapshot = store.snapshot
        XCTAssertEqual(snapshot.status, .ended)
        XCTAssertEqual(snapshot.source, MediaSource(kind: .url, location: "https://example.com/a.mp4"))
        XCTAssertNil(snapshot.timePos, "a change to null clears the field")
        XCTAssertEqual(snapshot.duration, 61)
        XCTAssertEqual(snapshot.volume, 70)
        XCTAssertTrue(snapshot.mute)
        XCTAssertEqual(snapshot.speed, 2)
        XCTAssertEqual(snapshot.subtitleDelay, 0.5)
        XCTAssertEqual(snapshot.tracks, [Track(id: 2, kind: .audio, selected: true, external: false, default: false,
                                               title: nil, lang: "deu")])
        XCTAssertEqual(snapshot.chapters, [])
        XCTAssertEqual(snapshot.endReason, .eof)
        XCTAssertEqual(snapshot.title, "a.mp4")
        XCTAssertNil(snapshot.video)
        XCTAssertTrue(snapshot.buffering)
        XCTAssertEqual(snapshot.cacheAhead, 3.5)
        XCTAssertEqual(store.lastError, "property unavailable", "the latest error wins")
        XCTAssertTrue(store.shutDown)
    }

    func testAnEmptyBatchChangesNothing() throws {
        let batch = try GoldenFixtures.decode(EventBatch.self, "events-empty.json")
        XCTAssertEqual(batch, EventBatch(schemaVersion: 1, events: []))
    }

    func testTheConfigRoundTrips() throws {
        let config = SessionConfig(
            app: AppIdentity(name: "OK Player", platform: .macos),
            dataDir: "/Users/example/Library/Application Support/OK Player",
            configDir: "/Users/example/Library/Preferences/OK Player",
            cacheDir: "/Users/example/Library/Caches/OK Player",
            hwdec: "videotoolbox",
            mpvOptions: [MpvOption(name: "ao", value: "coreaudio")],
            shellCommands: ["open-file", "fullscreen"])
        XCTAssertEqual(try GoldenFixtures.decode(SessionConfig.self, "config.json"), config)
        XCTAssertEqual(try GoldenFixtures.encodedObject(config), try GoldenFixtures.object("config.json"))
    }

    func testEveryEnumValueMatchesTheRustList() throws {
        let lists = try XCTUnwrap(JSONSerialization.jsonObject(with: GoldenFixtures.data("enums.json")) as? [String: [String]])
        let swift: [String: [String]] = [
            "status": Status.allCases.map(\.rawValue),
            "end_reason": EndReason.allCases.map(\.rawValue),
            "reject_reason": RejectReason.allCases.map(\.rawValue),
            "error_kind": ErrorKind.allCases.map(\.rawValue),
            "track_kind": TrackKind.allCases.map(\.rawValue),
            "source_kind": SourceKind.allCases.map(\.rawValue),
            "seek_mode": SeekMode.allCases.map(\.rawValue),
            "seek_precision": SeekPrecision.allCases.map(\.rawValue),
            "command_group": CommandGroup.allCases.map(\.rawValue),
            "command_menu": CommandMenu.allCases.map(\.rawValue),
        ]
        XCTAssertEqual(swift, lists)
    }
}
