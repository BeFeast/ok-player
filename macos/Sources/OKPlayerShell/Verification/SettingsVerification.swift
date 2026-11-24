import AppKit

@MainActor
extension VerifyRun {
    func performSettingsStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .settingsShortcut:
            app.window.makeKeyAndOrderFront(nil)
            guard let key = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .command,
                                            timestamp: ProcessInfo.processInfo.systemUptime,
                                            windowNumber: app.window.windowNumber, context: nil, characters: ",",
                                            charactersIgnoringModifiers: ",", isARepeat: false, keyCode: 43),
                  NSApp.mainMenu?.performKeyEquivalent(with: key) == true else {
                throw VerifyError("Cmd+, was not handled by the AppKit menu")
            }
            try await wait("the native Settings window is visible") { self.app.settings.window?.isVisible == true }
        case .settingsContext:
            let before = try settingsWindow()
            before.close()
            app.window.makeKeyAndOrderFront(nil)
            let welcome = app.history.welcomeView
            let background = welcome.convert(NSPoint(x: 12, y: welcome.bounds.midY), to: nil)
            let click = try settingsContextClick(at: background)
            guard !welcome.isHidden, let player = app.windowBehaviour?.window,
                  let menu = player.settingsMenu(for: click),
                  let item = menu.items.firstIndex(where: { $0.playerCommandID == "open-settings" }) else {
                throw VerifyError("The player context menu has no Settings action")
            }
            menu.performActionForItem(at: item)
            checkSettings(app.settings.window === before && before.isVisible
                          && NSApp.windows.filter { $0 is SettingsWindow && $0.isVisible }.count == 1,
                          "single_window")
        case .settingsLegacy:
            let window = try settingsWindow()
            checkSettings(window.shownTheme == .dark && window.themeControl.selectedSegment == -1
                          && settingsAppearance(window) == .darkAqua, "legacy_dark")
        case .settingsSearch:
            try await searchSettings()
        case .settingsResize:
            try resizeSettings()
        case .settingsSystem:
            let window = try settingsWindow()
            let current = settingsAppearance(window)
            let changed: NSAppearance.Name = current == .darkAqua ? .aqua : .darkAqua
            let initial = app.settings.autoAppearance
            app.settings.autoAppearance = NSAppearance(named: changed)
            checkSettings(settingsAppearance(window) == changed && window.shownTheme == .auto, "auto_follows_system")
            app.settings.autoAppearance = initial
        case .settingsLight, .settingsAuto:
            let window = try settingsWindow()
            try selectAppearancePage(window)
            if step.action == .settingsLight && options.control == "appearance_persistence_disabled" {
                // Keep the live native effect and disconnect only the write to the shared profile.
                app.settings.writeTheme = { selection in .success(selection == .light ? .light : .auto) }
            }
            window.themeControl.selectedSegment = step.action == .settingsLight ? 0 : 1
            guard NSApp.sendAction(window.themeControl.action!, to: window.themeControl.target,
                                   from: window.themeControl) else {
                throw VerifyError("The Settings theme control is not connected")
            }
        case .settingsPlayerInput:
            try await verifySettingsPlayerInput()
        case .capture:
            guard let name = step.name, let expected = step.expect else { throw VerifyError("Missing Settings checkpoint") }
            try await captureSettings(name, expected: expected)
        default:
            throw VerifyError("Unsupported Settings action")
        }
    }

    private func searchSettings() async throws {
        let window = try settingsWindow()
        let before = app.keyPresses
        guard window.makeFirstResponder(window.searchField),
              let editor = window.searchField.currentEditor() as? NSTextView else {
            throw VerifyError("The native Settings search field did not take focus")
        }
        editor.insertText("appearance", replacementRange: editor.selectedRange())
        try await wait("search finds Appearance") { window.resultButtons.contains { $0.page == .appearance } }
        editor.insertText("no such preference", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        try await wait("search clears stale results for unmatched text") { window.resultButtons.isEmpty }
        editor.insertText("history retention", replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
        try await wait("a shared cross-page search result") {
            window.resultButtons.contains { $0.title == "History retention — Integration" }
        }
        window.resultButtons.first { $0.page == .integration }?.performClick(nil)
        checkSettings(window.selectedPage == .integration && window.searchField.stringValue.isEmpty
                      && app.keyPresses == before, "search_navigation_and_focus")
        window.aboutButton.performClick(nil)
        checkSettings(window.selectedPage == .about && window.aboutButton.state == .on, "about_route")
        try selectAppearancePage(window)
    }

    private func selectAppearancePage(_ window: SettingsWindow) throws {
        guard let appearance = window.railButtons.first(where: { $0.page == .appearance }) else {
            throw VerifyError("Appearance is missing from the settings rail")
        }
        appearance.performClick(nil)
    }

    private func resizeSettings() throws {
        let window = try settingsWindow()
        guard let root = window.contentView, let screen = window.screen else {
            throw VerifyError("Settings is not attached to a screen", blocked: true)
        }
        let originalSize = root.bounds.size
        let originalOrigin = window.frame.origin
        window.setContentSize(NSSize(width: 680, height: 480))
        window.setFrameOrigin(NSPoint(x: screen.visibleFrame.minX + 20, y: screen.visibleFrame.minY + 30))
        root.layoutSubtreeIfNeeded()
        let aboutFrame = window.aboutButton.convert(window.aboutButton.bounds, to: root)
        checkSettings(root.bounds.width == 680 && root.bounds.height == 480 && window.isMovable
                      && window.frame.minX == screen.visibleFrame.minX + 20 && abs(aboutFrame.minY - 11) < 1,
                      "resizable_movable_anchored_about")
        window.setContentSize(originalSize)
        window.setFrameOrigin(originalOrigin)
    }

    private func verifySettingsPlayerInput() async throws {
        let settings = try settingsWindow()
        settings.close()
        app.window.makeKeyAndOrderFront(nil)
        let before = swaps
        app.application(NSApp, open: [options.fixture])
        try await wait("video still plays after editing Settings") {
            self.snapshot.source?.location == self.options.fixture.path &&
            self.snapshot.status == .playing && self.swaps > before
        }
        guard let video = app.video else { throw VerifyError("Playback has no video view") }
        let click = try settingsContextClick(at: video.convert(NSPoint(x: video.bounds.midX, y: video.bounds.midY), to: nil))
        guard let menu = video.menu(for: click),
              let item = menu.items.firstIndex(where: { $0.playerCommandID == "open-settings" }) else {
            throw VerifyError("The playback context menu has no Settings action")
        }
        menu.performActionForItem(at: item)
        checkSettings(app.settings.window === settings && settings.isVisible, "playback_context")
        settings.close()
        app.window.makeKeyAndOrderFront(nil)
        try pressKey(" ", code: 49)
        try await wait("Space reaches the native player responder after Settings") {
            self.snapshot.status == .paused
        }
        checkSettings(settingsAppearance(app.window) == settingsAppearance(settings)
                      && app.osc.panel.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua,
                      "fixed_over_video_appearance")
    }

    private func settingsContextClick(at point: NSPoint) throws -> NSEvent {
        guard let click = NSEvent.mouseEvent(with: .rightMouseDown, location: point, modifierFlags: [],
                                             timestamp: ProcessInfo.processInfo.systemUptime,
                                             windowNumber: app.window.windowNumber, context: nil,
                                             eventNumber: 0, clickCount: 1, pressure: 1) else {
            throw VerifyError("Could not make a native context click")
        }
        return click
    }

    private func captureSettings(_ name: String, expected: VerificationScenario.Expectation) async throws {
        let window = try settingsWindow()
        guard let root = window.contentView else { throw VerifyError("Settings has no content view") }
        _ = window.makeFirstResponder(nil)
        root.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        let shown = settingsAppearance(window)
        let isLight = expected == .settingsLight
        let wanted: NSAppearance.Name = isLight || options.appearance == "light" ? .aqua : .darkAqua
        checkSettings(window.shownTheme == (isLight ? .light : .auto)
                      && window.themeControl.selectedSegment == (isLight ? 0 : 1) && shown == wanted, name)
        checkSettings(window.railButtons.map(\.page) == [.appearance, .playback, .subtitles, .video, .audio,
                                                        .shortcuts, .integration, .updates, .advanced]
                      && window.aboutButton.page == .about && window.selectedPage == .appearance, "rail_order")
        let png: Data?
        let mode: String
        if let screen = await captureCompositor(width: Int((window.frame.width * 2).rounded()),
                                                height: Int((window.frame.height * 2).rounded()), window: window) {
            png = NSBitmapImageRep(cgImage: screen).representation(using: .png, properties: [:])
            mode = "window_compositor_2x"
        } else {
            let bounds = root.bounds
            guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int((bounds.width * 2).rounded()),
                                               pixelsHigh: Int((bounds.height * 2).rounded()), bitsPerSample: 8,
                                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else {
                throw VerifyError("Could not allocate a Settings bitmap")
            }
            bitmap.size = bounds.size
            root.effectiveAppearance.performAsCurrentDrawingAppearance { root.cacheDisplay(in: bounds, to: bitmap) }
            png = bitmap.representation(using: .png, properties: [:])
            mode = "app_content_2x"
        }
        guard SmokeSnapshots.write(png, in: options.output.path, as: name) else {
            throw VerifyError("Could not write the Settings PNG")
        }
        observations.append([
            "name": name, "png": "\(name).png", "capture": mode, "window_id": window.windowNumber,
            "theme": window.shownTheme.rawValue, "effective_appearance": shown?.rawValue ?? "unknown",
            "content_width": root.bounds.width, "content_height": root.bounds.height,
        ])
    }

    private func settingsAppearance(_ window: NSWindow) -> NSAppearance.Name? {
        window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua])
    }

    private func settingsWindow() throws -> SettingsWindow {
        guard let window = app.settings.window, window.isVisible else { throw VerifyError("Settings is not visible") }
        return window
    }

    private func checkSettings(_ condition: Bool, _ checkpoint: String) {
        if !condition { failures.append("settings_shell_appearance:\(checkpoint)") }
    }
}
