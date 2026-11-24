import AppKit

extension VerificationOptions {
    var isHistoryScenario: Bool {
        scenarioID == "history-page" || scenarioID == "welcome-resume-shelf"
    }

    var historyMedia: URL { profile.appendingPathComponent("Fixture media", isDirectory: true) }

    func historyFile(_ title: String) -> URL {
        historyMedia.appendingPathComponent(title).appendingPathExtension("mkv")
    }

    // New entries in both scenarios come from libmpv. Seed one fixed old day
    // and an absent local file so the page can also exercise those projections.
    func prepareHistory() throws {
        let files = FileManager.default
        try files.createDirectory(at: historyMedia, withIntermediateDirectories: true)
        let titles = scenarioID == "history-page"
            ? ["Episode 3", "Finished", "Dark fallback", "Archive episode"]
            : ["Episode 1", "Episode 2", "Episode 3", "Finished", "Dark fallback", "Barely started"]
        for title in titles {
            let source = title == "Dark fallback"
                ? fixture.deletingLastPathComponent().appendingPathComponent("history-dark.mkv") : fixture
            try files.linkItem(at: source, to: historyFile(title))
        }
        guard scenarioID == "history-page" else { return }
        let old = 1_700_000_000
        let sources = [
            (historyFile("Missing episode").path, 6.0, false),
            (historyFile("Archive episode").path, 0.0, true),
            ("https://example.invalid/recorded-stream.mp4", 5.0, false),
            ("https://example.invalid/older-stream.mp4", 8.0, false),
        ]
        let entries = Dictionary(uniqueKeysWithValues: sources.enumerated().map { index, source in
            (source.0, ["position": source.1, "duration": 18,
                        "finished": source.2, "updated_at_unix": old - index] as [String: Any])
        })
        let json = try JSONSerialization.data(withJSONObject: ["version": 2, "files": entries], options: [.sortedKeys])
        try json.write(to: profile.appendingPathComponent("data/history.json"), options: .atomic)
    }
}

@MainActor
extension VerifyRun {
    func performHistoryStep(_ step: VerificationScenario.Step) async throws {
        let history = app.history
        switch step.action {
        case .historyPlay:
            try await recordHistory()
        case .historyArrow, .historyFooter:
            guard history.showsWelcome, !history.showsInlineHistory else {
                throw VerifyError("The shelf must be visible before navigating to History")
            }
            try await activateHistory(step.action == .historyArrow ? "shelf-arrow" : "footer-navigation", shelf: true)
            try await wait("History takes the idle player surface") { history.showsInlineHistory }
            try verifyHistoryRows()
        case .historyBack:
            try await activateHistory(step.name == "footer" ? "footer-navigation" : "back")
            try await wait("Back returns to the shelf in the same window") {
                !history.showsHistory && !history.shelfView.isHiddenOrHasHiddenAncestor
            }
            if let initial = initialShelfBounds {
                let id = history.welcome.entries.first?.id ?? ""
                guard history.shelfBounds[id] == initial[id],
                      history.shelfBounds["shelf-arrow"] == initial["shelf-arrow"] else {
                    throw VerifyError("Back moved or resized the shelf")
                }
            }
        case .historySearch:
            try await wait("History search is laid out") {
                history.pageBounds["search"] != nil && self.historySearchField(in: history.historyPane) != nil
            }
            guard let field = historySearchField(in: history.historyPane), app.window.makeFirstResponder(field) else {
                throw VerifyError("The visible search field did not accept focus")
            }
            try await wait("History search has a live field editor") {
                self.app.window.firstResponder is NSTextView
            }
            guard let editor = app.window.firstResponder as? NSTextView else {
                throw VerifyError("The visible search field did not take text input")
            }
            editor.insertText("aRcHiVe", replacementRange: editor.selectedRange())
            try await wait("case-insensitive History search projects the archived row") {
                history.page.search == "aRcHiVe" && self.historyRows.map(\.title) == ["Archive episode"]
                    && !history.page.hasMore
            }
        case .historyClear:
            try await activateHistory("clear")
            try await wait("Clear restores the first page") {
                history.page.search.isEmpty && self.historyRows.count == 6 && history.page.hasMore
            }
        case .historyMore:
            let first = historyRows.map(\.id)
            guard first.count == 6, history.page.hasMore else { throw VerifyError("Incorrect first History page") }
            try await activateHistory("show-more")
            do {
                try await wait("Show more appends rows and reaches the end of history") {
                    self.historyRows.count == 7 && !history.page.hasMore
                }
            } catch {
                failures.append("history_page:pagination")
            }
            guard Array(historyRows.map(\.id).prefix(first.count)) == first else {
                throw VerifyError("Paging reordered or duplicated an existing row")
            }
        case .historyCompact:
            app.window.setContentSize(CGSize(width: 442, height: 580))
            try await wait("compact History keeps navigation and row actions in bounds") {
                self.historyGeometryFits()
            }
        case .historyResume:
            guard let first = history.welcome.entries.first, let position = first.resumeFrom, position > 4 else {
                throw VerifyError("The newest card has no saved resume point")
            }
            try await activateHistory(first.id, shelf: true)
            try await wait("clicking the card resumes the original source at its stored position") {
                self.snapshot.source == first.source && self.snapshot.video != nil
                    && (self.snapshot.status == .playing || self.snapshot.status == .paused)
                    && abs((self.snapshot.timePos ?? 0) - position) < 0.7
            }
            _ = app.client?.dispatch(.setPaused(paused: false))
            try await wait("the resumed source plays forward from its saved position") {
                self.snapshot.status == .playing && (self.snapshot.timePos ?? 0) > position + 0.4
            }
            try await closeHistoryMedia()
        case .capture:
            guard let name = step.name, step.expect == .historyEvidence else {
                throw VerifyError("Invalid History checkpoint")
            }
            try await captureHistory(name)
        default:
            throw VerifyError("Unsupported History action \(step.action.rawValue)")
        }
    }

