import AppKit

/// Pure AppKit presentation for local sidecar lyrics.
///
/// All LRC parsing, sidecar discovery, fallback text, and active-line timing
/// come from `okp-core` via `okp-session` (`LyricsSnapshot`); this view only
/// renders rows, highlights/scrolls the active line, and forwards clicks to seek.
final class LyricsPanelView: NSView {
    var onSeek: ((Double) -> Void)?
    var onClose: (() -> Void)?
    var usesMusicSurface = true {
        didSet {
            guard oldValue != usesMusicSurface else { return }
            updateRowTypography()
            needsLayout = true
            needsDisplay = true
        }
    }
    var reduceMotion = false

    private(set) var displayed = LyricsSnapshot.empty
    private(set) var syncedRowButtons: [LyricLineButton] = []
    private(set) var plainRowLabels: [NSTextField] = []

    let headerLabel = NSTextField(labelWithString: "LYRICS")
    let closeButton = TransportOverlay.iconButton("xmark", "Hide Lyrics")
    let scrollView = NSScrollView()
    let fallbackIcon = NSImageView()
    let fallbackMessageLabel = NSTextField(wrappingLabelWithString: "")
    let fallbackDetailLabel = NSTextField(wrappingLabelWithString: "")

    private let headerDivider = NSView()
    private let listView = FlippedLyricsDocumentView()
    private let fallbackView = FlippedLyricsDocumentView()
    private var lastSheetKey: SheetKey?
    private var pendingScrollIndex: Int?

    private struct SheetKey: Equatable {
        let source: MediaSource?
        let generation: UInt64
        let state: LyricsState
        let header: String
        let message: String?
        let lines: [LyricsLineView]
    }

    var displayedState: LyricsState { displayed.state }
    var displayedSource: MediaSource? { displayed.source }
    var displayedGeneration: UInt64 { displayed.generation }
    var displayedActiveIndex: Int? { displayed.activeIndex }
    var headerText: String { headerLabel.stringValue }
    var fallbackText: String { fallbackMessageLabel.stringValue }
    var scrollOffsetY: CGFloat { scrollView.contentView.bounds.origin.y }

    override var isFlipped: Bool { true }

