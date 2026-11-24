import AppKit

final class SettingsWindow: NSWindow, NSSearchFieldDelegate {
    let searchField = NSSearchField()
    let themeControl = NSSegmentedControl(labels: ["Light", "Auto"], trackingMode: .selectOne, target: nil, action: nil)
    let content = SettingsScrollView()
    private let rail = SettingsScrollView()
    private let problem = NSTextField(wrappingLabelWithString: "")
    private let themeNote = NSTextField(wrappingLabelWithString: "")
    private let destinations: [SettingsDestination]
    private(set) var railButtons: [SettingsRailButton] = []
    private(set) var aboutButton: SettingsRailButton!
    private(set) var resultButtons: [SettingsRailButton] = []
    private(set) var selectedPage: SettingsPage = .appearance
    private(set) var shownTheme: AppearanceTheme
    private(set) var aboutPane: AboutPane?
    private var saveProblem: String?
    var search: (String) -> [SettingsSearchHit] = { _ in [] }
    var onSelectTheme: ((AppearanceSelection) -> Void)?
    var aboutDiagnostics: () -> AboutDiagnostics = { AboutDiagnostics(runtime: nil) }
    var updates: PlayerUpdates?

    init(state: SettingsState) {
        shownTheme = state.theme
        destinations = state.sections + [state.about]
        super.init(contentRect: NSRect(x: 0, y: 0, width: 940, height: 698),
                   styleMask: [.titled, .closable, .miniaturizable, .resizable],
                   backing: .buffered, defer: false)
        title = "Settings"
        isReleasedWhenClosed = false
        contentMinSize = NSSize(width: 620, height: 450)
        backgroundColor = PaletteRole.canvas.color
        build(sections: state.sections, about: state.about)
        navigate(to: .appearance)
    }

    required init?(coder: NSCoder) { fatalError("Use init(state:)") }

