import AppKit

// The player window's decisions as plain values, so XCTest can check them
// without a window: which key or click asks for fullscreen, when a fit is owed,
// what the aspect lock is, and what the chrome shows. `PlayerWindowController`
// applies them to AppKit.

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

/// The fit one open is owed, as core's `InitialFitState` keeps it per source:
/// armed when a file is opened, spent on the first video size the session
/// reports for it. Later size changes and audio-only files never refit.
struct InitialWindowFit {
    private(set) var armed = false

    mutating func beginOpen() { armed = true }

    /// The video to fit the window to, once per open.
    mutating func take(_ snapshot: SessionSnapshot) -> VideoGeometry? {
        guard armed, let video = snapshot.video, video.width > 0, video.height > 0 else { return nil }
        armed = false
        return video
    }
}

enum WindowAspect {
    /// The content aspect ratio a resize keeps while `video` shows: its display
    /// size. Nil unlocks it (no video, an audio file, nothing open).
    static func lock(for video: VideoGeometry?) -> CGSize? {
        guard let video, video.width > 0, video.height > 0 else { return nil }
        return CGSize(width: CGFloat(video.width), height: CGFloat(video.height))
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
