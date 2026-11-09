import Foundation
import IOKit.pwr_mgt

/// Power assertions: IOKit in the app, a fake in tests.
protocol PowerAssertions: AnyObject {
    /// Keep the display from sleeping while the user is idle. `nil` when refused.
    func preventDisplaySleep(reason: String) -> UInt32?
    func release(_ assertion: UInt32)
}

/// `IOPMAssertionCreateWithName` with `kIOPMAssertionTypePreventUserIdleDisplaySleep`.
final class SystemPowerAssertions: PowerAssertions {
    func preventDisplaySleep(reason: String) -> UInt32? {
        var assertion = IOPMAssertionID(0)
        let result = IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleDisplaySleep as CFString,
                                                 IOPMAssertionLevel(kIOPMAssertionLevelOn), reason as CFString, &assertion)
        return result == kIOReturnSuccess ? assertion : nil
    }

    func release(_ assertion: UInt32) {
        IOPMAssertionRelease(assertion)
    }

    /// The type of a live assertion, `nil` once it is released.
    static func type(of assertion: UInt32) -> String? {
        guard let copied = IOPMAssertionCopyProperties(assertion) else { return nil }
        let properties = copied.takeRetainedValue() as NSDictionary
        return properties[kIOPMAssertionTypeKey as String] as? String
    }

    static var displaySleepType: String { kIOPMAssertionTypePreventUserIdleDisplaySleep as String }
}

/// Holds one display-sleep assertion while video plays and releases it on pause, end,
/// close, audio-only media and shutdown.
final class DisplaySleepGuard {
    static let reason = "OK Player is playing video"

    private let power: PowerAssertions
    private(set) var assertion: UInt32?
    private var keepingAwake = false

    init(power: PowerAssertions) {
        self.power = power
    }

    /// Video is playing (buffering included).
    static func keepsDisplayAwake(_ snapshot: SessionSnapshot) -> Bool {
        snapshot.status == .playing && snapshot.source != nil && snapshot.mediaKind == .video
    }

    func update(_ snapshot: SessionSnapshot) {
        set(keepAwake: DisplaySleepGuard.keepsDisplayAwake(snapshot))
    }

    /// Acts only on a change, so a refused assertion is tried again at the next
    /// start of playback, not on every update.
    func set(keepAwake: Bool) {
        guard keepAwake != keepingAwake else { return }
        keepingAwake = keepAwake
        if keepAwake {
            assertion = power.preventDisplaySleep(reason: DisplaySleepGuard.reason)
        } else if let held = assertion {
            power.release(held)
            assertion = nil
        }
    }

    func release() {
        set(keepAwake: false)
    }
}
