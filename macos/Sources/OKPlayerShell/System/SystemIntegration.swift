import AppKit
import MediaPlayer

/// The shell's macOS integration: Now Playing, remote commands (media keys, Control
/// Center, AirPods), the display-sleep assertion, recent documents and the Dock menu.
/// Everything is derived from the session snapshot it is handed after each drained
/// batch; it keeps no playback state of its own.
final class SystemIntegration: NSObject {
    let remote: RemoteCommandController
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

    /// While on, loaded files are not added to the recent documents. Set by whoever
    /// follows the session's private mode (the session reports it once #878 lands).
    var privateSession: Bool {
        get { recents.privateSession }
        set { recents.privateSession = newValue }
    }

    /// Next and previous item for the remote commands and the Dock menu.
    /// TODO(#877): set this once the shell decodes the session playlist; until then the
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

    /// Smoke, while the fixture plays: the system's Now Playing names it as playing
    /// video, IOKit lists the display-sleep assertion, and then the pause goes out
    /// through the remote-command path. Returns what failed.
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
        let status = remote.handle(.pause)
        guard status == .success else { return "The remote pause command returned status \(status.rawValue)" }
        print("OKP_SMOKE system=playing now_playing_state=playing now_playing_type=video "
              + "display_sleep_assertion=held remote_pause=success")
        fflush(stdout)
        return nil
    }

    /// Smoke, after two paused seconds: Now Playing shows the pause with a stopped
    /// position, the assertion is gone from IOKit, and playback resumes through the
    /// remote-command path. Returns what failed.
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
        let status = remote.handle(.play)
        guard status == .success else { return "The remote play command returned status \(status.rawValue)" }
        print("OKP_SMOKE system=paused now_playing_state=paused display_sleep_assertion=released remote_play=success")
        fflush(stdout)
        return nil
    }
}
