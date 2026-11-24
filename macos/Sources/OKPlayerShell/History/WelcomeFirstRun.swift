import AppKit

/// The first-run hero has one native button, covering the entire drop surface.
final class WelcomeFirstRun: NSView {
    let openButton = WelcomeOpenButton()
    let mark = NSImageView()
    private let problem = NSTextField(wrappingLabelWithString: "")

    init() {
        super.init(frame: .zero)
        mark.image = BrandIcon.welcome
        mark.imageScaling = .scaleProportionallyUpOrDown
        mark.setAccessibilityElement(false)
        let heading = NSTextField(labelWithString: "Welcome to OK Player")
        heading.font = .systemFont(ofSize: 17, weight: .semibold)
        heading.textColor = PaletteRole.text.color
        let subtitle = NSTextField(labelWithString: "Open a file to start playing.")
        subtitle.font = .systemFont(ofSize: 13)
        subtitle.textColor = PaletteRole.textMuted.color
        problem.font = .systemFont(ofSize: 11)
        problem.textColor = PaletteRole.danger.color
        problem.alignment = .center
        problem.isHidden = true

        let stack = NSStackView(views: [mark, heading, subtitle, openButton, problem])
        stack.orientation = .vertical
        stack.alignment = .centerX
        stack.spacing = 12
        stack.setCustomSpacing(3, after: heading)
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            mark.widthAnchor.constraint(equalToConstant: 48),
            mark.heightAnchor.constraint(equalTo: mark.widthAnchor),
            openButton.widthAnchor.constraint(equalToConstant: 320),
            openButton.heightAnchor.constraint(equalToConstant: 96),
            problem.widthAnchor.constraint(lessThanOrEqualTo: openButton.widthAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    func showProblem(_ message: String?) {
        problem.stringValue = message ?? ""
        problem.isHidden = message == nil
    }
}

final class WelcomeOpenButton: NSButton {
    static let dropHint = "or drop a file or folder"
    var onOpen: () -> Void = {}
    var dragActive = false {
        didSet { if dragActive != oldValue { needsDisplay = true } }
    }
    var allowsKeyFocus = true
    var showsKeyFocus: Bool { window?.firstResponder === self && window?.isKeyWindow == true }

    init() {
        super.init(frame: .zero)
        title = "Open File…"
        isBordered = false
        focusRingType = .none
        setAccessibilityElement(true)
        setAccessibilityRole(.button)
        setAccessibilityLabel("Open File")
        setAccessibilityHelp(Self.dropHint)
        target = self
        action = #selector(open)
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override var acceptsFirstResponder: Bool { allowsKeyFocus }
    override var canBecomeKeyView: Bool { allowsKeyFocus && isEnabled && !isHiddenOrHasHiddenAncestor }

    override func becomeFirstResponder() -> Bool {
        needsDisplay = true
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        return super.resignFirstResponder()
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let surface = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 10, yRadius: 10)
        let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let fill = (dark ? PaletteRole.text : PaletteRole.surface).color.withAlphaComponent(dark ? 0.04 : 0.5)
        let pressed = cell?.isHighlighted == true
        (dragActive || pressed ? PaletteRole.accent.color.withAlphaComponent(0.14) : fill).setFill()
        surface.fill()
        if showsKeyFocus || dragActive {
            (dragActive ? PaletteRole.link : PaletteRole.focus).color.setStroke()
            surface.lineWidth = 2
        } else {
            PaletteRole.border.color.withAlphaComponent(0.65).setStroke()
            surface.setLineDash([3, 2], count: 2, phase: 0)
        }
        surface.stroke()
        drawLine(title, color: .link, font: .systemFont(ofSize: 13, weight: .semibold),
                 offset: isFlipped ? -9 : 9)
        drawLine(Self.dropHint, color: .textMuted, font: .systemFont(ofSize: 11),
                 offset: isFlipped ? 9 : -9)
    }

    private func drawLine(_ text: String, color: PaletteRole, font: NSFont, offset: CGFloat) {
        let line = NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: color.color])
        let size = line.size()
        line.draw(at: CGPoint(x: bounds.midX - size.width / 2, y: bounds.midY + offset - size.height / 2))
    }

    @objc private func open() { onOpen() }
}
