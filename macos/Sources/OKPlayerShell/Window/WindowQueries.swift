import CoreGraphics
import Foundation

// The window queries of the session protocol (docs/session-abi.md, "Window
// queries"), mirroring rust/crates/okp-session/src/window.rs. The fit and the
// click rules are okp-core's; the shell passes the screen and the click count.

/// A size in whole points.
struct WindowSize: Codable, Equatable {
    var width: Int32
    var height: Int32
}

/// A rectangle in AppKit screen coordinates, in whole points. `x`, `y` is the
/// bottom-left corner.
struct WindowRect: Codable, Equatable {
    var x: Int32
    var y: Int32
    var width: Int32
    var height: Int32

    /// The whole points inside `rect`: fractional edges move inwards, so a frame
    /// that fits this rectangle fits `rect`.
    init(inside rect: CGRect) {
        let minX = rect.minX.rounded(.up), minY = rect.minY.rounded(.up)
        x = Int32(minX)
        y = Int32(minY)
        width = Int32(max(0, rect.maxX.rounded(.down) - minX))
        height = Int32(max(0, rect.maxY.rounded(.down) - minY))
    }

    init(x: Int32, y: Int32, width: Int32, height: Int32) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }

    var cgRect: CGRect {
        CGRect(x: CGFloat(x), y: CGFloat(y), width: CGFloat(width), height: CGFloat(height))
    }
}

/// What a primary-button release on the video means.
enum VideoClickIntent: String, Codable, CaseIterable {
    case ignore
    /// Toggle pause once the double-click interval passes without a second click.
    case schedulePlayPause = "schedule_play_pause"
    /// Drop the pending pause toggle and toggle fullscreen.
    case cancelPlayPauseAndToggleFullscreen = "cancel_play_pause_and_toggle_fullscreen"
}

/// What the player window asks the session: okp-core's fit and click rules.
/// `SessionClient` answers; tests answer with a stand-in.
protocol WindowRules: AnyObject {
    /// The frame (window content, in screen coordinates) of the fit the current
    /// source is owed, once per opened source; nil before its video size is
    /// known, after it was taken, or when the session cannot answer. Asking
    /// takes the fit. `chrome` is the room the window's own controls take
    /// outside the video area.
    func initialWindowFit(backingScale: CGFloat, visibleFrame: CGRect, minSize: CGSize, chrome: CGSize) -> CGRect?
    /// What a release with `clickCount` clicks on the video means.
    func videoClickIntent(clickCount: Int) -> VideoClickIntent
}

extension SessionClient: WindowRules {
    /// The frame of a window fitted to `video` on a screen; nil when there is
    /// nothing to fit or the session cannot answer. `chrome` is the room the
    /// window's own controls take outside the video area: the frame grows by it.
    func windowFit(video: VideoGeometry, backingScale: CGFloat, visibleFrame: CGRect,
                   minSize: CGSize, chrome: CGSize = .zero) -> CGRect? {
        let request = Query.windowFit(
            video: WindowSize(width: video.width, height: video.height),
            backingScale: Double(backingScale),
            visibleFrame: WindowRect(inside: visibleFrame),
            minSize: WindowSize(points: minSize),
            chrome: WindowSize(points: chrome))
        guard case .windowFit(let frame)? = query(request) else { return nil }
        return frame?.cgRect
    }

    func initialWindowFit(backingScale: CGFloat, visibleFrame: CGRect, minSize: CGSize, chrome: CGSize) -> CGRect? {
        let request = Query.initialWindowFit(
            backingScale: Double(backingScale),
            visibleFrame: WindowRect(inside: visibleFrame),
            minSize: WindowSize(points: minSize),
            chrome: WindowSize(points: chrome))
        guard case .windowFit(let frame)? = query(request) else { return nil }
        return frame?.cgRect
    }

    /// What a release with `clickCount` clicks on the video means; `ignore` when
    /// the session cannot answer.
    func videoClickIntent(clickCount: Int) -> VideoClickIntent {
        let count = Int32(clamping: clickCount)
        guard case .videoClick(let intent)? = query(.videoClick(pressCount: count)) else { return .ignore }
        return intent
    }
}

extension WindowSize {
    /// A size in points as whole points, rounded up, so a window of this size is
    /// never smaller than `points`.
    init(points: CGSize) {
        self.init(width: Int32(max(0, points.width).rounded(.up)), height: Int32(max(0, points.height).rounded(.up)))
    }
}
