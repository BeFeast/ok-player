import AppKit
import XCTest
@testable import OKPlayerShell

/// The session's registry and shortcut documents as the menu and key tests read
/// them (rust/crates/okp-session/fixtures): a muted local film playing fullscreen,
/// resolved for a shell that carries out the window, open and track commands.
enum MenuFixtures {
    struct NotTheExpectedReply: Error {
        let name: String
    }

    static func registry() throws -> [RegistryCommand] {
        let name = "query-reply-command-registry.json"
        guard case .commandRegistry(let commands) = try GoldenFixtures.decode(QueryReply.self, name).result else {
            throw NotTheExpectedReply(name: name)
        }
        return commands
    }

    static func shortcutTable() throws -> [ShortcutRow] {
        let name = "query-reply-shortcut-table.json"
        guard case .shortcutTable(let shortcuts) = try GoldenFixtures.decode(QueryReply.self, name).result else {
            throw NotTheExpectedReply(name: name)
        }
        return shortcuts
    }

    static func repeating() throws -> [String] {
        let name = "query-reply-repeating-shortcuts.json"
        guard case .repeatingShortcuts(let actions) = try GoldenFixtures.decode(QueryReply.self, name).result else {
            throw NotTheExpectedReply(name: name)
        }
        return actions
    }

    static func keys() throws -> ShortcutKeyMap {
        ShortcutKeyMap(table: try shortcutTable(), repeating: try repeating())
    }

    /// `rows` with `change` applied to the rows `ids` names.
    static func changing(_ rows: [RegistryCommand], _ ids: Set<String>,
                         _ change: (inout RegistryCommand) -> Void) -> [RegistryCommand] {
        rows.map { row in
            var row = row
            if ids.contains(row.id) { change(&row) }
            return row
        }
    }
}

/// A submenu provider that hands out fixed items and remembers the rows it was
/// asked for.
final class StandInSubmenu: DynamicSubmenuProvider {
    let commandID: String
    let titles: [String]
    private(set) var askedFor: [RegistryCommand] = []

    init(commandID: String, titles: [String]) {
        self.commandID = commandID
        self.titles = titles
    }

    func submenuItems(for command: RegistryCommand) -> [NSMenuItem] {
        askedFor.append(command)
        return titles.map { NSMenuItem(title: $0, action: nil, keyEquivalent: "") }
    }
}

