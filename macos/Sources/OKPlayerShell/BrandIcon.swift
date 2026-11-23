import AppKit

enum BrandIcon {
    static let application: NSImage? = {
        guard let file = Bundle.main.object(forInfoDictionaryKey: "CFBundleIconFile") as? String,
              let url = Bundle.main.url(forResource: file, withExtension: nil),
              let image = NSImage(contentsOf: url), image.isValid else { return nil }
        return image
    }()

    static let welcome: NSImage? = Bundle.main.url(forResource: "WelcomeIcon", withExtension: "png")
        .flatMap { NSImage(contentsOf: $0) }
}