    private func recordHistory() async throws {
        let common: [(String, Double)] = [("Finished", 17.75), ("Dark fallback", 5), ("Episode 3", 7)]
        let more: [(String, Double)] = [("Barely started", 0.4), ("Episode 1", 3), ("Episode 2", 4)]
        let sequence = options.scenarioID == "history-page" ? common : more + common
        for (index, record) in sequence.enumerated() {
            if index > 0 {
                let second = floor(Date().timeIntervalSince1970)
                try await wait("next history clock tick establishes recency") { Date().timeIntervalSince1970 >= second + 1 }
            }
            let (title, seconds) = record
            let path = options.historyFile(title).path
            app.play(path)
            try await wait("\(title) is loaded in libmpv") {
                self.snapshot.source?.location == path && self.snapshot.video != nil
                    && (self.snapshot.status == .playing || self.snapshot.status == .paused)
            }
            let finishes = title == "Finished"
            _ = app.client?.dispatch(.setPaused(paused: !finishes))
            if finishes {
                try await wait("the completed fixture plays before the first progress save") {
                    self.snapshot.status == .playing && (self.snapshot.timePos ?? 0) > 0.5
                }
                // EOF marks an existing row finished. Save through the normal
                // Pause path before seeking to the end of this short fixture.
                _ = app.client?.dispatch(.setPaused(paused: true))
                try await wait("the completed fixture has a saved progress row") {
                    self.snapshot.status == .paused && (self.savedHistory(title)?["position"] as? Double ?? 0) > 0
                }
                _ = app.client?.dispatch(.setPaused(paused: false))
                try await wait("the completed fixture resumes playing") { self.snapshot.status == .playing }
            } else {
                try await wait("\(title) is paused") { self.snapshot.status == .paused }
            }
            _ = app.client?.dispatch(.seek(seconds: seconds, mode: .absolute, precision: .exact))
            if finishes {
                try await wait("fixture reaches actual EOF") {
                    self.snapshot.status == .ended && self.snapshot.endReason == .eof
                }
                try await wait("the completed fixture is marked finished on disk") {
                    self.savedHistory(title)?["finished"] as? Bool == true
                }
            } else {
                try await wait("\(title) renders at \(seconds) s") {
                    abs((self.snapshot.timePos ?? -1) - seconds) < 0.12
                }
            }
            try await closeHistoryMedia()
        }
        try await wait("the latest played row is on disk") {
            guard let position = self.savedHistory("Episode 3")?["position"] as? Double else { return false }
            return abs(position - 7) < 0.12
        }
        app.window.setContentSize(CGSize(width: 1120, height: 780))
        app.window.center()
        let shelf = app.history.welcome.entries
        guard shelf.map(\.title) == ["Episode 3", "Dark fallback", "Finished"],
              shelf[0].state == .progress, abs(shelf[0].progress - 7.0 / 18) < 0.02,
              shelf[1].state == .progress, shelf[2].state == .finished else {
            throw VerifyError("The shelf does not project the newest playback records: \(shelf.map(\.title))")
        }
        if options.scenarioID == "welcome-resume-shelf" {
            guard case .history(.page(_, _, _, _, _, let sections))? =
                    app.client?.query(.history(query: .pageWindow(search: "", utcOffsetSeconds: 0, limit: 20))) else {
                throw VerifyError("The shared History query did not return the saved playback")
            }
            let all = sections.flatMap(\.entries)
            guard all.count == 6, all.filter({ $0.resumeFrom != nil }).count == 4,
                  all.first(where: { $0.title == "Barely started" })?.state == .barely else {
                throw VerifyError("The playback fixture did not create four eligible, one finished and one early entry")
            }
        }
    }

    private func savedHistory(_ title: String) -> [String: Any]? {
        let url = options.profile.appendingPathComponent("data/history.json")
        guard let document = try? Data(contentsOf: url),
              let root = (try? JSONSerialization.jsonObject(with: document)) as? [String: Any],
              let files = root["files"] as? [String: [String: Any]] else { return nil }
        return files[options.historyFile(title).path]
    }

