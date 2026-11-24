import AppKit
import UniformTypeIdentifiers

/// The audio and subtitle surfaces (#882): two OSC buttons with popovers, the
/// subtitle file panel, the on-screen readout of the delay and size steps, and the
/// menu bar's Audio and Subtitles submenus (`register(with:)`). The keys (S, Z,
/// Shift+Z, [ and ]) are the shortcut table's, run by `PlayerCommands` like every
/// other key; `commandRan(_:)` shows their readout.
///
/// What the surfaces list and send comes from `TrackMenus`, over the session's
/// `tracks` view; selecting, adding and stepping are session commands. Nothing here
/// keeps track state of its own.
final class TrackControls: NSObject {
    /// The registry commands this shell carries out for the track surfaces.
    static let shellCommands = [TrackMenus.addSubtitle]
    /// The readout when an added subtitle file was refused or libmpv could not load it.
    static let addFailed = "Could not load the subtitle file"
    /// The readout when the session reports an error for a track choice or step.
    static let changeFailed = "Could not change the subtitles or audio"
    /// How long a failure stays on screen.
    static let failureSeconds: TimeInterval = 3

    /// Sends a command to the session.
    var dispatch: ((Command) -> DispatchResult)?
    /// The session's `tracks` view.
    var query: (() -> TracksView?)?
    /// Holds the transport controls up (true) or lets them hide again (false)
    /// while a popover is open.
    var holdControls: ((Bool) -> Void)?
    /// The player window owns the track panel and the Add Subtitle sheet.
    weak var window: NSWindow?
    /// Shows the file panel as a sheet of the window and hands over the chosen file.
    var presentFilePanel: (NSWindow, @escaping (URL) -> Void) -> Void = TrackControls.openSubtitlePanel
    /// Shows a readout as a toast (the app's status layer); without it, readouts
    /// show in `readout`.
    var toast: ((String) -> Void)?

    let audioButton = TransportOverlay.iconButton("waveform", "Audio")
    let subtitleButton = TransportOverlay.iconButton("captions.bubble", "Subtitles")
    let readout = TrackReadoutView()

    let panel = TrackPanel()
    private(set) var panelKind: TrackKind?
    private var panelAnchor: NSLayoutConstraint?
    private weak var osc: TransportOverlay?
    /// The registry command whose readout is showing.
    private var readoutCommand: String?
    /// The latest requests these surfaces sent, with the readout for an error the
    /// session reports against one of them.
    private var requests: [(id: UInt64, failure: String)] = []
    /// Errors the readout showed instead of the app (smoke evidence).
    private(set) var failuresShown = 0
    /// The latest readout shown, as a toast or in `readout` (smoke evidence).
    private(set) var lastShown: String?
    private var smokeSubtitle: URL?

    override init() {
        super.init()
        panel.onAction = { [weak self] action in
            if case .select = action { self?.dismissPopover() }
            self?.choose(action)
        }
        audioButton.target = self
        audioButton.action = #selector(showAudio)
        subtitleButton.target = self
        subtitleButton.action = #selector(showSubtitles)
        for button in [audioButton, subtitleButton] {
            button.isEnabled = false
            // With Keyboard navigation on, a clicked button would keep keyboard
            // focus, and Space would click it again instead of pausing.
            button.refusesFirstResponder = true
        }
    }

