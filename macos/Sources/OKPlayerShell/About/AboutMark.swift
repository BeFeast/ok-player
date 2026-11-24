import AppKit

/// The About illustration from the Settings design handoff (124 × 96 viewbox).
final class AboutMark: NSView {
    override var isFlipped: Bool { true }
    override var intrinsicContentSize: NSSize { NSSize(width: 118, height: 94) }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        context.saveGState()
        defer { context.restoreGState() }
        let scale = min(bounds.width / 124, bounds.height / 96)
        context.translateBy(x: (bounds.width - 124 * scale) / 2, y: (bounds.height - 96 * scale) / 2)
        context.scaleBy(x: scale, y: scale)

        effectiveAppearance.performAsCurrentDrawingAppearance {
            let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            let ink = dark ? PaletteRole.link.color : PaletteRole.accent.color
            let bars: [(CGFloat, CGFloat, CGFloat, CGFloat)] = [
                (14, 43.5, 9, 0.10), (22, 41, 14, 0.18), (31, 38, 20, 0.30),
                (40.5, 34, 28, 0.44), (51, 30, 36, 0.62),
            ]
            for (x, y, height, opacity) in bars {
                ink.withAlphaComponent(opacity).setFill()
                NSBezierPath(roundedRect: NSRect(x: x, y: y, width: 5.5, height: height),
                             xRadius: 2.75, yRadius: 2.75).fill()
            }

            let triangle = CGMutablePath()
            triangle.move(to: CGPoint(x: 61, y: 48))
            triangle.addArc(tangent1End: CGPoint(x: 61, y: 18), tangent2End: CGPoint(x: 114, y: 48), radius: 3)
            triangle.addArc(tangent1End: CGPoint(x: 114, y: 48), tangent2End: CGPoint(x: 61, y: 78), radius: 3)
            triangle.addArc(tangent1End: CGPoint(x: 61, y: 78), tangent2End: CGPoint(x: 61, y: 48), radius: 3)
            triangle.closeSubpath()
            context.saveGState()
            context.translateBy(x: 2, y: 7)
            context.addPath(triangle)
            context.setFillColor(ink.withAlphaComponent(0.09).cgColor)
            context.fillPath()
            context.restoreGState()

            context.addPath(triangle)
            context.clip()
            let shade = ink.blended(withFraction: 0.38, of: .black) ?? ink
            NSGradient(starting: ink, ending: shade)?.draw(from: NSPoint(x: 61, y: 18), to: NSPoint(x: 114, y: 78),
                                                          options: [.drawsBeforeStartingLocation, .drawsAfterEndingLocation])
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }
}
