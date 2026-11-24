import AppKit
import MediaPlayer

@MainActor
extension VerifyRun {
    private var queuePanel: UpNextPanel { app.upNext.panel }
    private var queueNames: [String] {
        queuePanel.list.rows.map { URL(fileURLWithPath: $0.location).lastPathComponent }
    }
    private var episodes: [String] { ["S01E01.mp4", "S01E02.mp4", "S01E03.mp4"] }

    func performQueueStep(_ step: VerificationScenario.Step) async throws {
        switch step.action {
        case .openQueue, .openQueueURL:
            try await openQueue(remote: step.action == .openQueueURL)
        case .pause:
            try await perform(step)
        case .showQueue:
            try await showQueue()
        case .checkQueueJump:
            try await checkQueueJump()
        case .previewQueueMove:
            queuePanel.drag = (2, queuePanel.list)
            if !queuePanel.previewReorder(before: 1) || queuePanel.insertionLine.isHidden {
                failures.append("queue_edit:insertion")
            }
        case .checkQueueEdit:
            try await checkQueueEdit()
        case .openQueueMenu:
            try openQueueMenu()
        case .checkQueueActions:
            try await checkQueueActions()
        case .checkQueueAdd:
            try await checkQueueAdd()
        case .capture:
            guard let name = step.name, step.expect == .queuePanel else {
                throw VerifyError("Queue capture lacks a name or panel expectation")
            }
            try await captureQueue(name)
        default:
            throw VerifyError("Unsupported queue step \(step.action.rawValue)")
        }
    }

    private func openQueue(remote: Bool) async throws {
        let before = swaps
        let location: String
        if remote {
            guard let url = options.httpFixture?.appendingPathComponent("single.mp4") else {
                throw VerifyError("The direct URL fixture is unavailable")
            }
            location = url.absoluteString
            app.play(MediaSource(kind: .url, location: location))
        } else {
            location = options.fixture.path
            app.application(NSApp, open: [options.fixture])
        }
        try await wait("fixture playing and queue ready") {
            self.snapshot.status == .playing && self.snapshot.source?.location == location &&
                self.swaps > before && !self.queuePanel.list.scanning &&
                self.queuePanel.list.rows.count == (remote ? 1 : 3)
        }
    }

    private func showQueue() async throws {
        if let area = app.window.screen?.visibleFrame {
            let width = min(1280, area.width)
            let height = min(800, area.height)
            app.window.setFrame(NSRect(x: area.midX - width / 2, y: area.midY - height / 2,
                                       width: width, height: height), display: true)
        }
        app.osc.noteActivity()
        let before = swaps
        try seek(3.25)
        try await wait("reference frame at 3.25 seconds") {
            abs((self.snapshot.timePos ?? -1) - 3.25) <= 0.10 && self.swaps > before
        }
        app.upNext.button.performClick(nil)
        guard app.upNext.isShown, queuePanel.selectedTab == .chapters else {
            throw VerifyError("The OSC did not open the Chapters tab of the side panel")
        }
        queuePanel.upNextButton.performClick(nil)
        guard queuePanel.selectedTab == .upNext else { throw VerifyError("The Up Next tab did not select") }
        app.window.contentView?.layoutSubtreeIfNeeded()
        if options.scenarioID != "queue-single" {
            if queueNames != episodes || queuePanel.headingLabel.stringValue != "FROM THIS FOLDER · 3" ||
                queuePanel.tableView.numberOfRows != 3 || queuePanel.list.rows.map(\.state) != [.current, .next, .upcoming] ||
                queuePanel.nowPlayingTitle.stringValue != episodes[0] || queuePanel.nowPlayingCard.isHidden {
                failures.append("queue_show:projection")
            }
        } else if queuePanel.emptyLabel.isHidden || queuePanel.emptyLabel.stringValue != "Just this file" ||
                    queuePanel.addFilesButton.isHidden || queuePanel.tableView.numberOfRows != 0 ||
                    queuePanel.list.nowPlaying?.source != snapshot.source {
            failures.append("queue_single:empty")
        }
    }

