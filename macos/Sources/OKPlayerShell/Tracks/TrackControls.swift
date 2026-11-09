import AppKit
import UniformTypeIdentifiers

/// The audio and subtitle surfaces (#882): two OSC buttons with popovers, the
/// subtitle file panel, the delay and size keys with an on-screen readout, and the
/// Audio and Subtitles submenus for the menu bar (`populate(_:kind:)`).
///
/// What the surfaces list and send comes from `TrackMenus`, over the session's
/// `tracks` view; selecting, adding and stepping are session commands. Nothing here
/// keeps track state of its own.
final class TrackControls: NSObject, NSPopoverDelegate {
    /// The registry commands this shell carries out for the track surfaces.
    static let shellCommands = [TrackMenus.addSubtitle]

    /// Sends a command to the session.
    var dispatch: ((Command) -> DispatchResult)?
    /// The session's `tracks` view.
    var query: (() -> TracksView?)?
    /// Holds the transport controls up (true) or lets them hide again (false)
    /// while a popover is open.
    var holdControls: ((Bool) -> Void)?
    /// The window the file panel is a sheet of.
    weak var window: NSWindow?

    let audioButton = TransportOverlay.iconButton("waveform", "Audio")
    let subtitleButton = TransportOverlay.iconButton("captions.bubble", "Subtitles")
    let readout = TrackReadoutView()

    private var keys = TrackKeys(table: [])
    private let popover = NSPopover()
    private let content = TrackPopoverController()
    private var popoverKind: TrackKind?
    /// The registry command whose readout is showing.
    private var readoutCommand: String?
    private var smokeSubtitle: URL?

    override init() {
        super.init()
        popover.behavior = .transient
        popover.contentViewController = content
        popover.delegate = self
        content.onAction = { [weak self] action, closes in
            self?.choose(action)
            if closes { self?.popover.performClose(nil) } else { self?.refresh() }
        }
        audioButton.target = self
        audioButton.action = #selector(showAudio)
        subtitleButton.target = self
        subtitleButton.action = #selector(showSubtitles)
        for button in [audioButton, subtitleButton] { button.isEnabled = false }
    }

    /// Put the two buttons at the front of the transport controls' trailing group,
    /// and the readout over the top of `root`.
    func install(in osc: TransportOverlay, over root: NSView) {
        osc.trailingControls.insertArrangedSubview(subtitleButton, at: 0)
        osc.trailingControls.insertArrangedSubview(audioButton, at: 0)
        readout.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(readout)
        NSLayoutConstraint.activate([
            readout.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            readout.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 24),
        ])
    }

    /// The session's shortcut table, for the keys handled here.
    func setShortcuts(_ table: [ShortcutRow]) { keys = TrackKeys(table: table) }

    /// The session reported a batch.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        let loaded = snapshot.status == .playing || snapshot.status == .paused
        for button in [audioButton, subtitleButton] { button.isEnabled = loaded }
        if popover.isShown || readoutCommand != nil { refresh() }
    }

    // MARK: Commands

    /// A key the window's own commands did not take. True when it was one of
    /// these surfaces' keys.
    func handleKey(_ event: NSEvent) -> Bool {
        guard let command = keys.command(characters: event.charactersIgnoringModifiers,
                                         modifiers: event.modifierFlags.intersection(.deviceIndependentFlagsMask))
        else { return false }
        // A held S would open the file panel again and again; held steps repeat.
        if !(event.isARepeat && command == TrackMenus.addSubtitle) { run(command) }
        return true
    }

    /// Invoke the registry command `id`: the session runs the delay and size steps,
    /// and answers `perform` for the file panel.
    @discardableResult
    func run(_ id: String) -> DispatchResult? {
        guard let result = dispatch?(.invokeCommand(id: id)) else { return nil }
        if case .perform = result { _ = carryOut(id) }
        invoked(id, result)
        return result
    }

    /// Carry out a registry command the session answered with `perform`; false for
    /// one these surfaces do not own.
    func carryOut(_ id: String) -> Bool {
        guard id == TrackMenus.addSubtitle else { return false }
        chooseSubtitleFile()
        return true
    }

    /// The registry command `id` was invoked with `result`: show its readout.
    func invoked(_ id: String, _ result: DispatchResult) {
        guard case .accepted = result, let view = query?(), let text = TrackMenus.readout(after: id, in: view) else {
            return
        }
        readoutCommand = id
        readout.show(text) { [weak self] in self?.readoutCommand = nil }
        if popover.isShown { content.update(kind: popoverKind ?? .subtitle, view: view) }
    }

    /// Do what a menu or popover item stands for.
    func choose(_ action: TrackMenuAction) {
        switch action {
        case .addSubtitle: chooseSubtitleFile()
        case .invoke(let id): run(id)
        case .select: if let command = TrackMenus.command(for: action) { _ = dispatch?(command) }
        }
    }

    /// Load `url` as a subtitle for the playing media.
    @discardableResult
    func addSubtitle(_ url: URL) -> DispatchResult? {
        dispatch?(.addSubtitle(path: url.path))
    }

    private func chooseSubtitleFile() {
        guard let window else { return }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = SubtitleFiles.extensions.compactMap { UTType(filenameExtension: $0) }
        panel.message = "Add a subtitle file to the playing media"
        panel.prompt = "Add"
        panel.beginSheetModal(for: window) { [weak self] response in
            if response == .OK, let url = panel.url { self?.addSubtitle(url) }
        }
    }

    // MARK: Popovers and menus

    @objc private func showAudio() { show(.audio, from: audioButton) }
    @objc private func showSubtitles() { show(.subtitle, from: subtitleButton) }

    private func show(_ kind: TrackKind, from button: NSButton) {
        // A button closes whichever popover is open; the next click opens its own.
        guard !popover.isShown else { popover.performClose(nil); return }
        popoverKind = kind
        content.update(kind: kind, view: query?())
        holdControls?(true)
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .maxY)
    }

    private func refresh() {
        guard let view = query?() else { return }
        if popover.isShown, let kind = popoverKind { content.update(kind: kind, view: view) }
        if let id = readoutCommand, let text = TrackMenus.readout(after: id, in: view) { readout.update(text) }
    }

    func popoverDidClose(_ notification: Notification) {
        popoverKind = nil
        holdControls?(false)
    }

    /// Fill `menu` with the Audio or Subtitles items for the menu bar; call it from
    /// the menu's `menuNeedsUpdate(_:)`.
    func populate(_ menu: NSMenu, kind: TrackKind) {
        menu.removeAllItems()
        // The items' own enabled state, not whether the target answers the action.
        menu.autoenablesItems = false
        for item in TrackMenus.items(kind, query?()) {
            guard item.kind != .separator else { menu.addItem(.separator()); continue }
            let entry = NSMenuItem(title: item.title, action: item.action == nil ? nil : #selector(chooseMenuItem(_:)),
                                   keyEquivalent: "")
            entry.target = self
            entry.state = item.checked ? .on : .off
            entry.isEnabled = item.enabled
            entry.representedObject = item.action.map { TrackActionBox($0) }
            menu.addItem(entry)
        }
    }

    @objc private func chooseMenuItem(_ item: NSMenuItem) {
        guard let box = item.representedObject as? TrackActionBox else { return }
        choose(box.action)
    }
}

