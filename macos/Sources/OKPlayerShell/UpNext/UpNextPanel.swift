import AppKit

/// The Up Next side panel (#884): the session playlist under a pinned Now Playing
/// card, laid over the right of the video. A double click or Return on a row plays
/// it (a playlist `jump`); Delete or the row's menu removes it (`remove`). Every
/// row comes from `UpNextList`, and every edit is a session command (`dispatch`):
/// the panel keeps no playlist of its own.
final class UpNextPanel: NSView, NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    /// Sends a playlist command to the session.
    var dispatch: ((Command) -> Void)?
    /// The close button was pressed.
    var onClose: (() -> Void)?
    /// Where the keys the list does not handle go, and keyboard focus when the
    /// panel hides: the video, which handles Space.
    weak var focusReturn: NSView?

    /// What the panel shows now.
    private(set) var list = UpNextList()
    let titleLabel = NSTextField(labelWithString: "Up Next")
    let closeButton = TransportOverlay.iconButton("xmark", "Close Up Next")
    let nowPlayingCard = UpNextCardView()
    let nowPlayingIcon = NSImageView()
    let nowPlayingTitle = NSTextField(labelWithString: "Nothing is playing")
    let nowPlayingState = NSTextField(labelWithString: "")
    let headingLabel = NSTextField(labelWithString: "")
    let emptyLabel = NSTextField(labelWithString: "Nothing in Up Next")
    let tableView = UpNextTableView()
    private let scrollView = NSScrollView()

    init() {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Up Next")
        build()
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    /// Show `list`; the rows reload only when it changed.
    func show(_ list: UpNextList) {
        guard list != self.list else { return }
        self.list = list
        nowPlayingTitle.stringValue = list.nowPlaying?.title ?? "Nothing is playing"
        nowPlayingState.stringValue = list.nowPlaying == nil ? "" : "NOW PLAYING"
        nowPlayingIcon.isHidden = list.nowPlaying == nil
        nowPlayingCard.toolTip = list.nowPlaying?.location
        headingLabel.stringValue = list.heading
        emptyLabel.isHidden = !list.rows.isEmpty
        tableView.reloadData()
    }

    /// Give keyboard focus back to the video when the list holds it.
    func returnFocus() {
        guard let window, let responder = window.firstResponder as? NSView, responder.isDescendant(of: self) else { return }
        _ = window.makeFirstResponder(focusReturn)
    }

    /// The row menu for `row`: play it and remove it, each enabled when the session
    /// would act on it. Empty for a row that is not there.
    func menuItems(forRow row: Int) -> [NSMenuItem] {
        guard list.rows.indices.contains(row) else { return [] }
        let play = NSMenuItem(title: "Play", action: #selector(playChosen(_:)), keyEquivalent: "")
        play.isEnabled = list.activation(ofRow: row) != nil
        let remove = NSMenuItem(title: "Remove from Up Next", action: #selector(removeChosen(_:)), keyEquivalent: "")
        remove.isEnabled = list.removal(ofRow: row) != nil
        for item in [play, remove] {
            item.target = self
            item.tag = row
        }
        return [play, remove]
    }

    override func draw(_ dirtyRect: NSRect) {
        let body = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 12, yRadius: 12)
        PaletteRole.surface.color.setFill()
        body.fill()
        PaletteRole.borderSubtle.color.setStroke()
        body.lineWidth = 1
        body.stroke()
    }

    // MARK: Rows

    func numberOfRows(in tableView: NSTableView) -> Int { list.rows.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: UpNextCellView.reuseIdentifier, owner: self) as? UpNextCellView
            ?? UpNextCellView()
        cell.show(list.rows[row])
        return cell
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let view = UpNextRowView()
        view.isCurrent = list.rows[row].isCurrent
        return view
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        for item in menuItems(forRow: tableView.clickedRow) { menu.addItem(item) }
    }

    private func handle(_ key: UpNextKey, row: Int) {
        let command = key == .activate ? list.activation(ofRow: row) : list.removal(ofRow: row)
        if let command { dispatch?(command) }
    }

    @objc private func rowDoubleClicked(_ sender: Any?) {
        if let command = list.activation(ofRow: tableView.clickedRow) { dispatch?(command) }
    }

    @objc private func playChosen(_ item: NSMenuItem) {
        if let command = list.activation(ofRow: item.tag) { dispatch?(command) }
    }

    @objc private func removeChosen(_ item: NSMenuItem) {
        if let command = list.removal(ofRow: item.tag) { dispatch?(command) }
    }

    @objc private func closeChosen() { onClose?() }

    // MARK: Layout

    private func build() {
        titleLabel.font = NSFont.systemFont(ofSize: 13, weight: .semibold)
        titleLabel.textColor = PaletteRole.text.color
        closeButton.contentTintColor = PaletteRole.textMuted.color
        closeButton.target = self
        closeButton.action = #selector(closeChosen)

        nowPlayingIcon.image = NSImage(systemSymbolName: "play.fill", accessibilityDescription: "Now playing")?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 13, weight: .semibold))
        nowPlayingIcon.contentTintColor = PaletteRole.accent.color
        nowPlayingIcon.isHidden = true
        nowPlayingTitle.font = NSFont.systemFont(ofSize: 12.5, weight: .semibold)
        nowPlayingTitle.textColor = PaletteRole.text.color
        nowPlayingState.font = NSFont.systemFont(ofSize: 10, weight: .semibold)
        nowPlayingState.textColor = PaletteRole.textMuted.color
        headingLabel.font = NSFont.systemFont(ofSize: 10.5, weight: .semibold)
        headingLabel.textColor = PaletteRole.textMuted.color
        emptyLabel.font = NSFont.systemFont(ofSize: 12)
        emptyLabel.textColor = PaletteRole.textMuted.color
        for label in [titleLabel, nowPlayingTitle, nowPlayingState, headingLabel, emptyLabel] {
            label.lineBreakMode = .byTruncatingMiddle
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("item"))
        column.resizingMask = .autoresizingMask
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.style = .plain
        tableView.backgroundColor = .clear
        tableView.rowHeight = 28
        tableView.intercellSpacing = NSSize(width: 0, height: 2)
        tableView.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        tableView.allowsMultipleSelection = false
        tableView.focusRingType = .none
        tableView.setAccessibilityLabel("Up Next items")
        tableView.dataSource = self
        tableView.delegate = self
        tableView.target = self
        tableView.doubleAction = #selector(rowDoubleClicked(_:))
        tableView.onKey = { [weak self] key, row in self?.handle(key, row: row) }
        tableView.forward = { [weak self] event in self?.focusReturn?.keyDown(with: event) }
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.delegate = self
        tableView.menu = menu
        scrollView.documentView = tableView
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder

        for view in [titleLabel, closeButton, nowPlayingCard, headingLabel, scrollView, emptyLabel] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        for view in [nowPlayingIcon, nowPlayingTitle, nowPlayingState] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            nowPlayingCard.addSubview(view)
        }
        NSLayoutConstraint.activate([
            titleLabel.topAnchor.constraint(equalTo: topAnchor, constant: 12),
            titleLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            closeButton.centerYAnchor.constraint(equalTo: titleLabel.centerYAnchor),
            closeButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            closeButton.leadingAnchor.constraint(greaterThanOrEqualTo: titleLabel.trailingAnchor, constant: 8),

            nowPlayingCard.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 10),
            nowPlayingCard.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            nowPlayingCard.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            nowPlayingCard.heightAnchor.constraint(equalToConstant: 48),
            nowPlayingIcon.leadingAnchor.constraint(equalTo: nowPlayingCard.leadingAnchor, constant: 12),
            nowPlayingIcon.centerYAnchor.constraint(equalTo: nowPlayingCard.centerYAnchor),
            nowPlayingIcon.widthAnchor.constraint(equalToConstant: 16),
            nowPlayingTitle.leadingAnchor.constraint(equalTo: nowPlayingIcon.trailingAnchor, constant: 10),
            nowPlayingTitle.trailingAnchor.constraint(lessThanOrEqualTo: nowPlayingCard.trailingAnchor, constant: -10),
            nowPlayingTitle.bottomAnchor.constraint(equalTo: nowPlayingCard.centerYAnchor, constant: 1),
            nowPlayingState.leadingAnchor.constraint(equalTo: nowPlayingTitle.leadingAnchor),
            nowPlayingState.trailingAnchor.constraint(lessThanOrEqualTo: nowPlayingCard.trailingAnchor, constant: -10),
            nowPlayingState.topAnchor.constraint(equalTo: nowPlayingCard.centerYAnchor, constant: 2),

            headingLabel.topAnchor.constraint(equalTo: nowPlayingCard.bottomAnchor, constant: 12),
            headingLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            headingLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -14),
            scrollView.topAnchor.constraint(equalTo: headingLabel.bottomAnchor, constant: 4),
            scrollView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            scrollView.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
            scrollView.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
            emptyLabel.topAnchor.constraint(equalTo: scrollView.topAnchor, constant: 8),
            emptyLabel.centerXAnchor.constraint(equalTo: centerXAnchor),
            emptyLabel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 14),
        ])
    }
}

