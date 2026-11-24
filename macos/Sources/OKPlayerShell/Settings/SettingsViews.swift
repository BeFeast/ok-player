import AppKit

final class SettingsBackdrop: NSView {
    private let role: PaletteRole
    private let shaded: Bool

    init(_ role: PaletteRole, shaded: Bool = false) {
        self.role = role
        self.shaded = shaded
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError("Use init(_:shaded:)") }

    override func draw(_ dirtyRect: NSRect) {
        // AppKit can supply a rect that extends beyond the view's bounds.
        // A narrow divider must never paint over the neighboring rail.
        let paint = dirtyRect.intersection(bounds)
        effectiveAppearance.performAsCurrentDrawingAppearance {
            role.color.setFill()
            paint.fill()
            if shaded {
                PaletteRole.text.color.withAlphaComponent(0.04).setFill()
                paint.fill()
            }
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }
}

final class SettingsRailButton: NSButton {
    let page: SettingsPage
    private let symbol: NSImage?

    init(destination: SettingsDestination, label: String) {
        page = destination.id
        symbol = NSImage(systemSymbolName: destination.id.symbol, accessibilityDescription: nil)
        super.init(frame: .zero)
        title = label
        setButtonType(.onOff)
        isBordered = false
        focusRingType = .exterior
        setAccessibilityRole(.radioButton)
        heightAnchor.constraint(equalToConstant: 36).isActive = true
    }

    required init?(coder: NSCoder) { fatalError("Use init(destination:label:)") }

    override func draw(_ dirtyRect: NSRect) {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            let selected = state == .on
            let path = NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7)
            if selected || isHighlighted {
                PaletteRole.selected.color.setFill()
                path.fill()
            }
            if selected {
                NSGraphicsContext.saveGraphicsState()
                path.addClip()
                PaletteRole.accent.color.setFill()
                NSRect(x: 0, y: 0, width: 3, height: bounds.height).fill()
                NSGraphicsContext.restoreGraphicsState()
            }
            let color = (selected ? PaletteRole.link : .text).color
            let config = NSImage.SymbolConfiguration(pointSize: 13, weight: .medium)
                .applying(NSImage.SymbolConfiguration(paletteColors: [color]))
            symbol?.withSymbolConfiguration(config)?.draw(in: NSRect(x: 10, y: bounds.midY - 8, width: 16, height: 16))
            let paragraph = NSMutableParagraphStyle()
            paragraph.lineBreakMode = .byTruncatingTail
            let text = NSAttributedString(string: title, attributes: [
                .font: NSFont.systemFont(ofSize: 13, weight: selected ? .semibold : .regular),
                .foregroundColor: color, .paragraphStyle: paragraph
            ])
            text.draw(in: NSRect(x: 36, y: bounds.midY - text.size().height / 2,
                                 width: max(0, bounds.width - 46), height: text.size().height))
        }
    }

    override func drawFocusRingMask() {
        NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 2), xRadius: 6, yRadius: 6).fill()
    }

    override var focusRingMaskBounds: NSRect { bounds }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }
}

final class SettingsScrollView: NSScrollView {
    private final class Document: NSView {
        override var isFlipped: Bool { true }
    }

    let list = NSStackView()

    init() {
        super.init(frame: .zero)
        drawsBackground = false
        hasVerticalScroller = true
        scrollerStyle = .overlay
        verticalScrollElasticity = .none
        let document = Document()
        document.translatesAutoresizingMaskIntoConstraints = false
        documentView = document
        list.translatesAutoresizingMaskIntoConstraints = false
        list.orientation = .vertical
        list.alignment = .leading
        document.addSubview(list)
        NSLayoutConstraint.activate([
            document.widthAnchor.constraint(equalTo: contentView.widthAnchor),
            list.leadingAnchor.constraint(equalTo: document.leadingAnchor),
            list.trailingAnchor.constraint(equalTo: document.trailingAnchor),
            list.topAnchor.constraint(equalTo: document.topAnchor),
            list.bottomAnchor.constraint(equalTo: document.bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    func setRows(_ rows: [NSView]) {
        for view in list.arrangedSubviews { view.removeFromSuperview() }
        for row in rows {
            list.addArrangedSubview(row)
            let insets = list.edgeInsets
            row.widthAnchor.constraint(equalTo: list.widthAnchor, constant: -(insets.left + insets.right)).isActive = true
        }
        contentView.scroll(to: .zero)
        reflectScrolledClipView(contentView)
    }
}