    /// Put the two track buttons next to the other trailing transport controls,
    /// and the readout over the top of `root`.
    func install(in osc: TransportOverlay, over root: NSView) {
        self.osc = osc
        osc.addTrailingControl(subtitleButton, id: "subtitles") { [weak self] in
            Self.trackMenuItem("Subtitles", items: self?.menuItems(.subtitle) ?? [])
        }
        osc.addTrailingControl(audioButton, id: "audio") { [weak self] in
            Self.trackMenuItem("Audio", items: self?.menuItems(.audio) ?? [])
        }
        panel.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(panel)
        NSLayoutConstraint.activate([
            panel.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 8),
            panel.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -8),
            panel.topAnchor.constraint(greaterThanOrEqualTo: osc.topAnchor, constant: 8),
            panel.bottomAnchor.constraint(equalTo: osc.panel.topAnchor, constant: -4),
        ])
        readout.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(readout)
        NSLayoutConstraint.activate([
            readout.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            readout.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 24),
        ])
    }

    /// The session reported a batch.
    func sessionChanged(_ snapshot: SessionSnapshot) {
        let loaded = snapshot.status == .playing || snapshot.status == .paused
        for button in [audioButton, subtitleButton] { button.isEnabled = loaded }
        if !loaded { dismissPopover() }
        if isPopoverShown || readoutCommand != nil { refresh() }
    }

    // MARK: Commands

    /// The menu bar's Audio and Subtitles submenus, the file panel as the shell's
    /// `add-subtitle` command: register them with `commands` before the session
    /// starts, so the session config lists them as this shell's commands.
    func register(with commands: PlayerCommands) {
        commands.register(TrackMenus.addSubtitle) { [weak self] in _ = self?.carryOut(TrackMenus.addSubtitle) }
        commands.register(TrackSubmenu(controls: self, kind: .audio))
        commands.register(TrackSubmenu(controls: self, kind: .subtitle))
    }

    /// A key or a menu item ran a registry command (`PlayerCommands.onRun`): a
    /// delay or size step shows its readout, and an error against it its failure.
    func commandRan(_ invocation: PlayerCommands.Invocation) {
        if case .ran(let result) = invocation.outcome { invoked(invocation.id, result) }
    }

    /// Invoke the registry command `id` from the popover: the session runs the
    /// delay and size steps, and answers `perform` for the file panel.
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

    /// The registry command `id` was invoked with `result`. For a delay or size
    /// step, an error the session reports against the request reads as a failed
    /// change, and an accepted one shows its readout. Any other command is not
    /// these surfaces'.
    func invoked(_ id: String, _ result: DispatchResult) {
        guard TrackMenus.steps.contains(id) else { return }
        remember(result, failure: Self.changeFailed)
        guard case .accepted = result, let view = query?(), let text = TrackMenus.readout(after: id, in: view) else {
            return
        }
        present(text, following: id)
        if let panelKind { panel.update(kind: panelKind, tracks: view) }
    }

    /// Do what a menu or popover item stands for.
    func choose(_ action: TrackMenuAction) {
        switch action {
        case .addSubtitle: chooseSubtitleFile()
        case .invoke(let id): run(id)
        case .resetDelay, .resetSize:
            guard let command = TrackMenus.command(for: action) else { return }
            send(command, failure: Self.changeFailed)
            refresh()
        case .select: if let command = TrackMenus.command(for: action) { send(command, failure: Self.changeFailed) }
        }
    }

    /// Load `url` as a subtitle for the playing media. A refusal shows in the
    /// readout; so does libmpv failing to read the file later (`claimError`).
    @discardableResult
    func addSubtitle(_ url: URL) -> DispatchResult? {
        let result = send(.addSubtitle(path: url.path), failure: Self.addFailed)
        if case .rejected? = result { showFailure(Self.addFailed) }
        return result
    }

    /// The session reported an error for request `requestId`. True when it
    /// answers one of these surfaces' requests: the readout says what failed and
    /// playback goes on, so the error is not the app's to show.
    func claimError(requestId: UInt64?) -> Bool {
        guard let requestId, let index = requests.firstIndex(where: { $0.id == requestId }) else { return false }
        showFailure(requests.remove(at: index).failure)
        return true
    }

    /// Send `command`, remembering the request so an error against it reads `failure`.
    @discardableResult
    private func send(_ command: Command, failure: String) -> DispatchResult? {
        guard let result = dispatch?(command) else { return nil }
        remember(result, failure: failure)
        return result
    }

    /// Remember the request `result` answers, so an error against it reads `failure`.
    private func remember(_ result: DispatchResult, failure: String) {
        switch result {
        case .accepted(let id), .failed(let id, _):
            requests.append((id, failure))
            // A request that succeeded is never answered again; keep the latest few.
            if requests.count > 32 { requests.removeFirst(requests.count - 32) }
        default:
            break
        }
    }

    private func showFailure(_ text: String) {
        failuresShown += 1
        present(text, following: nil, for: Self.failureSeconds)
    }

    /// Show `text` as a toast, or in `readout` for `seconds`, where it follows the
    /// step `command` names as the session reports its value.
    private func present(_ text: String, following command: String?, for seconds: TimeInterval = 1.5) {
        lastShown = text
        readoutCommand = nil
        if let toast {
            // The toast keeps the value the session read when the step ran.
            toast(text)
            return
        }
        readoutCommand = command
        readout.show(text, for: seconds) { [weak self] in self?.readoutCommand = nil }
    }

    private func chooseSubtitleFile() {
        // As the menu item and the popover row, which are disabled then: no media
        // to add the file to.
        guard query?()?.adjustable == true, let window else { return }
        dismissPopover()
        presentFilePanel(window) { [weak self] url in self?.addSubtitle(url) }
    }

    /// The file panel, limited to subtitle files.
    static func openSubtitlePanel(on window: NSWindow, chosen: @escaping (URL) -> Void) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = SubtitleFiles.extensions.compactMap { UTType(filenameExtension: $0) }
        panel.message = "Add a subtitle file to the playing media"
        panel.prompt = "Add"
        panel.beginSheetModal(for: window) { response in
            if response == .OK, let url = panel.url { chosen(url) }
        }
    }

    // MARK: Popovers and menus

    @objc private func showAudio() { show(.audio, from: audioButton) }
    @objc private func showSubtitles() { show(.subtitle, from: subtitleButton) }

    private func show(_ kind: TrackKind, from button: NSButton) {
        guard let osc, let window, panel.window === window, !button.isHidden else { return }
        if panelKind == kind { dismissPopover(); return }
        osc.dismissVolume()
        osc.noteActivity()
        window.makeFirstResponder(osc.focusReturn)
        panelKind = kind
        panel.update(kind: kind, tracks: query?())
        panelAnchor?.isActive = false
        let anchor = panel.centerXAnchor.constraint(equalTo: button.centerXAnchor)
        anchor.priority = .defaultHigh
        anchor.isActive = true
        panelAnchor = anchor
        panel.isHidden = false
        holdControls?(true)
    }

    private func refresh() {
        guard let view = query?() else { return }
        if let panelKind { panel.update(kind: panelKind, tracks: view) }
        if let id = readoutCommand, let text = TrackMenus.readout(after: id, in: view) { readout.update(text) }
    }

    /// A click outside dismisses the panel and continues to its original target.
    /// Escape consumes the event so the queue and fullscreen stay in place.
    func handleWindowInput(_ event: NSEvent) -> Bool {
        guard isPopoverShown else { return false }
        switch event.type {
        case .keyDown:
            return event.keyCode == WindowKeys.escapeKeyCode && dismissPopover()
        case .leftMouseDown, .rightMouseDown, .otherMouseDown:
            let point = event.locationInWindow
            let surfaces = [panel, audioButton, subtitleButton] as [NSView]
            if !surfaces.contains(where: {
                !$0.isHiddenOrHasHiddenAncestor && $0.bounds.contains($0.convert(point, from: nil))
            }) {
                dismissPopover()
            }
        default:
            break
        }
        return false
    }

    private static func trackMenuItem(_ title: String, items: [NSMenuItem]) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        let menu = NSMenu()
        menu.autoenablesItems = false
        items.forEach { menu.addItem($0) }
        item.submenu = menu
        item.isEnabled = items.contains { !$0.isSeparatorItem && $0.isEnabled }
        return item
    }

    var isPopoverShown: Bool { panelKind != nil }

    @discardableResult
    func dismissPopover() -> Bool {
        guard panelKind != nil else { return false }
        panelKind = nil
        panel.isHidden = true
        panelAnchor?.isActive = false
        panelAnchor = nil
        holdControls?(false)
        return true
    }

    /// The Audio or Subtitles items for the menu bar, for the session's state now.
    /// They carry their own enabled state: the menu bar's provider submenus do not
    /// auto-enable their items.
    func menuItems(_ kind: TrackKind) -> [NSMenuItem] {
        TrackMenus.items(kind, query?()).map { (item: TrackMenuItem) -> NSMenuItem in
            guard item.kind != .separator else { return NSMenuItem.separator() }
            let entry = NSMenuItem(title: item.title, action: item.action == nil ? nil : #selector(chooseMenuItem(_:)),
                                   keyEquivalent: "")
            entry.target = self
            entry.state = item.checked ? .on : .off
            entry.isEnabled = item.enabled
            entry.representedObject = item.action.map { TrackActionBox($0) }
            return entry
        }
    }

    @objc private func chooseMenuItem(_ item: NSMenuItem) {
        guard let box = item.representedObject as? TrackActionBox else { return }
        choose(box.action)
    }
}

