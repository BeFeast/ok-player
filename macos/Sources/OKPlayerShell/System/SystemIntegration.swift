import AppKit
import MediaPlayer

/// The shell's macOS integration: Now Playing, remote commands (media keys, Control
/// Center, AirPods), the display-sleep assertion, recent documents and the Dock menu.
/// Everything is derived from the session snapshot it is handed after each drained
/// batch; it keeps no playback state of its own.
final class SystemIntegration: NSObject {
    private let remote: RemoteCommandController
    private let remoteSource: RemoteCommandSource
    private let nowPlaying: NowPlayingPublisher
    private let nowPlayingCenter: NowPlayingCenter
    private let displaySleep: DisplaySleepGuard
    private let recents: RecentDocuments
    private let recentList: RecentDocumentList
    private let session: (Command) -> DispatchResult
    private let open: (String) -> Void
    private var openRecentMenu: OpenRecentMenu?
    private(set) var snapshot = SessionSnapshot()
    /// The display-sleep assertion the smoke saw while playing.
    private var smokeAssertion: UInt32?

    /// While on, loaded files are not added to the recent documents. The app follows
    /// the session's `history.private_session` event (`SessionStore.privateSession`).
    var privateSession: Bool {
        get { recents.privateSession }
        set { recents.privateSession = newValue }
    }

    /// Next and previous item for the remote commands and the Dock menu, and the queue
    /// position Now Playing shows. The app sets a `SessionPlaylist`; without one the
    /// open file is the only item and Next and Previous are offered nowhere.
    var playlist: PlaylistNavigation? {
        didSet {
            remote.playlist = playlist
            remote.refresh()
        }
    }

    init(session: @escaping (Command) -> DispatchResult,
         open: @escaping (String) -> Void,
         nowPlayingCenter: NowPlayingCenter = SystemNowPlayingCenter(),
         remoteSource: RemoteCommandSource = SystemRemoteCommandSource(),
         power: PowerAssertions = SystemPowerAssertions(),
         recentList: RecentDocumentList = SystemRecentDocumentList(),
         artwork: MPMediaItemArtwork? = SystemIntegration.appIconArtwork(),
         clock: @escaping () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        self.session = session
        self.open = open
        self.nowPlayingCenter = nowPlayingCenter
        self.recentList = recentList
        self.remoteSource = remoteSource
        nowPlaying = NowPlayingPublisher(center: nowPlayingCenter, artwork: artwork, clock: clock)
        remote = RemoteCommandController(source: remoteSource, session: session)
        displaySleep = DisplaySleepGuard(power: power)
        recents = RecentDocuments(list: recentList)
        super.init()
    }

    /// The app icon as Now Playing artwork, until cover art arrives in Phase 2.
    static func appIconArtwork() -> MPMediaItemArtwork? {
        guard let icon = NSImage(named: NSImage.applicationIconName) else { return nil }
        return MPMediaItemArtwork(boundsSize: icon.size) { _ in icon }
    }

    /// Start receiving remote commands.
    func start() {
        remote.start()
    }

    /// Follow the session's state after a drained batch.
    func update(_ snapshot: SessionSnapshot) {
        self.snapshot = snapshot
        nowPlaying.update(NowPlayingState(snapshot: snapshot, queue: playlist?.position ?? .single))
        displaySleep.update(snapshot)
        recents.update(snapshot)
        remote.refresh()
    }

    /// Stop remote commands, let the display sleep and clear Now Playing.
    func shutdown() {
        remote.stop()
        displaySleep.release()
        nowPlaying.clear()
    }

    // MARK: Menus

    func makeDockMenu() -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let entries = DockMenuModel.entries(snapshot, canNext: playlist?.canStep(.next) ?? false,
                                            canPrevious: playlist?.canStep(.previous) ?? false)
        for entry in entries {
            let item = NSMenuItem(title: entry.title, action: #selector(dockMenuItemChosen(_:)), keyEquivalent: "")
            item.target = self
            item.isEnabled = entry.enabled
            item.representedObject = entry.action
            menu.addItem(item)
        }
        return menu
    }

    /// File > Open Recent.
    func makeOpenRecentItem() -> NSMenuItem {
        let menu = OpenRecentMenu(list: recentList) { [weak self] url in self?.open(url.path) }
        openRecentMenu = menu
        return menu.item
    }

    @objc func dockMenuItemChosen(_ sender: NSMenuItem) {
        guard let action = sender.representedObject as? DockMenuEntry.Action else { return }
        run(action)
    }

    @discardableResult
    func run(_ action: DockMenuEntry.Action) -> DispatchResult? {
        switch action {
        case .togglePause: return session(.togglePause)
        case .step(let step): return playlist?.step(step)
        }
    }