/// Carries a `TrackMenuAction` on an `NSMenuItem` or a control.
final class TrackActionBox: NSObject {
    let action: TrackMenuAction
    init(_ action: TrackMenuAction) { self.action = action }
}

// MARK: - Views

/// The popover content: the track choices, and for subtitles Add Subtitle File…
/// and the delay and size rows. Choosing a track closes the popover; the steps
/// keep it open.
final class TrackPopoverController: NSViewController {
    /// An action was chosen; `closes` when the popover should close after it.
    var onAction: ((TrackMenuAction, _ closes: Bool) -> Void)?
    private let stack = NSStackView()

    override func loadView() {
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        stack.edgeInsets = NSEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)
        view = stack
    }

    func update(kind: TrackKind, view tracks: TracksView?) {
        loadViewIfNeeded()
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        boxes = [:]
        let title = NSTextField(labelWithString: kind == .audio ? "Audio" : "Subtitles")
        title.font = .boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        title.textColor = .secondaryLabelColor
        stack.addArrangedSubview(title)
        for item in TrackMenus.items(kind, tracks) { stack.addArrangedSubview(control(for: item)) }
        guard kind == .subtitle else { return }
        stack.addArrangedSubview(separator())
        for adjustment in TrackMenus.adjustments(tracks) { stack.addArrangedSubview(row(for: adjustment)) }
    }

    private func control(for item: TrackMenuItem) -> NSView {
        switch item.kind {
        case .separator:
            return separator()
        case .placeholder:
            let label = NSTextField(labelWithString: item.title)
            label.textColor = .secondaryLabelColor
            return label
        case .choice:
            let button = NSButton(radioButtonWithTitle: item.title, target: self, action: #selector(choose(_:)))
            button.state = item.checked ? .on : .off
            button.isEnabled = item.enabled
            attach(item.action, to: button)
            return button
        case .command:
            let button = NSButton(title: item.title, target: self, action: #selector(choose(_:)))
            button.bezelStyle = .inline
            button.isEnabled = item.enabled
            attach(item.action, to: button)
            return button
        }
    }

    private func row(for adjustment: SubtitleAdjustment) -> NSView {
        let title = NSTextField(labelWithString: adjustment.title)
        let value = NSTextField(labelWithString: adjustment.value)
        value.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        value.alignment = .right
        value.widthAnchor.constraint(greaterThanOrEqualToConstant: 64).isActive = true
        let decrease = step("minus", adjustment.decreaseHelp, adjustment.decrease, adjustment.enabled)
        let increase = step("plus", adjustment.increaseHelp, adjustment.increase, adjustment.enabled)
        let row = NSStackView(views: [title, value, decrease, increase])
        row.orientation = .horizontal
        row.spacing = 6
        title.widthAnchor.constraint(greaterThanOrEqualToConstant: 44).isActive = true
        return row
    }

    private func step(_ symbol: String, _ help: String, _ action: TrackMenuAction, _ enabled: Bool) -> NSButton {
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: help) ?? NSImage()
        let button = NSButton(image: image, target: self, action: #selector(adjust(_:)))
        button.bezelStyle = .inline
        button.toolTip = help
        button.isEnabled = enabled
        attach(action, to: button)
        return button
    }

    private func separator() -> NSView {
        let line = NSBox()
        line.boxType = .separator
        line.widthAnchor.constraint(greaterThanOrEqualToConstant: 220).isActive = true
        return line
    }

    private func attach(_ action: TrackMenuAction?, to button: NSButton) {
        guard let action else { return }
        button.identifier = NSUserInterfaceItemIdentifier(UUID().uuidString)
        boxes[button.identifier!] = TrackActionBox(action)
    }

    private var boxes: [NSUserInterfaceItemIdentifier: TrackActionBox] = [:]

    @objc private func choose(_ sender: NSButton) {
        guard let id = sender.identifier, let box = boxes[id] else { return }
        // Add Subtitle File… opens a sheet; the popover gets out of its way too.
        onAction?(box.action, true)
    }

    @objc private func adjust(_ sender: NSButton) {
        guard let id = sender.identifier, let box = boxes[id] else { return }
        onAction?(box.action, false)
    }
}

/// The on-screen readout of the delay and size keys: the session's value for a
/// moment over the top of the video. It takes no clicks.
final class TrackReadoutView: NSVisualEffectView {
    private let label = NSTextField(labelWithString: "")
    private var hideWork: DispatchWorkItem?

    init() {
        super.init(frame: .zero)
        material = .hudWindow
        blendingMode = .withinWindow
        state = .active
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.masksToBounds = true
        label.font = .monospacedDigitSystemFont(ofSize: 15, weight: .semibold)
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -14),
            label.topAnchor.constraint(equalTo: topAnchor, constant: 7),
            label.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -7),
        ])
        isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    /// The text showing, empty while hidden.
    var text: String { isHidden ? "" : label.stringValue }

    /// Show `text` for `seconds`; `hidden` runs once it has gone.
    func show(_ text: String, for seconds: TimeInterval = 1.5, hidden: @escaping () -> Void = {}) {
        label.stringValue = text
        isHidden = false
        hideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.isHidden = true
            hidden()
        }
        hideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: work)
    }

    /// Replace the text while it shows, without extending the time.
    func update(_ text: String) {
        guard !isHidden else { return }
        label.stringValue = text
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}

