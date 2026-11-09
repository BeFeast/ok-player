import AppKit

// The menu bar and the window keys in the app (#880): the registry commands this
// shell carries out, the connection to the session, and the smoke's checks.

extension PlayerApp: PlayerCommandResponder, NSMenuItemValidation {
    /// A registry item of the menu bar, reached through the responder chain.
    @objc func performPlayerCommand(_ sender: Any?) {
        commands.perform(sender)
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        commands.validate(menuItem)
    }

    /// The registry commands this shell carries out. Call it before the session
    /// starts: the session config lists them, and the registry reports every
    /// command neither the session nor this shell runs as unavailable.
    func registerPlayerCommands() {
        commands.isFullscreen = { [weak self] in self?.windowBehaviour?.window.isInFullscreen ?? false }
        commands.register("open-file") { [weak self] in self?.openFile() }
        commands.register("fullscreen") { [weak self] in _ = self?.windowBehaviour?.perform(.toggleFullscreen) }
        commands.register("escape-fullscreen") { [weak self] in _ = self?.windowBehaviour?.perform(.exitFullscreen) }
        commands.register("previous-item") { [weak self] in _ = self?.playlist.step(.previous) }
        commands.register("next-item") { [weak self] in _ = self?.playlist.step(.next) }
        commands.register(PlaybackSpeedMenu(
            transport: { [weak self] in
                guard case .transport(let view)? = self?.client?.query(.transport(at: nil)) else { return nil }
                return view
            },
            dispatch: { [weak self] command in _ = self?.client?.dispatch(command) }))
    }

    /// Read the shortcut table from `client`, rebuild the menu bar from its
    /// registry and take the player window's keys.
    func connectPlayerCommands(_ client: SessionClient) {
        var table: [ShortcutRow] = []
        if case .shortcutTable(let rows)? = client.query(.shortcutTable) { table = rows }
        var repeating: [String] = []
        if case .repeatingShortcuts(let actions)? = client.query(.repeatingShortcuts) { repeating = actions }
        commands.connect(
            session: { [weak client] command in
                client?.dispatch(command) ?? .rejected(reason: .noActiveMedia, message: "No playback session")
            },
            registry: { [weak client] fullscreen in
                guard case .commandRegistry(let rows)? = client?.query(.commandRegistry(fullscreen: fullscreen)) else {
                    return []
                }
                return rows
            },
            keys: ShortcutKeyMap(table: table, repeating: repeating))
        menuBar?.install()
        windowBehaviour?.window.shortcutKeys = { [weak self] event in self?.commands.handle(event) ?? false }
    }
}

// The smoke's checks (`--smoke-test`, run by the macOS workflows).
extension PlayerApp {
    /// What is wrong with the menu bar and the window keys while the smoke's file
    /// plays, or nil: the bar has its nine menus with AppKit's window list and
    /// help menu, Play/Pause from the registry validates enabled through the
    /// responder chain, and a held Space's auto-repeat is taken without toggling.
    func smokeMenuFailure() -> String? {
        guard let menuBar, let window, let video else { return "The menu bar was not installed" }
        let titles = NSApp.mainMenu?.items.map(\.title) ?? []
        let expected = MenuBarMenu.allCases.map(\.title)
        guard titles == expected else { return "The menu bar is \(titles), not \(expected)" }
        guard let windows = menuBar.menus[.window], NSApp.windowsMenu === windows,
              let help = menuBar.menus[.help], NSApp.helpMenu === help else {
            return "The Window and Help menus are not AppKit's window list and help menu"
        }
        guard let playback = menuBar.menus[.playback],
              let playPause = playback.items.first(where: { $0.playerCommandID == "play-pause" }) else {
            return "The Playback menu has no Play/Pause item from the registry"
        }
        commands.invalidate()
        playback.update()
        guard playPause.isEnabled, playPause.title == commands.row("play-pause")?.label else {
            return "Play/Pause validated \(playPause.isEnabled ? "enabled" : "disabled") as \"\(playPause.title)\" "
                + "while playing"
        }
        guard window.makeFirstResponder(video),
              let space = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil, characters: " ",
                                           charactersIgnoringModifiers: " ", isARepeat: true, keyCode: 49) else {
            return "Could not press Space on the video"
        }
        window.sendEvent(space)
        guard commands.lastInvocation == PlayerCommands.Invocation(id: "play-pause", outcome: .heldRepeat) else {
            return "A held Space's auto-repeat came out as \(String(describing: commands.lastInvocation))"
        }
        print("OKP_SMOKE menu_bar=\(titles.count) play_pause=validated held_space=ignored")
        return nil
    }
}

extension PlayerCommands {
    /// What is wrong with how F and Escape reached their commands since
    /// `before` (`performedCounts` then), or nil when each ran its registry
    /// command once through the shortcut table.
    func smokeFullscreenKeysFailure(since before: [String: Int]) -> String? {
        let ran = ["fullscreen", "escape-fullscreen"].map { (performedCounts[$0] ?? 0) - (before[$0] ?? 0) }
        guard ran == [1, 1] else {
            return "F and Escape ran fullscreen \(ran[0]) and escape-fullscreen \(ran[1]) times through the shortcut table"
        }
        return nil
    }
}
