import AppKit

/// The action every registry item of the menu bar sends through the responder
/// chain. The app delegate implements it and validates the items.
@objc protocol PlayerCommandResponder {
    func performPlayerCommand(_ sender: Any?)
}

/// Supplies the items of a registry command the menu bar shows as a submenu:
/// the playback speeds (`PlaybackSpeedMenu`), and the audio and subtitle track
/// lists. Register it with `PlayerCommands.register(_:)` before the session
/// starts; that makes the command one this shell carries out, so the registry
/// reports it available.
protocol DynamicSubmenuProvider: AnyObject {
    /// The registry command whose row the submenu takes (`audio-track`, `subtitles`).
    var commandID: String { get }
    /// The submenu's items for the state now, asked each time it opens. `command`
    /// is the row as the registry resolves it at that moment. The items carry
    /// their own target, action and enabled state.
    func submenuItems(for command: RegistryCommand) -> [NSMenuItem]
}

extension NSMenuItem {
    /// A menu bar item for a registry command: it sends `performPlayerCommand(_:)`
    /// through the responder chain, and validation fills in its title, check mark
    /// and enabled state from the registry.
    static func playerCommand(_ id: String, title: String, keyEquivalent: MenuKeyEquivalent?) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(PlayerCommandResponder.performPlayerCommand(_:)),
                              keyEquivalent: keyEquivalent?.key ?? "")
        if let keyEquivalent { item.keyEquivalentModifierMask = keyEquivalent.modifiers }
        item.representedObject = id
        return item
    }

    /// The registry command this item runs, or nil for any other item.
    var playerCommandID: String? {
        action == #selector(PlayerCommandResponder.performPlayerCommand(_:)) ? representedObject as? String : nil
    }
}

/// Carries out registry commands (#876) for the menu bar and the player window's
/// keys. The session runs its own commands (`invoke_command`); the ones it hands
/// back (`perform`) run the handler this shell registered. Each runs only while
/// the registry says it is available and enabled, and the registry is asked
/// again after every session batch, every command and every menu opening.
final class PlayerCommands {
    enum Outcome: Equatable {
        /// Not in the registry, or neither the session nor this shell runs it.
        case unavailable
        /// The registry says it does not apply now.
        case disabled
        /// The session carried it out, or refused it, with this answer.
        case ran(DispatchResult)
        /// This shell carried it out.
        case performed
        /// An auto-repeat of a held key whose command runs once per press.
        case heldRepeat
    }

    struct Invocation: Equatable {
        var id: String
        var outcome: Outcome
    }

    /// Whether the player window is in fullscreen; the window commands are
    /// resolved against it.
    var isFullscreen: () -> Bool = { false }
    /// The shortcut table the window keys go through.
    private(set) var keys = ShortcutKeyMap()
    /// The latest command a menu item or a key ran, and what came of it.
    private(set) var lastInvocation: Invocation?
    /// How often this shell carried out each command.
    private(set) var performedCounts: [String: Int] = [:]
    /// Called after a menu item or a key ran a command, through the session or
    /// this shell (not for one that ran nothing, nor for a held toggle's taken
    /// auto-repeat). The app counts it as activity for the transport controls.
    var onRun: ((Invocation) -> Void)?

    private var handlers: [String: () -> Void] = [:]
    private var providers: [String: DynamicSubmenuProvider] = [:]
    private var session: ((Command) -> DispatchResult)?
    private var registryQuery: ((Bool) -> [RegistryCommand])?
    private var cached: (fullscreen: Bool, rows: [RegistryCommand])?

    /// The commands this shell carries out, for the session config's `shell_commands`.
    var shellCommands: [String] { Set(handlers.keys).union(providers.keys).sorted() }

    /// The commands a provider shows as submenus.
    var submenuCommands: Set<String> { Set(providers.keys) }

    /// Carry out `id` with `handler` when the session hands it back.
    func register(_ id: String, handler: @escaping () -> Void) {
        handlers[id] = handler
    }

    /// Show `provider`'s command as a submenu it fills.
    func register(_ provider: DynamicSubmenuProvider) {
        providers[provider.commandID] = provider
    }

    func submenuProvider(for id: String) -> DynamicSubmenuProvider? { providers[id] }

