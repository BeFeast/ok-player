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

    /// Every fixture, by its path below `directory` (`history/events.json`).
    static func names() throws -> [String] {
        let walker = try XCTUnwrap(FileManager.default.enumerator(atPath: directory.path))
        return walker.compactMap { $0 as? String }.filter { $0.hasSuffix(".json") }
    }

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
        "history/command-remove.json": .history(action: .remove(source: MediaSource(kind: .local, location: "/Users/example/Movies/film.mkv"))),
        "history/command-set-private.json": .history(action: .setPrivate(enabled: true)),
        "playlist-command-open.json": .playlist(action: .open(sources: [
            MediaSource(kind: .local, location: "/Users/example/Movies/Show/Episode 1.mkv"),
            MediaSource(kind: .local, location: "/Users/example/Movies/Extras"),
            MediaSource(kind: .local, location: "/Users/example/Music/queue.m3u8"),
            MediaSource(kind: .url, location: "https://example.com/clip.mp4"),
        ])),
        "playlist-command-next.json": .playlist(action: .next),
        "playlist-command-previous.json": .playlist(action: .previous),
        "playlist-command-jump.json": .playlist(action: .jump(index: 3)),
        "playlist-command-queue.json": .playlist(action: .queue(
            sources: [MediaSource(kind: .local, location: "/Users/example/Movies/next.mkv")], mode: .playNext)),
        "playlist-command-play-next.json": .playlist(action: .playNext(index: 4)),
        "playlist-command-move.json": .playlist(action: .move(from: 2, to: 0)),
        "playlist-command-remove.json": .playlist(action: .remove(index: 1)),
        "playlist-command-clear-queue.json": .playlist(action: .clearQueue),
        "playlist-command-set-repeat.json": .playlist(action: .setRepeat(mode: .one)),
        "playlist-command-set-shuffle.json": .playlist(action: .setShuffle(enabled: true, seed: 42)),
        "playlist-command-set-shuffle-unseeded.json": .playlist(action: .setShuffle(enabled: false)),
    ]

    static let replies: [String: DispatchResult] = [
        "reply-accepted.json": .accepted(requestId: 7),
        "reply-failed.json": .failed(requestId: 8, message: "error running command"),
        "reply-no-op.json": .noOp,
        "reply-perform.json": .perform,
        "reply-rejected.json": .rejected(reason: .noActiveMedia, message: "the command needs loaded media"),
        "history/reply-done.json": .history(result: .done),
        "history/reply-unchanged.json": .history(result: .unchanged),
        "history/reply-failed.json": .history(result: .failed(message: "the history could not be saved: Permission denied")),
        "playlist-reply-applied.json": .applied,
    ]

    static let queries: [String: Query] = [
        "query-snapshot.json": .snapshot,
        "query-command-registry.json": .commandRegistry(fullscreen: true, miniPlayer: false),
        "query-shortcut-table.json": .shortcutTable,
        "history/query-page.json": .history(query: .page(search: "severance", utcOffsetSeconds: -18000)),
        "history/query-welcome-shelf.json": .history(query: .welcomeShelf(limit: 8, utcOffsetSeconds: 7200)),
        "playlist-query.json": .playlist,
    ]

    func testEveryFixtureIsCoveredHere() throws {
        let names = try GoldenFixtures.names()
        let covered = Set(Self.commands.keys).union(Self.replies.keys).union(Self.queries.keys).union([
            "config.json", "enums.json", "events-batch.json", "events-empty.json",
            "query-reply-snapshot.json", "query-reply-rejected.json",
            "query-reply-command-registry.json", "query-reply-shortcut-table.json",
            "history/enums.json", "history/events.json",
            "history/query-reply-page.json", "history/query-reply-welcome-shelf.json",
            "playlist-enums.json", "playlist-events.json", "playlist-query-reply.json", "playlist-query-reply-empty.json",
        ]).union(TransportProtocolTests.covered.map { "transport/\($0)" })
            .union(StatusProtocolTests.covered.map { "status/\($0)" })
            .union(WindowSessionQueryTests.fixtures)
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

    func testEveryQueryFixtureDecodesAndEncodesBackToTheSameDocument() throws {
        for (name, query) in Self.queries {
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

    func testTheHistoryShelfReplyDecodesEveryField() throws {
        let name = "history/query-reply-welcome-shelf.json"
        let reply = try GoldenFixtures.decode(QueryReply.self, name)
        let entry = HistoryEntry(
            source: MediaSource(kind: .local, location: "/Users/example/Movies/Severance/Season 02/Severance S02E03.mkv"),
            title: "Severance S02E03", location: "Severance › Season 02",
            position: 1325.5, duration: 3120, progress: 0.42, state: .progress,
            stateLabel: "30m left", whenLabel: "Today 21:14", updatedAtUnix: 1_699_990_000, resumeFrom: 1325.5)
        XCTAssertEqual(reply.result, .history(result: .welcomeShelf(privateSession: false, entries: [entry])))
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object(name))
    }

    func testTheHistoryPageReplyDecodesEveryField() throws {
        let name = "history/query-reply-page.json"
        let reply = try GoldenFixtures.decode(QueryReply.self, name)
        let entry = HistoryEntry(
            source: MediaSource(kind: .url, location: "https://example.com/watch?v=abc123"),
            title: "Example talk", location: "https://example.com/watch?v=abc123",
            position: 0, duration: 900, progress: 0, state: .finished,
            stateLabel: "Finished", whenLabel: "12 Jun", updatedAtUnix: 1_686_528_000, resumeFrom: nil)
        XCTAssertEqual(reply.result, .history(result: .page(
            privateSession: true, readFailed: false, unreadableCopy: nil,
            summary: "Everything you’ve opened · kept forever", endCap: "End of history · kept forever",
            sections: [HistorySection(bucket: .earlier, header: "EARLIER", entries: [entry])])))
        XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object(name))
    }

    func testHistoryEventsDecodeAndLeaveTheSnapshotAlone() throws {
        let batch = try GoldenFixtures.decode(EventBatch.self, "history/events.json")
        XCTAssertEqual(batch.events, [
            .history(event: .resumed(position: 754.5)),
            .history(event: .changed),
            .history(event: .privateSession(enabled: true)),
            .history(event: .saveFailed(message: "the history could not be saved: No space left on device")),
        ])
        let store = SessionStore()
        store.apply(batch)
        XCTAssertEqual(store.snapshot, SessionSnapshot())
        XCTAssertNil(store.lastError)
        XCTAssertFalse(store.shutDown)
    }

    func testThePlaylistRepliesDecodeEveryField() throws {
        let replies: [String: PlaylistState] = [
            "playlist-query-reply.json": PlaylistState(
                items: [
                    PlaylistEntry(source: MediaSource(kind: .local, location: "/Users/example/Movies/Show/Episode 1.mkv"),
                                  title: "Episode 1.mkv"),
                    PlaylistEntry(source: MediaSource(kind: .local, location: "/Users/example/Movies/Show/Episode 2.mkv"),
                                  title: "Episode 2.mkv"),
                    PlaylistEntry(source: MediaSource(kind: .url, location: "https://example.com/clip.mp4?token=a"),
                                  title: "clip.mp4"),
                ],
                currentIndex: 1, repeat: .all, shuffle: true, scanning: false),
            "playlist-query-reply-empty.json": PlaylistState(),
        ]
        for (name, playlist) in replies {
            let reply = try GoldenFixtures.decode(QueryReply.self, name)
            XCTAssertEqual(reply.result, .playlist(playlist: playlist), name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(reply), try GoldenFixtures.object(name), name)
        }
    }

    func testAPlaylistEventFollowsTheSnapshotChangesAndLeavesTheSnapshotAlone() throws {
        let batch = try GoldenFixtures.decode(EventBatch.self, "playlist-events.json")
        let playlist = PlaylistState(
            items: [PlaylistEntry(source: MediaSource(kind: .local, location: "/Users/example/Movies/Show/Episode 2.mkv"),
                                  title: "Episode 2.mkv")],
            currentIndex: 0, repeat: .off, shuffle: false, scanning: true)
        XCTAssertEqual(batch.events, [.snapshotChanged(changes: [.status(value: .opening)]),
                                      .playlistChanged(playlist: playlist)])
        let store = SessionStore()
        store.apply(batch)
        XCTAssertEqual(store.snapshot, SessionSnapshot(status: .opening))
        XCTAssertEqual(store.playlist, playlist, "the store keeps the playlist the event carries")
    }

    func testTheStoreFollowsThePrivateSession() throws {
        let store = SessionStore()
        XCTAssertFalse(store.privateSession, "every session starts with it off")
        store.apply(try GoldenFixtures.decode(EventBatch.self, "history/events.json"))
        XCTAssertTrue(store.privateSession)
        store.apply(EventBatch(schemaVersion: 1, events: [.history(event: .privateSession(enabled: false))]))
        XCTAssertFalse(store.privateSession)
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

    func testAnUnknownEventIsSkippedAndTheRestOfTheBatchKept() throws {
        let json = #"""
            {"schema_version": 1, "events": [
              {"snapshot_changed": {"changes": [{"mute": {"value": true}}]}},
              {"a_later_event": {"anything": [1, 2]}},
              {"error": {"kind": "engine", "message": "gone", "request_id": null}},
              {"shutdown": {}}
            ]}
            """#
        let batch = try SessionJSON.makeDecoder().decode(EventBatch.self, from: Data(json.utf8))
        XCTAssertEqual(batch.events, [
            .snapshotChanged(changes: [.mute(value: true)]),
            .error(kind: .engine, message: "gone", requestId: nil),
            .shutdown,
        ])
    }

    func testAKnownEventThatDoesNotDecodeFailsTheBatch() {
        let json = #"{"schema_version": 1, "events": [{"shutdown": {}}, {"error": {"kind": "engine"}}]}"#
        XCTAssertThrowsError(try SessionJSON.makeDecoder().decode(EventBatch.self, from: Data(json.utf8)))
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
            "video_click_intent": VideoClickIntent.allCases.map(\.rawValue),
            "command_group": CommandGroup.allCases.map(\.rawValue),
            "command_menu": CommandMenu.allCases.map(\.rawValue),
        ]
        XCTAssertEqual(swift, lists)
    }

    func testEveryHistoryEnumValueMatchesTheRustList() throws {
        let lists = try XCTUnwrap(JSONSerialization.jsonObject(with: GoldenFixtures.data("history/enums.json")) as? [String: [String]])
        XCTAssertEqual(lists, [
            "history_state": HistoryState.allCases.map(\.rawValue),
            "history_bucket": HistoryBucket.allCases.map(\.rawValue),
        ])
    }

    func testEveryPlaylistEnumValueMatchesTheRustList() throws {
        let lists = try XCTUnwrap(JSONSerialization.jsonObject(with: GoldenFixtures.data("playlist-enums.json")) as? [String: [String]])
        XCTAssertEqual(lists, [
            "repeat_mode": RepeatMode.allCases.map(\.rawValue),
            "queue_mode": QueueMode.allCases.map(\.rawValue),
        ])
    }
}
