import AppKit

/// A command surface in the player's content plane. Keeping it in the player
/// window lets the same input router own focus, Escape, and dismissing clicks.
final class CommandPopover: PalettePanel, NSSearchFieldDelegate {
    private let commands: PlayerCommands
    private let search = NSSearchField()
    private let scroll = NSScrollView()
    private let list = CommandList()
    private let groupHeading = NSUserInterfaceItemIdentifier("command-group-heading")
    private weak var shadowHost: NSView?
    private weak var host: NSView?
    private weak var previousResponder: NSResponder?
    weak var focusReturn: NSView?
    private var anchor: NSPoint?
    private var page: CommandMenu = .topLevel
    private var pageTitle: String?
    private var submenuID: String?
    private var model: CommandSurface?
    private var selection: CommandPopoverRow?
    private var rows: [NSView] = []
    private var lastSearch = ""
    var query: ((CommandMenu, String) -> CommandSurface?)?
    var onVisibilityChange: ((Bool) -> Void)?
    var clampsToWindow = true

    var isShown: Bool { superview != nil }
    var currentPage: CommandMenu { page }
    var entries: [CommandSurfaceEntry] { model?.entries ?? [] }
    var searchText: String { search.stringValue }
    var isSearching: Bool {
        guard let editor = search.currentEditor() else { return false }
        return editor === window?.firstResponder
    }
    var showsEmptyResult: Bool {
        rows.contains { ($0 as? NSTextField)?.stringValue == "No matching commands" }
    }

    init(commands: PlayerCommands) {
        self.commands = commands
        let fill = NSColor(name: nil) { appearance in
            let role: PaletteRole = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
                ? .surfaceRaised : .surface
            let colors = role.srgb
            return .fromSRGB(role == .surfaceRaised ? colors.dark : colors.light)
        }
        super.init(fill: fill, radius: 8)
        search.placeholderString = "Search commands"
        search.setAccessibilityLabel("Search commands")
        search.delegate = self
        search.target = self
        search.action = #selector(searchAction)
        search.sendsSearchStringImmediately = true
        search.sendsWholeSearchString = false
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.scrollerStyle = .overlay
        scroll.documentView = list
        addSubview(search)
        addSubview(scroll)
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Player commands")
    }

    required init?(coder: NSCoder) { fatalError("Use init(commands:)") }

    deinit { NotificationCenter.default.removeObserver(self) }

