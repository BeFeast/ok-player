import AppKit

final class AboutPane: NSView {
    let diagnostics: AboutDiagnostics
    let mark = AboutMark()
    let copyButton = NSButton()
    let githubButton = NSButton()
    let licenseButton = NSButton()
    private(set) var footer: NSStackView!
    private(set) var cards: [NSView] = []
    private(set) var specRows: [NSStackView] = []
    var openURL: (URL) -> Void = { NSWorkspace.shared.open($0) }
    private var copyNotice: Timer?

    init(diagnostics: AboutDiagnostics) {
        self.diagnostics = diagnostics
        super.init(frame: .zero)

        let hostColumns = NSStackView(views: [
            column(Array(diagnostics.host.prefix(2)), labelWidth: 45),
            column(Array(diagnostics.host.suffix(2)), labelWidth: 45),
        ])
        hostColumns.distribution = .fillEqually
        hostColumns.alignment = .top
        hostColumns.spacing = 22
        cards = [
            card("APP", body: column(diagnostics.app, labelWidth: 109)),
            card("ENGINE", body: column(diagnostics.engine, labelWidth: 109)),
            card("HOST", body: hostColumns),
        ]
        configureFooter()
        let hero = makeHero()
        let separator = divider()
        let details = vertical(cards, spacing: 12)
        let line = divider()
        let content = vertical([hero, separator, details, line, footer], spacing: 10)
        content.setCustomSpacing(20, after: hero)
        content.setCustomSpacing(20, after: separator)
        pin(content, to: self)
        for view in content.arrangedSubviews { view.widthAnchor.constraint(equalTo: content.widthAnchor).isActive = true }
        for card in cards { card.widthAnchor.constraint(equalTo: details.widthAnchor).isActive = true }
    }

    required init?(coder: NSCoder) { fatalError("Use init(diagnostics:)") }

    private func makeHero() -> NSView {
        let title = label(diagnostics.name, size: 30, weight: .light)
        let wordmark = NSMutableAttributedString(string: diagnostics.name, attributes: [
            .font: title.font!, .foregroundColor: PaletteRole.text.color,
        ])
        if diagnostics.name.hasPrefix("OK ") {
            wordmark.addAttribute(.font, value: NSFont.systemFont(ofSize: 30, weight: .bold),
                                  range: NSRange(location: 0, length: 2))
        }
        title.attributedStringValue = wordmark
        let version = chip(diagnostics.version ?? "Unavailable", platform: false)
        let platform = chip("macOS", platform: true)
        let chips = NSStackView(views: [version, platform])
        chips.spacing = 8
        let tagline = label("Open-source media player", size: 13, muted: true)
        let credit = label("By Oleg Kossoy", size: 11, muted: true)
        let identity = vertical([title, chips, tagline, credit], spacing: 3)
        identity.setCustomSpacing(8, after: title)
        identity.setCustomSpacing(9, after: chips)
        for text in [title, tagline, credit] {
            text.widthAnchor.constraint(lessThanOrEqualTo: identity.widthAnchor).isActive = true
        }
        let hero = NSStackView(views: [mark, identity])
        hero.alignment = .centerY
        hero.spacing = 22
        mark.widthAnchor.constraint(equalToConstant: mark.intrinsicContentSize.width).isActive = true
        mark.heightAnchor.constraint(equalToConstant: mark.intrinsicContentSize.height).isActive = true
        return hero
    }

    private func chip(_ text: String, platform: Bool) -> NSView {
        let view = PalettePanel(fill: (platform ? PaletteRole.selected : .surfaceRaised).color,
                                border: .clear, radius: 6)
        let field = label(text, size: platform ? 10 : 12, weight: .semibold, monospaced: !platform)
        if platform { field.textColor = PaletteRole.link.color }
        field.setContentCompressionResistancePriority(.required, for: .horizontal)
        pin(field, to: view, horizontal: 9, vertical: 4)
        return view
    }

    private func card(_ title: String, body: NSView) -> NSView {
        let panel = PalettePanel(radius: 8)
        let heading = label(title, size: 11, weight: .semibold, muted: true)
        let stack = vertical([heading, body], spacing: 10)
        pin(stack, to: panel, horizontal: 16, vertical: 12)
        body.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        return panel
    }

