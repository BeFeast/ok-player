import AppKit

extension VerifyRun {
    func performUpdateStep(_ step: VerificationScenario.Step) async throws {
        guard step.action == .checkUpdate, let name = step.name,
              let port = ProcessInfo.processInfo.environment["OKP_VERIFY_UPDATE_PORT"],
              let key = ProcessInfo.processInfo.environment["OKP_VERIFY_UPDATE_KEY"] else {
            throw VerifyError("Missing signed-update fixture.")
        }
        let updates = app.settings.updates
        updates.fixture = PlayerUpdates.Fixture(port: port, key: key)
        app.settings.show(page: .updates, rememberFrame: false)
        guard let window = app.settings.window else { throw VerifyError("Updates Settings did not open.") }
        guard window.selectedPage == .updates else { throw VerifyError("The Updates rail did not select its page.") }
        window.displayIfNeeded()

        func button(_ title: String, in view: NSView?) -> NSButton? {
            guard let view else { return nil }
            if let button = view as? NSButton, button.title == title { return button }
            return view.subviews.lazy.compactMap { button(title, in: $0) }.first
        }
        guard let check = button("Check for Updates", in: window.contentView), check.isEnabled else {
            throw VerifyError("The packaged updater is unavailable.")
        }
        check.performClick(nil)
        try await wait("signed update response") { updates.lastCheck != nil && !updates.checking }
        let appearance: NSAppearance.Name = options.appearance == "dark" ? .darkAqua : .aqua
        guard window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == appearance else {
            throw VerifyError("Updates page did not show the requested appearance.")
        }
        let mode = try await captureSettingsImage(window, name: name)
        observations.append(["name": name, "png": "\(name).png", "capture": mode,
                             "effective_appearance": appearance.rawValue])
        if options.negativeControl {
            guard updates.offer == nil,
                  updates.problem?.contains("Update signature could not be verified") == true,
                  button("Install & Relaunch", in: window.contentView) == nil else {
                throw VerifyError("A bad manifest signature was not refused.")
            }
            return
        }
        guard let offer = updates.offer, offer.build != updates.build,
              let install = button("Install & Relaunch", in: window.contentView), install.isEnabled else {
            throw VerifyError(updates.problem ?? "No newer signed build was actionable.")
        }
        updates.willRelaunch = { [self] in
            try? options.write([
                "status": "passed", "control": options.control, "appearance": options.appearance,
                "source_sha": Bundle.main.object(forInfoDictionaryKey: "OKPSourceSHA") as? String ?? "",
                "pid": getpid(), "checkpoints": observations, "assertion_failures": [],
                "staged_build": offer.build, "staged_sha256": offer.sha256,
            ])
        }
        install.performClick(nil)
        let timeout = Date().addingTimeInterval(85)
        while updates.installing && Date() < timeout {
            try await Task.sleep(nanoseconds: 100_000_000)
        }
        throw VerifyError(updates.problem ?? "The staged build did not relaunch.")
    }
}
