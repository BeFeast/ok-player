import AppKit

final class UpNextTableView: NSTableView {
    var onKey: ((UpNextKey, Int) -> Void)?
    var forward: ((NSEvent) -> Void)?
    var menuForRow: ((Int) -> NSMenu?)?
    var onDragExit: (() -> Void)?

    override func keyDown(with event: NSEvent) {
        if let key = UpNextKey(keyCode: event.keyCode, modifiers: event.modifierFlags) {
            if !event.isARepeat { onKey?(key, selectedRow) }
        } else if UpNextKey.navigationKeyCodes.contains(event.keyCode) || forward == nil {
            super.keyDown(with: event)
        } else {
            forward?(event)
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let row = row(at: convert(event.locationInWindow, from: nil))
        return row < 0 ? nil : menuForRow?(row)
    }

    override func draggingExited(_ sender: (any NSDraggingInfo)?) {
        super.draggingExited(sender)
        onDragExit?()
    }
}

final class UpNextCellView: NSTableCellView {
    static let reuseIdentifier = NSUserInterfaceItemIdentifier("UpNextCell")

    private let handle = NSImageView()
    private let lane = UpNextMaterial.label(size: 10, weight: .semibold, color: UpNextMaterial.muted)
    private let source = NSImageView()
    private let title = UpNextMaterial.label(size: 12.5)
    let actions = TransportOverlay.iconButton("ellipsis", "Queue actions")
    var onActions: (() -> Void)?

    init() {
        super.init(frame: .zero)
        identifier = Self.reuseIdentifier
        textField = title
        handle.image = UpNextMaterial.dots(columns: 2)
        handle.contentTintColor = NSColor.black.withAlphaComponent(0.27)
        handle.toolTip = "Drag to reorder"
        lane.alignment = .center
        lane.wantsLayer = true
        lane.layer?.cornerRadius = 6
        actions.image = UpNextMaterial.dots(columns: 1)
        actions.contentTintColor = UpNextMaterial.muted
        actions.target = self
        actions.action = #selector(actionsChosen)
        for view in [handle, lane, source, title, actions] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
            view.centerYAnchor.constraint(equalTo: centerYAnchor).isActive = true
        }
        NSLayoutConstraint.activate([
            handle.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            handle.widthAnchor.constraint(equalToConstant: 16),
            lane.leadingAnchor.constraint(equalTo: handle.trailingAnchor, constant: 8),
            lane.widthAnchor.constraint(equalToConstant: 40),
            source.leadingAnchor.constraint(equalTo: lane.trailingAnchor, constant: 10),
            source.widthAnchor.constraint(equalToConstant: 16),
            title.leadingAnchor.constraint(equalTo: source.trailingAnchor, constant: 8),
            title.trailingAnchor.constraint(lessThanOrEqualTo: actions.leadingAnchor, constant: -4),
            actions.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    func show(_ row: UpNextRow) {
        handle.alphaValue = row.isCurrent ? 0.4 : 1
        switch row.state {
        case .current:
            lane.stringValue = "NOW"
            lane.textColor = .white
            lane.layer?.backgroundColor = UpNextMaterial.accent.cgColor
        case .next:
            lane.stringValue = "NEXT"
            lane.textColor = UpNextMaterial.link
            lane.layer?.backgroundColor = UpNextMaterial.tint(0.16).cgColor
        case .behind:
            lane.stringValue = "✓"
            lane.textColor = UpNextMaterial.muted
            lane.layer?.backgroundColor = nil
        case .upcoming:
            lane.stringValue = row.number
            lane.textColor = UpNextMaterial.muted
            lane.layer?.backgroundColor = nil
        }
        source.image = UpNextMaterial.symbol(row.source.kind == .url ? "network" : "video")
        source.contentTintColor = row.isCurrent ? UpNextMaterial.link : UpNextMaterial.muted
        title.stringValue = row.title
        title.textColor = row.isCurrent ? UpNextMaterial.link :
            row.state == .behind ? UpNextMaterial.muted : UpNextMaterial.ink
        toolTip = row.location
        let state = row.state == .current ? "Now playing" : row.state == .next ? "Next" : ""
        setAccessibilityLabel([state, row.title].filter { !$0.isEmpty }.joined(separator: ", "))
    }

    @objc private func actionsChosen() { onActions?() }
}

final class UpNextRowView: NSTableRowView {
    var isCurrent = false

    override func drawSelection(in dirtyRect: NSRect) { tint(fill: 0.12, border: 0.65) }
    override func drawBackground(in dirtyRect: NSRect) {
        if isCurrent { tint(fill: 0.10, border: 0.20) }
    }

    private func tint(fill: CGFloat, border: CGFloat) {
        let rect = bounds.insetBy(dx: 0.5, dy: 0.5)
        let path = NSBezierPath(roundedRect: rect, xRadius: 6, yRadius: 6)
        UpNextMaterial.tint(fill).setFill()
        path.fill()
        UpNextMaterial.tint(border).setStroke()
        path.stroke()
    }
}
