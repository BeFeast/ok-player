import AppKit

/// A title plane above the video. Its footprint stays fixed while the foreground chrome fades.
final class TitleStripView: NSView {
    static let height: CGFloat = 40
    static let playbackFill = NSColor.black
    let identity = NSView()
    let label = NSTextField(labelWithString: "OK Player")
    private let mark = TitleMarkView()
    private var pointerArea: NSTrackingArea?
    var onPointerActivity: (() -> Void)?

    var title: String {
        get { label.stringValue }
        set { if label.stringValue != newValue { label.stringValue = newValue } }
    }

    var hasMedia = false {
        didSet {
            label.textColor = hasMedia ? PaletteRole.onAccent.overVideo : PaletteRole.text.color
            mark.tint = hasMedia ? PaletteRole.onAccent.overVideo : PaletteRole.link.color
            needsDisplay = true
        }
    }

    init() {
        super.init(frame: .zero)
        identity.translatesAutoresizingMaskIntoConstraints = false
        mark.translatesAutoresizingMaskIntoConstraints = false
        label.translatesAutoresizingMaskIntoConstraints = false
        label.font = .systemFont(ofSize: 13, weight: .semibold)
        label.textColor = PaletteRole.text.color
        label.lineBreakMode = .byTruncatingMiddle
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        addSubview(identity)
        identity.addSubview(mark)
        identity.addSubview(label)
        NSLayoutConstraint.activate([
            identity.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 80),
            identity.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -54),
            identity.topAnchor.constraint(equalTo: topAnchor),
            identity.bottomAnchor.constraint(equalTo: bottomAnchor),
            mark.leadingAnchor.constraint(equalTo: identity.leadingAnchor),
            mark.centerYAnchor.constraint(equalTo: identity.centerYAnchor, constant: 3),
            mark.widthAnchor.constraint(equalToConstant: 18),
            mark.heightAnchor.constraint(equalToConstant: 11),
            label.leadingAnchor.constraint(equalTo: mark.trailingAnchor, constant: 8),
            label.trailingAnchor.constraint(equalTo: identity.trailingAnchor),
            label.centerYAnchor.constraint(equalTo: mark.centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func draw(_ dirtyRect: NSRect) {
        if hasMedia {
            Self.playbackFill.setFill()
            bounds.fill()
        } else {
            NSGradient(starting: NSColor.black.withAlphaComponent(0.48), ending: .clear)?
                .draw(in: bounds, angle: -90)
        }
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        super.hitTest(point) == nil ? nil : self
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let pointerArea { removeTrackingArea(pointerArea) }
        let area = NSTrackingArea(rect: .zero, options: [.inVisibleRect, .activeInKeyWindow, .mouseEnteredAndExited, .mouseMoved],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        pointerArea = area
    }

    override func mouseEntered(with event: NSEvent) { onPointerActivity?() }
    override func mouseMoved(with event: NSEvent) { onPointerActivity?() }
    override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
}

/// The title strip's small O|▶ mark, drawn at the scale of the reference caption.
private final class TitleMarkView: NSView {
    var tint = PaletteRole.link.color { didSet { needsDisplay = true } }

    override func draw(_ dirtyRect: NSRect) {
        tint.setStroke()
        tint.setFill()
        let ring = NSBezierPath(ovalIn: NSRect(x: 0.9, y: 1.4, width: 8.2, height: 8.2))
        ring.lineWidth = 1.9
        ring.stroke()
        NSRect(x: 10.2, y: 1.25, width: 1.2, height: 8.5).fill()
        let play = NSBezierPath()
        play.move(to: NSPoint(x: 12.1, y: 0.7))
        play.line(to: NSPoint(x: 18, y: 5.5))
        play.line(to: NSPoint(x: 12.1, y: 10.3))
        play.close()
        play.fill()
    }
}

/// The theme's canvas under the idle player and under the transparent title strip.
final class PlayerCanvas: NSView {
    override func draw(_ dirtyRect: NSRect) {
        Self.drawCanvas(in: bounds, appearance: effectiveAppearance)
    }

    static func drawCanvas(in bounds: NSRect, appearance: NSAppearance) {
        let dark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        appearance.performAsCurrentDrawingAppearance {
            let start = (dark ? PaletteRole.surface : PaletteRole.canvas).color
            let end = (dark ? PaletteRole.canvas : PaletteRole.surfaceRaised).color
            NSGradient(starting: start, ending: end)?.draw(in: bounds, angle: -45)
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }
}
