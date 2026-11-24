import AppKit

/// The seek bar: track, buffered range, progress, chapter ticks and a knob, in
/// palette colours. A click or drag reports the fraction under the pointer, and a
/// press it accepted always reports its release. It handles no keys: while it has
/// keyboard focus, a key press goes on up the responder chain to the player
/// window's shortcut table, as from the video.
final class ScrubberView: NSView {
    enum Phase { case began, moved, ended }

    static let inset: CGFloat = 7

    var progress = 0.0 { didSet { if progress != oldValue { needsDisplay = true } } }
    var buffered = 0.0 { didSet { if buffered != oldValue { needsDisplay = true } } }
    var ticks: [Double] = [] { didSet { if ticks != oldValue { needsDisplay = true } } }
    var isEnabled = false { didSet { if isEnabled != oldValue { needsDisplay = true } } }
    /// The controls are shown, so the bar may take keyboard focus.
    var allowsFocus = true
    var highContrast = false { didSet { needsDisplay = true } }
    var onThemedSurface = false { didSet { needsDisplay = true } }
    var onAccessibilitySeek: ((Double) -> Void)?
    var onScrub: ((Phase, Double) -> Void)?
    /// A press on the enabled bar that has not been released yet.
    private(set) var isPressed = false

    init() {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
        setAccessibilityLabel("Seek")
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override var intrinsicContentSize: NSSize { NSSize(width: NSView.noIntrinsicMetric, height: 18) }
    override var acceptsFirstResponder: Bool { isEnabled && allowsFocus }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private var track: NSRect {
        NSRect(x: Self.inset, y: bounds.midY - 2, width: Swift.max(0, bounds.width - 2 * Self.inset), height: 4)
    }

    private func span(_ fraction: Double) -> NSRect {
        var rect = track
        rect.size.width = track.width * CGFloat(Swift.min(Swift.max(fraction, 0), 1))
        return rect
    }

    override func draw(_ dirtyRect: NSRect) {
        let themed = highContrast || onThemedSurface
        (highContrast ? NSColor.secondaryLabelColor
            : onThemedSurface ? PaletteRole.border.color : PaletteRole.borderSubtle.overVideo).setFill()
        NSBezierPath(roundedRect: track, xRadius: 2, yRadius: 2).fill()
        guard isEnabled else { return }
        (themed ? NSColor.tertiaryLabelColor : PaletteRole.border.overVideo).setFill()
        NSBezierPath(roundedRect: span(buffered), xRadius: 2, yRadius: 2).fill()
        (themed ? NSColor.controlAccentColor : PaletteRole.accent.overVideo).setFill()
        NSBezierPath(roundedRect: span(progress), xRadius: 2, yRadius: 2).fill()
        (themed ? NSColor.labelColor : PaletteRole.text.overVideo).setFill()
        for tick in ticks where tick > 0 && tick < 1 {
            let x = track.minX + track.width * CGFloat(tick)
            NSRect(x: x - 0.75, y: track.minY - 2, width: 1.5, height: track.height + 4).fill()
        }
        let knob = NSRect(x: span(progress).maxX - 6, y: bounds.midY - 6, width: 12, height: 12)
        (themed ? NSColor.labelColor : PaletteRole.onAccent.overVideo).setFill()
        NSBezierPath(ovalIn: knob).fill()
        if window?.isKeyWindow == true, window?.firstResponder === self {
            let ring = NSBezierPath(ovalIn: knob.insetBy(dx: -2.5, dy: -2.5))
            ring.lineWidth = 2
            (themed ? NSColor.keyboardFocusIndicatorColor : PaletteRole.focus.overVideo).setStroke()
            ring.stroke()
        }
    }

    override func accessibilityPerformIncrement() -> Bool { accessibilitySeek(by: 0.05) }
    override func accessibilityPerformDecrement() -> Bool { accessibilitySeek(by: -0.05) }

    private func accessibilitySeek(by delta: Double) -> Bool {
        guard isEnabled, allowsFocus, let onAccessibilitySeek else { return false }
        onAccessibilitySeek(min(max(progress + delta, 0), 1))
        return true
    }

    private func fraction(of event: NSEvent) -> Double {
        let x = convert(event.locationInWindow, from: nil).x
        return TrackGeometry.fraction(atX: Double(x), width: Double(bounds.width), inset: Double(Self.inset))
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        isPressed = true
        onScrub?(.began, fraction(of: event))
    }

    override func mouseDragged(with event: NSEvent) {
        guard isPressed, isEnabled else { return }
        onScrub?(.moved, fraction(of: event))
    }

    /// Reported even when the bar was disabled during the press (the file ended
    /// under the pointer), so whatever the press started is let go.
    override func mouseUp(with event: NSEvent) {
        guard isPressed else { return }
        isPressed = false
        onScrub?(.ended, fraction(of: event))
    }

    override func becomeFirstResponder() -> Bool {
        needsDisplay = true
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        return super.resignFirstResponder()
    }
}

/// The volume slider: 0 to the session's maximum, the part up to 100 % in the
/// accent colour, boost above it in the warning colour, and a mark at 100 %.
/// A click or drag reports the fraction of the track under the pointer.
final class VolumeSliderView: NSView {
    static let inset: CGFloat = 7

