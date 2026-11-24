import AppKit
import AVFoundation
import ImageIO
import QuartzCore

final class MusicArtworkView: NSView {
    private let art = CALayer()
    private let placeholder = NSImageView()
    private var drifts = false
    var hasImage: Bool { art.contents != nil }

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = 10
        layer?.masksToBounds = true
        layer?.addSublayer(art)
        art.contentsGravity = .resizeAspectFill
        placeholder.image = NSImage(systemSymbolName: "music.note", accessibilityDescription: nil)?
            .withSymbolConfiguration(.init(pointSize: 48, weight: .light))
        placeholder.contentTintColor = .white
        addSubview(placeholder)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        show(nil)
    }

    required init?(coder: NSCoder) { fatalError("Use init()") }

    override func layout() {
        super.layout()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        art.bounds = bounds
        art.position = CGPoint(x: bounds.midX, y: bounds.midY)
        CATransaction.commit()
        let side = min(64, bounds.width * 0.55)
        placeholder.frame = NSRect(x: bounds.midX - side / 2, y: bounds.midY - side / 2,
                                   width: side, height: side)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard !hasImage else { return }
        NSGradient(starting: NSColor.fromSRGB(PaletteRole.surfaceRaised.srgb.dark),
                   ending: NSColor.fromSRGB(PaletteRole.accent.srgb.light))?.draw(in: bounds, angle: -55)
    }

    func show(_ image: CGImage?) {
        resetMotion()
        art.contents = image
        placeholder.isHidden = image != nil
        setAccessibilityLabel(image == nil ? "Artwork unavailable" : "Embedded artwork")
        needsDisplay = true
    }

    func updateMotion(playing: Bool, reduceMotion: Bool) {
        guard hasImage && !reduceMotion else { resetMotion(); return }
        guard playing != drifts else { return }
        drifts = playing
        let now = CACurrentMediaTime()
        if playing {
            let start = art.timeOffset
            art.speed = 1
            art.timeOffset = 0
            art.beginTime = now - start
            if art.animation(forKey: "artwork-drift") == nil {
                let animation = CABasicAnimation(keyPath: "transform.scale")
                animation.fromValue = 1
                animation.toValue = 1.06
                animation.duration = 22
                animation.autoreverses = true
                animation.repeatCount = .infinity
                animation.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
                art.add(animation, forKey: "artwork-drift")
            }
        } else {
            art.timeOffset = art.convertTime(now, from: nil)
            art.speed = 0
        }
    }

    private func resetMotion() {
        guard art.animation(forKey: "artwork-drift") != nil else { return }
        art.removeAnimation(forKey: "artwork-drift")
        art.speed = 1
        art.timeOffset = 0
        art.beginTime = 0
        drifts = false
    }
}

enum EmbeddedMusicArtwork {
    /// AVFoundation supplies the local file's native artwork metadata.
    /// Downsampling caps the decoded image size regardless of the original.
    static func load(_ source: MediaSource) async -> CGImage? {
        guard source.kind == .local else { return nil }
        let asset = AVURLAsset(url: URL(fileURLWithPath: source.location))
        guard let metadata = try? await asset.load(.commonMetadata) else { return nil }
        for item in AVMetadataItem.metadataItems(from: metadata, filteredByIdentifier: .commonIdentifierArtwork) {
            guard !Task.isCancelled, let data = try? await item.load(.dataValue),
                  data.count <= 24 * 1024 * 1024,
                  let imageSource = CGImageSourceCreateWithData(data as CFData, nil) else { continue }
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: 1200,
            ]
            if let image = CGImageSourceCreateThumbnailAtIndex(imageSource, 0, options as CFDictionary) {
                return image
            }
        }
        return nil
    }
}
