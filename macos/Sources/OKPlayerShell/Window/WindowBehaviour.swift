import AppKit

// The player window's decisions as plain values, so XCTest can check them
// without a window: which key or click asks for fullscreen, what size a resize
// settles on, and what the chrome shows. `PlayerWindowController` applies them
// to AppKit. The fit an open is owed is the session's (`initial_window_fit`).

/// A window operation a key or a click asks for.
enum WindowCommand: Equatable {
    case toggleFullscreen
    /// Leave fullscreen; nothing when the window is not fullscreen.
    case exitFullscreen

    /// Whether the command toggles native fullscreen on a window that is
    /// (`isFullscreen`) or is not fullscreen now.
    func togglesFullscreen(isFullscreen: Bool) -> Bool {
        switch self {
        case .toggleFullscreen: return true
        case .exitFullscreen: return isFullscreen
        }
    }
}

/// The keys the player window handles itself: F toggles fullscreen, Escape
/// leaves it, and Control-Command-F is the system's Enter/Exit Full Screen
/// equivalent (`toggleFullScreen:`).
enum WindowKeys {
    static let escapeKeyCode: UInt16 = 53
    private static let chordModifiers: NSEvent.ModifierFlags = [.command, .control, .option, .shift]

    /// The command for a key press the window received, if it has one.
    static func command(forKey characters: String?, keyCode: UInt16, modifiers: NSEvent.ModifierFlags,
                        isRepeat: Bool, isFullscreen: Bool) -> WindowCommand? {
        guard !isRepeat, modifiers.intersection(chordModifiers).isEmpty else { return nil }
        if keyCode == escapeKeyCode { return isFullscreen ? .exitFullscreen : nil }
        return characters?.lowercased() == "f" ? .toggleFullscreen : nil
    }

    /// The command for a key equivalent (a chord with Command), if it has one.
    static func command(forKeyEquivalent characters: String?, modifiers: NSEvent.ModifierFlags) -> WindowCommand? {
        guard modifiers.intersection(chordModifiers) == [.command, .control],
              characters?.lowercased() == "f" else { return nil }
        return .toggleFullscreen
    }
}

/// The aspect a resize keeps while video shows: the video area's, which is the
/// window content less the room the window's own controls take (`chrome`), not
/// the whole content's. AppKit's `contentAspectRatio` can only lock the whole
/// content, so the window applies this from `windowWillResize(_:to:)`.
enum VideoAreaAspect {
    /// The aspect to keep for `video`: its display size. Nil leaves resizing
    /// free (no video, an audio file, nothing open).
    static func lock(for video: VideoGeometry?) -> CGSize? {
        guard let video, video.width > 0, video.height > 0 else { return nil }
        return CGSize(width: CGFloat(video.width), height: CGFloat(video.height))
    }

    /// The content size a resize from `current` towards `proposed` settles on, so
    /// the video area (content less `chrome`) has the aspect of `video`. The
    /// dimension the user changed more leads and the other follows; neither
    /// goes below `minimum`. Sizes are window content sizes in points.
    static func constrain(proposed: CGSize, current: CGSize, video: CGSize, chrome: CGSize,
                          minimum: CGSize) -> CGSize {
        guard video.width > 0, video.height > 0 else { return proposed }
        let aspect = video.width / video.height
        func height(forWidth width: CGFloat) -> CGFloat { max(1, width - chrome.width) / aspect + chrome.height }
        func width(forHeight height: CGFloat) -> CGFloat { max(1, height - chrome.height) * aspect + chrome.width }
        var size = abs(proposed.width - current.width) >= abs(proposed.height - current.height)
            ? CGSize(width: proposed.width, height: height(forWidth: proposed.width))
            : CGSize(width: width(forHeight: proposed.height), height: proposed.height)
        if size.width < minimum.width { size = CGSize(width: minimum.width, height: height(forWidth: minimum.width)) }
        if size.height < minimum.height {
            size = CGSize(width: width(forHeight: minimum.height), height: minimum.height)
        }
        return CGSize(width: size.width.rounded(), height: size.height.rounded())
    }
}

/// What the window chrome shows. The traffic lights and the title strip hide and
/// show with the OSC; in fullscreen the system owns the title bar, so the lights
/// stay as it shows them and the strip stays hidden.
struct WindowChromeState: Equatable {
    var oscVisible = true
    var fullscreen = false

    var trafficLightsVisible: Bool { oscVisible || fullscreen }
    var titleStripVisible: Bool { oscVisible && !fullscreen }
}

/// The window chrome that follows the transport OSC: the traffic lights and the
/// title strip. The OSC (#879) calls it whenever its own visibility changes;
/// until something does, the chrome stays visible.
protocol WindowChromeVisibility: AnyObject {
    func oscVisibilityChanged(_ visible: Bool, animated: Bool)
}

/// A one-shot timer that can be cancelled before it fires.
protocol ClickTimer: AnyObject {
    func cancel()
}

extension DispatchWorkItem: ClickTimer {}

/// Clicks on the video: one toggles pause, two toggle fullscreen, and a double
/// click never pauses first. Core's `video_click` rules (through the session's
/// `video_click` query) classify AppKit's click count; the shell owns the timer
/// that holds a single click back for the double-click interval.
final class VideoClickArbiter {
    typealias Schedule = (TimeInterval, @escaping () -> Void) -> ClickTimer

    var onTogglePause: (() -> Void)?
    var onToggleFullscreen: (() -> Void)?
    private let interval: () -> TimeInterval
    private let classify: (Int) -> VideoClickIntent
    private let schedule: Schedule
    private var pending: ClickTimer?

    init(interval: @escaping () -> TimeInterval, classify: @escaping (Int) -> VideoClickIntent,
         schedule: @escaping Schedule) {
        self.interval = interval
        self.classify = classify
        self.schedule = schedule
    }

    /// The app's schedule: `body` runs on the main queue after `seconds`, unless
    /// the returned timer is cancelled first.
    static func onMainQueue(after seconds: TimeInterval, _ body: @escaping () -> Void) -> ClickTimer {
        let item = DispatchWorkItem(block: body)
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: item)
        return item
    }

    /// A single click is waiting to toggle pause.
    var pausePending: Bool { pending != nil }

    /// A primary-button release on the video with AppKit's click count.
    func released(clickCount: Int) {
        switch classify(clickCount) {
        case .schedulePlayPause:
            pending?.cancel()
            pending = schedule(interval()) { [weak self] in
                self?.pending = nil
                self?.onTogglePause?()
            }
        case .cancelPlayPauseAndToggleFullscreen:
            pending?.cancel()
            pending = nil
            onToggleFullscreen?()
        case .ignore:
            break
        }
    }
}
