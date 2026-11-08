import AppKit

/// The system's recent documents: `NSDocumentController` in the app (File > Open
/// Recent and the Dock's recent items), a fake in tests.
protocol RecentDocumentList: AnyObject {
    var urls: [URL] { get }
    func note(_ url: URL)
    func clear()
}

final class SystemRecentDocumentList: RecentDocumentList {
    var urls: [URL] { NSDocumentController.shared.recentDocumentURLs }

    func note(_ url: URL) {
        NSDocumentController.shared.noteNewRecentDocumentURL(url)
    }

    func clear() {
        NSDocumentController.shared.clearRecentDocuments(nil)
    }
}

/// Adds each local file to the recent documents once it has loaded, whether the shell
/// or the session's playlist opened it. A private session adds nothing, and a file
/// opened during one is not added later when the session stops being private.
final class RecentDocuments {
    private let list: RecentDocumentList
    var privateSession = false
    private var current: MediaSource?

    init(list: RecentDocumentList) {
        self.list = list
    }

    func update(_ snapshot: SessionSnapshot) {
        guard snapshot.status == .playing || snapshot.status == .paused, let source = snapshot.source,
              source != current else { return }
        current = source
        guard source.kind == .local, !privateSession else { return }
        list.note(URL(fileURLWithPath: source.location))
    }
}

struct RecentMenuEntry: Equatable {
    var title: String
    var url: URL
}

enum OpenRecentMenuModel {
    /// One entry per file, newest first, titled with the file name; where two names
    /// are the same, the folder is added.
    static func entries(_ urls: [URL]) -> [RecentMenuEntry] {
        var counts: [String: Int] = [:]
        for url in urls { counts[url.lastPathComponent, default: 0] += 1 }
        return urls.map { url in
            let name = url.lastPathComponent
            let folder = url.deletingLastPathComponent().lastPathComponent
            let title = (counts[name] ?? 0) > 1 && !folder.isEmpty && folder != "/" ? "\(name) — \(folder)" : name
            return RecentMenuEntry(title: title, url: url)
        }
    }
}

/// File > Open Recent, rebuilt from the recent documents each time it opens.
final class OpenRecentMenu: NSObject, NSMenuDelegate {
    let item: NSMenuItem
    private let list: RecentDocumentList
    private let open: (URL) -> Void

    init(list: RecentDocumentList, open: @escaping (URL) -> Void) {
        self.list = list
        self.open = open
        item = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
        super.init()
        let menu = NSMenu(title: "Open Recent")
        menu.delegate = self
        item.submenu = menu
        rebuild(menu)
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        rebuild(menu)
    }

    func rebuild(_ menu: NSMenu) {
        menu.removeAllItems()
        let entries = OpenRecentMenuModel.entries(list.urls)
        for entry in entries {
            let entryItem = NSMenuItem(title: entry.title, action: #selector(openEntry(_:)), keyEquivalent: "")
            entryItem.target = self
            entryItem.representedObject = entry.url
            entryItem.toolTip = entry.url.path
            menu.addItem(entryItem)
        }
        if !entries.isEmpty { menu.addItem(.separator()) }
        // Without an action the item is disabled.
        let clear = NSMenuItem(title: "Clear Menu", action: entries.isEmpty ? nil : #selector(clearEntries(_:)),
                               keyEquivalent: "")
        clear.target = self
        menu.addItem(clear)
    }

    @objc func openEntry(_ sender: NSMenuItem) {
        guard let url = sender.representedObject as? URL else { return }
        open(url)
    }

    @objc func clearEntries(_ sender: NSMenuItem) {
        list.clear()
    }
}
