import AppKit

/// Native button semantics with the command surface's two-column treatment.
final class CommandPopoverRow: NSButton {
    private let hint: String
    private let checked: Bool
    private let onClick: () -> Void
    var keySelected = false { didSet { needsDisplay = true } }
    private var hovered = false { didSet { needsDisplay = true } }
    private var tracking: NSTrackingArea?

    init(title: String, hints: [ShortcutChord], checked: Bool, enabled: Bool, hasPage: Bool,
         accessibilityLabel: String?, onClick: @escaping () -> Void) {
        self.checked = checked
        self.onClick = onClick
        let keys = hints.map(\.macOSHint).joined(separator: " / ")
        hint = [keys, hasPage ? "›" : ""].filter { !$0.isEmpty }.joined(separator: "   ")
        super.init(frame: .zero)
        self.title = title
        isBordered = false
        isEnabled = enabled
        focusRingType = .none
        setButtonType(.momentaryPushIn)
        target = self
        action = #selector(clicked)
        setAccessibilityLabel(accessibilityLabel ?? title)
        if checked { setAccessibilityValue("Selected") }
        if !hints.isEmpty { setAccessibilityHelp(hints.map(\.label).joined(separator: " or ")) }
    }

    required init?(coder: NSCoder) { fatalError("Use init(title:hints:checked:enabled:hasPage:accessibilityLabel:onClick:)") }

    @objc private func clicked() { if isEnabled { onClick() } }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { hovered = true }
    override func mouseExited(with event: NSEvent) { hovered = false }

    override func draw(_ dirtyRect: NSRect) {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            if isEnabled && (hovered || keySelected || isHighlighted || window?.firstResponder === self) {
                PaletteRole.selected.color.setFill()
                NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 1), xRadius: 5, yRadius: 5).fill()
            }
            let labelColor = isEnabled ? PaletteRole.text.color : PaletteRole.textMuted.color.withAlphaComponent(0.65)
            let detailsColor = isEnabled ? PaletteRole.textMuted.color : PaletteRole.textMuted.color.withAlphaComponent(0.65)
            let hint = NSAttributedString(string: hint, attributes: [
                .font: NSFont.systemFont(ofSize: 11), .foregroundColor: detailsColor,
            ])
            let right = bounds.maxX - 10
            let hintSize = hint.size()
            hint.draw(at: NSPoint(x: right - hintSize.width, y: bounds.midY - hintSize.height / 2))
            if checked { drawLabel("✓", color: labelColor, from: 8, to: 26) }
            drawLabel(title, color: labelColor, from: 28, to: right - hintSize.width - 8)
        }
    }

    private func drawLabel(_ text: String, color: NSColor, from left: CGFloat, to right: CGFloat) {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        let label = NSAttributedString(string: text, attributes: [
            .font: NSFont.systemFont(ofSize: 13), .foregroundColor: color, .paragraphStyle: style,
        ])
        let height = ceil(label.size().height)
        label.draw(with: CGRect(x: left, y: bounds.midY - height / 2, width: max(0, right - left), height: height),
                   options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}