    // MARK: Smoke

    /// Smoke: runs the handler the system's remote-command center holds for `command`,
    /// so the binding, the controller and the session are all on the path.
    private func smokeSend(_ command: MPRemoteCommand, _ name: String) -> String? {
        guard let source = remoteSource as? SystemRemoteCommandSource else { return "Remote commands are not the system's" }
        guard let status = source.deliver(command) else { return "No handler is registered for the remote \(name) command" }
        guard status == .success else { return "The remote \(name) command returned status \(status.rawValue)" }
        return nil
    }

    /// Smoke: the system's remote-command center offers what the shell handles.
    private func smokeCheckOffered() -> String? {
        let offer = RemoteCommandOffer.read()
        let expected = RemoteCommandOffer.installed(next: playlist?.canStep(.next) ?? false,
                                                    previous: playlist?.canStep(.previous) ?? false)
        guard offer == expected else { return "The remote-command center offers \(offer), not \(expected)" }
        return nil
    }

    /// Smoke, while the fixture plays: the system's Now Playing names it as playing
    /// video, IOKit lists the display-sleep assertion, the remote-command center offers
    /// the shell's commands, and then the pause goes out through the handler registered
    /// for the system's pause command. Returns what failed.
    func smokeCheckPlayingThenPause() -> String? {
        guard let center = nowPlayingCenter as? SystemNowPlayingCenter, let expected = nowPlaying.published else {
            return "Now Playing was not published while playing"
        }
        let (info, state) = center.current
        guard state == .playing else { return "Now Playing reports state \(state.rawValue) while playing" }
        guard let title = info?[MPMediaItemPropertyTitle] as? String, !title.isEmpty, title == expected.title else {
            return "Now Playing shows title \(String(describing: info?[MPMediaItemPropertyTitle])), not \(expected.title)"
        }
        guard (info?[MPNowPlayingInfoPropertyMediaType] as? NSNumber)?.uintValue == MPNowPlayingInfoMediaType.video.rawValue else {
            return "Now Playing does not report video"
        }
        guard let assertion = displaySleep.assertion,
              SystemPowerAssertions.type(of: assertion) == SystemPowerAssertions.displaySleepType else {
            return "No display-sleep assertion while video plays"
        }
        smokeAssertion = assertion
        if let problem = smokeCheckOffered() { return problem }
        if let problem = smokeSend(MPRemoteCommandCenter.shared().pauseCommand, "pause") { return problem }
        print("OKP_SMOKE system=playing now_playing_state=playing now_playing_type=video "
              + "display_sleep_assertion=held remote_commands=offered remote_pause=success")
        fflush(stdout)
        return nil
    }

    /// Smoke, after two paused seconds: Now Playing shows the pause with a stopped
    /// position, the assertion is gone from IOKit, and playback resumes through the
    /// handler registered for the system's play command. Returns what failed.
    func smokeCheckPausedThenResume() -> String? {
        guard let center = nowPlayingCenter as? SystemNowPlayingCenter else { return "Now Playing is not the system's" }
        let (info, state) = center.current
        let rate = (info?[MPNowPlayingInfoPropertyPlaybackRate] as? NSNumber)?.doubleValue
        guard state == .paused, rate == 0 else {
            return "Now Playing reports state \(state.rawValue) at rate \(String(describing: rate)) while paused"
        }
        guard displaySleep.assertion == nil else { return "The display-sleep assertion is held while paused" }
        if let smokeAssertion, SystemPowerAssertions.type(of: smokeAssertion) != nil {
            return "IOKit still lists the display-sleep assertion while paused"
        }
        if let problem = smokeSend(MPRemoteCommandCenter.shared().playCommand, "play") { return problem }
        print("OKP_SMOKE system=paused now_playing_state=paused display_sleep_assertion=released remote_play=success")
        fflush(stdout)
        return nil
    }

    /// Smoke, after `shutdown`: the remote-command center offers nothing and holds no
    /// handler of the shell's. Returns what failed.
    func smokeCheckWithdrawn() -> String? {
        let offer = RemoteCommandOffer.read()
        guard offer.enabled.isEmpty else { return "The remote-command center still offers \(offer.enabled) after shutdown" }
        if let source = remoteSource as? SystemRemoteCommandSource,
           source.deliver(MPRemoteCommandCenter.shared().playCommand) != nil {
            return "The remote play command still has a handler after shutdown"
        }
        print("OKP_SMOKE system=closed remote_commands=withdrawn")
        fflush(stdout)
        return nil
    }
}
