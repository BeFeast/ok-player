import AppKit

extension VerifyRun {
    func performAboutStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .aboutPlayback:
            let before = swaps
            app.application(NSApp, open: [options.fixture])
            try await wait("fixture playback through the active renderer") {
                self.snapshot.source?.location == self.options.fixture.path
                    && self.snapshot.status == .playing && self.swaps > before
            }
        case .aboutMenu:
            try verifyAboutMenu()
        case .aboutRail:
            app.showSettings()
            guard let window = app.settings.window,
                  let appearance = window.railButtons.first(where: { $0.page == .appearance }) else {
                throw VerifyError("Settings did not open with Appearance in its rail")
            }
            appearance.performClick(nil)
            checkAbout(window.selectedPage == .appearance, "appearance_route")
            window.aboutButton.performClick(nil)
            checkAbout(window.selectedPage == .about && window.aboutButton.state == .on
                       && app.settings.window === window, "rail_route")
        case .aboutCopy:
            try verifyAboutCopy()
        case .aboutResize:
            let (window, pane) = try aboutWindow()
            window.setContentSize(window.contentMinSize)
            window.contentView?.layoutSubtreeIfNeeded()
            let clip = window.content.contentView
            checkAbout(window.contentView?.bounds.size == window.contentMinSize
                       && pane.bounds.height > clip.bounds.height, "narrow_scroll")
        case .aboutFooter:
            let (_, pane) = try aboutWindow()
            let footer = pane.footer!
            footer.scrollToVisible(footer.bounds)
            checkAbout(footer.visibleRect.insetBy(dx: -0.5, dy: -0.5).contains(footer.bounds), "footer_reachable")
            var opened: [URL] = []
            pane.openURL = { opened.append($0) }
            pane.githubButton.performClick(nil)
            pane.licenseButton.performClick(nil)
            checkAbout(opened.map(\.absoluteString) == ["https://github.com/BeFeast/ok-player",
                                                        "https://github.com/BeFeast/ok-player/blob/main/LICENSE"],
                       "footer_destinations")
        case .aboutClose:
            let (window, _) = try aboutWindow()
            app.window.performClose(nil)
            checkAbout(app.closing && app.shuttingDown && app.client == nil && !app.window.isVisible
                       && !window.isVisible && app.settings.window == nil, "owner_close")
        case .capture:
            let (window, pane) = try aboutWindow()
            guard let name = step.name, let root = window.contentView else { throw VerifyError("Missing About checkpoint") }
            _ = window.makeFirstResponder(nil)
            root.layoutSubtreeIfNeeded()
            window.displayIfNeeded()
            let appearance = window.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua])
            checkAbout(appearance == (options.appearance == "dark" ? .darkAqua : .aqua), "appearance")
            let badRows = pane.specRows.compactMap(rowProblem)
            checkAbout(pane.cards.count == 3 && pane.specRows.count == 14 && badRows.isEmpty, "rows_fit")
            if step.expect == .aboutIdentity {
                checkAbout(!pane.mark.visibleRect.isEmpty, "mark_visible")
            }
            let mode = try await captureSettingsImage(window, name: name)
            observations.append([
                "name": name, "png": "\(name).png", "capture": mode, "window_id": window.windowNumber,
                "effective_appearance": appearance?.rawValue ?? "unknown",
                "content_width": root.bounds.width, "content_height": root.bounds.height,
                "layout_problems": badRows,
            ])
        default:
            throw VerifyError("Unsupported About action")
        }
    }

    private func verifyAboutMenu() throws {
        let existing = app.settings.window
        guard let menu = app.menuBar?.menus[.app],
              let index = menu.items.firstIndex(where: { $0.title == "About OK Player" }) else {
            throw VerifyError("The application menu has no About item")
        }
        menu.performActionForItem(at: index)
        let (window, _) = try aboutWindow()
        checkAbout(existing == nil || existing === window, "reused_settings_window")
        checkAbout(NSApp.windows.filter { $0.isVisible && $0 is SettingsWindow }.count == 1, "one_settings_window")
    }

    private func verifyAboutCopy() throws {
        let (_, pane) = try aboutWindow()
        let board = NSPasteboard.general
        let before = board.changeCount
        if options.control == "about_copy_disabled" { pane.copyButton.action = nil }
        pane.copyButton.performClick(nil)
        if board.changeCount != before {
            copiedAboutDiagnostics = board.string(forType: .string)
        }
        checkAbout(copiedAboutDiagnostics == pane.diagnostics.plainText
                   && pane.copyButton.title == "Diagnostics copied", "copy_diagnostics")
        checkAbout(SmokeSnapshots.captureIcons(in: options.output.path), "bundle_icon")
    }

    private func rowProblem(_ row: NSStackView) -> String? {
        let fields = row.arrangedSubviews
        guard fields.count == 2,
              let key = fields.first as? NSTextField, let value = fields.last as? NSTextField,
              let font = value.font else { return "Missing About fields" }
        let keyRect = key.convert(key.bounds, to: row)
        let valueRect = value.convert(value.bounds, to: row)
        let tolerance = row.bounds.insetBy(dx: -1, dy: -1)
        guard !keyRect.isEmpty && !valueRect.isEmpty && tolerance.contains(keyRect)
                && tolerance.contains(valueRect) && !keyRect.intersects(valueRect) else {
            return "\(key.stringValue): overlapping or clipped fields \(keyRect) \(valueRect) in \(row.bounds)"
        }
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byCharWrapping
        let text = NSAttributedString(string: value.stringValue, attributes: [.font: font, .paragraphStyle: paragraph])
        let measure = NSSize(width: max(1, valueRect.width - 4), height: .greatestFiniteMagnitude)
        let needed = text.boundingRect(with: measure, options: [.usesLineFragmentOrigin]).height
        guard needed <= valueRect.height + 1 else {
            return "\(key.stringValue): text needs \(needed) pt in a \(valueRect.height) pt field"
        }
        return nil
    }

    private func aboutWindow() throws -> (SettingsWindow, AboutPane) {
        guard let window = app.settings.window, window.isVisible, window.selectedPage == .about,
              let pane = window.aboutPane else { throw VerifyError("About is not visible in Settings") }
        return (window, pane)
    }

    private func checkAbout(_ condition: Bool, _ assertion: String) {
        if !condition { failures.append("\(options.scenarioID):\(assertion)") }
    }
}
