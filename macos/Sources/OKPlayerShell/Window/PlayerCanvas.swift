import AppKit

/// The theme canvas behind the player content.
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