    private func checkQueueJump() async throws {
        try pressKey(" ", code: 49)
        try await wait("playback resumed with Up Next open") { self.snapshot.status == .playing }
        let start = snapshot.timePos ?? 0
        try await wait("playback crossed the idle deadline") { (self.snapshot.timePos ?? 0) - start >= 2.9 }
        if !app.upNext.isShown || !app.osc.isRevealed { failures.append("queue_show:pin") }

        var jumps = 0
        let dispatch = app.upNext.dispatch
        app.upNext.dispatch = { command in
            if case .playlist(action: .jump) = command { jumps += 1 }
            return dispatch?(command) ?? .noOp
        }
        defer { app.upNext.dispatch = dispatch }
        let table = queuePanel.tableView
        guard app.window.makeFirstResponder(table) else { throw VerifyError("The queue list cannot take focus") }
        table.selectRowIndexes(IndexSet(integer: 1), byExtendingSelection: false)
        try pressKey("\r", code: UpNextKey.returnKeyCode, focusVideo: false)
        let next = options.fixture.deletingLastPathComponent().appendingPathComponent(episodes[1]).path
        try await wait("selected episode opened and started") {
            self.snapshot.source?.location == next && self.snapshot.status == .playing &&
                self.queuePanel.list.currentIndex == 1 && self.queuePanel.list.row(at: 2)?.state == .next
        }
        if jumps != 1 { failures.append("queue_show:single_activation") }
        queuePanel.closeButton.performClick(nil)
        if app.upNext.isShown || app.window.firstResponder !== app.video { failures.append("queue_show:close") }
        try await wait("closing Up Next restored OSC auto-hide") { !self.app.osc.isRevealed }
    }

    private func checkQueueEdit() async throws {
        if !queuePanel.completeReorder(before: 1) { failures.append("queue_edit:commit") }
        if options.control == "queue_move_disabled" {
            try await Task.sleep(nanoseconds: 300_000_000)
        } else {
            try await wait("session published the insertion order") {
                self.queueNames == [self.episodes[0], self.episodes[2], self.episodes[1]]
            }
        }
        if queueNames != [episodes[0], episodes[2], episodes[1]] { failures.append("queue_edit:reorder") }
        if queuePanel.tableView(queuePanel.tableView, pasteboardWriterForRow: 0) != nil ||
            queuePanel.menuItems(forRow: 0).contains(where: \.isEnabled) {
            failures.append("queue_edit:current_protection")
        }
    }

    private func openQueueMenu() throws {
        guard let row = queuePanel.list.rows.firstIndex(where: \.canPlayNext),
              let cell = queuePanel.tableView.view(atColumn: 0, row: row, makeIfNecessary: true) as? UpNextCellView else {
            throw VerifyError("No queue row with an enabled Play Next action")
        }
        cell.actions.performClick(nil)
        guard queuePanel.rowMenu != nil else { throw VerifyError("The queue row menu did not open") }
    }

    private func checkQueueActions() async throws {
        guard let row = queuePanel.list.rows.firstIndex(where: \.canPlayNext),
              let playNext = queuePanel.rowMenu?.subviews.compactMap({ $0 as? NSButton })
                .first(where: { $0.title == "Play Next" && $0.isEnabled }) else {
            throw VerifyError("The row menu did not offer Play Next")
        }
        let current = snapshot.source
        let next = queuePanel.list.rows[row].source
        playNext.performClick(nil)
        try await wait("Play Next moved the chosen item and updated the badge") {
            self.queuePanel.list.row(at: 1)?.source == next && self.queuePanel.list.row(at: 1)?.state == .next
        }

        let duplicate = options.fixture.deletingLastPathComponent().appendingPathComponent(episodes[1])
        let extra = extraQueueFile
        app.upNext.chooseFiles = { _, choose in choose([self.options.fixture, duplicate, extra, extra]) }
        defer { app.upNext.chooseFiles = nil }
        guard let entry = NSApp.mainMenu?.items.compactMap(\.submenu)
            .flatMap(\.items).first(where: { $0.playerCommandID == "add-to-queue" }),
              app.validateMenuItem(entry) else {
            throw VerifyError("Add to Queue is unavailable in File")
        }
        app.performPlayerCommand(entry)
        try await wait("the File menu appended the fourth file") {
            self.queueNames.count == 4 && self.queueNames.last == extra.lastPathComponent
        }
        if Set(queueNames).count != 4 || snapshot.source != current {
            failures.append("queue_edit:deduplication")
        }

        try chooseQueueMenuItem("Remove from Up Next", row: 2)
        try await wait("queue removal left current and next intact") {
            self.queueNames.count == 3 && self.queuePanel.list.row(at: 1)?.source == next
        }

        try pressKey(" ", code: 49)
        try await wait("the current file is playing before stepping through the queue") {
            self.snapshot.source == current && self.snapshot.status == .playing
        }
        let beforeMenu = swaps
        guard app.commands.run("next-item") == .performed else { throw VerifyError("Next Item menu command did not run") }
        try await wait("Next Item started the next entry in the edited queue") {
            self.snapshot.source == next && self.snapshot.status == .playing && self.swaps > beforeMenu
        }
        let remoteSource = SystemRemoteCommandSource()
        let remote = RemoteCommandController(source: remoteSource, session: { [unowned app] command in
            app.client?.dispatch(command) ?? .noOp
        })
        remote.playlist = app.playlist
        remote.start()
        defer { remote.stop() }
        let beforeRemote = swaps
        guard remoteSource.deliver(MPRemoteCommandCenter.shared().nextTrackCommand) == .success else {
            throw VerifyError("MediaPlayer Next Track was not dispatched")
        }
        try await wait("MediaPlayer Next Track played the appended file") {
            self.snapshot.source?.location == extra.path && self.snapshot.status == .playing && self.swaps > beforeRemote
        }
    }

