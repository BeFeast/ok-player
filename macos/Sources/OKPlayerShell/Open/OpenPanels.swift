import AppKit
import UniformTypeIdentifiers

/// How File > Open and File > Open Folder set up their panel. Both take several
/// items; whatever is chosen goes to the open router as one open.
struct OpenPanelConfiguration: Equatable {
    var canChooseFiles: Bool
    var canChooseDirectories: Bool
    var allowsMultipleSelection = true
    /// Empty offers every file.
    var allowedContentTypes: [UTType] = []

    /// File > Open: media, subtitle and playlist files, of the types the session's
    /// `open_formats` lists. Without them (no session) every file is offered.
    static func files(_ formats: OpenFormats?) -> OpenPanelConfiguration {
        OpenPanelConfiguration(canChooseFiles: true, canChooseDirectories: false,
                               allowedContentTypes: formats.map(contentTypes) ?? [])
    }

    /// File > Open Folder: folders only.
    static let folders = OpenPanelConfiguration(canChooseFiles: false, canChooseDirectories: true)

    /// One content type per listed extension, each once: the type macOS knows for
    /// the extension, or a dynamic one that matches files by it.
    static func contentTypes(_ formats: OpenFormats) -> [UTType] {
        var types: [UTType] = []
        for ext in formats.video + formats.audio + formats.subtitle + formats.playlist {
            guard let type = UTType(filenameExtension: ext), !types.contains(type) else { continue }
            types.append(type)
        }
        return types
    }

    func apply(to panel: NSOpenPanel) {
        panel.prompt = "Open"
        panel.canChooseFiles = canChooseFiles
        panel.canChooseDirectories = canChooseDirectories
        panel.allowsMultipleSelection = allowsMultipleSelection
        panel.allowedContentTypes = allowedContentTypes
    }

    /// Show the panel as a sheet on `window` and hand the chosen items to `open`.
    func present(on window: NSWindow, open: @escaping ([URL]) -> Void) {
        let panel = NSOpenPanel()
        apply(to: panel)
        panel.beginSheetModal(for: window) { response in
            if response == .OK, !panel.urls.isEmpty { open(panel.urls) }
        }
    }
}