    /// Use a session: `session` dispatches a command, `registry` answers the
    /// `command_registry` query for a fullscreen state, and `keys` is the
    /// session's shortcut table.
    func connect(session: @escaping (Command) -> DispatchResult,
                 registry: @escaping (_ fullscreen: Bool) -> [RegistryCommand], keys: ShortcutKeyMap) {
        self.session = session
        registryQuery = registry
        self.keys = keys
        invalidate()
    }

    /// Ask the session again next time. Call it whenever the session reports a batch.
    func invalidate() { cached = nil }

    /// Every registry command as the session resolves it now.
    var registry: [RegistryCommand] {
        let fullscreen = isFullscreen()
        if let cached, cached.fullscreen == fullscreen { return cached.rows }
        let rows = registryQuery?(fullscreen) ?? []
        cached = (fullscreen, rows)
        return rows
    }

    func row(_ id: String) -> RegistryCommand? { registry.first { $0.id == id } }

    /// Run `id` if the registry says it can run now.
    @discardableResult
    func run(_ id: String) -> Outcome {
        let outcome = carryOut(id)
        let invocation = Invocation(id: id, outcome: outcome)
        lastInvocation = invocation
        switch outcome {
        case .ran, .performed: onRun?(invocation)
        case .unavailable, .disabled, .heldRepeat: break
        }
        return outcome
    }

    /// Handle a key press in the player window; true when the press was taken.
    /// A key that runs nothing now (unbound, unavailable or disabled) is left to
    /// the rest of the window, so Escape outside fullscreen still cancels.
    func handle(_ press: KeyPress) -> Bool {
        switch keys.match(press) {
        case nil:
            return false
        case .heldRepeat(let id):
            guard let row = self.row(id), row.available, row.enabled else { return false }
            lastInvocation = Invocation(id: id, outcome: .heldRepeat)
            return true
        case .run(let id):
            switch run(id) {
            case .unavailable, .disabled: return false
            case .ran, .performed, .heldRepeat: return true
            }
        }
    }

    func handle(_ event: NSEvent) -> Bool {
        guard let press = KeyPress(event: event) else { return false }
        return handle(press)
    }

    /// Validate a menu item: a registry item takes the registry's label and check
    /// mark, and is enabled while its command is available and enabled. Any other
    /// item is left alone.
    func validate(_ item: NSMenuItem) -> Bool {
        guard let id = item.playerCommandID else { return true }
        guard let row = self.row(id) else { return false }
        if item.title != row.label { item.title = row.label }
        item.state = row.checked ? .on : .off
        return row.available && row.enabled
    }

    /// The menu item `sender` ran.
    func perform(_ sender: Any?) {
        guard let id = (sender as? NSMenuItem)?.playerCommandID else { return }
        run(id)
    }

    private func carryOut(_ id: String) -> Outcome {
        guard let row = self.row(id), row.available, let session = self.session else { return .unavailable }
        guard row.enabled else { return .disabled }
        let reply = session(.invokeCommand(id: id))
        // The command may change what the registry resolves.
        invalidate()
        guard reply == .perform else { return .ran(reply) }
        guard let handler = handlers[id] else { return .unavailable }
        handler()
        performedCounts[id, default: 0] += 1
        return .performed
    }
}

/// The Playback menu's speed submenu: the speeds the session's `transport` query
/// offers, the current one checked.
final class PlaybackSpeedMenu: NSObject, DynamicSubmenuProvider {
    let commandID = "playback-speed"
    private let transport: () -> TransportView?
    private let dispatch: (Command) -> Void

    init(transport: @escaping () -> TransportView?, dispatch: @escaping (Command) -> Void) {
        self.transport = transport
        self.dispatch = dispatch
    }

    func submenuItems(for command: RegistryCommand) -> [NSMenuItem] {
        guard let view = transport() else { return [] }
        return view.speeds.map { (choice: SpeedChoice) -> NSMenuItem in
            let item = NSMenuItem(title: choice.label, action: #selector(PlaybackSpeedMenu.chooseSpeed(_:)),
                                  keyEquivalent: "")
            item.target = self
            item.representedObject = choice.speed
            item.state = choice.selected ? .on : .off
            item.isEnabled = command.enabled
            return item
        }
    }

    @objc func chooseSpeed(_ sender: NSMenuItem) {
        guard let speed = sender.representedObject as? Double else { return }
        dispatch(.setSpeed(speed: speed))
    }
}
