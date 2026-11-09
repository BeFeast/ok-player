import COKPCore
import Foundation
import Observation

/// The shell's view of the session: the snapshot and the playlist as of the last
/// drain, whether the private session is on, the latest error the session reported,
/// and whether the engine has shut down. It changes only on the main queue, when
/// `SessionClient` drains a batch. SwiftUI islands bind to it; AppKit code reads it
/// from `SessionClient.onBatch`.
@Observable
final class SessionStore {
    private(set) var snapshot = SessionSnapshot()
    private(set) var playlist = PlaylistState()
    /// Follows the `history.private_session` event; every session starts with it off.
    private(set) var privateSession = false
    private(set) var lastError: String?
    private(set) var shutDown = false

    func replace(with snapshot: SessionSnapshot) {
        self.snapshot = snapshot
    }

    func replace(playlist: PlaylistState) {
        self.playlist = playlist
    }

    func apply(_ batch: EventBatch) {
        for event in batch.events {
            switch event {
            case .snapshotChanged(let changes):
                for change in changes { snapshot.apply(change) }
            case .playlistChanged(let playlist):
                // The whole playlist as it is at the drain.
                self.playlist = playlist
            case .history(.privateSession(let enabled)):
                privateSession = enabled
            case .history:
                // A shell showing the history queries it again.
                break
            case .notice:
                // Shown by the status layer; nothing to keep.
                break
            case .error(_, let message, _):
                lastError = message
            case .shutdown:
                shutDown = true
            }
        }
    }
}

struct SessionClientError: Error, CustomStringConvertible {
    let description: String
}

/// The Rust session behind the `okp_session_*` C ABI, used from the main thread.
///
/// The session's event thread calls the wakeup once per batch; the wakeup only hops
/// to the main queue, where the batch is drained, decoded, applied to `store` and
/// handed to `onBatch`. Nothing polls.
final class SessionClient {
    let store = SessionStore()
    /// Called on the main queue after each drained batch was applied to `store`.
    var onBatch: ((EventBatch) -> Void)?
    /// The session handle, for the render layer's `okp_session_render_*` calls.
    /// `nil` once closed.
    private(set) var handle: OpaquePointer?
    private let mailbox = WakeupMailbox()

    init(config: SessionConfig) throws {
        let json = try SessionClient.encode(config)
        var error = [CChar](repeating: 0, count: 2048)
        let created = json.withCString { okp_session_new($0, &error, numericCast(error.count)) }
        guard let created else {
            throw SessionClientError(description: String(cString: error))
        }
        handle = created
        // The state so far, so the drained changes apply to it.
        if case .snapshot(let snapshot)? = query(.snapshot) {
            store.replace(with: snapshot)
        }
        if case .playlist(let playlist)? = query(.playlist) {
            store.replace(playlist: playlist)
        }
        mailbox.client = self
        okp_session_set_wakeup(created, sessionWakeup, Unmanaged.passUnretained(mailbox).toOpaque())
    }

    deinit { close() }

    @discardableResult
    func dispatch(_ command: Command) -> DispatchResult {
        guard let handle else {
            return .rejected(reason: .invalidArgument, message: "the session is closed")
        }
        do {
            let json = try SessionClient.encode(CommandEnvelope(command: command))
            let reply = json.withCString { okp_session_dispatch(handle, $0) }
            return try SessionClient.decode(DispatchReply.self, from: reply).result
        } catch {
            return .rejected(reason: .malformed, message: "\(error)")
        }
    }

    func query(_ query: Query) -> QueryResult? {
        guard let handle else { return nil }
        do {
            let json = try SessionClient.encode(QueryEnvelope(query: query))
            let reply = json.withCString { okp_session_query(handle, $0) }
            return try SessionClient.decode(QueryReply.self, from: reply).result
        } catch {
            return .rejected(reason: .malformed, message: "\(error)")
        }
    }

    /// Take the waiting batch. Runs on the main queue after a wakeup.
    func drainEvents() {
        guard let handle else { return }
        let batch: EventBatch
        do {
            batch = try SessionClient.decode(EventBatch.self, from: okp_session_drain_events(handle))
        } catch {
            batch = EventBatch(schemaVersion: SessionJSON.schemaVersion,
                               events: [.error(kind: .engine, message: "Unreadable session events: \(error)", requestId: nil)])
        }
        store.apply(batch)
        onBatch?(batch)
    }

    /// Remove the wakeup and free the session. Destroy the render context first.
    func close() {
        guard let handle else { return }
        // When this returns the wakeup is not running and never runs again.
        okp_session_set_wakeup(handle, nil, nil)
        okp_session_free(handle)
        self.handle = nil
    }

    static func encode<Value: Encodable>(_ value: Value) throws -> String {
        String(decoding: try SessionJSON.makeEncoder().encode(value), as: UTF8.self)
    }

    /// Decode a document the session returned and release it.
    static func decode<Value: Decodable>(_ type: Value.Type, from text: UnsafeMutablePointer<CChar>?) throws -> Value {
        guard let text else { throw SessionClientError(description: "the session returned no document") }
        defer { okp_string_free(text) }
        return try SessionJSON.makeDecoder().decode(type, from: Data(bytes: text, count: strlen(text)))
    }
}

/// What the session's event thread holds: a weak way back to the client, which
/// keeps the mailbox alive for as long as the wakeup is installed.
private final class WakeupMailbox {
    weak var client: SessionClient?

    func wake() {
        DispatchQueue.main.async { [weak self] in self?.client?.drainEvents() }
    }
}

/// The C wakeup. It runs on the session's event thread and only schedules a drain.
private func sessionWakeup(_ ctx: UnsafeMutableRawPointer?) {
    guard let ctx else { return }
    Unmanaged<WakeupMailbox>.fromOpaque(ctx).takeUnretainedValue().wake()
}
