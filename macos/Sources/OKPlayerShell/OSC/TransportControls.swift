import AppKit

/// The seek bar: track, buffered range, progress, chapter ticks and a knob, in
/// palette colours. A click or drag reports the fraction under the pointer;
/// Left and Right report a five-second step while it has keyboard focus.
final class ScrubberView: NSView {
    enum Phase { case began, moved, ended }

    static let inset: CGFloat = 7

    var progress = 0.0 { didSet { if progress != oldValue { needsDisplay = true } } }
    var buffered = 0.0 { didSet { if buffered != oldValue { needsDisplay = true } } }
    var ticks: [Double] = [] { didSet { if ticks != oldValue { needsDisplay = true } } }
    var isEnabled = false { didSet { if isEnabled != oldValue { needsDisplay = true } } }
    /// The controls are shown, so the bar may take keyboard focus.
    var allowsFocus = true
    var onScrub: ((Phase, Double) -> Void)?
    var onStep: ((Double) -> Void)?

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
        PaletteRole.borderSubtle.color.setFill()
        NSBezierPath(roundedRect: track, xRadius: 2, yRadius: 2).fill()
        guard isEnabled else { return }
        PaletteRole.border.color.setFill()
        NSBezierPath(roundedRect: span(buffered), xRadius: 2, yRadius: 2).fill()
        PaletteRole.accent.color.setFill()
        NSBezierPath(roundedRect: span(progress), xRadius: 2, yRadius: 2).fill()
        PaletteRole.text.color.setFill()
        for tick in ticks where tick > 0 && tick < 1 {
            let x = track.minX + track.width * CGFloat(tick)
            NSRect(x: x - 0.75, y: track.minY - 2, width: 1.5, height: track.height + 4).fill()
        }
        let knob = NSRect(x: span(progress).maxX - 6, y: bounds.midY - 6, width: 12, height: 12)
        PaletteRole.accent.color.setFill()
        NSBezierPath(ovalIn: knob).fill()
        if window?.isKeyWindow == true, window?.firstResponder === self {
            let ring = NSBezierPath(ovalIn: knob.insetBy(dx: -2.5, dy: -2.5))
            ring.lineWidth = 2
            PaletteRole.focus.color.setStroke()
            ring.stroke()
        }
    }

    private func fraction(of event: NSEvent) -> Double {
        let x = convert(event.locationInWindow, from: nil).x
        return TrackGeometry.fraction(atX: Double(x), width: Double(bounds.width), inset: Double(Self.inset))
    }

    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        onScrub?(.began, fraction(of: event))
    }

    override func mouseDragged(with event: NSEvent) {
        guard isEnabled else { return }
        onScrub?(.moved, fraction(of: event))
    }

    override func mouseUp(with event: NSEvent) {
        guard isEnabled else { return }
        onScrub?(.ended, fraction(of: event))
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 123: onStep?(-5)  // Left
        case 124: onStep?(5)   // Right
        default: super.keyDown(with: event)
        }
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
    static let inset: CGFloat = 5

    var volume: VolumeView? { didSet { if volume != oldValue { needsDisplay = true } } }
    var onFraction: ((Double) -> Void)?

    init() {
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.slider)
        setAccessibilityLabel("Volume")
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override var intrinsicContentSize: NSSize { NSSize(width: 72, height: 18) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        let track = NSRect(x: Self.inset, y: bounds.midY - 2, width: Swift.max(0, bounds.width - 2 * Self.inset), height: 4)
        PaletteRole.borderSubtle.color.setFill()
        NSBezierPath(roundedRect: track, xRadius: 2, yRadius: 2).fill()
        guard let volume else { return }
        let unityX = track.minX + track.width * CGFloat(volume.unityFraction)
        let levelX = track.minX + track.width * CGFloat(Swift.min(Swift.max(volume.fraction, 0), 1))
        (volume.muted ? PaletteRole.textMuted : PaletteRole.accent).color.setFill()
        NSRect(x: track.minX, y: track.minY, width: Swift.min(levelX, unityX) - track.minX, height: track.height).fill()
        if levelX > unityX {
            (volume.muted ? PaletteRole.textMuted : PaletteRole.warning).color.setFill()
            NSRect(x: unityX, y: track.minY, width: levelX - unityX, height: track.height).fill()
        }
        PaletteRole.text.color.setFill()
        NSRect(x: unityX - 0.5, y: track.minY - 3, width: 1, height: track.height + 6).fill()
        let knob = NSRect(x: levelX - 5, y: bounds.midY - 5, width: 10, height: 10)
        (volume.muted ? PaletteRole.textMuted : PaletteRole.accent).color.setFill()
        NSBezierPath(ovalIn: knob).fill()
    }

    private func report(_ event: NSEvent) {
        let x = convert(event.locationInWindow, from: nil).x
        onFraction?(TrackGeometry.fraction(atX: Double(x), width: Double(bounds.width), inset: Double(Self.inset)))
    }

    override func mouseDown(with event: NSEvent) { report(event) }
    override func mouseDragged(with event: NSEvent) { report(event) }
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