    var onThemedSurface = false { didSet { needsDisplay = true } }
    var volume: VolumeView? { didSet { if volume != oldValue { needsDisplay = true } } }
    var onFraction: ((Double) -> Void)?
    var onRelease: ((NSEvent) -> Void)?
    var onNudge: ((Double) -> Void)?
    private(set) var isPressed = false

    init() {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
        setAccessibilityLabel("Volume")
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        let track = NSRect(x: Self.inset, y: bounds.midY - 3, width: Swift.max(0, bounds.width - 2 * Self.inset), height: 6)
        (onThemedSurface ? PaletteRole.border.color : PaletteRole.borderSubtle.overVideo).setFill()
        NSBezierPath(roundedRect: track, xRadius: 2, yRadius: 2).fill()
        guard let volume else { return }
        let unityX = track.minX + track.width * CGFloat(volume.unityFraction)
        let levelX = track.minX + track.width * CGFloat(Swift.min(Swift.max(volume.fraction, 0), 1))
        let fill = onThemedSurface
            ? (volume.muted ? NSColor.secondaryLabelColor : NSColor.controlAccentColor)
            : (volume.muted ? PaletteRole.textMuted : PaletteRole.accent).overVideo
        fill.setFill()
        NSRect(x: track.minX, y: track.minY, width: Swift.min(levelX, unityX) - track.minX, height: track.height).fill()
        if levelX > unityX {
            (volume.muted ? PaletteRole.textMuted : PaletteRole.warning).overVideo.setFill()
            NSRect(x: unityX, y: track.minY, width: levelX - unityX, height: track.height).fill()
        }
        (onThemedSurface ? NSColor.labelColor : PaletteRole.text.overVideo).setFill()
        NSRect(x: unityX - 0.5, y: track.minY - 3, width: 1, height: track.height + 6).fill()
        let knob = NSRect(x: levelX - 7, y: bounds.midY - 7, width: 14, height: 14)
        (onThemedSurface ? NSColor.labelColor
            : (volume.muted ? PaletteRole.textMuted : PaletteRole.text).overVideo).setFill()
        NSBezierPath(ovalIn: knob).fill()
    }

    private func report(_ event: NSEvent) {
        let x = convert(event.locationInWindow, from: nil).x
        onFraction?(TrackGeometry.fraction(atX: Double(x), width: Double(bounds.width), inset: Double(Self.inset)))
    }

    override func mouseDown(with event: NSEvent) {
        isPressed = true
        window?.makeFirstResponder(self)
        report(event)
    }

    override func mouseDragged(with event: NSEvent) {
        if isPressed { report(event) }
    }

    override func mouseUp(with event: NSEvent) {
        guard isPressed else { return }
        isPressed = false
        onRelease?(event)
    }

    func cancelPress() { isPressed = false }

    override func keyDown(with event: NSEvent) {
        let delta = event.modifierFlags.contains(.shift) ? 0.1 : 1
        switch KeyPress(event: event)?.key {
        case "Left", "Down": onNudge?(-delta)
        case "Right", "Up": onNudge?(delta)
        default: super.keyDown(with: event)
        }
    }

