import AppKit

// The Up Next panel's content as plain values (#884), so XCTest can check it
// without a window: the rows the session's playlist (docs/session-abi.md,
// "Playlist") lists under a pinned Now Playing row, and the playlist command a
// row asks for. The session owns the playlist; nothing here orders, steps or
// edits it.

/// One item of the playlist as the panel lists it.
struct UpNextRow: Equatable {
    /// The item's place in `PlaylistState.items`, which playlist commands take.
    var index: Int
    var title: String
    /// The file path or URL, for the tooltip.
    var location: String
    /// The item playing now. The panel neither plays it again nor removes it,
    /// and the session never removes it.
    var isCurrent: Bool

    /// The item's number in the list, shown before the title.
    var number: String { "\(index + 1)" }
}

/// What the panel shows for one playlist state.
struct UpNextList: Equatable {
    /// The pinned card for the current item.
    struct NowPlaying: Equatable {
        var title: String
        var location: String
    }

    var nowPlaying: NowPlaying?
    var rows: [UpNextRow]
    /// The label over the rows: how many items there are, and whether a folder or
    /// playlist file is still being read. Empty when there are none.
    var heading: String

    /// `mediaTitle` is libmpv's title for the open media (`SessionSnapshot.title`);
    /// without one the Now Playing card shows the item's own label.
    init(playlist: PlaylistState, mediaTitle: String?) {
        let current = playlist.currentIndex.flatMap { playlist.items.indices.contains($0) ? $0 : nil }
        rows = playlist.items.enumerated().map { index, entry in
            UpNextRow(index: index, title: entry.title, location: entry.source.location, isCurrent: index == current)
        }
        nowPlaying = current.map { index in
            let entry = playlist.items[index]
            let title = mediaTitle.flatMap { $0.isEmpty ? nil : $0 } ?? entry.title
            return NowPlaying(title: title, location: entry.source.location)
        }
        let count = playlist.items.count
        let items = count == 1 ? "1 ITEM" : "\(count) ITEMS"
        heading = count == 0 ? "" : playlist.scanning ? "\(items) · SCANNING…" : items
    }

    /// An empty playlist.
    init() {
        self.init(playlist: PlaylistState(), mediaTitle: nil)
    }

    /// What plays the item at `row`: a jump, except for the current item, which is
    /// already playing. Nil for a row that is not there.
    func activation(ofRow row: Int) -> Command? {
        guard rows.indices.contains(row), !rows[row].isCurrent else { return nil }
        return .playlist(action: .jump(index: rows[row].index))
    }

    /// What takes the item at `row` out of the playlist; nil for the current item,
    /// which stays, and for a row that is not there.
    func removal(ofRow row: Int) -> Command? {
        guard rows.indices.contains(row), !rows[row].isCurrent else { return nil }
        return .playlist(action: .remove(index: rows[row].index))
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
