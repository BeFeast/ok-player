import AppKit

extension PlayerApp {
    func installCommandPopover(in root: NSView, behaviour: PlayerWindowController) {
        let context = commandPopover
        context.install(in: root)
        context.focusReturn = video
        context.clampsToWindow = verification?.control != "context_clamp_disabled"
        context.query = { [weak self, weak behaviour] page, query in
            guard let self, let behaviour else { return nil }
            let request: Query = self.miniPlayer.isActive
                ? .compactCommandSurface(page: page, query: query)
                : .commandSurface(fullscreen: behaviour.window.isInFullscreen, page: page, query: query)
            guard case .commandSurface(let surface)? = self.client?.query(request) else { return nil }
            let entries = surface.entries.map { entry in
                guard case .command(let row) = entry else { return entry }
                return .command(row: self.music.project(row))
            }
            return CommandSurface(entries: entries, preferredWidth: surface.preferredWidth, maxHeight: surface.maxHeight)
        }
        context.onVisibilityChange = { [weak self] shown in
            guard let self else { return }
            if self.music.isActive {
                self.music.setContextShown(shown)
                return
            }
            if self.miniPlayer.isActive {
                self.miniPlayer.setContextShown(shown)
                return
            }
            if shown {
                self.osc.dismissVolume()
                self.tracks.dismissPopover()
                self.osc.noteActivity()
            }
            self.osc.setPinned(shown, by: .context)
        }
        inputRouter.protectsTextInput = verification?.control != "text_priority_disabled"
        inputRouter.dismissTransient = { [weak behaviour] in behaviour?.dismissTransient?() == true }
        inputRouter.dismissQueueMenu = { [weak self] point in
            self?.upNext.panel.dismissRowMenu(outside: point) == true
        }
        inputRouter.canOpenContext = { [weak self, weak behaviour, weak root] event, point in
            guard let self, let behaviour, let root, let parent = root.superview else { return false }
            let local = root.convert(point, from: nil)
            guard root.bounds.contains(local) else { return false }
            let hit = root.hitTest(parent.convert(point, from: nil))
            // Controls, title chrome and the queue retain their own input.
            let interactive: [NSView] = [behaviour.titleChrome, self.osc.panel, self.osc.volumeCapsule,
                                         self.upNext.panel, self.tracks.panel, self.miniPlayer.controls]
            guard !interactive.contains(where: { hit?.isDescendant(of: $0) == true }),
                  !(hit is NSControl) else { return false }
            var candidate = hit
            while let view = candidate {
                if view.menu(for: event) != nil { return false }
                if view === root { break }
                candidate = view.superview
            }
            return true
        }
    }

    func refreshCommandPopover(_ batch: EventBatch) {
        guard commandPopover.isShown else { return }
        let relevant = batch.events.contains { event in
            switch event {
            case .playlistChanged, .history: return true
            case .snapshotChanged(let changes):
                return changes.contains { change in
                    switch change {
                    case .source, .status, .video, .tracks, .speed: return true
                    default: return false
                    }
                }
            default: return false
            }
        }
        if relevant { commandPopover.refresh(rebuildSubmenu: true) }
    }
}
