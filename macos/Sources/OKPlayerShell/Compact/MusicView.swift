import AppKit

/// The audio layout in the player window. The queue reuses the standard
/// Up Next panel and takes the artwork's space when expanded.
final class MusicView: NSView {
    static let defaultSize = CGSize(width: 366, height: 560)
    static let minimumSize = CGSize(width: 320, height: 494)

    let artwork = MusicArtworkView()
    let queueHost = NSView()
    let lyricsHost = NSView()
    let play = TransportOverlay.iconButton("play.fill", "Play")
    let previous = TransportOverlay.iconButton("backward.end.fill", "Previous Item")
    let next = TransportOverlay.iconButton("forward.end.fill", "Next Item")
    let queue = TransportOverlay.iconButton("list.bullet.rectangle", "Chapters & Up Next")
    let lyrics = TransportOverlay.iconButton("quote.bubble", "Lyrics")
    let more = TransportOverlay.iconButton("ellipsis", "More commands")
    let mute = TransportOverlay.iconButton("speaker.wave.2.fill", "Mute")
    let seek = ScrubberView()
    let volume = VolumeSliderView()
    let switchToVideo = NSButton(title: "Switch to Video", target: nil, action: nil)
    let title = NSTextField(labelWithString: "")
    let detail = NSTextField(labelWithString: "")

    private let material = NSVisualEffectView()
    private let header = MusicTitleBar()
    private let controls = MusicControlsPlane()
    private let elapsed = NSTextField(labelWithString: "00:00")
    private let remaining = NSTextField(labelWithString: "--:--")