    init() {
        super.init(frame: .zero)
        isHidden = true
        wantsLayer = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Lyrics")

        headerLabel.alignment = .center
        headerLabel.lineBreakMode = .byTruncatingTail
        closeButton.contentTintColor = PaletteRole.textMuted.color
        closeButton.target = self
        closeButton.action = #selector(closePressed)

        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay
        scrollView.documentView = listView

        fallbackIcon.imageScaling = .scaleProportionallyDown
        fallbackMessageLabel.alignment = .center
        fallbackMessageLabel.font = .systemFont(ofSize: 14, weight: .medium)
        fallbackMessageLabel.textColor = PaletteRole.text.color
        fallbackDetailLabel.alignment = .center
        fallbackDetailLabel.font = .systemFont(ofSize: 12, weight: .regular)
        fallbackDetailLabel.textColor = PaletteRole.textMuted.color
        fallbackView.addSubview(fallbackIcon)
        fallbackView.addSubview(fallbackMessageLabel)
        fallbackView.addSubview(fallbackDetailLabel)

        for view in [headerLabel, closeButton, headerDivider, scrollView, fallbackView] as [NSView] {
            addSubview(view)
        }
        applyHeaderStyle("LYRICS")
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func draw(_ dirtyRect: NSRect) {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            if usesMusicSurface {
                let rect = bounds.insetBy(dx: 0.5, dy: 0.5)
                let path = NSBezierPath(roundedRect: rect, xRadius: 10, yRadius: 10)
                PaletteRole.surface.color.setFill()
                path.fill()
                PaletteRole.borderSubtle.color.setStroke()
                path.lineWidth = 1
                path.stroke()
            } else {
                PaletteRole.canvas.color.setFill()
                bounds.fill()
            }
            PaletteRole.borderSubtle.color.setFill()
            headerDivider.frame.fill()
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyHeaderStyle(displayed.header)
        closeButton.contentTintColor = PaletteRole.textMuted.color
        fallbackIcon.contentTintColor = PaletteRole.accent.color.withAlphaComponent(0.85)
        fallbackMessageLabel.textColor = PaletteRole.text.color
        fallbackDetailLabel.textColor = PaletteRole.textMuted.color
        for row in syncedRowButtons { row.refreshAppearance() }
        for label in plainRowLabels { label.textColor = PaletteRole.text.color.withAlphaComponent(0.88) }
        needsDisplay = true
    }

    override func layout() {
        super.layout()
        let headerHeight: CGFloat = usesMusicSurface ? 32 : 44
        let sideInset: CGFloat = usesMusicSurface ? 12 : 28
        headerLabel.frame = NSRect(x: 34, y: usesMusicSurface ? 8 : 14,
                                   width: max(0, bounds.width - 68), height: 18)
        closeButton.frame = NSRect(x: max(4, bounds.width - 30), y: usesMusicSurface ? 4 : 10, width: 24, height: 24)
        headerDivider.frame = NSRect(x: usesMusicSurface ? 1 : 0, y: headerHeight,
                                     width: max(0, bounds.width - (usesMusicSurface ? 2 : 0)), height: 1)

        let bodyRect = NSRect(x: 0, y: headerHeight + 1,
                              width: bounds.width, height: max(0, bounds.height - headerHeight - 1))
        scrollView.frame = bodyRect
        fallbackView.frame = bodyRect

        layoutFallback(in: bodyRect, sideInset: sideInset)
        layoutRows(in: bodyRect, sideInset: sideInset)

        if let pending = pendingScrollIndex, bodyRect.height > 20, syncedRowButtons.indices.contains(pending) {
            pendingScrollIndex = nil
            scrollRowToCenter(pending)
        }
    }

    /// Update the rendered lyrics from the session's snapshot. Rebuilds row views
    /// only when the sheet changes; moves highlight and auto-scrolls when only
    /// `activeIndex` changes.
    func show(_ snapshot: LyricsSnapshot) {
        let key = SheetKey(source: snapshot.source, generation: snapshot.generation,
                           state: snapshot.state, header: snapshot.header,
                           message: snapshot.message, lines: snapshot.lines)
        let previousActive = displayed.activeIndex
        displayed = snapshot
        if key != lastSheetKey {
            lastSheetKey = key
            rebuild(snapshot)
            if let active = snapshot.activeIndex {
                applyActiveIndex(active, previous: nil)
            }
        } else if snapshot.activeIndex != previousActive {
            applyActiveIndex(snapshot.activeIndex, previous: previousActive)
        }
    }

    /// Immediately clear all rows and reset to unavailable (used on track switch).
    func clear() {
        show(.empty)
    }

    /// Whether the synced row at `index` is currently visible inside the scroll viewport.
    func isRowVisibleInViewport(_ index: Int) -> Bool {
        guard syncedRowButtons.indices.contains(index) else { return false }
        layoutSubtreeIfNeeded()
        let visible = scrollView.documentVisibleRect
        guard !visible.isEmpty else { return false }
        return visible.intersects(syncedRowButtons[index].frame)
    }

    private func rebuild(_ snapshot: LyricsSnapshot) {
        for view in listView.subviews { view.removeFromSuperview() }
        syncedRowButtons.removeAll()
        plainRowLabels.removeAll()
        pendingScrollIndex = nil
        applyHeaderStyle(snapshot.header)

        switch snapshot.state {
        case .synced:
            scrollView.isHidden = false
            fallbackView.isHidden = true
            for (index, line) in snapshot.lines.enumerated() {
                let button = LyricLineButton(line: line, compact: usesMusicSurface)
                button.tag = index
                button.target = self
                button.action = #selector(lyricRowClicked(_:))
                listView.addSubview(button)
                syncedRowButtons.append(button)
            }
            scrollView.contentView.scroll(to: .zero)
            scrollView.reflectScrolledClipView(scrollView.contentView)
            setAccessibilityValue("Synced lyrics, \(snapshot.lines.count) lines")

        case .plain:
            scrollView.isHidden = false
            fallbackView.isHidden = true
            for line in snapshot.lines {
                let label = NSTextField(wrappingLabelWithString: line.text)
                label.alignment = .center
                label.font = .systemFont(ofSize: usesMusicSurface ? 14 : 16, weight: .regular)
                label.textColor = PaletteRole.text.color.withAlphaComponent(0.88)
                label.setAccessibilityRole(.staticText)
                listView.addSubview(label)
                plainRowLabels.append(label)
            }
            scrollView.contentView.scroll(to: .zero)
            scrollView.reflectScrolledClipView(scrollView.contentView)
            setAccessibilityValue("Plain lyrics, \(snapshot.lines.count) lines")

        case .missing, .invalid, .loading, .unavailable:
            scrollView.isHidden = true
            fallbackView.isHidden = false
            let (symbol, message, detail) = fallbackContent(for: snapshot)
            fallbackIcon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?
                .withSymbolConfiguration(.init(pointSize: 24, weight: .regular))
            fallbackIcon.contentTintColor = PaletteRole.accent.color.withAlphaComponent(0.85)
            fallbackMessageLabel.stringValue = message
            fallbackDetailLabel.stringValue = detail
            fallbackDetailLabel.isHidden = detail.isEmpty
            setAccessibilityValue(message)
        }
        needsLayout = true
        needsDisplay = true
    }

    private func fallbackContent(for snapshot: LyricsSnapshot) -> (String, String, String) {
        switch snapshot.state {
        case .missing:
            return ("music.note", snapshot.message ?? "No lyrics found for this track",
                    "Place a matching .lrc file next to the audio file.")
        case .invalid:
            return ("exclamationmark.triangle", snapshot.message ?? "Local lyrics file could not be read",
                    "Check that the sidecar .lrc file contains valid UTF-8 lyrics.")
        case .loading:
            return ("ellipsis.circle", "Loading local lyrics…", "")
        case .unavailable, .synced, .plain:
            return ("music.note", "Lyrics unavailable", "")
        }
    }

    private func applyActiveIndex(_ active: Int?, previous: Int?) {
        if let previous, syncedRowButtons.indices.contains(previous) {
            syncedRowButtons[previous].isActiveLine = false
        }
        if let active, syncedRowButtons.indices.contains(active) {
            syncedRowButtons[active].isActiveLine = true
            needsLayout = true
            layoutSubtreeIfNeeded()
            if scrollView.contentView.bounds.height > 20 && syncedRowButtons[active].frame.height > 0 {
                scrollRowToCenter(active)
            } else {
                pendingScrollIndex = active
            }
        }
    }

    private func scrollRowToCenter(_ index: Int) {
        guard syncedRowButtons.indices.contains(index) else { return }
        let rowFrame = syncedRowButtons[index].frame
        let clipHeight = scrollView.contentView.bounds.height
        let docHeight = listView.frame.height
        guard clipHeight > 0, docHeight > clipHeight else { return }
        let targetY = max(0, min(rowFrame.midY - clipHeight / 2, docHeight - clipHeight))
        let target = NSPoint(x: 0, y: targetY)
        scrollView.contentView.scroll(to: target)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    private func layoutRows(in bodyRect: NSRect, sideInset: CGFloat) {
        let viewportWidth = max(120, bodyRect.width)
        let contentWidth = usesMusicSurface
            ? max(100, viewportWidth - 2 * sideInset)
            : min(520, max(160, viewportWidth - 2 * sideInset))
        let x = (viewportWidth - contentWidth) / 2
        let verticalPad: CGFloat = max(24, min(72, bodyRect.height * 0.24))
        var y = verticalPad

        if !syncedRowButtons.isEmpty {
            for button in syncedRowButtons {
                let height = button.measuredHeight(forWidth: contentWidth)
                button.frame = NSRect(x: x, y: y, width: contentWidth, height: height)
                y += height + 4
            }
        } else if !plainRowLabels.isEmpty {
            for label in plainRowLabels {
                let cellSize = label.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: contentWidth, height: .greatestFiniteMagnitude))
                    ?? CGSize(width: contentWidth, height: 22)
                let height = max(20, ceil(cellSize.height))
                label.frame = NSRect(x: x, y: y, width: contentWidth, height: height)
                y += height + 8
            }
        }
        listView.frame = NSRect(x: 0, y: 0, width: viewportWidth, height: max(bodyRect.height, y + verticalPad))
    }

