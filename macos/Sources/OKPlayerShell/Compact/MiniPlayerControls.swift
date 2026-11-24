import AppKit

struct MiniPlayerAccessibility: Equatable {
    let reduceMotion: Bool
    let reduceTransparency: Bool
    let increaseContrast: Bool

    static var system: Self {
        let workspace = NSWorkspace.shared
        return Self(reduceMotion: workspace.accessibilityDisplayShouldReduceMotion,
                    reduceTransparency: workspace.accessibilityDisplayShouldReduceTransparency,
                    increaseContrast: workspace.accessibilityDisplayShouldIncreaseContrast)
    }
}

/// Three small surfaces over the existing video: title and window actions,
/// play/pause, and the seek and sound controls.
final class MiniPlayerControls: NSView {
    let expand = TransportOverlay.iconButton("arrow.up.left.and.arrow.down.right", "Expand Mini-Player")
    let closeMedia = TransportOverlay.iconButton("xmark", "Close Media")
    let play = TransportOverlay.iconButton("play.fill", "Play")
    let mute = TransportOverlay.iconButton("speaker.wave.2.fill", "Mute")
    let more = TransportOverlay.iconButton("ellipsis", "More commands")
    let seek = ScrubberView()
    private let caption = MiniMaterial(radius: 12)
    private let playPlate = MiniMaterial(radius: 22)
    private let rail = MiniMaterial(radius: 12)
    private let title = NSTextField(labelWithString: "")
    private let elapsed = NSTextField(labelWithString: "00:00")
    private var surfaces: [MiniMaterial] { [caption, playPlate, rail] }
    private var animationGeneration = 0
    private(set) var revealed = false
    var hasSubtitles = false {
        didSet { if hasSubtitles != oldValue { needsLayout = true } }
    }

    var accessibility = MiniPlayerAccessibility.system {
        didSet {
            guard accessibility != oldValue else { return }
            updateMaterial()
            if accessibility.reduceMotion { present(revealed, animated: false) }
        }
    }
    var onLayout: (() -> Void)?

