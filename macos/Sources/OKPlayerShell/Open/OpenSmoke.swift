import AppKit

// The smoke's check of the open route (`--smoke-test`, run by the macOS workflows).
extension PlayerApp {
    /// Once the fixture plays: the session planned it as a playlist open and the
    /// playlist plays it, so the open event went through the router; a subtitle beside
    /// it is planned onto the playing media; and the window takes file drops. Prints
    /// its `OKP_SMOKE open_route=…` line and returns nil when all hold.
    func smokeOpenFailure(fixture path: String) -> String? {
        let fixture = URL(fileURLWithPath: path).standardizedFileURL
        guard case .open(let sources) = openRouter.plan(for: [fixture]),
              sources == [MediaSource(kind: .local, location: fixture.path)] else {
            return "The session did not plan the fixture as a playlist open"
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
        print("OKP_SMOKE open_route=application_open open_plan=playlist_open playlist_items=\(playlist.items.count) "
              + "subtitle_plan=add_subtitles drop_types=file_url")
        return nil
    }
}
