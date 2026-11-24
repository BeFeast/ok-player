import AppKit

/// A transparent overlay for the title. AppKit owns the traffic lights and pin accessory.
final class TitleChrome: NSView {
    let label = NSTextField(labelWithString: "OK Player")
    var verificationOffset: CGFloat = 0
    weak var pinButton: NSButton?
    var trafficLights: (() -> [NSButton])?

    var title: String {
        get { label.stringValue }
        set {
            guard label.stringValue != newValue else { return }
            label.stringValue = newValue
            needsLayout = true
        }
    }

    var hasMedia = false {
        didSet {
            label.textColor = hasMedia ? PaletteRole.onAccent.overVideo : PaletteRole.text.color
            if hasMedia {
                let shadow = NSShadow()
                shadow.shadowColor = NSColor.black
                shadow.shadowOffset = NSSize(width: 0, height: -1)
                shadow.shadowBlurRadius = 3
                label.shadow = shadow
            } else {
                label.shadow = nil
            }
        }
    }

    init() {
        super.init(frame: .zero)
        label.font = .systemFont(ofSize: 13, weight: .semibold)
        label.textColor = PaletteRole.text.color
        label.lineBreakMode = .byTruncatingMiddle
        addSubview(label)
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsLayout = true
    }

    override func layout() {
        super.layout()
        let lights = trafficLights?().map { $0.convert($0.bounds, to: self) } ?? []
        guard let center = lights.first?.midY else { return }
        let leading = (lights.map(\.maxX).max() ?? 0) + 10
        let trailing: CGFloat
        if let pinButton, pinButton.window === window {
            trailing = pinButton.convert(pinButton.bounds, to: self).minX
        } else {
            trailing = bounds.maxX - 44
        }
        let size = label.intrinsicContentSize
        label.frame = CGRect(x: leading, y: center - size.height / 2 + verificationOffset,
                             width: min(size.width, max(0, trailing - leading - 8)), height: size.height)
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard !isHidden, let superview, label.frame.contains(convert(point, from: superview)) else { return nil }
        return self
    }

    override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
}
