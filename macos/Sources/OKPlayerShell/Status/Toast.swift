import AppKit

/// The toast OSD: one short line that goes away by itself. Anything in the shell
/// shows one with `show(_:)`; the session's notices arrive the same way
/// (`StatusLayer`). A newer toast replaces the one showing and starts its time again.
final class Toast {
    /// How long a toast stays up: long enough to notice a confirmation, as on Linux.
    static let dwell: TimeInterval = 1.7

    /// The line showing, or `nil`.
    private(set) var text: String?
    /// Called with the line when a toast shows and with `nil` when it goes away.
    var onChange: ((String?) -> Void)?
    private let timer: HideTimer

    init(timer: HideTimer = MainQueueHideTimer()) {
        self.timer = timer
    }

    func show(_ text: String) {
        let line = Toast.oneLine(text)
        guard !line.isEmpty else { return }
        self.text = line
        onChange?(line)
        timer.schedule(after: Toast.dwell) { [weak self] in self?.dismiss() }
    }

    func dismiss() {
        timer.cancel()
        guard text != nil else { return }
        text = nil
        onChange?(nil)
    }

    /// Line breaks, tabs and other control characters become spaces, so a toast
    /// never grows a second line over the video (a file name may contain them).
    static func oneLine(_ text: String) -> String {
        var flattened = String.UnicodeScalarView()
        for scalar in text.unicodeScalars {
            let breaksLine = scalar.properties.generalCategory == .control
                || scalar == "\u{2028}" || scalar == "\u{2029}"
            flattened.append(breaksLine ? " " : scalar)
        }
        return String(flattened).trimmingCharacters(in: .whitespaces)
    }
}

/// Draws the toast: a rounded pill near the top of the video, under the title bar,
/// so it stays clear of captions at the bottom. It never takes clicks.
final class ToastView: NSView {
    let pill = NSVisualEffectView()
    let label = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        pill.material = .popover
        pill.blendingMode = .withinWindow
        pill.state = .active
        pill.wantsLayer = true
        pill.layer?.cornerRadius = 10
        pill.layer?.masksToBounds = true
        pill.translatesAutoresizingMaskIntoConstraints = false
        addSubview(pill)
        label.font = NSFont.systemFont(ofSize: 13, weight: .medium)
        label.textColor = PaletteRole.text.color
        label.lineBreakMode = .byTruncatingMiddle
        label.maximumNumberOfLines = 1
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        label.translatesAutoresizingMaskIntoConstraints = false
        pill.addSubview(label)
        NSLayoutConstraint.activate([
            pill.topAnchor.constraint(equalTo: topAnchor),
            pill.bottomAnchor.constraint(equalTo: bottomAnchor),
            pill.centerXAnchor.constraint(equalTo: centerXAnchor),
            pill.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor),
            pill.widthAnchor.constraint(lessThanOrEqualToConstant: 520),
            label.topAnchor.constraint(equalTo: pill.topAnchor, constant: 7),
            label.bottomAnchor.constraint(equalTo: pill.bottomAnchor, constant: -7),
            label.leadingAnchor.constraint(equalTo: pill.leadingAnchor, constant: 14),
            label.trailingAnchor.constraint(equalTo: pill.trailingAnchor, constant: -14),
        ])
        isHidden = true
        setAccessibilityElement(true)
        setAccessibilityRole(.staticText)
    }

    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }

    /// Show `text`, or hide for `nil`.
    func show(_ text: String?) {
        if let text {
            label.stringValue = text
            setAccessibilityLabel(text)
        }
        isHidden = text == nil
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
