import AppKit

/// A single side panel with Chapters and Up Next tabs.
final class UpNextPanel: NSView, NSTableViewDataSource, NSTableViewDelegate {
    enum Tab: Int { case chapters, upNext }

    var dispatch: ((Command) -> Void)?
    var onClose: (() -> Void)?
    var onAddFiles: (() -> Void)?
    var onDragInvalidated: (() -> Void)?
    weak var focusReturn: NSView?

    private(set) var list = UpNextList()
    private(set) var selectedTab: Tab = .chapters
    let chaptersButton = NSButton(title: "Chapters", target: nil, action: nil)
    let upNextButton = NSButton(title: "Up Next", target: nil, action: nil)
    let closeButton = TransportOverlay.iconButton("xmark", "Close Chapters & Up Next")
    let nowPlayingCard = UpNextCardView()
    let nowPlayingTitle = UpNextMaterial.label(size: 12.5, weight: .semibold, color: UpNextMaterial.link)
    let nowPlayingState = UpNextMaterial.label("NOW PLAYING", size: 10.5, weight: .semibold, color: UpNextMaterial.accent)
    let headingLabel = UpNextMaterial.label(size: 11, weight: .semibold, color: UpNextMaterial.muted)
    let emptyLabel = UpNextMaterial.label("Just this file", size: 13, weight: .semibold)
    let addFilesButton = AccentButton(label: "Add files…", target: nil, action: nil)
    let tableView = UpNextTableView()
    let insertionLine = NSView()

    private let queue = NSView()
    private let chapters = NSScrollView()
    private let chapterText = NSTextField(wrappingLabelWithString: "No chapters in this file")
    private let emptyDetail = NSTextField(wrappingLabelWithString: "")
    private let nowPlayingIcon = UpNextThumbnail()
    private let scrollView = NSScrollView()
    private var displayedChapters: [Chapter] = []
    private(set) var rowMenu: NSView?
    private var rowMenuItems: [NSMenuItem] = []
    var drag: (row: Int, list: UpNextList)?

    init() {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Chapters & Up Next")
        build()
        refreshTabs()
        refreshQueue()
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func draw(_ dirtyRect: NSRect) {
        // Extend the shape past the right edge so only its left corners are rounded.
        let rect = NSRect(x: 0.5, y: 0.5, width: bounds.width + 12, height: bounds.height - 1)
        let body = NSBezierPath(roundedRect: rect, xRadius: 12, yRadius: 12)
        UpNextMaterial.paper.setFill()
        body.fill()
        NSColor.black.withAlphaComponent(0.12).setStroke()
        body.stroke()
    }

    func show(_ list: UpNextList) {
        guard list != self.list else { return }
        dismissRowMenu()
        if drag != nil {
            drag = nil
            onDragInvalidated?()
        }
        let selection = self.list.row(at: tableView.selectedRow)
        self.list = list
        refreshQueue()
        if let selection,
           let row = list.rows.firstIndex(where: { $0.index == selection.index && $0.source == selection.source }),
           list.heading != nil {
            tableView.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        } else {
            tableView.deselectAll(nil)
        }
    }

    func showChapters(_ entries: [Chapter]) {
        guard displayedChapters != entries else { return }
        displayedChapters = entries
        chapterText.stringValue = entries.isEmpty ? "No chapters in this file" :
            entries.enumerated().map { index, chapter in
                let seconds = chapter.time.isFinite ? Int(min(max(0, chapter.time), Double(Int32.max))) : 0
                let time = String(format: "%02d:%02d", seconds / 60, seconds % 60)
                return "\(time)    \(chapter.title ?? "Chapter \(index + 1)")"
            }.joined(separator: "\n\n")
    }

    func select(_ tab: Tab) {
        guard tab != selectedTab else { refreshTabs(); return }
        dismissRowMenu()
        if tab == .chapters, window?.firstResponder === tableView {
            window?.makeFirstResponder(chaptersButton)
        }
        selectedTab = tab
        refreshTabs()
    }

    func returnFocus() {
        guard let window, let responder = window.firstResponder as? NSView, responder.isDescendant(of: self) else { return }
        _ = window.makeFirstResponder(focusReturn)
    }

    private func refreshTabs() {
        let buttons: [(Tab, NSButton)] = [(.chapters, chaptersButton), (.upNext, upNextButton)]
        for (tab, button) in buttons {
            let selected = tab == selectedTab
            button.state = selected ? .on : .off
            button.layer?.backgroundColor = selected ? NSColor.white.withAlphaComponent(0.96).cgColor : nil
            button.contentTintColor = selected ? UpNextMaterial.link : UpNextMaterial.muted
            button.attributedTitle = NSAttributedString(string: button.title, attributes: [
                .font: NSFont.systemFont(ofSize: 12.5, weight: .semibold),
                .foregroundColor: selected ? UpNextMaterial.link : UpNextMaterial.muted,
            ])
        }
        chapters.isHidden = selectedTab != .chapters
        queue.isHidden = selectedTab != .upNext
    }

    private func refreshQueue() {
        let now = list.nowPlaying
        nowPlayingCard.isHidden = now == nil
        nowPlayingTitle.stringValue = now?.title ?? ""
        nowPlayingIcon.image = now.flatMap { UpNextMaterial.symbol($0.source.kind == .url ? "network" : "video") }
        nowPlayingCard.toolTip = now?.location
        headingLabel.stringValue = list.heading ?? ""
        headingLabel.isHidden = list.heading == nil
        scrollView.isHidden = list.heading == nil
        emptyLabel.isHidden = list.heading != nil
        emptyDetail.isHidden = list.heading != nil
        addFilesButton.isHidden = list.heading != nil
        emptyLabel.stringValue = list.scanning ? "Scanning folder…" : "Just this file"
        emptyDetail.stringValue = now?.source.kind == .url
            ? "Single URL — no folder. Add files to build a queue."
            : "Add files to build a queue."
        showInsertion(before: nil)
        tableView.reloadData()
    }

    // MARK: Rows and actions

    func numberOfRows(in tableView: NSTableView) -> Int { list.heading == nil ? 0 : list.rows.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: UpNextCellView.reuseIdentifier, owner: self) as? UpNextCellView
            ?? UpNextCellView()
        cell.show(list.rows[row])
        cell.onActions = { [weak self, weak cell] in
            guard let self, let cell else { return }
            self.showRowMenu(forRow: row, anchoredTo: cell.actions)
        }
        return cell
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let view = UpNextRowView()
        view.isCurrent = list.rows[row].isCurrent
        return view
    }