    var reduceTransparency = false {
        didSet { material.isHidden = reduceTransparency; needsDisplay = true }
    }
    var queueShown = false {
        didSet {
            guard oldValue != queueShown else { return }
            if queueShown && lyricsShown { lyricsShown = false }
            queueHost.isHidden = !queueShown
            needsLayout = true
            queue.state = queueShown ? .on : .off
            queue.toolTip = queueShown ? "Hide Chapters & Up Next" : "Chapters & Up Next"
            queue.setAccessibilityLabel(queue.toolTip)
        }
    }
    var lyricsShown = false {
        didSet {
            guard oldValue != lyricsShown else { return }
            if lyricsShown && queueShown { queueShown = false }
            lyricsHost.isHidden = !lyricsShown
            needsLayout = true
            lyrics.state = lyricsShown ? .on : .off
            lyrics.toolTip = lyricsShown ? "Hide Lyrics" : "Lyrics"
            lyrics.setAccessibilityLabel(lyrics.toolTip)
        }
    }
    var panelExpanded: Bool { queueShown || lyricsShown }
    var controlsShown: Bool { !controls.isHidden }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    init() {
        super.init(frame: .zero)
        isHidden = true
        setAccessibilityElement(true)
        setAccessibilityRole(.group)
        setAccessibilityLabel("Compact Music")

        material.material = .windowBackground
        material.blendingMode = .behindWindow
        material.state = .followsWindowActiveState
        ([material, header, artwork, title, detail, controls, queueHost, lyricsHost] as [NSView]).forEach(addSubview)
        ([play, previous, next, queue, lyrics, more, mute, seek, elapsed, remaining, volume, switchToVideo] as [NSView])
            .forEach(controls.addSubview)
        queueHost.isHidden = true
        lyricsHost.isHidden = true
        queue.setButtonType(.onOff)
        lyrics.setButtonType(.onOff)
        play.isBordered = true
        play.bezelStyle = .circular
        play.bezelColor = .controlAccentColor
        play.contentTintColor = .white
        for button in [previous, next, queue, lyrics, more, mute] {
            button.contentTintColor = .labelColor
            button.refusesFirstResponder = false
        }
        play.refusesFirstResponder = false
        switchToVideo.controlSize = .small
        seek.onThemedSurface = true
        volume.onThemedSurface = true
        detail.font = .systemFont(ofSize: 13)
        detail.textColor = .secondaryLabelColor
        for label in [elapsed, remaining] {
            label.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
            label.textColor = .secondaryLabelColor
        }
        remaining.alignment = .right
        for label in [title, detail, header.label] { label.lineBreakMode = .byTruncatingTail }
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func draw(_ dirtyRect: NSRect) {
        PaletteRole.canvas.color.setFill()
        bounds.fill()
    }

    override func layout() {
        super.layout()
        let inset: CGFloat = 20
        let width = bounds.width - 2 * inset
        material.frame = bounds
        header.frame = NSRect(x: 0, y: 0, width: bounds.width, height: 36)
        header.label.frame = NSRect(x: 82, y: 8, width: max(0, bounds.width - 130), height: 17)

        let side = panelExpanded ? 72 : max(80, min(width, bounds.height - 224))
        artwork.frame = NSRect(x: panelExpanded ? inset : bounds.midX - side / 2,
                               y: 42, width: side, height: side)
        let titleX = panelExpanded ? artwork.frame.maxX + 14 : inset
        let titleTop = panelExpanded ? artwork.frame.minY + 13 : artwork.frame.maxY + 14
        title.frame = NSRect(x: titleX, y: titleTop, width: bounds.width - inset - titleX, height: 28)
        detail.frame = NSRect(x: titleX, y: titleTop + 29, width: title.frame.width, height: 20)

        let metadataEnd = max(detail.frame.maxY, artwork.frame.maxY)
        controls.frame = NSRect(x: inset, y: metadataEnd + 6, width: width, height: 105)
        elapsed.frame = NSRect(x: 0, y: 5, width: 52, height: 16)
        remaining.frame = NSRect(x: width - 56, y: 5, width: 56, height: 16)
        seek.frame = NSRect(x: 52, y: 0, width: max(0, width - 108), height: 24)
        play.frame = NSRect(x: width / 2 - 22, y: 25, width: 44, height: 44)
        previous.frame = NSRect(x: play.frame.minX - 46, y: 31, width: 34, height: 32)
        next.frame = NSRect(x: play.frame.maxX + 12, y: 31, width: 34, height: 32)
        queue.frame = NSRect(x: 0, y: 31, width: 32, height: 32)
        lyrics.frame = NSRect(x: width - 66, y: 31, width: 32, height: 32)
        more.frame = NSRect(x: width - 32, y: 31, width: 32, height: 32)
        mute.frame = NSRect(x: 0, y: 74, width: 28, height: 26)
        volume.frame = NSRect(x: 30, y: 74, width: switchToVideo.isHidden ? width - 30 : width - 155, height: 26)
        switchToVideo.frame = NSRect(x: width - 120, y: 75, width: 120, height: 24)

        let panelTop = controls.frame.maxY + 8
        let panelFrame = NSRect(x: 10, y: panelTop, width: bounds.width - 20,
                                height: max(0, bounds.height - panelTop - 12))
        queueHost.frame = panelFrame
        lyricsHost.frame = panelFrame
    }

    func show(title name: String, detail subtitle: String, hasVideo: Bool) {
        if title.stringValue != name {
            title.stringValue = name
            header.label.stringValue = name
        }
        let titleSize: CGFloat = artwork.hasImage ? 20 : 23
        if title.font?.pointSize != titleSize { title.font = .systemFont(ofSize: titleSize, weight: .semibold) }
        if detail.stringValue != subtitle { detail.stringValue = subtitle }
        if switchToVideo.isHidden == hasVideo {
            switchToVideo.isHidden = !hasVideo
            needsLayout = true
        }
    }

    func showTransport(_ transport: TransportView, playing: Bool, canStep: Bool) {
        elapsed.stringValue = transport.elapsed
        remaining.stringValue = transport.remaining
        seek.isEnabled = transport.seekable
        seek.progress = transport.progress
        seek.buffered = transport.buffered
        seek.ticks = transport.chapterTicks
        seek.setAccessibilityValue(NSNumber(value: transport.progress))
        volume.volume = transport.volume
        volume.setAccessibilityValue(transport.volume.readout)
        previous.isEnabled = canStep
        next.isEnabled = canStep
        setSymbol(play, playing ? "pause.fill" : "play.fill", playing ? "Pause" : "Play")
        setSymbol(mute, transport.volume.muted ? "speaker.slash.fill" : "speaker.wave.2.fill",
                  transport.volume.muted ? "Unmute" : "Mute")
    }

    func showControls(_ shown: Bool) {
        if !shown, let responder = window?.firstResponder as? NSView, responder.isDescendant(of: controls) {
            window?.makeFirstResponder(self)
        }
        seek.allowsFocus = shown
        controls.isHidden = !shown
    }

    func pointerOverControls(_ location: NSPoint) -> Bool {
        controlsShown && controls.bounds.contains(controls.convert(location, from: nil))
    }

    private func setSymbol(_ button: NSButton, _ symbol: String, _ label: String) {
        guard button.toolTip != label else { return }
        button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)?
            .withSymbolConfiguration(.init(pointSize: 14, weight: .medium))
        button.toolTip = label
        button.setAccessibilityLabel(label)
    }
}

private class MusicControlsPlane: NSView {
    override var isFlipped: Bool { true }
}

private final class MusicTitleBar: MusicControlsPlane {
    let label = NSTextField(labelWithString: "")

    override init(frame: NSRect) {
        super.init(frame: frame)
        label.font = .systemFont(ofSize: 12, weight: .medium)
        label.textColor = .secondaryLabelColor
        addSubview(label)
    }

    convenience init() { self.init(frame: .zero) }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func mouseDown(with event: NSEvent) { window?.performDrag(with: event) }
}
