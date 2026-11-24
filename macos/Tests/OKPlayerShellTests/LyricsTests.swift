import AppKit
import XCTest
@testable import OKPlayerShell

final class LyricsTests: XCTestCase {
    static let fixtures: [String] = [
        "lyrics/enums.json",
        "lyrics/events.json",
        "lyrics/query-lyrics.json",
        "lyrics/query-lyrics-at.json",
        "lyrics/query-reply-synced.json",
        "lyrics/query-reply-plain.json",
        "lyrics/query-reply-missing.json",
        "lyrics/query-reply-invalid.json",
    ]

    func testGoldenLyricsProtocolDocumentsRoundTrip() throws {
        let queries: [(String, Query)] = [
            ("lyrics/query-lyrics.json", .lyrics(at: nil)),
            ("lyrics/query-lyrics-at.json", .lyrics(at: 17.5)),
        ]
        for (name, query) in queries {
            let envelope = QueryEnvelope(query: query)
            XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, name), envelope, name)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(name), name)
        }

        let synced = try GoldenFixtures.decode(QueryReply.self, "lyrics/query-reply-synced.json")
        XCTAssertEqual(synced, QueryReply(schemaVersion: SessionJSON.schemaVersion, result: .lyrics(
            lyrics: LyricsSnapshot(
                source: MediaSource(kind: .local, location: "/Users/example/Music/Neon Skyline.mp3"),
                generation: 2, header: "LYRICS", state: .synced, message: nil,
                lines: [
                    LyricsLineView(timeSeconds: 0.0, text: "City lights blur into the rain", isGap: false),
                    LyricsLineView(timeSeconds: 5.4, text: "♪", isGap: true),
                    LyricsLineView(timeSeconds: 16.8, text: "A neon skyline out of sight", isGap: false),
                ],
                activeIndex: 2))))
        XCTAssertEqual(try GoldenFixtures.encodedObject(synced), try GoldenFixtures.object("lyrics/query-reply-synced.json"))

        let plain = try GoldenFixtures.decode(QueryReply.self, "lyrics/query-reply-plain.json")
        XCTAssertEqual(plain, QueryReply(schemaVersion: SessionJSON.schemaVersion, result: .lyrics(
            lyrics: LyricsSnapshot(
                source: MediaSource(kind: .local, location: "/Users/example/Music/Plain Song.flac"),
                generation: 3, header: "LYRICS · NOT SYNCED", state: .plain, message: nil,
                lines: [
                    LyricsLineView(timeSeconds: 0.0, text: "Wander down the boulevard", isGap: false),
                    LyricsLineView(timeSeconds: 0.0, text: "Counting every falling star", isGap: false),
                ],
                activeIndex: nil))))
        XCTAssertEqual(try GoldenFixtures.encodedObject(plain), try GoldenFixtures.object("lyrics/query-reply-plain.json"))

        let missing = try GoldenFixtures.decode(QueryReply.self, "lyrics/query-reply-missing.json")
        XCTAssertEqual(missing, QueryReply(schemaVersion: SessionJSON.schemaVersion, result: .lyrics(
            lyrics: LyricsSnapshot(
                source: MediaSource(kind: .local, location: "/Users/example/Music/Instrumental.mp3"),
                generation: 4, header: "LYRICS", state: .missing, message: "No lyrics found for this track",
                lines: [], activeIndex: nil))))
        XCTAssertEqual(try GoldenFixtures.encodedObject(missing), try GoldenFixtures.object("lyrics/query-reply-missing.json"))

        let invalid = try GoldenFixtures.decode(QueryReply.self, "lyrics/query-reply-invalid.json")
        XCTAssertEqual(invalid, QueryReply(schemaVersion: SessionJSON.schemaVersion, result: .lyrics(
            lyrics: LyricsSnapshot(
                source: MediaSource(kind: .local, location: "/Users/example/Music/Corrupt.mp3"),
                generation: 5, header: "LYRICS", state: .invalid, message: "Local lyrics file could not be read",
                lines: [], activeIndex: nil))))
        XCTAssertEqual(try GoldenFixtures.encodedObject(invalid), try GoldenFixtures.object("lyrics/query-reply-invalid.json"))

        let events = try GoldenFixtures.decode(EventBatch.self, "lyrics/events.json")
        XCTAssertEqual(events.events, [.lyricsChanged])
        let store = SessionStore()
        store.apply(events)
        XCTAssertEqual(store.snapshot, SessionSnapshot())

        let lists = try XCTUnwrap(JSONSerialization.jsonObject(with: GoldenFixtures.data("lyrics/enums.json")) as? [String: [String]])
        XCTAssertEqual(lists, ["lyrics_state": LyricsState.allCases.map(\.rawValue)])
    }

    func testSyncedLyricsViewHighlightsScrollsAndSeeksOnClick() {
        _ = NSApplication.shared
        let panel = LyricsPanelView()
        panel.reduceMotion = true
        panel.isHidden = false
        panel.frame = NSRect(x: 0, y: 0, width: 346, height: 240)

        let source = MediaSource(kind: .local, location: "/music/neon.mp3")
        let lines = (0..<16).map { index in
            LyricsLineView(timeSeconds: Double(index) * 1.5, text: "Synced line \(index)", isGap: false)
        }
        var sought: [Double] = []
        panel.onSeek = { sought.append($0) }

        var view = LyricsSnapshot(source: source, generation: 1, header: "LYRICS", state: .synced,
                                  message: nil, lines: lines, activeIndex: 0)
        panel.show(view)
        panel.layoutSubtreeIfNeeded()
        XCTAssertEqual(panel.syncedRowButtons.count, 16)
        XCTAssertTrue(panel.syncedRowButtons[0].isActiveLine)
        XCTAssertFalse(panel.syncedRowButtons[10].isActiveLine)

        // Advancing moves highlight and auto-scrolls the later line into view.
        view.activeIndex = 12
        panel.show(view)
        panel.layoutSubtreeIfNeeded()
        XCTAssertFalse(panel.syncedRowButtons[0].isActiveLine)
        XCTAssertTrue(panel.syncedRowButtons[12].isActiveLine)
        XCTAssertGreaterThan(panel.scrollOffsetY, 0)
        XCTAssertTrue(panel.isRowVisibleInViewport(12))

        // Seeking backward moves highlight and scroll back toward the start.
        let deepOffset = panel.scrollOffsetY
        view.activeIndex = 1
        panel.show(view)
        panel.layoutSubtreeIfNeeded()
        XCTAssertTrue(panel.syncedRowButtons[1].isActiveLine)
        XCTAssertLessThan(panel.scrollOffsetY, deepOffset)
        XCTAssertTrue(panel.isRowVisibleInViewport(1))

        // Clicking a timed line dispatches seek to its timestamp.
        panel.syncedRowButtons[7].performClick(nil)
        XCTAssertEqual(sought, [10.5])
    }

    func testPlainMissingInvalidAndStaleTrackRejection() {
        _ = NSApplication.shared
        let controller = LyricsController()
        controller.panel.reduceMotion = true
        controller.panel.frame = NSRect(x: 0, y: 0, width: 346, height: 240)

        let trackA = MediaSource(kind: .local, location: "/music/track-a.mp3")
        let trackB = MediaSource(kind: .local, location: "/music/track-b.mp3")
        let trackC = MediaSource(kind: .local, location: "/music/track-c.mp3")
        let audio = AudioMetadata(title: "Track", artist: "Artist", album: "Album")

        var reply = LyricsSnapshot(
            source: trackA, generation: 1, header: "LYRICS", state: .synced, message: nil,
            lines: [LyricsLineView(timeSeconds: 1.0, text: "Track A line", isGap: false)],
            activeIndex: 0)
        controller.query = { _ in .lyrics(lyrics: reply) }

        var snapshot = SessionSnapshot(status: .playing, source: trackA, timePos: 1.5, duration: 20)
        controller.sessionChanged(snapshot: snapshot, audioMetadata: audio,
                                  batch: EventBatch(schemaVersion: 1, events: [.lyricsChanged]))
        XCTAssertTrue(controller.show())
        XCTAssertEqual(controller.panel.displayedState, .synced)
        XCTAssertEqual(controller.panel.syncedRowButtons.count, 1)

        // Switching to Track B while the query still returns stale Track A data must reject it.
        snapshot.source = trackB
        controller.sessionChanged(snapshot: snapshot, audioMetadata: audio,
                                  batch: EventBatch(schemaVersion: 1, events: [.lyricsChanged]))
        XCTAssertEqual(controller.panel.displayedState, .unavailable)
        XCTAssertTrue(controller.panel.syncedRowButtons.isEmpty, "stale Track A rows must never linger on Track B")

        // Plain LRC renders readable labels without seek buttons.
        reply = LyricsSnapshot(
            source: trackB, generation: 2, header: "LYRICS · NOT SYNCED", state: .plain, message: nil,
            lines: [
                LyricsLineView(timeSeconds: 0, text: "Plain verse one", isGap: false),
                LyricsLineView(timeSeconds: 0, text: "Plain verse two", isGap: false),
            ],
            activeIndex: nil)
        controller.sessionChanged(snapshot: snapshot, audioMetadata: audio,
                                  batch: EventBatch(schemaVersion: 1, events: [.lyricsChanged]))
        XCTAssertEqual(controller.panel.displayedState, .plain)
        XCTAssertEqual(controller.panel.headerText, "LYRICS · NOT SYNCED")
        XCTAssertEqual(controller.panel.plainRowLabels.map(\.stringValue), ["Plain verse one", "Plain verse two"])
        XCTAssertTrue(controller.panel.syncedRowButtons.isEmpty)

        // Invalid / malformed sidecar shows honest fallback and clears previous rows.
        snapshot.source = trackC
        reply = LyricsSnapshot(
            source: trackC, generation: 3, header: "LYRICS", state: .invalid,
            message: "Local lyrics file could not be read", lines: [], activeIndex: nil)
        controller.sessionChanged(snapshot: snapshot, audioMetadata: audio,
                                  batch: EventBatch(schemaVersion: 1, events: [.lyricsChanged]))
        XCTAssertEqual(controller.panel.displayedState, .invalid)
        XCTAssertEqual(controller.panel.fallbackText, "Local lyrics file could not be read")
        XCTAssertTrue(controller.panel.plainRowLabels.isEmpty)

        // Missing sidecar shows honest fallback.
        reply = LyricsSnapshot(
            source: trackC, generation: 4, header: "LYRICS", state: .missing,
            message: "No lyrics found for this track", lines: [], activeIndex: nil)
        controller.sessionChanged(snapshot: snapshot, audioMetadata: audio,
                                  batch: EventBatch(schemaVersion: 1, events: [.lyricsChanged]))
        XCTAssertEqual(controller.panel.displayedState, .missing)
        XCTAssertEqual(controller.panel.fallbackText, "No lyrics found for this track")
    }
}