/// The menu bar's Audio or Subtitles submenu: the registry's `audio-track` or
/// `subtitles` row, filled with the track items each time it opens.
final class TrackSubmenu: DynamicSubmenuProvider {
    let kind: TrackKind
    private weak var controls: TrackControls?

    init(controls: TrackControls, kind: TrackKind) {
        self.controls = controls
        self.kind = kind
    }

    var commandID: String { kind == .audio ? TrackMenus.audioCommand : TrackMenus.subtitlesCommand }

    /// The items enable themselves from the `tracks` view: nothing can be chosen
    /// while no media is loaded.
    func submenuItems(for command: RegistryCommand) -> [NSMenuItem] {
        controls?.menuItems(kind) ?? []
    }
}

/// Carries a `TrackMenuAction` on an `NSMenuItem` or a control.
final class TrackActionBox: NSObject {
    let action: TrackMenuAction
    init(_ action: TrackMenuAction) { self.action = action }
}

// MARK: - Views

/// The on-screen readout of the delay and size keys: the session's value for a
/// moment over the top of the video. It takes no clicks.
final class TrackReadoutView: PalettePanel {
    private let label = NSTextField(labelWithString: "")
    private var hideWork: DispatchWorkItem?

    init() {
        super.init(fill: PaletteRole.surfaceRaised.color, radius: 8)
        label.textColor = PaletteRole.text.color
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
    /// shows the session's readout. A subtitle file that does not exist is added,
    /// which libmpv reports as an error against that request, and then a generated
    /// SRT file. The message says what failed.
    func smokeStart(window: NSWindow) -> String? {
        guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                           timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: window.windowNumber, context: nil, characters: "]",
                                           charactersIgnoringModifiers: "]", isARepeat: false, keyCode: 30)
        else { return "Could not make a key event for ]" }
        window.sendEvent(event)
        guard lastShown == "Subtitle size: 110%" else {
            return "] did not show the subtitle size readout (\(lastShown ?? "nothing"))"
        }
        let directory = FileManager.default.temporaryDirectory
        let stem = "okp-smoke-\(ProcessInfo.processInfo.processIdentifier)"
        let missing = directory.appendingPathComponent("\(stem)-missing.srt")
        let file = directory.appendingPathComponent("\(stem).srt")
        do {
            try "1\n00:00:00,000 --> 00:01:00,000\nOK Player smoke\n\n".write(to: file, atomically: true, encoding: .utf8)
        } catch {
            return "Could not write a subtitle file: \(error)"
        }
        smokeSubtitle = file
        guard case .accepted? = addSubtitle(missing) else { return "The session refused the missing subtitle file" }
        guard case .accepted? = addSubtitle(file) else { return "The session refused the subtitle file" }
        return nil
    }

