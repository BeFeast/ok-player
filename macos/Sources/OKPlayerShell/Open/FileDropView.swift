import AppKit

/// The window's drop target. It holds the window's whole content, so files dropped
/// anywhere over the window (the video, the controls, the title strip) arrive here:
/// AppKit hands a drag to the nearest view up the hierarchy that registered for
/// its type, and none of the content views does.
///
/// While a drag the router accepts is over the window, a highlight frames the
/// content. What a drop does is the router's: `accepts` and `perform` are its
/// `accepts(_:)` and the app's open.
final class FileDropView: NSView {
    /// Whether dropping these files would do anything.
    var accepts: ([URL]) -> Bool = { _ in false }
    /// Opens dropped files; returns whether anything was opened.
    var perform: ([URL]) -> Bool = { _ in false }
    let highlight = DropHighlightView()

    init(content: NSView) {
        super.init(frame: .zero)
        for view in [content, highlight] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
            NSLayoutConstraint.activate([
                view.topAnchor.constraint(equalTo: topAnchor), view.bottomAnchor.constraint(equalTo: bottomAnchor),
                view.leadingAnchor.constraint(equalTo: leadingAnchor), view.trailingAnchor.constraint(equalTo: trailingAnchor),
            ])
        }
        highlight.isHidden = true
        registerForDraggedTypes([.fileURL])
    }

    required init?(coder: NSCoder) { fatalError("Use init(content:)") }

    // AppKit's drag messages; each hands the dragged files to the methods below.
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        enter(DropPayload.urls(from: sender.draggingPasteboard))
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        highlight.isHidden ? [] : .copy
    }

    override func draggingExited(_ sender: NSDraggingInfo?) { leave() }

    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool { !highlight.isHidden }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        drop(DropPayload.urls(from: sender.draggingPasteboard))
    }

    override func draggingEnded(_ sender: NSDraggingInfo) { leave() }

    /// Files entered the window: copy them and show the highlight if a drop would
    /// do anything, else refuse them.
    func enter(_ urls: [URL]) -> NSDragOperation {
        let accepted = accepts(urls)
        highlight.isHidden = !accepted
        return accepted ? .copy : []
    }

    /// The drag left the window or ended.
    func leave() {
        highlight.isHidden = true
    }

    /// Files were dropped: open them.
    func drop(_ urls: [URL]) -> Bool {
        leave()
        return perform(urls)
    }
}

/// The frame shown while files the player accepts are dragged over the window: an
/// accent outline over a light accent wash. It takes no clicks.
final class DropHighlightView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        let frame = NSBezierPath(roundedRect: bounds.insetBy(dx: 6, dy: 6), xRadius: 10, yRadius: 10)
        PaletteRole.accent.color.withAlphaComponent(0.12).setFill()
        frame.fill()
        frame.lineWidth = 3
        PaletteRole.accent.color.setStroke()
        frame.stroke()
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