    private func layoutFallback(in bodyRect: NSRect, sideInset: CGFloat) {
        let contentWidth = min(280, max(120, bodyRect.width - 2 * sideInset))
        let messageSize = fallbackMessageLabel.cell?.cellSize(
            forBounds: NSRect(x: 0, y: 0, width: contentWidth, height: .greatestFiniteMagnitude))
            ?? CGSize(width: contentWidth, height: 20)
        let detailSize = fallbackDetailLabel.isHidden ? .zero
            : (fallbackDetailLabel.cell?.cellSize(
                forBounds: NSRect(x: 0, y: 0, width: contentWidth, height: .greatestFiniteMagnitude))
                ?? CGSize(width: contentWidth, height: 18))
        let iconSide: CGFloat = 28
        let totalHeight = iconSide + 10 + ceil(messageSize.height) + (detailSize.height > 0 ? 6 + ceil(detailSize.height) : 0)
        let top = max(16, (bodyRect.height - totalHeight) / 2)
        let x = (bodyRect.width - contentWidth) / 2
        fallbackIcon.frame = NSRect(x: (bodyRect.width - iconSide) / 2, y: top, width: iconSide, height: iconSide)
        fallbackMessageLabel.frame = NSRect(x: x, y: fallbackIcon.frame.maxY + 10,
                                            width: contentWidth, height: ceil(messageSize.height))
        fallbackDetailLabel.frame = NSRect(x: x, y: fallbackMessageLabel.frame.maxY + 6,
                                           width: contentWidth, height: ceil(detailSize.height))
    }

