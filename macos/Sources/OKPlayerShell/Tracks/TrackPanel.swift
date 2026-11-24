import AppKit

/// A light switcher over video in either player appearance.
final class TrackPanel: PalettePanel {
    private let scroll = NSScrollView()
    private let contents = TrackPanelContents()
    private let title = NSTextField(labelWithString: "")
    private var preferredHeight: NSLayoutConstraint!
    private var projectedItems: [TrackMenuItem] = []
    private var choiceRows: [NSView] = []
    private lazy var titleRow = TrackLabelRow(title)
    private var adjustmentRows: [TrackAdjustmentRow] = []
    private let adjustmentDivider = PaletteSurface(fill: .borderSubtle)

    var onAction: ((TrackMenuAction) -> Void)?
    var choiceButtons: [TrackRowButton] { choiceRows.compactMap { $0 as? TrackRowButton } }
    var adjustmentValues: [String] { adjustmentRows.map(\.value.stringValue) }
    var adjustmentButtons: [TrackStepButton] { adjustmentRows.flatMap(\.buttons) }
    var showsEmptyAudio: Bool { projectedItems.first?.kind == .placeholder }

    init() {
        super.init(radius: 8)
        appearance = NSAppearance(named: .aqua)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        isHidden = true
        title.font = .systemFont(ofSize: 13, weight: .bold)
        title.textColor = PaletteRole.text.color
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.scrollerStyle = .overlay
        scroll.autohidesScrollers = true
        scroll.documentView = contents
        scroll.translatesAutoresizingMaskIntoConstraints = false
        addSubview(scroll)
        preferredHeight = heightAnchor.constraint(equalToConstant: 40)
        preferredHeight.priority = .defaultHigh
        NSLayoutConstraint.activate([
            preferredHeight,
            widthAnchor.constraint(equalToConstant: 256),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            scroll.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    func update(kind: TrackKind, tracks: TracksView?) {
        let heading = kind == .audio ? "Audio" : "Subtitles"
        let kindChanged = title.stringValue != heading
        title.stringValue = heading
        setAccessibilityLabel(heading)
        let items = TrackMenus.items(kind, tracks)
        let adjustments = kind == .subtitle ? TrackMenus.adjustments(tracks) : []
        if projectedItems != items || adjustmentRows.count != adjustments.count || kindChanged {
            projectedItems = items
            choiceRows = items.map(makeRow)
            adjustmentRows = adjustments.map { adjustment in
                TrackAdjustmentRow(adjustment) { [weak self] action in self?.onAction?(action) }
            }
            let rowHeights: [CGFloat] = items.map { item in
                switch item.kind {
                case .separator: return 1
                case .placeholder: return 36
                case .command: return 38
                case .choice: return kind == .audio ? 54 : 44
                }
            }
            var rows: [(NSView, CGFloat)] = [(titleRow, 24)] + Array(zip(choiceRows, rowHeights))
            if !adjustmentRows.isEmpty { rows.append((adjustmentDivider, 1)) }
            rows += adjustmentRows.map { ($0, 38) }
            contents.replaceRows(rows)
            preferredHeight.constant = contents.frame.height + 16
        }
        for (row, adjustment) in zip(adjustmentRows, adjustments) { row.update(adjustment) }
        if kindChanged || isHidden {
            scroll.contentView.scroll(to: .zero)
            scroll.reflectScrolledClipView(scroll.contentView)
        }
    }

    private func makeRow(_ item: TrackMenuItem) -> NSView {
        switch item.kind {
        case .separator:
            return PaletteSurface(fill: .borderSubtle)
        case .placeholder:
            let label = NSTextField(labelWithString: item.title)
            label.font = .systemFont(ofSize: 12)
            label.textColor = PaletteRole.textMuted.color
            return TrackLabelRow(label)
        case .choice, .command:
            guard let action = item.action else { return NSView() }
            return TrackRowButton(item: item, action: action) { [weak self] action in self?.onAction?(action) }
        }
    }

    override func layout() {
        super.layout()
        contents.setFrameSize(NSSize(width: scroll.contentSize.width, height: contents.frame.height))
    }
}

private final class TrackPanelContents: NSView {
    private var rows: [(NSView, CGFloat)] = []
    override var isFlipped: Bool { true }

    func replaceRows(_ rows: [(NSView, CGFloat)]) {
        subviews.forEach { $0.removeFromSuperview() }
        self.rows = rows
        rows.forEach { addSubview($0.0) }
        setFrameSize(NSSize(width: frame.width, height: rows.reduce(0) { $0 + $1.1 + 2 } - 2))
        needsLayout = true
    }

    override func layout() {
        super.layout()
        var y: CGFloat = 0
        for (row, height) in rows {
            let inset: CGFloat = row is TrackRowButton ? 0 : 6
            row.frame = NSRect(x: inset, y: y, width: max(0, bounds.width - 2 * inset), height: height)
            y += height + 2
        }
    }
}

private final class TrackLabelRow: NSView {
    init(_ label: NSTextField) {
        super.init(frame: .zero)
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init(_:)") }
}

final class TrackRowButton: NSButton {
    let trackAction: TrackMenuAction
    private let item: TrackMenuItem
    private let onAction: (TrackMenuAction) -> Void
    private var hovering = false

    init(item: TrackMenuItem, action: TrackMenuAction, onAction: @escaping (TrackMenuAction) -> Void) {
        self.item = item
        self.trackAction = action
        self.onAction = onAction
        super.init(frame: .zero)
        isBordered = false
        refusesFirstResponder = true
        setButtonType(.momentaryPushIn)
        title = item.title
        isEnabled = item.enabled
        toolTip = item.title
        target = self
        self.action = #selector(activate)
        setAccessibilityLabel(item.title)
        if item.kind == .choice {
            state = item.checked ? .on : .off
            setAccessibilityRole(.radioButton)
            setAccessibilityValue(NSNumber(value: item.checked))
        }
    }

    required init?(coder: NSCoder) { fatalError("Use init(item:action:onAction:)") }

    @objc private func activate() { onAction(trackAction) }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                                       owner: self))
    }

    override func mouseEntered(with event: NSEvent) { hovering = true; needsDisplay = true }
    override func mouseExited(with event: NSEvent) { hovering = false; needsDisplay = true }

    override func draw(_ dirtyRect: NSRect) {
        let background = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 4, yRadius: 4)
        let selected = item.checked && isEnabled
        if selected {
            PaletteRole.accent.color.withAlphaComponent(0.12).setFill()
            background.fill()
            PaletteRole.accent.color.withAlphaComponent(0.28).setStroke()
            background.stroke()
        }
        if isEnabled && (hovering || isHighlighted) {
            NSColor.black.withAlphaComponent(isHighlighted ? 0.09 : 0.055).setFill()
            background.fill()
        }

        let color = selected ? PaletteRole.link.color : PaletteRole.text.color
        let textColor = color.withAlphaComponent(isEnabled ? 1 : 0.38)
        let y = bounds.midY
        let path = NSBezierPath()
        if item.kind == .command {
            path.move(to: NSPoint(x: 10, y: y))
            path.line(to: NSPoint(x: 20, y: y))
            path.move(to: NSPoint(x: 15, y: y - 5))
            path.line(to: NSPoint(x: 15, y: y + 5))
        } else if selected {
            let up: CGFloat = isFlipped ? -1 : 1
            path.move(to: NSPoint(x: 10, y: y))
            path.line(to: NSPoint(x: 13, y: y - 3 * up))
            path.line(to: NSPoint(x: 20, y: y + 4 * up))
        }
        path.lineWidth = 1.5
        path.lineCapStyle = .round
        path.lineJoinStyle = .round
        textColor.setStroke()
        path.stroke()

        let detail = item.detail
        let suffix = detail.map { " · \($0)" }
        let name = suffix.flatMap { title.hasSuffix($0) ? String(title.dropLast($0.count)) : nil } ?? title
        let offset: CGFloat = isFlipped ? -7 : 7
        drawLine(name, y: y + (detail == nil ? 0 : offset),
                 font: .systemFont(ofSize: 12, weight: selected ? .semibold : .regular), color: textColor)
        if let detail {
            drawLine(detail, y: y - offset, font: .systemFont(ofSize: 10.5), color: PaletteRole.textMuted.color)
        }
    }

