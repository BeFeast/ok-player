import AppKit

/// The GTK side panel uses the same translucent paper over the video in both themes.
enum UpNextMaterial {
    static let paper = NSColor.fromSRGB(0xF4F4F4).withAlphaComponent(0.96)
    static let ink = NSColor.black.withAlphaComponent(0.78)
    static let muted = NSColor.black.withAlphaComponent(0.45)
    static let accent = NSColor.fromSRGB(PaletteRole.accent.srgb.light)
    static let link = NSColor.fromSRGB(PaletteRole.link.srgb.light)

    static func tint(_ opacity: CGFloat) -> NSColor { accent.withAlphaComponent(opacity) }

    static func label(_ text: String = "", size: CGFloat, weight: NSFont.Weight = .regular,
                      color: NSColor = ink) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = .systemFont(ofSize: size, weight: weight)
        label.textColor = color
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return label
    }

    static func symbol(_ name: String, size: CGFloat = 13) -> NSImage? {
        NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: size, weight: .semibold))
    }
}

final class UpNextCardView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        UpNextMaterial.tint(0.12).setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 8, yRadius: 8).fill()
    }
}

final class UpNextThumbnail: NSImageView {
    override func draw(_ dirtyRect: NSRect) {
        let start = NSColor.fromSRGB(PaletteRole.surfaceRaised.srgb.dark)
        NSGradient(starting: start, ending: UpNextMaterial.accent)?
            .draw(in: NSBezierPath(roundedRect: bounds, xRadius: 5, yRadius: 5), angle: -35)
        super.draw(dirtyRect)
    }
}