    private func updateRowTypography() {
        for button in syncedRowButtons { button.compact = usesMusicSurface }
        for label in plainRowLabels {
            label.font = .systemFont(ofSize: usesMusicSurface ? 14 : 16, weight: .regular)
        }
    }

    private func applyHeaderStyle(_ text: String) {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 11, weight: .bold),
            .foregroundColor: PaletteRole.link.color,
            .kern: 1.8,
        ]
        headerLabel.attributedStringValue = NSAttributedString(string: text, attributes: attrs)
    }

    @objc private func lyricRowClicked(_ sender: LyricLineButton) {
        guard syncedRowButtons.indices.contains(sender.tag) else { return }
        onSeek?(sender.line.timeSeconds)
    }

    @objc private func closePressed() {
        onClose?()
    }
}

/// One clickable synced lyric line button.
final class LyricLineButton: NSButton {
    let line: LyricsLineView
    var compact: Bool {
        didSet { updateStyle() }
    }
    var isActiveLine = false {
        didSet {
            guard oldValue != isActiveLine else { return }
            updateStyle()
            setAccessibilityValue(isActiveLine ? "Current line" : nil)
            needsDisplay = true
        }
    }

    private var hovered = false { didSet { needsDisplay = true } }
    private var tracking: NSTrackingArea?
    private let label = NSTextField(wrappingLabelWithString: "")

    override var isFlipped: Bool { true }

    init(line: LyricsLineView, compact: Bool) {
        self.line = line
        self.compact = compact
        super.init(frame: .zero)
        isBordered = false
        title = ""
        setButtonType(.momentaryChange)
        focusRingType = .none
        label.alignment = .center
        label.stringValue = line.text
        label.isEditable = false
        label.isSelectable = false
        label.drawsBackground = false
        label.isBezeled = false
        addSubview(label)
        setAccessibilityRole(.button)
        setAccessibilityLabel(line.isGap ? "Instrumental break" : line.text)
        updateStyle()
    }

    required init?(coder: NSCoder) { fatalError("Use init(line:compact:)") }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeInKeyWindow], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { hovered = true }
    override func mouseExited(with event: NSEvent) { hovered = false }

    override func layout() {
        super.layout()
        label.frame = bounds.insetBy(dx: 10, dy: 6)
    }

    override func draw(_ dirtyRect: NSRect) {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            let rect = bounds.insetBy(dx: 2, dy: 1)
            let pill = NSBezierPath(roundedRect: rect, xRadius: 8, yRadius: 8)
            if isActiveLine {
                PaletteRole.accent.color.withAlphaComponent(0.16).setFill()
                pill.fill()
            } else if hovered || isHighlighted {
                PaletteRole.text.color.withAlphaComponent(0.07).setFill()
                pill.fill()
            }
            if window?.firstResponder === self {
                PaletteRole.focus.color.setStroke()
                pill.lineWidth = 2
                pill.stroke()
            }
        }
    }

    func measuredHeight(forWidth width: CGFloat) -> CGFloat {
        let inner = max(40, width - 20)
        let size = label.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: inner, height: .greatestFiniteMagnitude))
            ?? CGSize(width: inner, height: 22)
        return max(30, ceil(size.height) + 12)
    }

    func refreshAppearance() {
        updateStyle()
        needsDisplay = true
    }

    private func updateStyle() {
        let size: CGFloat
        let weight: NSFont.Weight
        if isActiveLine {
            size = compact ? 16 : 20
            weight = .bold
        } else {
            size = compact ? 14 : 17
            weight = .medium
        }
        label.font = .systemFont(ofSize: size, weight: weight)
        if isActiveLine {
            label.textColor = PaletteRole.text.color
        } else if line.isGap {
            label.textColor = PaletteRole.textMuted.color.withAlphaComponent(0.45)
        } else {
            label.textColor = PaletteRole.textMuted.color.withAlphaComponent(0.75)
        }
    }
}

private final class FlippedLyricsDocumentView: NSView {
    override var isFlipped: Bool { true }
}
