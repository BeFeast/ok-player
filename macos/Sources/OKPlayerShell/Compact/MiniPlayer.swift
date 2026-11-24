import AppKit

/// A layout of the existing player window and renderer, with one saved
/// standard frame and one owner of compact pointer interactions.
final class MiniPlayer: NSObject {
    let controls = MiniPlayerControls()
    weak var session: SessionClient?
    var onModeChange: ((Bool) -> Void)?
    var showCommands: ((NSPoint) -> Void)?
    var toast: ((String) -> Void)?
    var accessibilityOverride: MiniPlayerAccessibility? {
        didSet { updateAccessibility() }
    }
    var snapsToCorners = true
    var liftsSubtitles = true

    private weak var controller: PlayerWindowController?
    private weak var video: NSView?
    private weak var root: NSView?
    private let autoHide = AutoHide()
    private var snapshot = SessionSnapshot()
    private var standard: StandardWindow?
    private var aspectVideo: VideoGeometry?
    private var scroll = VolumeScrollAccumulator()
    private var scrub = ScrubState()
    private var pointerPin = false
    private var contextPin = false
    private var clearance: CGSize?
    private var drag: Drag?

    private struct StandardWindow {
        let frame: CGRect
        let minimum: CGSize
        let style: NSWindow.StyleMask
        let behavior: NSWindow.CollectionBehavior
        let background: NSColor
        let opaque: Bool
        let shadow: Bool
    }

    private struct Edges: OptionSet {
        let rawValue: Int
        static let left = Self(rawValue: 1)
        static let right = Self(rawValue: 2)
        static let bottom = Self(rawValue: 4)
        static let top = Self(rawValue: 8)