    init() {
        super.init(frame: .zero)
        isHidden = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Mini-Player controls")
        surfaces.forEach(addSubview)
        [expand, title, closeMedia].forEach(caption.addSubview)
        playPlate.addSubview(play)
        [elapsed, seek, mute, more].forEach(rail.addSubview)
        [expand, play, closeMedia, mute, more].forEach { $0.refusesFirstResponder = false }
        title.font = .systemFont(ofSize: 12, weight: .semibold)
        title.lineBreakMode = .byTruncatingTail
        elapsed.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        updateMaterial()
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func layout() {
        super.layout()
        let width = bounds.width
        caption.frame = NSRect(x: 8, y: bounds.height - 50, width: width - 16, height: 42)
        expand.frame = NSRect(x: 8, y: 7, width: 28, height: 28)
        closeMedia.frame = NSRect(x: caption.bounds.width - 36, y: 7, width: 28, height: 28)
        title.frame = NSRect(x: 46, y: 12, width: max(0, caption.bounds.width - 90), height: 17)
        playPlate.frame = NSRect(x: bounds.midX - 22, y: bounds.midY - 22, width: 44, height: 44)
        rail.frame = NSRect(x: 10, y: 9, width: width - 20, height: 40)
        // A centered play button would cover the lifted cue on a short
        // video. Keep it in the rail while that subtitle track is selected.
        let playInRail = hasSubtitles && bounds.height < 220
        let playHost = playInRail ? rail : playPlate
        if play.superview !== playHost {
            play.removeFromSuperview()
            playHost.addSubview(play)
        }
        playPlate.isHidden = playInRail
        play.frame = playInRail ? NSRect(x: 5, y: 6, width: 28, height: 28) : playPlate.bounds
        more.frame = NSRect(x: rail.bounds.width - 34, y: 6, width: 28, height: 28)
        mute.isHidden = width < 360
        mute.frame = more.frame.offsetBy(dx: -30, dy: 0)
        let end = mute.isHidden ? more.frame.minX : mute.frame.minX
        let elapsedWidth = min(58, ceil(elapsed.intrinsicContentSize.width))
        elapsed.isHidden = playInRail && width < 236
        elapsed.frame = NSRect(x: playInRail ? play.frame.maxX + 3 : 9, y: 12,
                               width: elapsed.isHidden ? 0 : elapsedWidth, height: 16)
        seek.frame = NSRect(x: elapsed.frame.maxX + 3, y: 8,
                            width: max(0, end - elapsed.frame.maxX - 5), height: 24)
        onLayout?()
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !usesSolidMaterial,
              let fade = NSGradient(starting: NSColor.black.withAlphaComponent(0.5), ending: .clear) else { return }
        for surface in surfaces where !surface.isHidden {
            fade.draw(in: surface.frame.insetBy(dx: -24, dy: -28), relativeCenterPosition: .zero)
        }
    }

    func update(_ view: TransportView, title name: String, playing: Bool) {
        title.stringValue = name
        if elapsed.stringValue != view.elapsed {
            elapsed.stringValue = view.elapsed
            needsLayout = true
        }
        seek.isEnabled = view.seekable
        seek.progress = view.progress
        seek.buffered = view.buffered
        seek.setAccessibilityValue(NSNumber(value: view.progress))
        updateSymbol(play, playing ? "pause.fill" : "play.fill", label: playing ? "Pause" : "Play")
        updateSymbol(mute, view.volume.muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
                     label: view.volume.muted ? "Unmute" : "Mute")
    }

    private func updateSymbol(_ button: NSButton, _ symbol: String, label: String) {
        guard button.toolTip != label else { return }
        button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?
            .withSymbolConfiguration(.init(pointSize: 14, weight: .medium))
        button.toolTip = label
        button.setAccessibilityLabel(label)
    }

    private func updateMaterial() {
        needsDisplay = true
        for view in surfaces { view.accessibility = accessibility }
        let text = accessibility.increaseContrast ? NSColor.labelColor : PaletteRole.text.overVideo
        [expand, play, closeMedia, mute, more].forEach { $0.contentTintColor = text }
        title.textColor = text
        elapsed.textColor = text
        seek.highContrast = accessibility.increaseContrast
    }

    func present(_ visible: Bool, animated: Bool) {
        guard revealed != visible || !animated else { return }
        revealed = visible
        animationGeneration += 1
        let generation = animationGeneration
        seek.allowsFocus = visible
        if visible { isHidden = false }
        if animated && !accessibility.reduceMotion {
            NSAnimationContext.runAnimationGroup({ context in
                context.duration = visible ? 0.15 : 0.12
                animator().alphaValue = visible ? 1 : 0
            }, completionHandler: { [weak self] in
                guard let self, self.animationGeneration == generation else { return }
                self.isHidden = !visible
            })
        } else {
            alphaValue = visible ? 1 : 0
            isHidden = !visible
        }
    }

    var subtitleClearance: CGFloat { revealed ? rail.frame.maxY + 8 : 0 }
    var usesSolidMaterial: Bool { surfaces.allSatisfy(\.isSolid) }
    var visiblePlates: [CGRect] { revealed ? surfaces.filter { !$0.isHidden }.map(\.frame) : [] }

    func interactive(at point: NSPoint) -> NSView? {
        guard revealed else { return nil }
        return [expand, closeMedia, play, seek, mute, more].first { view in
            !view.isHidden && view.bounds.contains(view.convert(point, from: nil))
        }
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard let parent = superview else { return nil }
        return interactive(at: parent.convert(point, to: nil))
    }
}

private final class MiniMaterial: NSView {
    let radius: CGFloat
    var accessibility = MiniPlayerAccessibility.system { didSet { needsDisplay = true } }
    var isSolid: Bool { accessibility.reduceTransparency || accessibility.increaseContrast }

    init(radius: CGFloat) {
        self.radius = radius
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { fatalError("Use init(radius:)") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let body = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: radius, yRadius: radius)
        effectiveAppearance.performAsCurrentDrawingAppearance {
            NSGraphicsContext.saveGraphicsState()
            if !isSolid {
                let shadow = NSShadow()
                shadow.shadowColor = NSColor.black.withAlphaComponent(0.45)
                shadow.shadowOffset = NSSize(width: 0, height: -6)
                shadow.shadowBlurRadius = 18
                shadow.set()
            }
            let fill = accessibility.increaseContrast ? NSColor.windowBackgroundColor
                : NSColor(srgbRed: 0.09, green: 0.10, blue: 0.11, alpha: isSolid ? 1 : 0.82)
            fill.setFill()
            body.fill()
            NSGraphicsContext.restoreGraphicsState()
            let stroke = accessibility.increaseContrast ? NSColor.labelColor : PaletteRole.borderSubtle.overVideo
            stroke.setStroke()
            body.lineWidth = 1
            body.stroke()
        }
    }
}
