import AppKit

// The queue view projected by okp-session. Row indices address the session playlist.
enum UpNextRowState: String, Codable {
    case current, next, behind, upcoming
}

struct UpNextRow: Codable, Equatable {
    var index: Int
    var source: MediaSource
    var title: String
    var state: UpNextRowState
    var canPlayNext: Bool

    var location: String { source.location }
    var isCurrent: Bool { state == .current }
    var number: String { "\(index + 1)" }
}

struct UpNextList: Codable, Equatable {
    var rows: [UpNextRow] = []
    var currentIndex: Int?
    var heading: String?
    var scanning = false

    var nowPlaying: UpNextRow? { currentIndex.flatMap(row(at:)) }

    func row(at index: Int) -> UpNextRow? {
        rows.indices.contains(index) ? rows[index] : nil
    }

    func activation(ofRow row: Int) -> Command? {
        guard let entry = self.row(at: row), !entry.isCurrent else { return nil }
        return .playlist(action: .jump(index: entry.index))
    }

    func removal(ofRow row: Int) -> Command? {
        guard rows.count > 1, let entry = self.row(at: row), !entry.isCurrent else { return nil }
        return .playlist(action: .remove(index: entry.index))
    }

    func playNext(ofRow row: Int) -> Command? {
        guard let entry = self.row(at: row), entry.canPlayNext else { return nil }
        return .playlist(action: .playNext(index: entry.index))
    }

    /// AppKit reports an insertion boundary before a row (including the end).
    /// The session takes the final index after the source has been removed.
    func moving(from row: Int, before boundary: Int) -> Command? {
        guard let entry = self.row(at: row), !entry.isCurrent,
              (0...rows.count).contains(boundary) else { return nil }
        let destination = boundary > row ? boundary - 1 : boundary
        guard destination != row else { return nil }
        return .playlist(action: .move(from: entry.index, to: destination))
    }
}

/// The keys the Up Next list handles itself. The arrow keys move its selection;
/// every other key goes on to the video.
enum UpNextKey: Equatable {
    /// Return or Enter: play the selected item.
    case activate
    /// Delete or Forward Delete: remove the selected item.
    case remove

    static let returnKeyCode: UInt16 = 36
    static let enterKeyCode: UInt16 = 76
    static let deleteKeyCode: UInt16 = 51
    static let forwardDeleteKeyCode: UInt16 = 117
    /// Up, Down, Home, End, Page Up and Page Down: the list's own selection keys.
    static let navigationKeyCodes: Set<UInt16> = [126, 125, 115, 119, 116, 121]

    private static let chordModifiers: NSEvent.ModifierFlags = [.command, .control, .option, .shift]

    /// The key for a press, if the list handles it; a chord is never one.
    init?(keyCode: UInt16, modifiers: NSEvent.ModifierFlags) {
        guard modifiers.intersection(Self.chordModifiers).isEmpty else { return nil }
        switch keyCode {
        case Self.returnKeyCode, Self.enterKeyCode: self = .activate
        case Self.deleteKeyCode, Self.forwardDeleteKeyCode: self = .remove
        default: return nil
        }
    }
}