/// The Now Playing card's rounded fill.
final class UpNextCardView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        PaletteRole.selected.color.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 8, yRadius: 8).fill()
    }
}

/// The list of the panel. Return/Enter and Delete/Forward Delete reach `onKey`
/// with the selected row, the selection keys move the selection, and every other
/// key goes to `forward` (the video), so Space and F keep working while the list
/// has keyboard focus.
final class UpNextTableView: NSTableView {
    var onKey: ((UpNextKey, Int) -> Void)?
    var forward: ((NSEvent) -> Void)?

    override func keyDown(with event: NSEvent) {
        if let key = UpNextKey(keyCode: event.keyCode, modifiers: event.modifierFlags) {
            onKey?(key, selectedRow)
        } else if UpNextKey.navigationKeyCodes.contains(event.keyCode) || forward == nil {
            super.keyDown(with: event)
        } else {
            forward?(event)
        }
    }
}

/// One row: the item's number, or a play mark for the current item, and its title.
final class UpNextCellView: NSTableCellView {
    static let reuseIdentifier = NSUserInterfaceItemIdentifier("UpNextCell")

    let lane = NSTextField(labelWithString: "")
    let playing = NSImageView()
    let title = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = Self.reuseIdentifier
        textField = title
        lane.font = NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium)
        lane.textColor = PaletteRole.textMuted.color
        lane.alignment = .center
        playing.image = NSImage(systemSymbolName: "play.fill", accessibilityDescription: "Playing")?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 10, weight: .semibold))
        playing.contentTintColor = PaletteRole.accent.color
        title.textColor = PaletteRole.text.color
        title.lineBreakMode = .byTruncatingMiddle
        title.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for view in [lane, playing, title] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            lane.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            lane.widthAnchor.constraint(equalToConstant: 22),
            lane.centerYAnchor.constraint(equalTo: centerYAnchor),
            playing.centerXAnchor.constraint(equalTo: lane.centerXAnchor),
            playing.centerYAnchor.constraint(equalTo: centerYAnchor),
            title.leadingAnchor.constraint(equalTo: lane.trailingAnchor, constant: 6),
            title.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -8),
            title.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    func show(_ row: UpNextRow) {
        lane.stringValue = row.number
        lane.isHidden = row.isCurrent
        playing.isHidden = !row.isCurrent
        title.stringValue = row.title
        title.font = NSFont.systemFont(ofSize: 12.5, weight: row.isCurrent ? .semibold : .regular)
        toolTip = row.location
    }
}

/// A row's background: the current item sits on the selected fill.
final class UpNextRowView: NSTableRowView {
    var isCurrent = false { didSet { if isCurrent != oldValue { needsDisplay = true } } }

    override func drawBackground(in dirtyRect: NSRect) {
        guard isCurrent else { return }
        PaletteRole.selected.color.setFill()
        NSBezierPath(roundedRect: bounds.insetBy(dx: 4, dy: 0), xRadius: 6, yRadius: 6).fill()
    }
}
