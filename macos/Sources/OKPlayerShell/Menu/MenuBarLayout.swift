import AppKit

// Where the registry's commands sit in the macOS menu bar (#880). The registry
// (the session's `command_registry` query) owns every player command's label,
// chord, enabled and checked state; this file only places command ids among the
// standard macOS items. A command it does not place goes to the menu of its
// registry group, so a new registry command still shows up.

/// The menus of the menu bar, in order.
enum MenuBarMenu: String, CaseIterable {
    case app, file, edit, view, playback, audio, subtitles, window, help

    var title: String {
        switch self {
        case .app: return MenuBarLayout.appName
        case .file: return "File"
        case .edit: return "Edit"
        case .view: return "View"
        case .playback: return "Playback"
        case .audio: return "Audio"
        case .subtitles: return "Subtitles"
        case .window: return "Window"
        case .help: return "Help"
        }
    }
}

/// A key equivalent: the key as a menu item takes it and the modifiers.
struct MenuKeyEquivalent: Equatable {
    var key: String
    var modifiers: NSEvent.ModifierFlags

    /// A chord's key equivalent; only Command chords are menu key equivalents
    /// (single keys are the player window's own, `ShortcutKeyMap`).
    init?(chord: ShortcutChord?) {
        guard let chord, chord.modifiers.cmd, let key = KeyNames.keyEquivalent(forKey: chord.key) else { return nil }
        self.init(key: key, modifiers: chord.modifiers.flags)
    }

    init(key: String, modifiers: NSEvent.ModifierFlags) {
        self.key = key
        self.modifiers = modifiers
    }
}

/// The standard macOS items, carried out by AppKit through the responder chain.
enum StandardMenuItem: Equatable {
    case about, services, hide, hideOthers, showAll, quit
    /// File > Open Recent, which the system integration (#886) provides.
    case openRecent
    case close
    case undo, redo, cut, copy, paste, delete, selectAll
    case minimize, zoom, bringAllToFront

    var title: String {
        switch self {
        case .about: return "About \(MenuBarLayout.appName)"
        case .services: return "Services"
        case .hide: return "Hide \(MenuBarLayout.appName)"
        case .hideOthers: return "Hide Others"
        case .showAll: return "Show All"
        case .quit: return "Quit \(MenuBarLayout.appName)"
        case .openRecent: return "Open Recent"
        case .close: return "Close"
        case .undo: return "Undo"
        case .redo: return "Redo"
        case .cut: return "Cut"
        case .copy: return "Copy"
        case .paste: return "Paste"
        case .delete: return "Delete"
        case .selectAll: return "Select All"
        case .minimize: return "Minimize"
        case .zoom: return "Zoom"
        case .bringAllToFront: return "Bring All to Front"
        }
    }

    /// The responder-chain action; nil for the two submenus.
    var action: Selector? {
        switch self {
        case .about: return #selector(NSApplication.orderFrontStandardAboutPanel(_:))
        case .services, .openRecent: return nil
        case .hide: return #selector(NSApplication.hide(_:))
        case .hideOthers: return #selector(NSApplication.hideOtherApplications(_:))
        case .showAll: return #selector(NSApplication.unhideAllApplications(_:))
        case .quit: return #selector(NSApplication.terminate(_:))
        case .close: return #selector(NSWindow.performClose(_:))
        case .undo: return Selector(("undo:"))
        case .redo: return Selector(("redo:"))
        case .cut: return #selector(NSText.cut(_:))
        case .copy: return #selector(NSText.copy(_:))
        case .paste: return #selector(NSText.paste(_:))
        case .delete: return #selector(NSText.delete(_:))
        case .selectAll: return #selector(NSText.selectAll(_:))
        case .minimize: return #selector(NSWindow.performMiniaturize(_:))
        case .zoom: return #selector(NSWindow.performZoom(_:))
        case .bringAllToFront: return #selector(NSApplication.arrangeInFront(_:))
        }
    }

    var keyEquivalent: MenuKeyEquivalent? {
        switch self {
        case .hide: return MenuKeyEquivalent(key: "h", modifiers: .command)
        case .hideOthers: return MenuKeyEquivalent(key: "h", modifiers: [.command, .option])
        case .quit: return MenuKeyEquivalent(key: "q", modifiers: .command)
        case .close: return MenuKeyEquivalent(key: "w", modifiers: .command)
        case .undo: return MenuKeyEquivalent(key: "z", modifiers: .command)
        case .redo: return MenuKeyEquivalent(key: "z", modifiers: [.command, .shift])
        case .cut: return MenuKeyEquivalent(key: "x", modifiers: .command)
        case .copy: return MenuKeyEquivalent(key: "c", modifiers: .command)
        case .paste: return MenuKeyEquivalent(key: "v", modifiers: .command)
        case .selectAll: return MenuKeyEquivalent(key: "a", modifiers: .command)
        case .minimize: return MenuKeyEquivalent(key: "m", modifiers: .command)
        case .about, .services, .showAll, .openRecent, .delete, .zoom, .bringAllToFront: return nil
        }
    }
}

/// One entry of a menu.
enum MenuBarEntry: Equatable {
    case standard(StandardMenuItem)
    /// A registry command, titled with its label; `submenu` when a provider fills it.
    case command(id: String, title: String, keyEquivalent: MenuKeyEquivalent?, submenu: Bool)
    case separator
}

/// One menu of the bar with its entries.
struct MenuBarModel: Equatable {
    var menu: MenuBarMenu
    var entries: [MenuBarEntry]
}

enum MenuBarLayout {
    static let appName = "OK Player"