    /// Smoke, seconds later: the missing file's error showed in the readout and
    /// playback went on; the added file is the selected subtitle with the session's
    /// external-file label; the audio track is listed; and libmpv reports the size.
    func smokeFinish() -> String? {
        defer { if let smokeSubtitle { try? FileManager.default.removeItem(at: smokeSubtitle) } }
        guard failuresShown >= 1 else { return "The missing subtitle file's error did not reach the readout" }
        guard let view = query?() else { return "The session answered no tracks view" }
        guard !view.audio.isEmpty else { return "No audio track is listed" }
        guard let added = view.subtitles.first(where: { $0.selected && $0.id != nil }), added.label.hasSuffix("External") else {
            return "The added subtitle is not the selected external track: \(view.subtitles.map(\.label))"
        }
        guard view.subtitleScale.label == "110%", let reported = view.subtitleScale.reported, abs(reported - 1.1) < 0.001
        else {
            return "libmpv reports the subtitle size as \(view.subtitleScale.reported.map { "\($0)" } ?? "nothing") "
                + "(\(view.subtitleScale.label) asked)"
        }
        return nil
    }

    /// The smoke line's part for these surfaces.
    var smokeSummary: String {
        let view = query?()
        let reported = view?.subtitleScale.reported.map { "\(Int(($0 * 100).rounded()))%" } ?? "?"
        return "tracks_audio=\(view?.audio.count ?? 0) subtitles_listed=\((view?.subtitles.count ?? 1) - 1) "
            + "subtitle_added=external subtitle_add_error=readout subtitle_size_reported=\(reported)"
    }
}