    override func accessibilityPerformIncrement() -> Bool {
        onNudge?(1)
        return onNudge != nil
    }

    override func accessibilityPerformDecrement() -> Bool {
        onNudge?(-1)
        return onNudge != nil
    }
}

extension ScrollInput {
    /// The volume control's reading of a scroll event.
    init(event: NSEvent) {
        let delta = Double(event.scrollingDeltaY)
        self.init(deltaY: event.isDirectionInvertedFromDevice ? -delta : delta,
                  precise: event.hasPreciseScrollingDeltas,
                  phase: ScrollInput.Phase(event.phase),
                  momentum: !event.momentumPhase.isEmpty)
    }
}

extension ScrollInput.Phase {
    init(_ phase: NSEvent.Phase) {
        if phase.contains(.began) || phase.contains(.mayBegin) {
            self = .began
        } else if phase.contains(.ended) || phase.contains(.cancelled) {
            self = .ended
        } else if phase.isEmpty {
            self = .none
        } else {
            self = .changed
        }
    }
}

/// A contrast layer between video and the single transport row.
final class TransportScrim: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        let gradient = NSGradient(colors: [.black.withAlphaComponent(0.48), .clear])
        gradient?.draw(in: bounds, angle: 90)
    }
}

/// The resting speaker slot shows gain without changing the width of the row.
final class VolumeWick: NSView {
    var volume: VolumeView? { didSet { if volume != oldValue { needsDisplay = true } } }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        PaletteRole.border.overVideo.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 1.5, yRadius: 1.5).fill()
        guard let volume else { return }
        var fill = bounds
        fill.size.width *= volume.muted ? 0 : volume.fraction
        (volume.boosted ? PaletteRole.warning : PaletteRole.accent).overVideo.setFill()
        NSBezierPath(roundedRect: fill, xRadius: 1.5, yRadius: 1.5).fill()
    }
}

final class TransportTextButton: NSButton {
    var value: String {
        didSet {
            if value != oldValue {
                title = value + "  ⌄"
                setAccessibilityValue(value)
            }
        }
    }

    init(title value: String, label: String) {
        self.value = value
        super.init(frame: .zero)
        self.title = value + "  ⌄"
        font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .semibold)
        isBordered = false
        toolTip = label
        setAccessibilityLabel(label)
        setAccessibilityValue(value)
    }

    required init?(coder: NSCoder) { fatalError("Use init(title:label:)") }

    override func draw(_ dirtyRect: NSRect) {
        PaletteRole.text.overVideo.withAlphaComponent(isHighlighted ? 0.24 : 0.12).setFill()
        NSBezierPath(roundedRect: bounds.insetBy(dx: 0, dy: 3), xRadius: 7, yRadius: 7).fill()
        super.draw(dirtyRect)
    }
}

/// Inline numeric entry keeps exact gain changes on the same session command path.
final class VolumeReadoutField: NSTextField, NSTextFieldDelegate {
    var onCommit: ((Double) -> Void)?
    var onFinish: (() -> Void)?
    private var volume: VolumeView?
    private var cancelled = false

    init() {
        super.init(frame: .zero)
        isEditable = true
        isSelectable = true
        isBordered = false
        drawsBackground = false
        alignment = .center
        font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .semibold)
        focusRingType = .none
        toolTip = "Enter volume"
        setAccessibilityLabel("Exact volume")
        delegate = self
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    func update(_ volume: VolumeView) {
        self.volume = volume
        textColor = (volume.muted || volume.boosted ? PaletteRole.warning : PaletteRole.text).overVideo
        if currentEditor() == nil { stringValue = volume.readout }
    }

    func controlTextDidBeginEditing(_ notification: Notification) {
        cancelled = false
    }

    func controlTextDidEndEditing(_ notification: Notification) {
        let plain = stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: ",", with: ".")
        let number = plain.hasSuffix("%") ? String(plain.dropLast()) : plain
        if !cancelled, let value = Double(number), value.isFinite, value != volume?.level { onCommit?(value) }
        stringValue = volume?.readout ?? ""
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.cancelOperation(_:)):
            cancelled = true
        case #selector(NSResponder.insertNewline(_:)):
            break
        default:
            return false
        }
        onFinish?()
        return true
    }
}