    private func closeHistoryMedia() async throws {
        app.closeMedia()
        try await wait("closed playback and restored idle Welcome") {
            self.snapshot.source == nil && self.app.history.showsWelcome
        }
    }

    private var historyRows: [HistoryEntry] {
        guard case .sections(let sections) = app.history.page.content else { return [] }
        return sections.flatMap(\.entries)
    }

    private func verifyHistoryRows() throws {
        let history = app.history
        let rows = historyRows
        guard let first = rows.first, first.id == history.welcome.entries.first?.id else {
            throw VerifyError("History and Continue watching disagree about the newest source")
        }
        guard options.scenarioID == "history-page", history.page.search.isEmpty else { return }
        guard case .sections(let groups) = history.page.content, groups.map(\.bucket) == [.today, .earlier],
              rows.prefix(6).map(\.title) == ["Episode 3", "Dark fallback", "Finished", "Missing episode",
                                               "Archive episode", "recorded-stream"],
              rows[2].state == .finished, !rows[3].isAvailable, rows[3].resumeFrom == nil,
              rows.count == 6 || rows.count == 7 else {
            throw VerifyError("History has unexpected order, grouping or states: \(rows.map(\.title))")
        }
    }

    private func historyGeometryFits() -> Bool {
        let history = app.history
        let extent = CGRect(origin: .zero, size: history.historyPane.bounds.size).insetBy(dx: -0.5, dy: -0.5)
        guard extent.width > 1, extent.width < 510, let first = historyRows.first else { return false }
        return ["back", "search", first.id, "actions:\(first.id)", "footer", "footer-navigation"].allSatisfy {
            guard let rect = history.pageBounds[$0]?.frame else { return false }
            return !rect.isEmpty && extent.contains(rect)
        }
    }

    private func historySearchField(in view: NSView) -> HistorySearchTextField? {
        if let field = view as? HistorySearchTextField { return field }
        return view.subviews.lazy.compactMap { self.historySearchField(in: $0) }.first
    }

    private func activateHistory(_ id: String, shelf: Bool = false) async throws {
        let history = app.history
        let host = shelf ? history.shelfView : history.historyPane
        try await wait("History control \(id) is laid out and reachable") {
            guard let control = (shelf ? history.shelfBounds : history.pageBounds)[id],
                  control.activate != nil else { return false }
            let rect = control.frame
            return !rect.isEmpty && !host.isHiddenOrHasHiddenAncestor && host.bounds.size.height > rect.midY
                && host.bounds.size.width > rect.midX && rect.midX >= 0 && rect.midY >= 0
        }
        guard let activate = (shelf ? history.shelfBounds : history.pageBounds)[id]?.activate else {
            throw VerifyError("History control \(id) disappeared")
        }
        activate()
    }

    private func captureHistory(_ name: String) async throws {
        let history = app.history
        let onPage = history.showsInlineHistory
        if name == "shelf" || name == "history" {
            let first = history.welcome.entries.first?.id ?? ""
            do {
                try await wait("actual decoded frame is laid out for the newest record") {
                    (onPage ? history.pageBounds : history.shelfBounds)["poster:\(first)"] != nil
                }
            } catch {
                failures.append(onPage ? "history_page:poster" : "welcome_resume_shelf:poster")
            }
            let dark = history.welcome.entries.first(where: { $0.title == "Dark fallback" })?.id ?? ""
            let bounds = onPage ? history.pageBounds : history.shelfBounds
            guard let firstRect = bounds[first]?.frame, let darkRect = bounds[dark]?.frame,
                  firstRect.size == darkRect.size, bounds["poster:\(dark)"] == nil else {
                throw VerifyError("The dark fallback changes card/row geometry or displays a black poster")
            }
            if !onPage { initialShelfBounds = bounds }
        }
        try await Task.sleep(nanoseconds: 200_000_000)
        guard let root = app.window.contentView, let video = app.video else {
            throw VerifyError("History lost its live window", blocked: true)
        }
        root.layoutSubtreeIfNeeded()
        guard let image = windowComposite(root: root, video: video, frame: nil),
              let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Could not capture the History checkpoint at 2×", blocked: true)
        }
        let file = name + ".png"
        try png.write(to: options.output.appendingPathComponent(file))
        let bounds = onPage ? history.pageBounds : history.shelfBounds
        let entries = onPage ? historyRows : history.welcome.entries
        observations.append([
            "name": name, "png": file, "capture": "app_composite_2x", "render_scale": 2,
            "surface": onPage ? "history" : "continue-watching", "window_points": dimensions(root.bounds),
            "geometry": bounds.filter { !$0.key.contains(":") }.mapValues { dimensions($0.frame) },
            "rows": entries.map { entry in [
                "title": entry.title, "state": entry.state.rawValue, "progress": entry.progress,
                "available": entry.isAvailable, "poster": bounds["poster:\(entry.id)"] != nil,
            ] as [String: Any] },
        ])
    }
}