    func install(in host: NSView) {
        self.host = host
        guard let window = host.window else { return }
        for name in [NSWindow.didMoveNotification, NSWindow.didResizeNotification, NSWindow.didChangeScreenNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(windowMoved(_:)),
                                                   name: name, object: window)
        }
        for name in [NSWindow.didResignKeyNotification, NSWindow.willCloseNotification,
                     NSWindow.willEnterFullScreenNotification, NSWindow.willExitFullScreenNotification] {
            NotificationCenter.default.addObserver(self, selector: #selector(windowDismissed(_:)),
                                                   name: name, object: window)
        }
    }

    @discardableResult
    func show(at windowPoint: NSPoint? = nil) -> Bool {
        guard let host, let window = host.window, window.attachedSheet == nil,
              !isShown, let initial = query?(.topLevel, "") else { return false }
        host.layoutSubtreeIfNeeded()
        let firstResponder = window.firstResponder
        let editingControl: NSResponder? = (firstResponder as? NSTextView)?.delegate as? NSView
        previousResponder = editingControl ?? firstResponder
        anchor = windowPoint.map { host.convert($0, from: nil) }
        search.stringValue = ""
        lastSearch = ""
        page = .topLevel
        pageTitle = nil
        submenuID = nil
        model = initial
        let shadowHost = NSView()
        shadowHost.wantsLayer = true
        shadowHost.layer?.shadowOpacity = 0.30
        shadowHost.layer?.shadowRadius = 14
        shadowHost.layer?.shadowOffset = CGSize(width: 0, height: -9)
        shadowHost.addSubview(self)
        host.addSubview(shadowHost, positioned: .above, relativeTo: nil)
        self.shadowHost = shadowHost
        // Set a provisional frame before asking the field editor to take focus.
        rebuildRows(initial.entries)
        reposition()
        guard isShown else { return false }
        scrollToTop()
        search.selectText(nil)
        onVisibilityChange?(true)
        return true
    }

    @discardableResult
    func dismiss() -> Bool {
        guard isShown else { return false }
        if let window, isSearching || (window.firstResponder as? NSView)?.isDescendant(of: self) == true {
            var restored = false
            if let responder = previousResponder as? NSView, responder.window === window, !responder.isHiddenOrHasHiddenAncestor {
                restored = window.makeFirstResponder(responder)
            }
            if !restored { window.makeFirstResponder(focusReturn) }
        }
        previousResponder = nil
        selection = nil
        removeFromSuperview()
        shadowHost?.removeFromSuperview()
        onVisibilityChange?(false)
        return true
    }

    func contains(windowPoint: NSPoint) -> Bool {
        isShown && bounds.contains(convert(windowPoint, from: nil))
    }

    var frameInWindow: NSRect { convert(bounds, to: nil) }

    func escape() -> Bool {
        guard isShown else { return false }
        if submenuID != nil || page != .topLevel {
            goBack()
        } else {
            dismiss()
        }
        return true
    }

    /// The field editor retains text input and IME composition. Up/Down selects
    /// an enabled row without taking focus away from search.
    func handleKey(_ press: KeyPress) -> Bool {
        guard isShown,
              (search.currentEditor() as? NSTextView)?.hasMarkedText() != true else { return false }
        if press.key == "Tab", press.modifiers == ShortcutModifiers(shift: press.modifiers.shift) {
            moveSelection(by: press.modifiers.shift ? -1 : 1)
            return true
        }
        guard press.modifiers == ShortcutModifiers() else { return false }
        switch press.key {
        case "Escape":
            return press.isRepeat || escape()
        case "Up", "Down":
            moveSelection(by: press.key == "Down" ? 1 : -1)
            return true
        case "Return", "KP_Enter":
            if !press.isRepeat {
                let focused = window?.firstResponder as? CommandPopoverRow
                let row = focused ?? selection ?? selectableRows.first
                row?.performClick(nil)
            }
            return true
        default:
            return false
        }
    }

    func refresh(resetScroll: Bool = false, rebuildSubmenu: Bool = false) {
        guard isShown, let updated = query?(page, search.stringValue),
              model != updated || resetScroll || (rebuildSubmenu && submenuID != nil) else { return }
        model = updated
        rebuildRows(updated.entries)
        reposition()
        if resetScroll { scrollToTop() }
    }

    private func scrollToTop() {
        scroll.contentView.scroll(to: .zero)
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    func controlTextDidChange(_ notification: Notification) { searchChanged() }
    @objc private func searchAction() { searchChanged() }
    @objc private func windowMoved(_ notification: Notification) { reposition() }
    @objc private func windowDismissed(_ notification: Notification) { dismiss() }

    private func searchChanged() {
        guard search.stringValue != lastSearch else { return }
        lastSearch = search.stringValue
        submenuID = nil
        refresh(resetScroll: true)
    }

    private func goBack() {
        if submenuID != nil {
            submenuID = nil
        } else {
            page = .topLevel
            pageTitle = nil
            search.stringValue = ""
            lastSearch = ""
        }
        refresh(resetScroll: true)
        window?.makeFirstResponder(search)
    }

    private func openPage(_ page: CommandMenu, title: String) {
        self.page = page
        pageTitle = title
        search.stringValue = ""
        lastSearch = ""
        refresh(resetScroll: true)
        window?.makeFirstResponder(search)
    }

    private func run(_ row: RegistryCommand) {
        // The session may have changed between displaying and choosing a row.
        guard let current = commands.row(row.id), current.available, current.enabled else {
            refresh()
            return
        }
        if commands.submenuProvider(for: row.id) != nil {
            submenuID = row.id
            refresh(resetScroll: true)
            window?.makeFirstResponder(search)
        } else {
            dismiss()
            commands.run(row.id)
        }
    }

    private func rebuildRows(_ entries: [CommandSurfaceEntry]) {
        for row in rows { row.removeFromSuperview() }
        rows = []
        selection = nil
        if let id = submenuID, let command = commands.row(id), command.available, command.enabled,
           let provider = commands.submenuProvider(for: id) {
            appendBack(command.label)
            for item in provider.submenuItems(for: command) {
                if item.isSeparatorItem {
                    rows.append(PaletteSurface(fill: .borderSubtle))
                } else {
                    appendRow(item.title, checked: item.state == .on, enabled: item.isEnabled) { [weak self] in
                        guard let self, self.commands.row(id)?.enabled == true, let action = item.action else { return }
                        self.dismiss()
                        NSApp.sendAction(action, to: item.target, from: item)
                    }
                }
            }
        } else {
            submenuID = nil
            if let title = pageTitle, search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                appendBack(title)
            }
            for entry in entries {
                switch entry {
                case .command(let row):
                    let hints = commands.keys.hints(for: row)
                    appendRow(row.label, hints: hints, checked: row.checked, enabled: row.available && row.enabled,
                              hasPage: commands.submenuProvider(for: row.id) != nil) { [weak self] in
                        self?.run(row)
                    }
                case .page(let page, let label):
                    appendRow(label, hasPage: true) { [weak self] in self?.openPage(page, title: label) }
                case .group(let label):
                    let heading = NSTextField(labelWithString: label)
                    heading.identifier = groupHeading
                    heading.font = .systemFont(ofSize: 11, weight: .semibold)
                    heading.textColor = PaletteRole.textMuted.color
                    rows.append(heading)
                case .separator:
                    rows.append(PaletteSurface(fill: .borderSubtle))
                }
            }
        }
        if rows.isEmpty {
            let empty = NSTextField(labelWithString: "No matching commands")
            empty.font = .systemFont(ofSize: 13)
            empty.textColor = PaletteRole.textMuted.color
            empty.alignment = .center
            rows.append(empty)
        }
        rows.forEach { list.addSubview($0) }
    }

    private func appendBack(_ title: String) {
        appendRow("‹  " + title, accessibilityLabel: "Back from " + title) { [weak self] in self?.goBack() }
    }

    private func appendRow(_ title: String, hints: [ShortcutChord] = [], checked: Bool = false,
                           enabled: Bool = true, hasPage: Bool = false, accessibilityLabel: String? = nil,
                           action: @escaping () -> Void) {
        rows.append(CommandPopoverRow(title: title, hints: hints, checked: checked, enabled: enabled, hasPage: hasPage,
                                      accessibilityLabel: accessibilityLabel, onClick: action))
    }

    private var selectableRows: [CommandPopoverRow] {
        rows.compactMap { $0 as? CommandPopoverRow }.filter(\.isEnabled)
    }

    private func moveSelection(by delta: Int) {
        let choices = selectableRows
        guard !choices.isEmpty else { return }
        let current = selection.flatMap { selected in choices.firstIndex { $0 === selected } }
        let next = current.map { min(max($0 + delta, 0), choices.count - 1) } ?? (delta > 0 ? 0 : choices.count - 1)
        selection?.keySelected = false
        selection = choices[next]
        selection?.keySelected = true
        list.scrollToVisible(choices[next].frame)
    }

    private func reposition() {
        guard isShown else { return }
        guard let model, let host, let shadowHost, let window = host.window else { dismiss(); return }
        let work = window.screen.map { host.convert(window.convertFromScreen($0.visibleFrame), from: nil) }
        let area = (work.map { host.bounds.intersection($0) } ?? host.bounds).insetBy(dx: 8, dy: 8)
        guard area.width > 0, area.height > 0 else { dismiss(); return }
        let height = rows.reduce(CGFloat(0)) { $0 + rowHeight($1) }
        let size = CGSize(width: min(CGFloat(model.preferredWidth), area.width),
                          height: min(CGFloat(model.maxHeight), area.height, height + 56))
        let placement: CGRect
        if let anchor {
            let x = anchor.x + size.width <= area.maxX ? anchor.x : anchor.x - size.width
            let y = anchor.y - size.height >= area.minY ? anchor.y - size.height : anchor.y
            let origin = clampsToWindow ? CGPoint(x: x, y: y) : anchor
            placement = CGRect(origin: origin, size: size)
        } else {
            placement = CGRect(x: area.midX - size.width / 2, y: area.maxY - size.height - 20,
                               width: size.width, height: size.height)
        }
        shadowHost.frame = clampsToWindow ? placement.fitted(inside: area) : placement
        shadowHost.layer?.shadowPath = CGPath(roundedRect: shadowHost.bounds, cornerWidth: 8, cornerHeight: 8,
                                              transform: nil)
        self.frame = shadowHost.bounds
        search.frame = NSRect(x: 8, y: size.height - 44, width: max(0, size.width - 16), height: 36)
        scroll.frame = NSRect(x: 8, y: 8, width: max(0, size.width - 16), height: max(0, size.height - 56))
        let width = scroll.contentSize.width
        list.frame = NSRect(x: 0, y: 0, width: width, height: max(height, scroll.contentSize.height))
        var y: CGFloat = 0
        for row in rows {
            let height = rowHeight(row)
            row.frame = row is PaletteSurface
                ? NSRect(x: 6, y: y + 5, width: max(0, width - 12), height: 1)
                : NSRect(x: 0, y: y, width: width, height: height)
            if row.identifier == groupHeading {
                row.frame = NSRect(x: 10, y: y + 6, width: max(0, width - 20), height: 16)
            } else if row is NSTextField {
                row.frame.origin.y += 12
            }
            y += height
        }
    }

    private func rowHeight(_ row: NSView) -> CGFloat {
        if row is PaletteSurface { return 11 }
        return row.identifier == groupHeading ? 24 : 36
    }
}

private final class CommandList: NSView {
    override var isFlipped: Bool { true }
}