// MARK: - Smoke

extension TrackControls {
    /// Smoke, while media plays: `]` pressed on `window` steps the subtitle size and
    /// shows the session's readout, and a generated SRT file is added. The message
    /// says what failed.
    func smokeStart(window: NSWindow) -> String? {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil, characters: "]",
                                           charactersIgnoringModifiers: "]", isARepeat: false, keyCode: 30)
        else { return "Could not make a key event for ]" }
        window.sendEvent(event)
        guard readout.text == "Subtitle size 110%" else {
            return "] did not show the subtitle size readout (\(readout.text.isEmpty ? "nothing" : readout.text))"
        }
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("okp-smoke-\(ProcessInfo.processInfo.processIdentifier).srt")
        do {
            try "1\n00:00:00,000 --> 00:01:00,000\nOK Player smoke\n\n".write(to: file, atomically: true, encoding: .utf8)
        } catch {
            return "Could not write a subtitle file: \(error)"
        }
        smokeSubtitle = file
        guard case .accepted? = addSubtitle(file) else { return "The session refused the subtitle file" }
        return nil
    }

    /// Smoke, seconds later: the added file is the selected subtitle with the
    /// session's external-file label, the audio track is listed, and the size holds.
    func smokeFinish() -> String? {
        defer { if let smokeSubtitle { try? FileManager.default.removeItem(at: smokeSubtitle) } }
        guard let view = query?() else { return "The session answered no tracks view" }
        guard !view.audio.isEmpty else { return "No audio track is listed" }
        guard let added = view.subtitles.first(where: { $0.selected && $0.id != nil }), added.label.hasSuffix("EXT") else {
            return "The added subtitle is not the selected external track: \(view.subtitles.map(\.label))"
        }
        guard view.subtitleScale.label == "110%" else { return "The subtitle size is \(view.subtitleScale.label)" }
        return nil
    }

    /// The smoke line's part for these surfaces.
    var smokeSummary: String {
        let view = query?()
        return "tracks_audio=\(view?.audio.count ?? 0) subtitles_listed=\((view?.subtitles.count ?? 1) - 1) "
            + "subtitle_added=external subtitle_size=\(view?.subtitleScale.label ?? "?")"
    }
}