        var cursor: NSCursor {
            if #available(macOS 15.0, *) {
                if contains(.left) && contains(.top) || contains(.right) && contains(.bottom) {
                    return .frameResize(position: .topLeft, directions: .all)
                }
                if contains(.right) && contains(.top) || contains(.left) && contains(.bottom) {
                    return .frameResize(position: .topRight, directions: .all)
                }
            }
            return contains(.left) || contains(.right) ? .resizeLeftRight : .resizeUpDown
        }
    }

    private struct Drag {
        let start: CGPoint
        let frame: CGRect
        let edges: Edges
        var moved = false
    }

    var isActive: Bool { standard != nil }
    private var window: PlayerWindow? { controller?.window }
    private var visibleFrame: CGRect? { window?.screen?.visibleFrame ?? NSScreen.main?.visibleFrame }

    override init() {
        super.init()
        autoHide.onChange = { [weak self] visible in self?.present(visible, animated: true) }
        controls.onLayout = { [weak self] in self?.updateSubtitleClearance() }
        controls.seek.onScrub = { [weak self] phase, fraction in self?.scrubbed(phase, fraction) }
        controls.seek.onAccessibilitySeek = { [weak self] fraction in self?.seekExactly(to: fraction) }
        for (button, action) in [(controls.expand, #selector(restore)), (controls.closeMedia, #selector(closeMedia)),
                                 (controls.play, #selector(togglePause)), (controls.mute, #selector(toggleMute)),
                                 (controls.more, #selector(openCommands))] {
            button.target = self
            button.action = action
        }
        NSWorkspace.shared.notificationCenter.addObserver(
            self, selector: #selector(accessibilityChanged(_:)),
            name: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(displaysChanged(_:)),
                                               name: NSApplication.didChangeScreenParametersNotification, object: nil)
    }

    deinit {
        NSWorkspace.shared.notificationCenter.removeObserver(self)
        NotificationCenter.default.removeObserver(self)
    }

    func install(in root: NSView, video: NSView, above osc: NSView, controller: PlayerWindowController) {
        self.controller = controller
        self.root = root
        self.video = video
        controller.onExitMiniPlayer = { [weak self] in self?.restore() }
        root.wantsLayer = true
        root.addTrackingArea(NSTrackingArea(rect: .zero,
                                            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                                            owner: root, userInfo: nil))
        controls.autoresizingMask = [.width, .height]
        controls.frame = root.bounds
        root.addSubview(controls, positioned: .above, relativeTo: osc)
    }

    func toggle() {
        if isActive { restore() } else { enter() }
    }

    private func enter() {
        guard let controller, controller.canEnterMiniPlayer, let window, let root,
              let video = snapshot.video, let area = visibleFrame,
              let session, let size = session.compactSize(for: video, maximum: area.size) else { return }

        let origin = CGPoint(x: area.maxX - size.width, y: area.minY)
        let proposed = CGRect(origin: origin, size: size)
        let frame = session.compactPlacement(of: proposed, inside: area, snap: true) ?? proposed
        standard = StandardWindow(frame: window.frame, minimum: window.minSize, style: window.styleMask,
                                  behavior: window.collectionBehavior, background: window.backgroundColor ?? .windowBackgroundColor,
                                  opaque: window.isOpaque, shadow: window.hasShadow)
        aspectVideo = video
        window.isMiniPlayer = true
        onModeChange?(true)
        changeStyle(of: window, to: .borderless)
        window.minSize = NSSize(width: 1, height: 1)
        window.collectionBehavior.remove(.fullScreenPrimary)
        window.collectionBehavior.insert(.fullScreenNone)
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = true
        setRounded(true, in: root)
        controller.miniPlayerChanged()
        window.setFrame(frame, display: true, animate: !controls.accessibility.reduceMotion)
        window.makeFirstResponder(self.video)
        autoHide.activity()
        refresh()
        present(true, animated: false)
        root.layoutSubtreeIfNeeded()
        window.invalidateShadow()
        (self.video as? VideoView)?.redraw()
    }

    @objc func restore() {
        guard let standard, let window, let root else { return }
        // Drop the compact geometry and callbacks before laying out the
        // standard planes. The standard OSC then owns subtitle clearance.
        self.standard = nil
        drag = nil
        aspectVideo = nil
        clearance = nil
        cancelScrub()
        pointerPin = false
        contextPin = false
        autoHide.unpinAll()
        autoHide.update(allowed: false, hideAfter: 0)
        present(false, animated: false)
        NSCursor.arrow.set()
        window.isMiniPlayer = false
        changeStyle(of: window, to: standard.style)
        window.collectionBehavior = standard.behavior
        window.backgroundColor = standard.background
        window.isOpaque = standard.opaque
        window.hasShadow = standard.shadow
        window.minSize = standard.minimum
        setRounded(false, in: root)
        controller?.miniPlayerChanged()

        let screen = NSScreen.screens.max {
            overlap(standard.frame, $0.frame) < overlap(standard.frame, $1.frame)
        }.flatMap { overlap(standard.frame, $0.frame) > 0 ? $0 : nil }
        let area = screen?.visibleFrame ?? visibleFrame
        let frame = area.flatMap { session?.compactPlacement(of: standard.frame, inside: $0, snap: false) }
            ?? standard.frame
        window.setFrame(frame, display: true, animate: !controls.accessibility.reduceMotion)
        window.makeFirstResponder(video)
        onModeChange?(false)
        root.layoutSubtreeIfNeeded()
        window.invalidateShadow()
        (video as? VideoView)?.redraw()
    }

    private func changeStyle(of window: PlayerWindow, to style: NSWindow.StyleMask) {
        let content = window.contentView
        window.styleMask = style
        // AppKit can rebuild the frame view when the title bar disappears.
        if window.contentView !== content { window.contentView = content }
    }

    private func setRounded(_ rounded: Bool, in root: NSView) {
        root.layer?.cornerRadius = rounded ? 14 : 0
        root.layer?.masksToBounds = rounded
    }

    private func overlap(_ lhs: CGRect, _ rhs: CGRect) -> CGFloat {
        let rect = lhs.intersection(rhs)
        return rect.isNull ? 0 : rect.width * rect.height
    }

    @objc private func accessibilityChanged(_ notification: Notification) { updateAccessibility() }

    @objc private func displaysChanged(_ notification: Notification) {
        guard isActive, let window else { return }
        sessionChanged(snapshot)
        settle(window, snap: false)
    }

    private func updateAccessibility() {
        controls.accessibility = accessibilityOverride ?? .system
    }

    func noteActivity() {
        if isActive { autoHide.activity() }
    }

    func setContextShown(_ shown: Bool) {
        guard isActive, shown != contextPin else { return }
        contextPin = shown
        if shown && pointerPin {
            pointerPin = false
            autoHide.unpin()
        }
        if shown { autoHide.pin() } else { autoHide.unpin() }
    }

    func sessionChanged(_ snapshot: SessionSnapshot) {
        if snapshot.source != self.snapshot.source ||
            (snapshot.status == .opening && self.snapshot.status != .opening) {
            cancelScrub()
            controller?.cancelVideoClick()
            clearance = nil
        }
        self.snapshot = snapshot
        guard isActive else { return }
        if snapshot.source == nil {
            restore()
            return
        }
        if let video = snapshot.video, video != aspectVideo, let window, let area = visibleFrame {
            let original = window.frame
            if let size = session?.compactSize(for: video, maximum: area.size),
               let frame = session?.compactPlacement(
                of: CGRect(x: original.midX - size.width / 2, y: original.midY - size.height / 2,
                           width: size.width, height: size.height), inside: area, snap: true) {
                aspectVideo = video
                window.setFrame(frame, display: true)
            }
        }
        refresh()
    }

    private func refresh() {
        guard isActive, case .transport(let view)? = session?.query(
            .transport(at: scrub.dragPosition(duration: snapshot.duration))) else { return }
        if !view.seekable { cancelScrub() }
        let title = snapshot.title ?? snapshot.source.map { ($0.location as NSString).lastPathComponent } ?? ""
        controls.hasSubtitles = snapshot.tracks.contains { $0.kind == .subtitle && $0.selected }
        controls.update(view, title: title, playing: snapshot.status == .playing)
        autoHide.update(allowed: view.osc.autoHide, hideAfter: view.osc.hideAfter)
        updateSubtitleClearance()
    }

    private func present(_ visible: Bool, animated: Bool) {
        controls.present(isActive && visible, animated: animated)
        if !visible, let focused = window?.firstResponder as? NSView, focused.isDescendant(of: controls) {
            window?.makeFirstResponder(video)
        }
        updateSubtitleClearance()
    }

    private func updateSubtitleClearance() {
        guard isActive, controls.bounds.height > 0 else { return }
        let height = controls.bounds.height
        let bottom = liftsSubtitles ? controls.subtitleClearance : 0
        let update = CGSize(width: bottom, height: height)
        guard clearance != update else { return }
        if session?.setSubtitleClearance(surfaceHeight: height, obscuredBottom: bottom) == true {
            clearance = update
        }
    }

    @objc private func closeMedia() { session?.dispatch(.close) }
    @objc private func togglePause() { session?.dispatch(.togglePause) }
    @objc private func toggleMute() { session?.dispatch(.toggleMute) }
    @objc private func openCommands() {
        showCommands?(controls.more.convert(NSPoint(x: controls.more.bounds.midX, y: controls.more.bounds.maxY), to: nil))
    }

    private func seekExactly(to fraction: Double) {
        guard controls.seek.isEnabled, let duration = snapshot.duration else { return }
        session?.dispatch(.seek(seconds: duration * fraction, mode: .absolute, precision: .exact))
    }

    private func cancelScrub() {
        if scrub.isDragging { autoHide.unpin() }
        scrub.cancel()
    }

    private func scrubbed(_ phase: ScrubberView.Phase, _ fraction: Double) {
        let duration = controls.seek.isEnabled ? snapshot.duration : nil
        let command: Command?
        switch phase {
        case .began:
            cancelScrub()
            command = scrub.begin(at: fraction, duration: duration)
            if scrub.isDragging { autoHide.pin() }
        case .moved:
            let wasDragging = scrub.isDragging
            command = scrub.move(to: fraction, duration: duration)
            if wasDragging && !scrub.isDragging { autoHide.unpin() }
        case .ended:
            let wasDragging = scrub.isDragging
            command = scrub.end(at: fraction, duration: duration)
            if wasDragging { autoHide.unpin() }
        }
        if let command { session?.dispatch(command) }
        refresh()
    }

    func handle(_ event: NSEvent) -> Bool {
        guard isActive, let window, let root, let video, let host = root.superview else { return false }
        let point = event.windowNumber == window.windowNumber ? event.locationInWindow
            : window.convertPoint(fromScreen: event.locationInWindow)
        let screenPoint = window.convertPoint(toScreen: point)
        let edges = resizeEdges(at: root.convert(point, from: nil), in: root.bounds)
        switch event.type {
        case .scrollWheel:
            guard let hit = root.hitTest(host.convert(point, from: nil)),
                  hit === video || hit.isDescendant(of: controls) else { return false }
            adjustVolume(event)
            return true
        case .mouseEntered, .mouseMoved:
            noteActivity()
            let overControl = controls.interactive(at: point) != nil
            if overControl != pointerPin {
                pointerPin = overControl
                if overControl { autoHide.pin() } else { autoHide.unpin() }
            }
            (edges.isEmpty || overControl ? NSCursor.arrow : edges.cursor).set()
        case .mouseExited:
            if pointerPin {
                pointerPin = false
                autoHide.unpin()
            }
        case .leftMouseDown:
            noteActivity()
            let edgeGrab = !edges.isEmpty && controls.interactive(at: point) == nil
            guard edgeGrab || root.hitTest(host.convert(point, from: nil)) === video else { return false }
            window.makeKey()
            window.makeFirstResponder(video)
            drag = Drag(start: screenPoint, frame: window.frame, edges: edges)
            return true
        case .leftMouseDragged:
            guard var drag else { return false }
            let delta = CGPoint(x: screenPoint.x - drag.start.x, y: screenPoint.y - drag.start.y)
            if !drag.moved {
                guard hypot(delta.x, delta.y) >= 6 else { return true }
                drag.moved = true
                self.drag = drag
                controller?.cancelVideoClick()
            }
            noteActivity()
            let frame = drag.edges.isEmpty ? drag.frame.offsetBy(dx: delta.x, dy: delta.y)
                : resized(drag, by: delta, on: visibleFrame)
            window.setFrame(frame, display: true)
            return true
        case .leftMouseUp:
            guard let drag else { return false }
            self.drag = nil
            if drag.moved { settle(window, snap: drag.edges.isEmpty) }
            else if drag.edges.isEmpty { controller?.compactVideoClick(count: event.clickCount) }
            return true
        default:
            break
        }
        return false
    }

    private func resizeEdges(at point: CGPoint, in bounds: CGRect) -> Edges {
        guard bounds.contains(point) else { return [] }
        let grab: CGFloat = 8
        var edges: Edges = []
        if point.x < bounds.minX + grab { edges.insert(.left) }
        if point.x > bounds.maxX - grab { edges.insert(.right) }
        if point.y < bounds.minY + grab { edges.insert(.bottom) }
        if point.y > bounds.maxY - grab { edges.insert(.top) }
        return edges
    }

    private func resized(_ drag: Drag, by delta: CGPoint, on area: CGRect?) -> CGRect {
        let old = drag.frame
        let width = old.width + (drag.edges.contains(.left) ? -delta.x : drag.edges.contains(.right) ? delta.x : 0)
        let height = old.height + (drag.edges.contains(.bottom) ? -delta.y : drag.edges.contains(.top) ? delta.y : 0)
        guard let area, let video = aspectVideo,
              let size = session?.compactSize(for: video, maximum: area.size,
                                              proposed: CGSize(width: width, height: height), current: old.size) else {
            return old
        }
        let x = drag.edges.contains(.left) ? old.maxX - size.width
            : drag.edges.contains(.right) ? old.minX : old.midX - size.width / 2
        let y = drag.edges.contains(.bottom) ? old.maxY - size.height
            : drag.edges.contains(.top) ? old.minY : old.midY - size.height / 2
        return CGRect(origin: CGPoint(x: x, y: y), size: size)
    }

    private func settle(_ window: PlayerWindow, snap: Bool) {
        guard let area = visibleFrame, let session else { return }
        var proposed = window.frame
        if let video = aspectVideo, proposed.width > area.width || proposed.height > area.height,
           let size = session.compactSize(for: video, maximum: area.size, proposed: proposed.size, current: proposed.size) {
            proposed.size = size
        }
        guard let frame = session.compactPlacement(of: proposed, inside: area, snap: snap && snapsToCorners),
              frame != window.frame else { return }
        window.setFrame(frame, display: true, animate: !controls.accessibility.reduceMotion)
    }

    private func adjustVolume(_ event: NSEvent) {
        let input = ScrollInput(event: event)
        if input.deltaY != 0 && !input.momentum { noteActivity() }
        guard case .nudgeVolume(let step)? = VolumeScrollAccumulator.command(steps: scroll.steps(for: input)),
              let session else { return }
        session.dispatch(.nudgeVolume(delta: event.modifierFlags.contains(.shift) ? step / 10 : step))
        if case .transport(let view)? = session.query(.transport(at: nil)) { toast?(view.volume.readout) }
    }
}
