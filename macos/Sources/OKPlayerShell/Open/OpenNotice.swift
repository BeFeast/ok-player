import AppKit

/// What the window says when an open plays nothing. While media is loaded the
/// notice shows over the video for a moment and playback goes on; with nothing
/// loaded it is the Open prompt's status line.
enum OpenNotice: Equatable {
    /// The session's plan opens nothing: no media file, folder or playlist file,
    /// and no subtitle file to load onto playing media.
    case nothingToOpen
    /// Only subtitle files, onto playing media, which this shell cannot load yet
    /// (#910).
    case subtitlesNotLoaded
    /// The folders and playlist files of an open held nothing playable (the
    /// session's `nothing_playable` error).
    case nothingPlayable

    var text: String {
        switch self {
        case .nothingToOpen: return "Nothing to play in the selection"
        case .subtitlesNotLoaded: return "Subtitle files cannot be added yet"
        case .nothingPlayable: return "Nothing playable was found"
        }
    }

    /// The notice for a router outcome that played nothing; `nil` for one that
    /// opened something, was held for the launch or failed (a failure is shown as
    /// one).
    init?(_ outcome: OpenOutcome) {
        switch outcome {
        case .nothing, .opened(_, .noOp, _): self = .nothingToOpen
        case .unsupported: self = .subtitlesNotLoaded
        case .opened, .subtitlesAdded, .held: return nil
        }
    }

    /// The window title while `source` is loaded, as an open sets it.
    static func windowTitle(loaded source: MediaSource?) -> String {
        guard let source else { return "OK Player" }
        return "\((source.location as NSString).lastPathComponent) — OK Player"
    }
}

/// A short message over the video: a rounded label near the top of the window that
/// hides itself after a few seconds. It takes no clicks.
final class OpenNoticeView: NSView {
    let label = NSTextField(labelWithString: "")
    /// How long a notice stays up.
    var duration: TimeInterval = 3
    /// Bumped by every `show`, so only the latest notice's timer hides the view.
    private var generation = 0

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8
        label.alignment = .center
        label.textColor = PaletteRole.text.color
        label.lineBreakMode = .byTruncatingTail
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.topAnchor.constraint(equalTo: topAnchor, constant: 8),
            label.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -8),
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -14),
        ])
        isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }

    /// Show `text`, replacing any notice that is up, and hide it after `duration`.
    func show(_ text: String) {
        label.stringValue = text
        isHidden = false
        generation += 1
        let shown = generation
        DispatchQueue.main.asyncAfter(deadline: .now() + duration) { [weak self] in
            guard let self, self.generation == shown else { return }
            self.isHidden = true
        }
    }

    // The layer's colour is resolved here, against the view's current appearance.
    override func updateLayer() {
        layer?.backgroundColor = PaletteRole.surfaceRaised.color.withAlphaComponent(0.92).cgColor
    }

    override var wantsUpdateLayer: Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
