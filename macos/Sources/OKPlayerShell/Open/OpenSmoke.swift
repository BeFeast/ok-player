import AppKit

// The smoke's check of the open route (`--smoke-test`, run by the macOS workflows).
extension PlayerApp {
    /// Once the fixture plays: the open event the smoke sent went through the router,
    /// which sent the session the playlist open of the fixture and had it taken; the
    /// playlist plays it; a subtitle beside it is planned onto the playing media; and
    /// the window takes file drops. Prints its `OKP_SMOKE open_route=…` line and
    /// returns nil when all hold.
    func smokeOpenFailure(fixture path: String) -> String? {
        let fixture = URL(fileURLWithPath: path).standardizedFileURL
        let expected = Command.playlist(action: .open(sources: [MediaSource(kind: .local, location: fixture.path)]))
        guard case .opened(let command, let result, _)? = openRouter.lastOutcome, command == expected else {
            let outcome = openRouter.lastOutcome.map(String.init(describing:)) ?? "the router opened nothing"
            return "The fixture's open event did not reach the session as its playlist open through the router "
                + "(\(outcome))"
        }
        let taken: String
        switch result {
        case .accepted: taken = "accepted"
        case .applied: taken = "applied"
        default: return "The session answered the fixture's playlist open with \(result)"
        }
        let playlist = client?.store.playlist ?? PlaylistState()
        guard let index = playlist.currentIndex, playlist.items.indices.contains(index),
              playlist.items[index].source.location == fixture.path else {
            return "The playlist does not play the fixture opened through the open route"
        }
        let subtitle = fixture.deletingPathExtension().appendingPathExtension("srt")
        guard openRouter.plan(for: [subtitle]) == .addSubtitles(paths: [subtitle.path]) else {
            return "A subtitle dropped on the playing fixture was not planned onto it"
        }
        guard window.contentView?.registeredDraggedTypes.contains(.fileURL) == true else {
            return "The window does not take file drops"
        }
        print("OKP_SMOKE open_route=application_open open_command=playlist_open open_result=\(taken) "
              + "playlist_items=\(playlist.items.count) subtitle_plan=add_subtitles drop_types=file_url")
        return nil
    }
}