    private func chooseQueueMenuItem(_ name: String, row: Int) throws {
        guard let item = queuePanel.menuItems(forRow: row).first(where: { $0.title == name && $0.isEnabled }),
              let action = item.action, NSApp.sendAction(action, to: item.target, from: item) else {
            throw VerifyError("Queue action is missing: \(name)")
        }
    }

    private var extraQueueFile: URL {
        options.fixture.deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("S01E04.mp4")
    }

    private func checkQueueAdd() async throws {
        let current = snapshot.source
        let currentTime = snapshot.timePos ?? 0
        let extra = extraQueueFile
        app.upNext.chooseFiles = { _, choose in choose([extra]) }
        defer { app.upNext.chooseFiles = nil }
        queuePanel.addFilesButton.performClick(nil)
        if options.control == "queue_add_disabled" {
            try await Task.sleep(nanoseconds: 300_000_000)
        } else {
            try await wait("Add files grew the queue while the URL stayed open") {
                self.queuePanel.list.row(at: 1)?.location == extra.path
            }
        }
        if snapshot.source != current || snapshot.status != .paused ||
            abs((snapshot.timePos ?? -1) - currentTime) > 0.06 ||
            queuePanel.list.row(at: 1)?.location != extra.path ||
            queuePanel.headingLabel.stringValue != "QUEUE · 2" ||
            queuePanel.list.activation(ofRow: 1) != .playlist(action: .jump(index: 1)) {
            failures.append("queue_single:add")
        }
    }

    private func captureQueue(_ name: String) async throws {
        try await Task.sleep(nanoseconds: 250_000_000)
        guard let root = app.window.contentView, let video = app.video else {
            throw VerifyError("The player window is unavailable", blocked: true)
        }
        root.layoutSubtreeIfNeeded()
        let rect = queuePanel.convert(queuePanel.bounds, to: root)
        let bar = app.osc.panel.convert(app.osc.panel.bounds, to: root)
        if queuePanel.isHidden || queuePanel.selectedTab != .upNext ||
            abs(rect.maxX - root.bounds.maxX) > 0.5 || abs(rect.minY - bar.maxY - 2) > 0.5 ||
            abs(rect.width - 316) > 0.5 || rect.height <= 0 {
            failures.append("queue_panel:geometry")
        }
        guard let frame = video.videoLayer.verificationFrame(width: Int(video.bounds.width * 2),
                                                             height: Int(video.bounds.height * 2)), frame.hasPicture else {
            throw VerifyError("No decoded 2× video frame for the queue", blocked: true)
        }
        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        let compositor = await captureCompositor(width: width, height: height)
        guard let image = compositor ?? windowComposite(root: root, video: video, frame: frame.image),
              let bytes = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw VerifyError("Cannot capture the queue window", blocked: true)
        }
        let filename = name + ".png"
        try bytes.write(to: options.output.appendingPathComponent(filename))
        let menuLabels = queuePanel.rowMenu?.subviews.compactMap { ($0 as? NSButton)?.title } ?? []
        observations.append([
            "name": name, "png": filename, "render_scale": 2,
            "capture": compositor == nil ? "app_composite_2x" : "window_compositor_2x",
            "panel_bounds_points": dimensions(rect), "osc_rect_points": dimensions(bar),
            "rows": queueNames, "states": queuePanel.list.rows.map { $0.state.rawValue },
            "heading": queuePanel.headingLabel.stringValue,
            "row_menu": menuLabels, "insertion_visible": !queuePanel.insertionLine.isHidden,
        ])
    }
}
