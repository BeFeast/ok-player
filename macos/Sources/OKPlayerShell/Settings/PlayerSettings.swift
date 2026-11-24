import AppKit

/// The application owns one Settings window and applies only themes acknowledged
/// by the session. The Settings window has its own responder chain and key loop.
final class PlayerSettings {
    private(set) var window: SettingsWindow?
    private weak var client: SessionClient?
    private var theme: AppearanceTheme = .auto
    var autoAppearance: NSAppearance? { didSet { applyAppearance() } }
    var writeTheme: ((AppearanceSelection) -> Result<AppearanceTheme, SessionClientError>)?
    let updates = PlayerUpdates()

    func connect(_ client: SessionClient) {
        self.client = client
        if case .settings(let state)? = client.query(.settings) { theme = state.theme }
        applyAppearance()
        window?.showTheme(theme, problem: nil)
    }

    func show(page: SettingsPage? = nil, rememberFrame: Bool) {
        guard let client, case .settings(let state)? = client.query(.settings) else { return }
        if window == nil {
            let made = SettingsWindow(state: state)
            made.updates = updates
            updates.changed = { [weak made] in made?.refreshUpdates() }
            made.search = { [weak self] text in
                guard case .settingsSearch(let hits)? = self?.client?.query(.settingsSearch(text: text)) else { return [] }
                return hits
            }
            made.onSelectTheme = { [weak self] choice in self?.selectTheme(choice) }
            made.aboutDiagnostics = { [weak self] in
                let runtime: EngineDiagnostics?
                if case .engineDiagnostics(let diagnostics)? = self?.client?.query(.engineDiagnostics) {
                    runtime = diagnostics
                } else {
                    runtime = nil
                }
                return AboutDiagnostics(runtime: runtime)
            }
            made.center()
            if rememberFrame { _ = made.setFrameAutosaveName("OKPlayerSettings") }
            window = made
        }
        if window?.isMiniaturized == true { window?.deminiaturize(nil) }
        if let page { window?.navigate(to: page) }
        window?.makeKeyAndOrderFront(nil)
    }

    func close() {
        window?.close()
        window = nil
    }

    private func applyAppearance() {
        NSApp.appearance = theme.appearanceOverride ?? autoAppearance
    }

    private func selectTheme(_ selection: AppearanceSelection) {
        let result = writeTheme?(selection) ?? saveTheme(selection)
        switch result {
        case .success(let confirmed):
            theme = confirmed
            applyAppearance()
            window?.showTheme(confirmed, problem: nil)
        case .failure(let error):
            window?.showTheme(theme, problem: error.description)
        }
    }

    private func saveTheme(_ selection: AppearanceSelection) -> Result<AppearanceTheme, SessionClientError> {
        switch client?.dispatch(.setAppearanceTheme(theme: selection)) {
        case .applied?, .noOp?:
            if case .settings(let state)? = client?.query(.settings) { return .success(state.theme) }
        case .rejected(_, let message)?: return .failure(SessionClientError(description: message))
        default: break
        }
        return .failure(SessionClientError(description: "Could not read the app theme."))
    }
}

extension PlayerApp {
    func showSettings() {
        settings.show(rememberFrame: verification == nil && !smoke)
    }

    @objc func showAbout() {
        settings.show(page: .about, rememberFrame: verification == nil && !smoke)
    }

    func makeSettingsContextMenu(in menu: NSMenu = NSMenu()) -> NSMenu {
        if menu.numberOfItems > 0 { menu.addItem(.separator()) }
        let item = NSMenuItem.playerCommand("open-settings", title: "Settings…", keyEquivalent: nil)
        item.target = self
        menu.addItem(item)
        return menu
    }
}