/// The menu bar (#880) built from the fixture registry: where every command
/// goes, which commands show, which chords become key equivalents, and how
/// validation reads the registry.
final class MenuBarTests: XCTestCase {
    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
    }

    private func command(_ id: String, _ title: String, _ key: MenuKeyEquivalent? = nil,
                         submenu: Bool = false) -> MenuBarEntry {
        .command(id: id, title: title, keyEquivalent: key, submenu: submenu)
    }

    private func entries(_ models: [MenuBarModel], _ menu: MenuBarMenu) -> [MenuBarEntry] {
        models.first { $0.menu == menu }?.entries ?? []
    }

    private func ids(_ entries: [MenuBarEntry]) -> [String] {
        entries.compactMap { entry -> String? in
            if case .command(let id, _, _, _) = entry { return id }
            return nil
        }
    }

    // MARK: The menu tree

    func testTheMenuTreeFromTheFixtureRegistry() throws {
        let models = MenuBarLayout.build(registry: try MenuFixtures.registry(),
                                         submenus: ["playback-speed", "audio-track", "subtitles"])
        XCTAssertEqual(models.map(\.menu), [.app, .file, .edit, .view, .playback, .audio, .subtitles, .window, .help])
        XCTAssertEqual(models.map(\.menu.title),
                       ["OK Player", "File", "Edit", "View", "Playback", "Audio", "Subtitles", "Window", "Help"])

        XCTAssertEqual(entries(models, .app), [
            .standard(.about), .separator, .standard(.services), .separator,
            .standard(.hide), .standard(.hideOthers), .standard(.showAll), .separator, .standard(.quit),
        ], "Settings is unavailable here, so it is not shown")
        XCTAssertEqual(entries(models, .file), [
            command("open-file", "Open file...", MenuKeyEquivalent(key: "o", modifiers: .command)),
            command("open-folder", "Open folder..."),
            .standard(.openRecent), .separator,
            command("close-media", "Close media"), .standard(.close),
        ])
        XCTAssertEqual(entries(models, .edit), [
            .standard(.undo), .standard(.redo), .separator,
            .standard(.cut), .standard(.copy), .standard(.paste), .standard(.delete), .standard(.selectAll),
        ])
        XCTAssertEqual(entries(models, .view), [command("fullscreen", "Exit fullscreen")],
                       "the label is the registry's, resolved for the fullscreen window")
        XCTAssertEqual(entries(models, .playback), [
            command("play-pause", "Play / Pause"), .separator,
            command("seek-back", "Seek Back"), command("seek-forward", "Seek Forward"), .separator,
            command("playback-speed", "Playback speed", submenu: true),
        ])
        XCTAssertEqual(entries(models, .audio), [
            command("volume-up", "Volume Up"), command("volume-down", "Volume Down"), command("mute", "Mute"),
            .separator, command("audio-track", "Audio track", submenu: true),
        ])
        XCTAssertEqual(entries(models, .subtitles), [
            command("subtitles", "Subtitles", submenu: true), .separator,
            command("subtitle-delay-forward", "Subtitle Delay Forward"),
            command("subtitle-delay-back", "Subtitle Delay Back"),
        ])
        XCTAssertEqual(entries(models, .window), [
            .standard(.minimize), .standard(.zoom), .separator,
            command("fit-window-to-media", "Fit window to media"), .separator, .standard(.bringAllToFront),
        ])
        XCTAssertEqual(entries(models, .help), [])
    }

    func testEveryRegistryCommandHasOnePlaceAndEscapeIsAKeyOnly() throws {
        let rows = try MenuFixtures.registry()
        let everything = MenuFixtures.changing(rows, Set(rows.map(\.id))) { $0.available = true }
        let shown = MenuBarLayout.build(registry: everything, submenus: []).flatMap { ids($0.entries) }
        for row in everything where row.id != "escape-fullscreen" {
            XCTAssertEqual(shown.filter { $0 == row.id }.count, 1, row.id)
        }
        XCTAssertFalse(shown.contains("escape-fullscreen"), "Escape leaves fullscreen; the menu has the toggle")
    }

    func testUnavailableCommandsAreHidden() throws {
        let rows = MenuFixtures.changing(try MenuFixtures.registry(), ["seek-back", "mute"]) { $0.available = false }
        let shown = MenuBarLayout.build(registry: rows, submenus: []).flatMap { ids($0.entries) }
        XCTAssertFalse(shown.contains("seek-back"))
        XCTAssertFalse(shown.contains("mute"))
        XCTAssertTrue(shown.contains("seek-forward"))
    }

    func testACommandNoSlotNamesGoesToItsGroupsMenu() throws {
        var rows = try MenuFixtures.registry()
        rows.append(RegistryCommand(id: "rotate-counterclockwise", group: .viewWindow, menu: .video,
                                    label: "Rotate 90° left", available: true, enabled: true, checked: false,
                                    shortcut: nil))
        rows.append(RegistryCommand(id: "loudness", group: .tracksSubtitles, menu: nil, label: "Loudness",
                                    available: false, enabled: true, checked: false, shortcut: nil))
        let models = MenuBarLayout.build(registry: rows, submenus: [])
        XCTAssertEqual(entries(models, .view), [
            command("fullscreen", "Exit fullscreen"), .separator, command("rotate-counterclockwise", "Rotate 90° left"),
        ])
        XCTAssertFalse(models.flatMap { ids($0.entries) }.contains("loudness"), "unavailable")
    }

    func testOnlyCommandChordsBecomeKeyEquivalents() throws {
        let rows = MenuFixtures.changing(try MenuFixtures.registry(),
                                         ["open-url", "open-settings", "media-info", "subtitle-next-cue"]) {
            $0.available = true
        }
        let models = MenuBarLayout.build(registry: rows, submenus: [])
        var keys: [String: MenuKeyEquivalent] = [:]
        var shown: Set<String> = []
        for case .command(let id, _, let key, _) in models.flatMap(\.entries) {
            shown.insert(id)
            keys[id] = key
        }
        XCTAssertEqual(keys["open-file"], MenuKeyEquivalent(key: "o", modifiers: .command))
        XCTAssertEqual(keys["open-url"], MenuKeyEquivalent(key: "o", modifiers: [.shift, .command]))
        XCTAssertEqual(keys["open-settings"], MenuKeyEquivalent(key: ",", modifiers: .command))
        XCTAssertEqual(keys["media-info"], MenuKeyEquivalent(key: "i", modifiers: .command))
        XCTAssertEqual(keys["subtitle-next-cue"], MenuKeyEquivalent(key: "\u{F703}", modifiers: .command))
        // Single keys are the player window's: Space, M, Left and F show none.
        for id in ["play-pause", "mute", "seek-back", "fullscreen"] {
            XCTAssertTrue(shown.contains(id), id)
            XCTAssertNil(keys[id], id)
        }
    }

    // MARK: AppKit menus

    private func connected(_ rows: [RegistryCommand]) -> PlayerCommands {
        let commands = PlayerCommands()
        commands.connect(session: { _ in .noOp }, registry: { _ in rows }, keys: ShortcutKeyMap())
        return commands
    }

    func testTheMenusUseTheResponderChainAndTheProvidersFillTheirSubmenus() throws {
        let commands = connected(try MenuFixtures.registry())
        let speeds = StandInSubmenu(commandID: "playback-speed", titles: ["0.5×", "1×", "2×"])
        commands.register(speeds)
        let openRecent = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
        let bar = MenuBar(commands: commands, openRecent: openRecent)
        bar.rebuild()

        XCTAssertEqual(bar.mainMenu.items.map(\.title), MenuBarMenu.allCases.map(\.title))
        let edit = try XCTUnwrap(bar.menus[.edit]).items.filter { !$0.isSeparatorItem }
        XCTAssertEqual(edit.map(\.action), [Selector(("undo:")), Selector(("redo:")), #selector(NSText.cut(_:)),
                                            #selector(NSText.copy(_:)), #selector(NSText.paste(_:)),
                                            #selector(NSText.delete(_:)), #selector(NSText.selectAll(_:))])
        XCTAssertEqual(edit.map(\.keyEquivalent), ["z", "z", "x", "c", "v", "", "a"])
        XCTAssertTrue(edit.allSatisfy { $0.target == nil }, "the focused text field takes them")

        let playback = try XCTUnwrap(bar.menus[.playback])
        let playPause = try XCTUnwrap(playback.items.first { $0.playerCommandID == "play-pause" })
        XCTAssertEqual(playPause.action, #selector(PlayerCommandResponder.performPlayerCommand(_:)))
        XCTAssertNil(playPause.target, "validated and run through the responder chain")
        let file = try XCTUnwrap(bar.menus[.file])
        let openFile = try XCTUnwrap(file.items.first { $0.playerCommandID == "open-file" })
        XCTAssertEqual(openFile.keyEquivalent, "o")
        XCTAssertEqual(openFile.keyEquivalentModifierMask, .command)
        XCTAssertTrue(openRecent.menu === file)

        let speedItem = try XCTUnwrap(playback.items.first { $0.title == "Playback speed" })
        let submenu = try XCTUnwrap(speedItem.submenu)
        bar.menuNeedsUpdate(submenu)
        XCTAssertEqual(submenu.items.map(\.title), ["0.5×", "1×", "2×"])
        XCTAssertEqual(speeds.askedFor.map(\.id), ["playback-speed"])

        // Built again once the session is up: Open Recent moves to the new File menu.
        bar.rebuild()
        XCTAssertTrue(openRecent.menu === bar.menus[.file])
    }

    func testValidationTakesTheRegistrysLabelCheckMarkAndEnabledState() throws {
        let rows = MenuFixtures.changing(try MenuFixtures.registry(), ["seek-back"]) { $0.enabled = false }
        let commands = connected(rows)
        let mute = NSMenuItem.playerCommand("mute", title: "", keyEquivalent: nil)
        XCTAssertTrue(commands.validate(mute))
        XCTAssertEqual(mute.title, "Mute")
        XCTAssertEqual(mute.state, .on, "the fixture film is muted")

        let fullscreen = NSMenuItem.playerCommand("fullscreen", title: "Enter fullscreen", keyEquivalent: nil)
        XCTAssertTrue(commands.validate(fullscreen))
        XCTAssertEqual(fullscreen.title, "Exit fullscreen")
        XCTAssertEqual(fullscreen.state, .on)

        XCTAssertFalse(commands.validate(NSMenuItem.playerCommand("seek-back", title: "", keyEquivalent: nil)),
                       "disabled now")
        XCTAssertFalse(commands.validate(NSMenuItem.playerCommand("zoom-out", title: "", keyEquivalent: nil)),
                       "unavailable")
        XCTAssertFalse(commands.validate(NSMenuItem.playerCommand("no-such-command", title: "", keyEquivalent: nil)))

        let other = NSMenuItem(title: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        XCTAssertTrue(commands.validate(other))
        XCTAssertNil(other.playerCommandID)
        XCTAssertEqual(other.title, "Copy")
    }

    func testTheRegistryIsAskedAgainAfterABatchACommandAndAFullscreenChange() throws {
        let rows = try MenuFixtures.registry()
        var asked: [Bool] = []
        var fullscreen = false
        let commands = PlayerCommands()
        commands.isFullscreen = { fullscreen }
        commands.connect(session: { _ in .accepted(requestId: 1) },
                         registry: { state in asked.append(state); return rows }, keys: ShortcutKeyMap())
        _ = commands.registry
        _ = commands.row("mute")
        XCTAssertEqual(asked, [false], "one menu opening asks once")
        commands.invalidate()
        _ = commands.registry
        XCTAssertEqual(asked, [false, false], "a session batch")
        fullscreen = true
        _ = commands.registry
        XCTAssertEqual(asked, [false, false, true], "the window state is part of the query")
        commands.run("volume-up")
        _ = commands.registry
        XCTAssertEqual(asked, [false, false, true, true], "a command can change the registry")
    }
}