    private func build(sections: [SettingsDestination], about: SettingsDestination) {
        let root = SettingsBackdrop(.canvas)
        contentView = root
        let sidebar = SettingsBackdrop(.canvas, shaded: true)
        let divider = SettingsBackdrop(.borderSubtle)
        let footerLine = SettingsBackdrop(.borderSubtle)
        aboutButton = makeButton(about)
        searchField.placeholderString = "Search settings"
        searchField.setAccessibilityLabel("Search settings")
        searchField.sendsWholeSearchString = true
        searchField.target = self
        searchField.action = #selector(openFirstSearchResult)
        searchField.delegate = self
        initialFirstResponder = searchField
        themeControl.setAccessibilityLabel("App theme")
        themeControl.target = self
        themeControl.action = #selector(selectTheme)
        themeControl.setWidth(90, forSegment: 0)
        themeControl.setWidth(90, forSegment: 1)

        rail.list.spacing = 2
        railButtons = sections.map { makeButton($0) }
        rail.setRows(railButtons)
        content.list.edgeInsets = NSEdgeInsets(top: 28, left: 24, bottom: 28, right: 44)

        for view in [sidebar, divider, content] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(view)
        }
        for view in [searchField, rail, footerLine, aboutButton!] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            sidebar.addSubview(view)
        }
        let width = sidebar.widthAnchor.constraint(equalTo: root.widthAnchor, multiplier: 0.30)
        width.priority = .defaultHigh
        NSLayoutConstraint.activate([
            width, sidebar.widthAnchor.constraint(lessThanOrEqualToConstant: 282),
            sidebar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            sidebar.topAnchor.constraint(equalTo: root.topAnchor),
            sidebar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            divider.leadingAnchor.constraint(equalTo: sidebar.trailingAnchor),
            divider.topAnchor.constraint(equalTo: root.topAnchor),
            divider.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            divider.widthAnchor.constraint(equalToConstant: 1),
            content.leadingAnchor.constraint(equalTo: divider.trailingAnchor),
            content.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            content.topAnchor.constraint(equalTo: root.topAnchor),
            content.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            searchField.leadingAnchor.constraint(equalTo: sidebar.leadingAnchor, constant: 10),
            searchField.trailingAnchor.constraint(equalTo: sidebar.trailingAnchor, constant: -10),
            searchField.topAnchor.constraint(equalTo: sidebar.topAnchor, constant: 12),
            searchField.heightAnchor.constraint(equalToConstant: 30),
            rail.leadingAnchor.constraint(equalTo: searchField.leadingAnchor),
            rail.trailingAnchor.constraint(equalTo: searchField.trailingAnchor),
            rail.topAnchor.constraint(equalTo: searchField.bottomAnchor, constant: 8),
            rail.bottomAnchor.constraint(equalTo: footerLine.topAnchor, constant: -8),
            footerLine.leadingAnchor.constraint(equalTo: sidebar.leadingAnchor, constant: 19),
            footerLine.trailingAnchor.constraint(equalTo: sidebar.trailingAnchor, constant: -19),
            footerLine.bottomAnchor.constraint(equalTo: aboutButton.topAnchor, constant: -10),
            footerLine.heightAnchor.constraint(equalToConstant: 1),
            aboutButton.leadingAnchor.constraint(equalTo: searchField.leadingAnchor),
            aboutButton.trailingAnchor.constraint(equalTo: searchField.trailingAnchor),
            aboutButton.bottomAnchor.constraint(equalTo: sidebar.bottomAnchor, constant: -11)
        ])
    }

    func controlTextDidChange(_ notification: Notification) {
        guard notification.object as? NSSearchField === searchField else { return }
        updateSearch()
    }

    private func updateSearch() {
        let text = searchField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { showPage(); return }
        let hits = search(text)
        aboutPane = nil
        resultButtons = hits.map { hit in
            let label = hit.label == hit.destination.title ? hit.label : "\(hit.label) — \(hit.destination.title)"
            return makeButton(hit.destination, label: label)
        }
        let heading = label(hits.isEmpty ? "No matching settings" : "Search results", weight: .semibold)
        content.setRows([card([heading] + resultButtons.map { $0 as NSView })])
    }

    @objc private func openFirstSearchResult() {
        guard !searchField.stringValue.isEmpty else { showPage(); return }
        if let button = resultButtons.first { navigate(to: button.page) }
    }

    private func makeButton(_ destination: SettingsDestination, label: String? = nil) -> SettingsRailButton {
        let button = SettingsRailButton(destination: destination, label: label ?? destination.title)
        button.target = self
        button.action = #selector(openPage(_:))
        return button
    }

    @objc private func openPage(_ sender: SettingsRailButton) {
        navigate(to: sender.page)
    }

    func navigate(to page: SettingsPage) {
        selectedPage = page
        if searchField.currentEditor() != nil, !searchField.stringValue.isEmpty {
            _ = makeFirstResponder(nil)
        }
        searchField.stringValue = ""
        showPage()
    }

    private func showPage() {
        resultButtons = []
        for button in railButtons + [aboutButton!] {
            button.state = button.page == selectedPage ? .on : .off
            button.needsDisplay = true
        }
        guard let destination = destinations.first(where: { $0.id == selectedPage }) else { return }
        aboutPane = nil
        if destination.available && selectedPage == .appearance {
            content.setRows([appearanceCard()])
        } else if destination.available && selectedPage == .updates, let updates {
            content.setRows([updatesCard(updates)])
        } else if destination.available && selectedPage == .about {
            let pane = AboutPane(diagnostics: aboutDiagnostics())
            aboutPane = pane
            content.setRows([pane])
        } else {
            content.setRows([card([
                label(destination.title, weight: .semibold),
                label("This section is unavailable in this version of OK Player.", muted: true)
            ])])
        }
    }

    func refreshUpdates() {
        if selectedPage == .updates && searchField.stringValue.isEmpty { showPage() }
    }

    private func updatesCard(_ updates: PlayerUpdates) -> NSView {
        let channel = NSSegmentedControl(labels: ["Beta", "Stable"], trackingMode: .selectOne,
                                         target: self, action: #selector(selectUpdateChannel(_:)))
        channel.setAccessibilityLabel("Release channel")
        channel.selectedSegment = updates.channel == .beta ? 0 : 1
        channel.isEnabled = updates.canCheck && !updates.checking && !updates.installing
        let check = NSButton(title: "Check for Updates", target: self, action: #selector(checkForUpdates))
        check.isEnabled = updates.canCheck && !updates.checking && !updates.installing
        let automatic = NSButton(checkboxWithTitle: "Automatic checks", target: self, action: #selector(changeAutomaticChecks(_:)))
        automatic.state = updates.automatic ? .on : .off
        automatic.isEnabled = updates.canCheck
        var views: [NSView] = [
            label("Updates", weight: .semibold),
            label("Installed version \(updates.version) · Build \(updates.build)"),
            label("Release channel", muted: true),
            channel, automatic,
            label("Last checked \(updates.lastCheck?.formatted(date: .abbreviated, time: .shortened) ?? "Never")", muted: true)
        ]
        if let offer = updates.offer {
            views.append(label("Available version \(offer.version) · Build \(offer.build)"))
        }
        let status: String
        if updates.installing { status = "Downloading and verifying update…" }
        else if updates.checking { status = "Checking for updates…" }
        else if let error = updates.problem { status = error }
        else if updates.skipped { status = "You skipped this build." }
        else if !updates.canCheck { status = "Updates are unavailable for this build." }
        else if updates.offer != nil && !updates.canInstall { status = updates.installExplanation }
        else if updates.offer == nil && updates.lastCheck != nil { status = "You’re up to date." }
        else { status = "" }
        if !status.isEmpty { views.append(label(status, muted: updates.problem == nil)) }
        var actions = [check]
        if updates.offer != nil {
            let install = NSButton(title: updates.skipped ? "Install Anyway" : "Install & Relaunch",
                                   target: self, action: #selector(installUpdate))
            install.isEnabled = updates.canInstall && !updates.checking && !updates.installing
            let release = NSButton(title: "Open Download", target: self, action: #selector(openUpdateDownload))
            let skip = NSButton(title: "Skip This Build", target: self, action: #selector(skipUpdate))
            skip.isEnabled = !updates.skipped && !updates.checking && !updates.installing
            actions += [install, skip, release]
        }
        let buttons = NSStackView(views: actions)
        buttons.orientation = .vertical
        buttons.alignment = .leading
        views.append(buttons)
        return card(views)
    }

    @objc private func selectUpdateChannel(_ sender: NSSegmentedControl) {
        updates?.choose(sender.selectedSegment == 0 ? .beta : .stable)
    }

    @objc private func checkForUpdates() { updates?.check() }
    @objc private func changeAutomaticChecks(_ sender: NSButton) { updates?.setAutomatic(sender.state == .on) }
    @objc private func installUpdate() { updates?.install() }
    @objc private func skipUpdate() { updates?.skip() }

    @objc private func openUpdateDownload() {
        guard let key = updates?.offer?.key else { return }
        NSWorkspace.shared.open(URL(string: "https://updates.befeast.com")!.appendingPathComponent(key))
    }

    func showTheme(_ theme: AppearanceTheme, problem: String?) {
        shownTheme = theme
        saveProblem = problem
        updateThemeControls()
    }

    private func updateThemeControls() {
        switch shownTheme {
        case .light: themeControl.selectedSegment = 0
        case .auto: themeControl.selectedSegment = 1
        case .dark: themeControl.selectedSegment = -1
        }
        themeNote.stringValue = shownTheme == .dark
            ? "Dark is set in your existing profile. Auto follows macOS appearance."
            : "Auto follows macOS appearance."
        problem.stringValue = saveProblem ?? ""
        problem.isHidden = saveProblem == nil
    }

    @objc private func selectTheme() {
        guard themeControl.selectedSegment >= 0 else { return }
        onSelectTheme?(themeControl.selectedSegment == 0 ? .light : .auto)
    }

    private func appearanceCard() -> NSView {
        let title = label("Appearance", size: 11, weight: .semibold, muted: true)
        let themeTitle = label("App theme", muted: true)
        themeNote.font = .systemFont(ofSize: 12)
        themeNote.textColor = PaletteRole.textMuted.color
        problem.font = .systemFont(ofSize: 12)
        problem.textColor = PaletteRole.danger.color
        let notes = NSStackView(views: [themeTitle, themeNote, problem])
        notes.orientation = .vertical
        notes.alignment = .leading
        notes.spacing = 2
        notes.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let theme = NSStackView(views: [notes, NSView(), themeControl])
        theme.alignment = .centerY
        let details = [
            ("Player surface", "Dark video plane"),
            ("Window chrome", "Native window controls"),
            ("Fullscreen caption", "Hidden"),
            ("Accent", "BeFeast blue"),
        ].map { name, value -> NSView in
            let name = label(name, muted: true)
            name.widthAnchor.constraint(equalToConstant: 112).isActive = true
            let value = label(value)
            value.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
            let row = NSStackView(views: [name, value, NSView()])
            row.spacing = 0
            row.heightAnchor.constraint(greaterThanOrEqualToConstant: 26).isActive = true
            return row
        }
        updateThemeControls()
        return card([title, theme] + details)
    }

    private func label(_ text: String, size: CGFloat = 12, weight: NSFont.Weight = .regular,
                       muted: Bool = false) -> NSTextField {
        let label = NSTextField(wrappingLabelWithString: text)
        label.font = .systemFont(ofSize: size, weight: weight)
        label.textColor = (muted ? PaletteRole.textMuted : .text).color
        return label
    }

    private func card(_ rows: [NSView]) -> NSView {
        let panel = PalettePanel(radius: 8)
        let stack = NSStackView(views: rows)
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 18
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 17, bottom: 18, right: 17)
        panel.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: panel.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: panel.trailingAnchor),
            stack.topAnchor.constraint(equalTo: panel.topAnchor),
            stack.bottomAnchor.constraint(equalTo: panel.bottomAnchor)
        ])
        for row in rows {
            row.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -34).isActive = true
        }
        return panel
    }
}
