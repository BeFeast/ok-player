import AppKit

/// The loading, buffering and failure states over the video (`StatusDisplay`): a
/// spinner with a line beside it, or a card with the failure's title, message and
/// buttons. Only the card takes clicks; everywhere else they reach the video.
final class StatusOverlay: NSView {
    /// A card button was pressed, with the card it belongs to.
    var onAction: ((FailureAction, FailureCard) -> Void)?

    let progressPanel = NSVisualEffectView()
    let spinner = NSProgressIndicator()
    let progressLabel = NSTextField(labelWithString: "")
    let cardPanel = NSVisualEffectView()
    let iconView = NSImageView()
    let titleLabel = NSTextField(labelWithString: "")
    let messageLabel = NSTextField(wrappingLabelWithString: "")
    let buttonRow = NSStackView()
    private(set) var display = StatusDisplay.clear
    /// The card's buttons, in the order the session gave them.
    private(set) var buttons: [(action: FailureAction, button: NSButton)] = []

    override init(frame: NSRect) {
        super.init(frame: frame)
        buildProgress()
        buildCard()
        progressPanel.isHidden = true
        cardPanel.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }

    func button(for action: FailureAction) -> NSButton? {
        buttons.first { $0.action == action }?.button
    }

    func show(_ display: StatusDisplay) {
        guard display != self.display else { return }
        self.display = display
        switch display {
        case .clear:
            progressPanel.isHidden = true
            cardPanel.isHidden = true
        case .progress(let line):
            progressLabel.stringValue = line
            progressPanel.isHidden = false
            cardPanel.isHidden = true
        case .failure(let card):
            showCard(title: card.title, message: card.message, actions: card.actions)
        case .shellFailure(let message):
            showCard(title: StatusRules.shellFailureTitle, message: message, actions: [])
        }
        if progressPanel.isHidden { spinner.stopAnimation(nil) } else { spinner.startAnimation(nil) }
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard !cardPanel.isHidden else { return nil }
        let hit = super.hitTest(point)
        return hit === self ? nil : hit
    }

    private func showCard(title: String, message: String, actions: [FailureActionView]) {
        titleLabel.stringValue = title
        messageLabel.stringValue = message
        for (_, button) in buttons { button.removeFromSuperview() }
        buttons = actions.enumerated().map { index, view -> (action: FailureAction, button: NSButton) in
            // The first action is the card's primary one (Retry).
            let button: NSButton = index == 0
                ? AccentButton(label: view.label, target: self, action: #selector(pressed(_:)))
                : NSButton(title: view.label, target: self, action: #selector(pressed(_:)))
            button.tag = index
            buttonRow.addArrangedSubview(button)
            return (action: view.action, button: button)
        }
        buttonRow.isHidden = buttons.isEmpty
        progressPanel.isHidden = true
        cardPanel.isHidden = false
    }

    @objc private func pressed(_ sender: NSButton) {
        guard case .failure(let card) = display, buttons.indices.contains(sender.tag) else { return }
        onAction?(buttons[sender.tag].action, card)
    }

    // MARK: Layout

    private static func panel(_ view: NSVisualEffectView, radius: CGFloat) {
        view.material = .popover
        view.blendingMode = .withinWindow
        view.state = .active
        view.wantsLayer = true
        view.layer?.cornerRadius = radius
        view.layer?.masksToBounds = true
        view.translatesAutoresizingMaskIntoConstraints = false
    }

    private func buildProgress() {
        Self.panel(progressPanel, radius: 12)
        addSubview(progressPanel)
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        progressLabel.font = NSFont.systemFont(ofSize: 13, weight: .medium)
        progressLabel.textColor = PaletteRole.text.color
        progressLabel.lineBreakMode = .byTruncatingMiddle
        progressLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let row = NSStackView(views: [spinner, progressLabel])
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 10
        row.translatesAutoresizingMaskIntoConstraints = false
        progressPanel.addSubview(row)
        NSLayoutConstraint.activate([
            progressPanel.centerXAnchor.constraint(equalTo: centerXAnchor),
            progressPanel.centerYAnchor.constraint(equalTo: centerYAnchor),
            progressPanel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 16),
            progressPanel.widthAnchor.constraint(lessThanOrEqualToConstant: 420),
            row.topAnchor.constraint(equalTo: progressPanel.topAnchor, constant: 10),
            row.bottomAnchor.constraint(equalTo: progressPanel.bottomAnchor, constant: -10),
            row.leadingAnchor.constraint(equalTo: progressPanel.leadingAnchor, constant: 14),
            row.trailingAnchor.constraint(equalTo: progressPanel.trailingAnchor, constant: -16),
        ])
    }

    private func buildCard() {
        Self.panel(cardPanel, radius: 14)
        addSubview(cardPanel)
        iconView.image = NSImage(systemSymbolName: "exclamationmark.triangle.fill", accessibilityDescription: "Error")?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 26, weight: .regular))
        iconView.contentTintColor = PaletteRole.danger.color
        titleLabel.font = NSFont.systemFont(ofSize: 15, weight: .semibold)
        titleLabel.textColor = PaletteRole.text.color
        titleLabel.alignment = .center
        messageLabel.font = NSFont.systemFont(ofSize: 13)
        messageLabel.textColor = PaletteRole.textMuted.color
        messageLabel.alignment = .center
        messageLabel.preferredMaxLayoutWidth = 360
        buttonRow.orientation = .horizontal
        buttonRow.spacing = 8
        let column = NSStackView(views: [iconView, titleLabel, messageLabel, buttonRow])
        column.orientation = .vertical
        column.alignment = .centerX
        column.spacing = 8
        column.setCustomSpacing(14, after: messageLabel)
        column.translatesAutoresizingMaskIntoConstraints = false
        cardPanel.addSubview(column)
        NSLayoutConstraint.activate([
            cardPanel.centerXAnchor.constraint(equalTo: centerXAnchor),
            cardPanel.centerYAnchor.constraint(equalTo: centerYAnchor),
            cardPanel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 16),
            cardPanel.widthAnchor.constraint(lessThanOrEqualToConstant: 420),
            column.topAnchor.constraint(equalTo: cardPanel.topAnchor, constant: 18),
            column.bottomAnchor.constraint(equalTo: cardPanel.bottomAnchor, constant: -16),
            column.leadingAnchor.constraint(equalTo: cardPanel.leadingAnchor, constant: 20),
            column.trailingAnchor.constraint(equalTo: cardPanel.trailingAnchor, constant: -20),
            messageLabel.widthAnchor.constraint(lessThanOrEqualToConstant: 360),
        ])
    }
}
