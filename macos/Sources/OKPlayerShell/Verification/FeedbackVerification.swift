import AppKit

@MainActor
extension VerifyRun {
    func performFeedbackStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .open, .pause:
            try await performWindowStep(step)
        case .feedbackOpen:
            guard let help = app.menuBar?.menus[.help],
                  let item = help.items.first(where: { $0.playerCommandID == "send-feedback" }),
                  let index = help.items.firstIndex(of: item) else {
                throw VerifyError("Help > Send Feedback… is unavailable")
            }
            help.performActionForItem(at: index)
            try await wait("Feedback opened as a native sheet") {
                guard let panel = self.app.feedback else { return false }
                return panel.window.isVisible && self.app.window.attachedSheet === panel.window
                    && panel.window.firstResponder === panel.descriptionView
            }
            guard let panel = app.feedback else { throw VerifyError("Feedback disappeared") }
            panel.descriptionView.insertText("OK Player verifier — \(options.appearance)", replacementRange: NSRange(location: 0, length: 0))
            panel.captureButton.performClick(nil)
            guard let png = panel.screenshot, NSImage(data: png)?.isValid == true else {
                throw VerifyError("Capture Player Window did not attach a PNG")
            }
            try png.write(to: options.output.appendingPathComponent("attached-player.png"))
        case .capture:
            guard let name = step.name, let panel = app.feedback, let root = panel.window.contentView else {
                throw VerifyError("The Feedback sheet disappeared")
            }
            let appearance = panel.window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua])
            guard appearance == (options.appearance == "dark" ? .darkAqua : .aqua) else {
                throw VerifyError("The Feedback sheet did not use the requested appearance")
            }
            root.layoutSubtreeIfNeeded()
            panel.window.displayIfNeeded()
            guard let bitmap = root.bitmapImageRepForCachingDisplay(in: root.bounds) else {
                throw VerifyError("Could not allocate a Feedback sheet bitmap")
            }
            root.effectiveAppearance.performAsCurrentDrawingAppearance { root.cacheDisplay(in: root.bounds, to: bitmap) }
            let png = bitmap.representation(using: .png, properties: [:])
            guard SmokeSnapshots.write(png, in: options.output.path, as: name) else {
                throw VerifyError("Could not save Feedback sheet evidence")
            }
            observations.append(["name": name, "png": "\(name).png", "appearance": options.appearance])
        case .feedbackSend:
            guard let panel = app.feedback else { throw VerifyError("Feedback was closed before Send") }
            panel.sendButton.performClick(nil)
            try await wait("Feedback displayed the loopback issue receipt") {
                panel.issue?.issueNumber == 123 && panel.sendButton.title == "Open Issue"
            }
        default:
            throw VerifyError("Unsupported Feedback action \(step.action.rawValue)")
        }
    }
}
