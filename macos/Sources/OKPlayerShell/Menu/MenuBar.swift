import AppKit

/// The macOS menu bar (#880): App, File, Edit, View, Playback, Audio, Subtitles,
/// Window and Help, laid out by `MenuBarLayout` from the session's command
/// registry. A registry item sends `performPlayerCommand(_:)` through the
/// responder chain, whose validation takes the registry's label, check mark and
/// enabled state (`PlayerCommands.validate`). The standard items use AppKit's own
/// actions, so text fields get Undo, Cut, Copy, Paste and Select All, and the
/// Window and Help menus are AppKit's window list and help search.
final class MenuBar: NSObject, NSMenuDelegate {
    let mainMenu = NSMenu(title: "Main Menu")
    /// Each menu of the bar as last built.
    private(set) var menus: [MenuBarMenu: NSMenu] = [:]
    private(set) var servicesMenu: NSMenu?
    private let commands: PlayerCommands
    private let openRecent: NSMenuItem?
    private let history: NSMenuItem?
    /// The registry command each provider-filled submenu stands for.
    private var dynamic: [ObjectIdentifier: String] = [:]

    /// `openRecent` and `history` are the items other parts of the app provide for
    /// File > Open Recent and File > History…; nil leaves them out.
    init(commands: PlayerCommands, openRecent: NSMenuItem?, history: NSMenuItem? = nil) {
        self.commands = commands
        self.openRecent = openRecent
        self.history = history
        super.init()
    }

    /// Build the bar from the registry as it is now and make it the app's menu
    /// bar. Call it again once the session is up: until then only the standard
    /// items are there.
    func install() {
        // AppKit would add an Enter Full Screen item of its own to the View menu,
        // next to the registry's fullscreen command.
        UserDefaults.standard.register(defaults: ["NSFullScreenMenuItemEverywhere": false])
        rebuild()
        NSApp.mainMenu = mainMenu
        NSApp.servicesMenu = servicesMenu
        NSApp.windowsMenu = menus[.window]
        NSApp.helpMenu = menus[.help]
    }

    /// Build every menu again from the registry.
    func rebuild() {
        mainMenu.removeAllItems()
        menus = [:]
        dynamic = [:]
        servicesMenu = nil
        // The Open Recent item moves to the new File menu.
        for item in [openRecent, history].compactMap({ $0 }) { item.menu?.removeItem(item) }
        for model in MenuBarLayout.build(registry: commands.registry, submenus: commands.submenuCommands) {
            let menu = NSMenu(title: model.menu.title)
            menu.delegate = self
            for entry in model.entries {
                if let item = makeItem(entry) { menu.addItem(item) }
            }
            let item = NSMenuItem(title: model.menu.title, action: nil, keyEquivalent: "")
            item.submenu = menu
            mainMenu.addItem(item)
            menus[model.menu] = menu
        }
    }

    /// A menu is about to open: its registry items are validated against the
    /// session's state now, and a provider's submenu is filled again.
    func menuNeedsUpdate(_ menu: NSMenu) {
        commands.invalidate()
        guard let id = dynamic[ObjectIdentifier(menu)], let provider = commands.submenuProvider(for: id),
              let row = commands.row(id) else { return }
        menu.removeAllItems()
        for item in provider.submenuItems(for: row) { menu.addItem(item) }
    }

    private func makeItem(_ entry: MenuBarEntry) -> NSMenuItem? {
        switch entry {
        case .separator:
            return NSMenuItem.separator()
        case .standard(.openRecent):
            return openRecent
        case .standard(.history):
            return history
        case .standard(.services):
            let services = NSMenu(title: StandardMenuItem.services.title)
            servicesMenu = services
            let item = NSMenuItem(title: StandardMenuItem.services.title, action: nil, keyEquivalent: "")
            item.submenu = services
            return item
        case .standard(let standard):
            let item = NSMenuItem(title: standard.title, action: standard.action,
                                  keyEquivalent: standard.keyEquivalent?.key ?? "")
            if let keyEquivalent = standard.keyEquivalent { item.keyEquivalentModifierMask = keyEquivalent.modifiers }
            return item
        case .command(let id, let title, let keyEquivalent, let filledByProvider):
            guard filledByProvider else {
                return NSMenuItem.playerCommand(id, title: title, keyEquivalent: keyEquivalent)
            }
            let submenu = NSMenu(title: title)
            submenu.autoenablesItems = false
            submenu.delegate = self
            dynamic[ObjectIdentifier(submenu)] = id
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.submenu = submenu
            return item
        }
    }
}
