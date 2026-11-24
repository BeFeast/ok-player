import AppKit

/// Resolves the player window's input before AppKit sends unclaimed events to
/// the first responder. The active surface or focused native control gets first
/// refusal, then the session's shared bindings can run a command.
final class PlayerInputRouter {
    let commands: PlayerCommands
    let context: CommandPopover
    var dismissTransient: (() -> Bool)?
    var canOpenContext: ((NSPoint) -> Bool)?
    var protectsTextInput = true
    private var consumedMouseButton: NSEvent.EventType?
    private var dismissedByLeftClick = false

    init(commands: PlayerCommands, context: CommandPopover) {
        self.commands = commands
        self.context = context
    }

    func handleKey(_ event: NSEvent, in window: PlayerWindow) -> Bool {
        guard window.attachedSheet == nil, let press = KeyPress(event: event) else { return false }
        if context.handleKey(press) { return true }

        let inText = Self.isTextInput(window.firstResponder)
        if protectsTextInput && inText && (!press.modifiers.cmd || Self.reservesForEditing(press.key)) {
            return false
        }

        if press.key == "Escape", press.modifiers == ShortcutModifiers() {
            // A held Escape must never work its way through the stack of
            // surfaces and then exit fullscreen after dismissing one.
            if press.isRepeat { return true }
            if dismissTransient?() == true { return true }
        }

        if context.isShown && !press.modifiers.cmd { return false }
        if !press.modifiers.cmd, let focus = window.firstResponder {
            if focus is UpNextTableView &&
                (UpNextKey.navigationKeyCodes.contains(event.keyCode) ||
                 UpNextKey(keyCode: event.keyCode, modifiers: event.modifierFlags) != nil) { return false }
            if focus is VolumeSliderView, ["Left", "Right", "Up", "Down"].contains(press.key) { return false }
            if focus is NSButton, press.key == "space", press.modifiers == ShortcutModifiers() { return false }
        }
        return commands.handle(press, includeCommand: true)
    }

    func cancel(in window: PlayerWindow) -> Bool {
        if (window.firstResponder as? NSTextView)?.hasMarkedText() == true { return true }
        return context.escape() || Self.isTextInput(window.firstResponder) || dismissTransient?() == true
    }

    func handlePointer(_ event: NSEvent, in window: PlayerWindow) -> Bool {
        switch event.type {
        case .leftMouseUp, .rightMouseUp, .otherMouseUp:
            guard consumedMouseButton == event.type else { return false }
            consumedMouseButton = nil
            return true
        case .leftMouseDragged: return consumedMouseButton == .leftMouseUp
        case .rightMouseDragged: return consumedMouseButton == .rightMouseUp
        case .otherMouseDragged: return consumedMouseButton == .otherMouseUp
        default: break
        }

        guard event.window == nil || event.window === window else { return false }
        let point = event.window == nil ? window.convertPoint(fromScreen: event.locationInWindow) : event.locationInWindow
        if event.type == .scrollWheel { return context.isShown && !context.contains(windowPoint: point) }

        let mouseUp: NSEvent.EventType
        switch event.type {
        case .leftMouseDown: mouseUp = .leftMouseUp
        case .rightMouseDown: mouseUp = .rightMouseUp
        case .otherMouseDown: mouseUp = .otherMouseUp
        default: return false
        }
        // A second release belonging to the click that dismissed the popover
        // must also stay out of the video click arbiter.
        if event.type == .leftMouseDown {
            if dismissedByLeftClick && event.clickCount > 1 {
                consumedMouseButton = mouseUp
                return true
            }
            dismissedByLeftClick = false
        }
        if context.isShown {
            guard !context.contains(windowPoint: point) else { return false }
            context.dismiss()
            dismissedByLeftClick = event.type == .leftMouseDown
            consumedMouseButton = mouseUp
            return true
        }

        let mods = ShortcutModifiers(flags: event.modifierFlags)
        let contextClick = event.type == .rightMouseDown
            || (event.type == .leftMouseDown && mods == ShortcutModifiers(ctrl: true))
        guard contextClick, window.attachedSheet == nil, canOpenContext?(point) == true,
              context.show(at: point) else { return false }
        consumedMouseButton = mouseUp
        return true
    }

    static func isTextInput(_ responder: NSResponder?) -> Bool {
        responder is NSTextView || responder is NSTextField || responder is any NSTextInputClient
    }

    static func reservesForEditing(_ key: String) -> Bool {
        ["Left", "Right", "Up", "Down", "Home", "End", "Page_Up", "Page_Down",
         "BackSpace", "Delete", "a", "c", "v", "x", "z"].contains(key)
    }
}
