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

extension SessionClient {
    /// The frame of a window fitted to `video` on a screen; nil when there is
    /// nothing to fit or the session cannot answer.
    func windowFit(video: VideoGeometry, backingScale: CGFloat, visibleFrame: CGRect,
                   minSize: CGSize) -> CGRect? {
        let request = Query.windowFit(
            video: WindowSize(width: video.width, height: video.height),
            backingScale: Double(backingScale),
            visibleFrame: WindowRect(inside: visibleFrame),
            minSize: WindowSize(width: Int32(minSize.width.rounded(.up)), height: Int32(minSize.height.rounded(.up))))
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