    private func drawLine(_ text: String, y: CGFloat, font: NSFont, color: NSColor) {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = title.hasSuffix(" · External") ? .byTruncatingMiddle : .byTruncatingTail
        (text as NSString).draw(in: NSRect(x: 30, y: y - 8, width: max(0, bounds.width - 38), height: 16),
                                withAttributes: [.font: font, .foregroundColor: color, .paragraphStyle: paragraph])
    }
}

final class TrackAdjustmentRow: NSStackView {
    let value = NSTextField(labelWithString: "")
    private(set) var buttons: [TrackStepButton] = []
    private let onAction: (TrackMenuAction) -> Void

    init(_ adjustment: SubtitleAdjustment, onAction: @escaping (TrackMenuAction) -> Void) {
        self.onAction = onAction
        super.init(frame: .zero)
        orientation = .horizontal
        alignment = .centerY
        distribution = .fill
        spacing = 2
        let title = NSTextField(labelWithString: adjustment.title)
        title.font = .systemFont(ofSize: 12)
        title.textColor = PaletteRole.textMuted.color
        title.setContentHuggingPriority(.defaultLow, for: .horizontal)
        value.font = .monospacedDigitSystemFont(ofSize: 11, weight: .semibold)
        value.textColor = PaletteRole.link.color
        value.alignment = .right
        value.setContentHuggingPriority(.required, for: .horizontal)
        addArrangedSubview(title)
        addArrangedSubview(value)
        buttons = [
            step("minus", adjustment.decreaseHelp, adjustment.decrease),
            step("arrow.counterclockwise", "Reset subtitle \(adjustment.title.lowercased())", adjustment.reset),
            step("plus", adjustment.increaseHelp, adjustment.increase),
        ]
        buttons.forEach(addArrangedSubview)
    }

    required init?(coder: NSCoder) { fatalError("Use init(_:onAction:)") }

    func update(_ adjustment: SubtitleAdjustment) {
        value.stringValue = adjustment.value
        value.setAccessibilityLabel("\(adjustment.title), \(adjustment.value)")
        buttons.forEach { $0.isEnabled = adjustment.enabled }
    }

    private func step(_ symbol: String, _ help: String, _ action: TrackMenuAction) -> TrackStepButton {
        let button = TrackStepButton(trackAction: action)
        button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: help)
        button.target = self
        button.action = #selector(activate(_:))
        button.isBordered = false
        button.refusesFirstResponder = true
        button.contentTintColor = PaletteRole.textMuted.color
        button.toolTip = help
        button.setAccessibilityLabel(help)
        button.widthAnchor.constraint(equalToConstant: 28).isActive = true
        button.heightAnchor.constraint(equalToConstant: 28).isActive = true
        return button
    }

    @objc private func activate(_ sender: TrackStepButton) { onAction(sender.trackAction) }
}

final class TrackStepButton: NSButton {
    let trackAction: TrackMenuAction

    init(trackAction: TrackMenuAction) {
        self.trackAction = trackAction
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError("Use init(trackAction:)") }
}