    @discardableResult
    func dismissRowMenu() -> Bool {
        guard let rowMenu else { return false }
        rowMenu.removeFromSuperview()
        self.rowMenu = nil
        rowMenuItems.removeAll()
        return true
    }

    func dismissRowMenu(outside windowPoint: NSPoint) -> Bool {
        guard let rowMenu, !rowMenu.bounds.contains(rowMenu.convert(windowPoint, from: nil)) else {
            return false
        }
        return dismissRowMenu()
    }

    private func showRowMenu(forRow row: Int, anchoredTo anchor: NSView) {
        dismissRowMenu()
        let items = menuItems(forRow: row)
        guard !items.isEmpty else { return }
        let width: CGFloat = 178
        let height = CGFloat(items.count) * 26 + 12
        let rect = anchor.convert(anchor.bounds, to: self)
        let menu = PalettePanel(fill: UpNextMaterial.paper, border: NSColor.black.withAlphaComponent(0.14), radius: 8)
        menu.frame = NSRect(x: max(4, rect.maxX - width), y: max(6, rect.minY - height),
                            width: width, height: height)
        menu.setAccessibilityElement(true)
        menu.setAccessibilityRole(.group)
        menu.setAccessibilityLabel("Queue actions")
        for (index, item) in items.enumerated() {
            let button = NSButton(title: item.title, target: self, action: #selector(rowMenuButtonChosen(_:)))
            button.frame = NSRect(x: 8, y: height - 32 - CGFloat(index) * 26, width: width - 16, height: 26)
            button.alignment = .left
            button.isBordered = false
            button.isEnabled = item.isEnabled
            button.contentTintColor = item.isEnabled ? UpNextMaterial.ink : UpNextMaterial.muted
            button.tag = index
            menu.addSubview(button)
        }
        rowMenuItems = items
        rowMenu = menu
        addSubview(menu, positioned: .above, relativeTo: queue)
    }

    @objc private func rowMenuButtonChosen(_ button: NSButton) {
        guard rowMenuItems.indices.contains(button.tag) else { return }
        let item = rowMenuItems[button.tag]
        dismissRowMenu()
        runMenuItem(item)
    }

    func makeMenu(forRow row: Int) -> NSMenu {
        let menu = NSMenu(title: "Queue actions")
        menu.autoenablesItems = false
        for item in menuItems(forRow: row) { menu.addItem(item) }
        return menu
    }

    func menuItems(forRow row: Int) -> [NSMenuItem] {
        guard list.row(at: row) != nil else { return [] }
        let actions = [
            ("Play", list.activation(ofRow: row)),
            ("Play Next", list.playNext(ofRow: row)),
            ("Move Up", list.moving(from: row, before: row - 1)),
            ("Move Down", list.moving(from: row, before: row + 2)),
            ("Remove from Up Next", list.removal(ofRow: row)),
        ]
        return actions.map { title, command in
            let item = NSMenuItem(title: title, action: #selector(runMenuItem(_:)), keyEquivalent: "")
            item.isEnabled = command != nil
            item.target = self
            if let command { item.representedObject = MenuAction(rows: list.rows, command: command) }
            return item
        }
    }

    private struct MenuAction {
        let rows: [UpNextRow]
        let command: Command
    }

    @objc private func runMenuItem(_ item: NSMenuItem) {
        guard let action = item.representedObject as? MenuAction, list.rows == action.rows else { return }
        dispatch?(action.command)
    }

    private func handle(_ key: UpNextKey, row: Int) {
        let command = key == .activate ? list.activation(ofRow: row) : list.removal(ofRow: row)
        if let command { dispatch?(command) }
    }

    @objc private func rowDoubleClicked(_ sender: Any?) {
        if let command = list.activation(ofRow: tableView.clickedRow) { dispatch?(command) }
    }

    @objc private func tabChosen(_ sender: NSButton) {
        select(sender === chaptersButton ? .chapters : .upNext)
    }

    @objc private func addFilesChosen() { onAddFiles?() }
    @objc private func closeChosen() { onClose?() }

    // MARK: Layout

    private func build() {
        let tabs = NSView()
        tabs.wantsLayer = true
        tabs.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.08).cgColor
        tabs.layer?.cornerRadius = 8
        for button in [chaptersButton, upNextButton] {
            button.setButtonType(.pushOnPushOff)
            button.isBordered = false
            button.wantsLayer = true
            button.layer?.cornerRadius = 6
            button.target = self
            button.action = #selector(tabChosen(_:))
            button.translatesAutoresizingMaskIntoConstraints = false
            tabs.addSubview(button)
            button.widthAnchor.constraint(equalToConstant: 85).isActive = true
        }
        closeButton.contentTintColor = UpNextMaterial.muted
        closeButton.target = self
        closeButton.action = #selector(closeChosen)

        buildRows()
        buildChapters()
        nowPlayingIcon.contentTintColor = NSColor.black.withAlphaComponent(0.58)
        emptyDetail.textColor = UpNextMaterial.muted
        emptyDetail.font = .systemFont(ofSize: 12)
        addFilesButton.font = .systemFont(ofSize: 12, weight: .semibold)
        addFilesButton.target = self
        addFilesButton.action = #selector(addFilesChosen)

        for view in [tabs, closeButton, queue, chapters] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        for view in [nowPlayingCard, headingLabel, scrollView, emptyLabel, emptyDetail, addFilesButton] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            queue.addSubview(view)
        }
        for view in [nowPlayingIcon, nowPlayingTitle, nowPlayingState] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            nowPlayingCard.addSubview(view)
        }
        NSLayoutConstraint.activate([
            tabs.topAnchor.constraint(equalTo: topAnchor, constant: 14),
            tabs.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            chaptersButton.leadingAnchor.constraint(equalTo: tabs.leadingAnchor, constant: 3),
            chaptersButton.topAnchor.constraint(equalTo: tabs.topAnchor, constant: 3),
            chaptersButton.bottomAnchor.constraint(equalTo: tabs.bottomAnchor, constant: -3),
            chaptersButton.heightAnchor.constraint(equalToConstant: 28),
            upNextButton.leadingAnchor.constraint(equalTo: chaptersButton.trailingAnchor),
            upNextButton.topAnchor.constraint(equalTo: chaptersButton.topAnchor),
            upNextButton.bottomAnchor.constraint(equalTo: chaptersButton.bottomAnchor),
            upNextButton.trailingAnchor.constraint(equalTo: tabs.trailingAnchor, constant: -3),
            closeButton.centerYAnchor.constraint(equalTo: tabs.centerYAnchor),
            closeButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            closeButton.leadingAnchor.constraint(greaterThanOrEqualTo: tabs.trailingAnchor),

            queue.topAnchor.constraint(equalTo: tabs.bottomAnchor, constant: 12),
            queue.leadingAnchor.constraint(equalTo: leadingAnchor),
            queue.trailingAnchor.constraint(equalTo: trailingAnchor),
            queue.bottomAnchor.constraint(equalTo: bottomAnchor),
            chapters.topAnchor.constraint(equalTo: queue.topAnchor),
            chapters.leadingAnchor.constraint(equalTo: queue.leadingAnchor, constant: 14),
            chapters.trailingAnchor.constraint(equalTo: queue.trailingAnchor, constant: -14),
            chapters.bottomAnchor.constraint(equalTo: queue.bottomAnchor, constant: -10),

            nowPlayingCard.topAnchor.constraint(equalTo: queue.topAnchor),
            nowPlayingCard.leadingAnchor.constraint(equalTo: queue.leadingAnchor, constant: 14),
            nowPlayingCard.trailingAnchor.constraint(equalTo: queue.trailingAnchor, constant: -14),
            nowPlayingCard.heightAnchor.constraint(equalToConstant: 66),
            nowPlayingIcon.leadingAnchor.constraint(equalTo: nowPlayingCard.leadingAnchor, constant: 8),
            nowPlayingIcon.centerYAnchor.constraint(equalTo: nowPlayingCard.centerYAnchor),
            nowPlayingIcon.widthAnchor.constraint(equalTo: nowPlayingCard.widthAnchor, multiplier: 0.42),
            nowPlayingIcon.heightAnchor.constraint(equalToConstant: 34),
            nowPlayingTitle.leadingAnchor.constraint(equalTo: nowPlayingIcon.trailingAnchor, constant: 10),
            nowPlayingTitle.trailingAnchor.constraint(lessThanOrEqualTo: nowPlayingCard.trailingAnchor, constant: -8),
            nowPlayingTitle.bottomAnchor.constraint(equalTo: nowPlayingCard.centerYAnchor, constant: 1),
            nowPlayingState.leadingAnchor.constraint(equalTo: nowPlayingTitle.leadingAnchor),
            nowPlayingState.topAnchor.constraint(equalTo: nowPlayingCard.centerYAnchor, constant: 2),

            headingLabel.topAnchor.constraint(equalTo: nowPlayingCard.bottomAnchor, constant: 15),
            headingLabel.leadingAnchor.constraint(equalTo: queue.leadingAnchor, constant: 20),
            headingLabel.trailingAnchor.constraint(lessThanOrEqualTo: queue.trailingAnchor, constant: -14),
            scrollView.topAnchor.constraint(equalTo: headingLabel.bottomAnchor, constant: 4),
            scrollView.leadingAnchor.constraint(equalTo: queue.leadingAnchor, constant: 8),
            scrollView.trailingAnchor.constraint(equalTo: queue.trailingAnchor, constant: -8),
            scrollView.bottomAnchor.constraint(equalTo: queue.bottomAnchor, constant: -10),

            emptyLabel.topAnchor.constraint(equalTo: nowPlayingCard.bottomAnchor, constant: 18),
            emptyLabel.leadingAnchor.constraint(equalTo: nowPlayingCard.leadingAnchor),
            emptyDetail.topAnchor.constraint(equalTo: emptyLabel.bottomAnchor, constant: 4),
            emptyDetail.leadingAnchor.constraint(equalTo: emptyLabel.leadingAnchor),
            emptyDetail.trailingAnchor.constraint(equalTo: nowPlayingCard.trailingAnchor),
            addFilesButton.topAnchor.constraint(equalTo: emptyDetail.bottomAnchor, constant: 8),
            addFilesButton.leadingAnchor.constraint(equalTo: emptyLabel.leadingAnchor),
        ])
    }

    private func buildRows() {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("item"))
        column.resizingMask = .autoresizingMask
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.style = .plain
        tableView.backgroundColor = .clear
        tableView.rowHeight = 54
        tableView.intercellSpacing = NSSize(width: 0, height: 4)
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
        tableView.menuForRow = { [weak self] in self?.makeMenu(forRow: $0) }
        tableView.onDragExit = { [weak self] in self?.showInsertion(before: nil) }
        tableView.registerForDraggedTypes([Self.reorderType])
        tableView.setDraggingSourceOperationMask(.move, forLocal: true)
        tableView.draggingDestinationFeedbackStyle = .none
        scrollView.documentView = tableView
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        insertionLine.wantsLayer = true
        insertionLine.layer?.backgroundColor = UpNextMaterial.accent.cgColor
        insertionLine.layer?.cornerRadius = 1
        insertionLine.isHidden = true
        scrollView.contentView.addSubview(insertionLine, positioned: .above, relativeTo: tableView)
    }

    private func buildChapters() {
        chapterText.textColor = UpNextMaterial.ink
        chapterText.font = .systemFont(ofSize: 13)
        chapters.drawsBackground = false
        chapters.hasVerticalScroller = true
        chapters.autohidesScrollers = true
        let page = NSView()
        chapters.documentView = page
        page.translatesAutoresizingMaskIntoConstraints = false
        chapterText.translatesAutoresizingMaskIntoConstraints = false
        page.addSubview(chapterText)
        NSLayoutConstraint.activate([
            page.leadingAnchor.constraint(equalTo: chapters.contentView.leadingAnchor),
            page.topAnchor.constraint(equalTo: chapters.contentView.topAnchor),
            page.widthAnchor.constraint(equalTo: chapters.contentView.widthAnchor),
            chapterText.topAnchor.constraint(equalTo: page.topAnchor, constant: 5),
            chapterText.bottomAnchor.constraint(equalTo: page.bottomAnchor, constant: -5),
            chapterText.leadingAnchor.constraint(equalTo: page.leadingAnchor, constant: 6),
            chapterText.trailingAnchor.constraint(equalTo: page.trailingAnchor, constant: -6),
        ])
    }
}