    private func column(_ rows: [AboutDiagnostics.Row], labelWidth: CGFloat) -> NSStackView {
        let labels = rows.map { row in
            let key = label(row.name, muted: true)
            key.widthAnchor.constraint(equalToConstant: labelWidth).isActive = true
            let value = label(row.value, monospaced: true)
            value.alignment = .right
            value.lineBreakMode = .byCharWrapping
            let line = NSStackView(views: [key, value])
            line.alignment = .firstBaseline
            line.distribution = .fill
            line.spacing = 8
            return line
        }
        specRows.append(contentsOf: labels)
        let stack = vertical(labels, spacing: 6)
        for line in labels { line.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        return stack
    }

    private func configureFooter() {
        copyButton.title = "Copy diagnostics"
        copyButton.image = NSImage(systemSymbolName: "document.on.document", accessibilityDescription: nil)
        copyButton.imagePosition = .imageLeading
        copyButton.bezelStyle = .rounded
        copyButton.controlSize = .regular
        copyButton.target = self
        copyButton.action = #selector(copyDiagnostics)
        copyButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 145).isActive = true
        configureLink(githubButton, title: "GitHub", action: #selector(openProject))
        configureLink(licenseButton, title: "License", action: #selector(openLicense))
        footer = NSStackView(views: [copyButton, NSView(), githubButton, licenseButton])
        footer.alignment = .centerY
        footer.spacing = 12
    }

    private func configureLink(_ button: NSButton, title: String, action: Selector) {
        button.title = "\(title) ↗"
        button.attributedTitle = NSAttributedString(string: button.title, attributes: [
            .font: NSFont.systemFont(ofSize: 12, weight: .semibold),
            .foregroundColor: PaletteRole.link.color,
        ])
        button.isBordered = false
        button.setAccessibilityLabel(title)
        button.setAccessibilityRole(.link)
        button.target = self
        button.action = action
    }

    @objc private func openProject() { openURL(AboutDiagnostics.projectURL) }
    @objc private func openLicense() { openURL(AboutDiagnostics.licenseURL) }

    @objc private func copyDiagnostics() {
        let board = NSPasteboard.general
        board.clearContents()
        guard board.setString(diagnostics.plainText, forType: .string) else { return }
        copyButton.title = "Diagnostics copied"
        NSAccessibility.post(element: NSApplication.shared, notification: .announcementRequested,
                             userInfo: [.announcement: "Diagnostics copied", .priority: NSAccessibilityPriorityLevel.medium.rawValue])
        copyNotice?.invalidate()
        copyNotice = Timer.scheduledTimer(withTimeInterval: 2, repeats: false) { [weak self] _ in
            self?.copyButton.title = "Copy diagnostics"
        }
    }

    private func label(_ text: String, size: CGFloat = 12, weight: NSFont.Weight = .regular,
                       muted: Bool = false, monospaced: Bool = false) -> NSTextField {
        let label = NSTextField(wrappingLabelWithString: text)
        label.font = monospaced ? .monospacedSystemFont(ofSize: size, weight: weight)
                               : .systemFont(ofSize: size, weight: weight)
        label.textColor = (muted ? PaletteRole.textMuted : .text).color
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        label.setContentCompressionResistancePriority(.required, for: .vertical)
        return label
    }

    private func divider() -> NSView {
        let line = SettingsBackdrop(.borderSubtle)
        line.heightAnchor.constraint(equalToConstant: 1).isActive = true
        return line
    }

    private func vertical(_ views: [NSView], spacing: CGFloat) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = spacing
        return stack
    }

    private func pin(_ child: NSView, to parent: NSView, horizontal: CGFloat = 0, vertical: CGFloat = 0) {
        child.translatesAutoresizingMaskIntoConstraints = false
        parent.addSubview(child)
        NSLayoutConstraint.activate([
            child.leadingAnchor.constraint(equalTo: parent.leadingAnchor, constant: horizontal),
            child.trailingAnchor.constraint(equalTo: parent.trailingAnchor, constant: -horizontal),
            child.topAnchor.constraint(equalTo: parent.topAnchor, constant: vertical),
            child.bottomAnchor.constraint(equalTo: parent.bottomAnchor, constant: -vertical),
        ])
    }
}