    /// A place in a menu's layout.
    enum Slot: Equatable {
        case standard(StandardMenuItem)
        case command(String)
        case separator
        /// Where the menu's commands that no slot names go, in registry order.
        case others
    }

    /// Registry commands that are keys only and never menu items.
    static let keysOnly: Set<String> = ["escape-fullscreen"]

    /// Each menu's layout.
    static func slots(_ menu: MenuBarMenu) -> [Slot] {
        switch menu {
        case .app:
            return [.standard(.about), .separator, .command("open-settings"), .others, .separator,
                    .standard(.services), .separator, .standard(.hide), .standard(.hideOthers), .standard(.showAll),
                    .separator, .standard(.quit)]
        case .file:
            return [.command("open-file"), .command("open-folder"), .command("open-url"), .command("open-playlist"),
                    .standard(.openRecent), .separator,
                    .command("add-to-queue"), .command("play-next"), .command("save-playlist"), .separator,
                    .command("media-info"), .command("open-file-location"), .command("save-video"), .separator,
                    .command("save-frame"), .command("save-frame-with-subtitles"), .command("copy-frame"),
                    .command("export-clip"), .separator,
                    .command("private-session"), .command("clear-history"), .separator,
                    .others, .separator,
                    .command("close-media"), .standard(.close)]
        case .edit:
            return [.standard(.undo), .standard(.redo), .separator,
                    .standard(.cut), .standard(.copy), .standard(.paste), .standard(.delete), .standard(.selectAll),
                    .separator, .command("copy-current-time"), .others]
        case .view:
            return [.command("fullscreen"), .separator,
                    .command("aspect-auto"), .command("aspect-wide"), .command("aspect-standard"),
                    .command("aspect-cinema"), .separator,
                    .command("zoom-in"), .command("zoom-out"), .command("pan-left"), .command("pan-right"),
                    .command("pan-up"), .command("pan-down"), .command("center-image"), .separator,
                    .command("rotate-clockwise"), .command("fill-screen"), .command("deinterlace"),
                    .command("reset-video"), .separator,
                    .others]
        case .playback:
            return [.command("play-pause"), .separator,
                    .command("seek-back"), .command("seek-forward"), .command("frame-back"), .command("frame-forward"),
                    .command("go-to-time"), .separator,
                    .command("previous-item"), .command("next-item"), .command("chapters-up-next"), .separator,
                    .command("playback-speed"), .command("repeat-mode"), .command("shuffle"),
                    .command("auto-advance"), .command("ab-loop"), .command("add-bookmark"), .separator,
                    .others]
        case .audio:
            return [.command("volume-up"), .command("volume-down"), .command("mute"), .separator,
                    .command("audio-track"), .others]
        case .subtitles:
            return [.command("subtitles"), .command("add-subtitle"), .separator,
                    .command("subtitle-delay-forward"), .command("subtitle-delay-back"), .separator,
                    .command("subtitle-size-up"), .command("subtitle-size-down"), .separator,
                    .command("subtitle-previous-cue"), .command("subtitle-next-cue"), .separator,
                    .others]
        case .window:
            return [.standard(.minimize), .standard(.zoom), .separator,
                    .command("fit-window-to-media"), .command("mini-player"), .others, .separator,
                    .standard(.bringAllToFront)]
        case .help:
            return [.others]
        }
    }

    /// The menu a command that no slot names goes to, by its registry group.
    static func menu(for group: CommandGroup) -> MenuBarMenu {
        switch group {
        case .playbackNavigation, .tracksSubtitles: return .playback
        case .viewWindow: return .view
        case .mediaFile, .tools: return .file
        case .settings: return .app
        }
    }

    /// The menu bar for `registry`: every available command once, in its slot,
    /// titled with its label, with its Command chord as the key equivalent. An
    /// unavailable command is left out. `submenus` are the commands a provider
    /// shows as submenus.
    static func build(registry: [RegistryCommand], submenus: Set<String>) -> [MenuBarModel] {
        let shown = registry.filter { $0.available && !keysOnly.contains($0.id) }
        let placed = Set(MenuBarMenu.allCases.flatMap { menu in
            slots(menu).compactMap { slot -> String? in
                if case .command(let id) = slot { return id }
                return nil
            }
        })
        func entry(_ command: RegistryCommand) -> MenuBarEntry {
            .command(id: command.id, title: command.label, keyEquivalent: MenuKeyEquivalent(chord: command.shortcut),
                     submenu: submenus.contains(command.id))
        }
        return MenuBarMenu.allCases.map { (menu: MenuBarMenu) -> MenuBarModel in
            var entries: [MenuBarEntry] = []
            for slot in slots(menu) {
                switch slot {
                case .standard(let item):
                    entries.append(.standard(item))
                case .separator:
                    entries.append(.separator)
                case .command(let id):
                    if let command = shown.first(where: { $0.id == id }) { entries.append(entry(command)) }
                case .others:
                    for command in shown where !placed.contains(command.id) && self.menu(for: command.group) == menu {
                        entries.append(entry(command))
                    }
                }
            }
            return MenuBarModel(menu: menu, entries: collapsingSeparators(entries))
        }
    }

    /// No separator first, last or next to another.
    static func collapsingSeparators(_ entries: [MenuBarEntry]) -> [MenuBarEntry] {
        var result: [MenuBarEntry] = []
        for entry in entries {
            if entry == .separator, result.isEmpty || result.last == .separator { continue }
            result.append(entry)
        }
        if result.last == .separator { result.removeLast() }
        return result
    }
}
