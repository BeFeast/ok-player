import AppKit

/// The application owns one Settings window and applies only themes acknowledged
/// by the session. The Settings window has its own responder chain and key loop.
final class PlayerSettings {
    private(set) var window: SettingsWindow?
    private weak var client: SessionClient?
    private var theme: AppearanceTheme = .auto
    var autoAppearance: NSAppearance? { didSet { applyAppearance() } }
    var writeTheme: ((AppearanceSelection) -> Result<AppearanceTheme, SessionClientError>)?

    func connect(_ client: SessionClient) {
        self.client = client
        if case .settings(let state)? = client.query(.settings) { theme = state.theme }
        applyAppearance()
        window?.showTheme(theme, problem: nil)
    }

    func show(rememberFrame: Bool) {
        guard let client, case .settings(let state)? = client.query(.settings) else { return }
        if window == nil {
            let made = SettingsWindow(state: state)
            made.search = { [weak self] text in
                guard case .settingsSearch(let hits)? = self?.client?.query(.settingsSearch(text: text)) else { return [] }
                return hits
            }
            made.onSelectTheme = { [weak self] choice in self?.selectTheme(choice) }
            made.center()
            if rememberFrame { _ = made.setFrameAutosaveName("OKPlayerSettings") }
            window = made
        }
        if window?.isMiniaturized == true { window?.deminiaturize(nil) }
        window?.makeKeyAndOrderFront(nil)
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

    func installSettingsContextMenu(on views: [NSView]) {
        let menu = NSMenu()
        let item = NSMenuItem.playerCommand("open-settings", title: "Settings…", keyEquivalent: nil)
        item.target = self
        menu.addItem(item)
        for view in views { view.menu = menu }
    }
}
