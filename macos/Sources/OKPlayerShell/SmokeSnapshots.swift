import AppKit
import UniformTypeIdentifiers

/// Visual evidence from the relocated preview bundle, using its own AppKit views.
enum SmokeSnapshots {
    static let appearances: [(String, NSAppearance.Name)] = [("light", .aqua), ("dark", .darkAqua)]

    /// The OpenGL video layer is outside AppKit's display cache.
    static func capture(_ window: NSWindow, in directory: String, as prefix: String) {
        guard let view = window.contentView else { return }
        let original = window.appearance
        defer { window.appearance = original }
        for (name, appearanceName) in appearances {
            guard let appearance = NSAppearance(named: appearanceName) else { continue }
            window.appearance = appearance
            view.layoutSubtreeIfNeeded()
            window.displayIfNeeded()
            guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { continue }
            appearance.performAsCurrentDrawingAppearance { view.cacheDisplay(in: view.bounds, to: bitmap) }
            write(bitmap.representation(using: .png, properties: [:]), in: directory, as: "\(prefix)-\(name)")
        }
    }

    /// Finder resolves the icon by bundle path. The Dock takes the app's own icon.
    static func captureIcons(in directory: String) -> Bool {
        let workspace = NSWorkspace.shared
        guard BrandIcon.application != nil, BrandIcon.welcome != nil,
              let dockIcon = NSImage(named: NSImage.applicationIconName),
              let dock = iconPNG(dockIcon),
              let finder = iconPNG(workspace.icon(forFile: Bundle.main.bundlePath)),
              let generic = iconPNG(workspace.icon(for: .applicationBundle)) else { return false }
        guard dock != generic, finder != generic else {
            fputs("OK Player: macOS resolved a generic app icon for the relocated bundle\n", stderr)
            return false
        }
        return write(dock, in: directory, as: "dock-icon-512")
            && write(finder, in: directory, as: "finder-icon-512")
    }

    private static func iconPNG(_ image: NSImage) -> Data? {
        let size = 512
        guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
                                            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let context = NSGraphicsContext(bitmapImageRep: bitmap) else { return nil }
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = context
        bitmap.bitmapData?.initialize(repeating: 0, count: bitmap.bytesPerPlane)
        let rect = NSRect(x: 0, y: 0, width: CGFloat(size), height: CGFloat(size))
        image.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1)
        return bitmap.representation(using: .png, properties: [:])
    }

    @discardableResult
    static func write(_ png: Data?, in directory: String, as name: String) -> Bool {
        let file = URL(fileURLWithPath: directory).appendingPathComponent("\(name).png")
        do {
            guard let png else { throw CocoaError(.fileWriteUnknown) }
            try png.write(to: file)
            print("OKP_SNAPSHOT \(name)=\(file.lastPathComponent)")
            return true
        } catch {
            fputs("OK Player: could not write \(file.lastPathComponent): \(error)\n", stderr)
            return false
        }
    }
}
