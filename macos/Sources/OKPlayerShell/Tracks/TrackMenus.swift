import Foundation

// The audio and subtitle menus as plain values, built from the session's `tracks`
// view. The OSC popovers and the menu bar's Audio and Subtitles submenus render the
// same items, so what they list and what choosing an item sends is decided here
// and tested without AppKit. Every label and readout is the session's.

/// What choosing a track menu item does.
enum TrackMenuAction: Equatable {
    /// `Command.selectTrack` with this kind and id; `nil` turns subtitles off.
    case select(kind: TrackKind, id: Int64?)
    /// Open the subtitle file panel, which the shell carries out (`add-subtitle`).
    case addSubtitle
    /// Run a registry command the session carries out: the delay and size steps.
    case invoke(id: String)
}

/// One item of a track menu.
struct TrackMenuItem: Equatable {
    enum Kind: Equatable {
        /// A track to switch to; checked while it plays.
        case choice
        /// An action such as Add Subtitle File….
        case command
        /// Text in place of an empty list.
        case placeholder
        case separator
    }

    var kind: Kind
    var title = ""
    var checked = false
    var enabled = false
    var action: TrackMenuAction?

    static let separator = TrackMenuItem(kind: .separator)
}

/// A subtitle adjustment row of the subtitle popover: the value and its two steps.
struct SubtitleAdjustment: Equatable {
    var title: String
    /// The session's readout: `+50 ms`, `110%`.
    var value: String
    var enabled: Bool
    var decrease: TrackMenuAction
    var increase: TrackMenuAction
    var decreaseHelp: String
    var increaseHelp: String
}

enum TrackMenus {
    /// The registry command that opens the subtitle file panel; the shell carries it out.
    static let addSubtitle = "add-subtitle"
    static let delayForward = "subtitle-delay-forward"
    static let delayBack = "subtitle-delay-back"
    static let sizeUp = "subtitle-size-up"
    static let sizeDown = "subtitle-size-down"
    /// The delay and size steps, which show a readout.
    static let steps: Set<String> = [delayForward, delayBack, sizeUp, sizeDown]
    /// The registry rows the menu bar shows as the Audio and Subtitles submenus.
    static let audioCommand = "audio-track"
    static let subtitlesCommand = "subtitles"

    /// The Audio menu: one item per audio track, the playing one checked.
    static func audioItems(_ view: TracksView?) -> [TrackMenuItem] {
        let rows = view?.audio ?? []
        guard !rows.isEmpty else {
            return [TrackMenuItem(kind: .placeholder, title: "No audio tracks")]
        }
        return rows.map { choice(.audio, $0, enabled: view?.adjustable ?? false) }
    }

    /// The Subtitles menu: Off and one item per subtitle track, then Add Subtitle File….
    static func subtitleItems(_ view: TracksView?) -> [TrackMenuItem] {
        let enabled = view?.adjustable ?? false
        let rows = view?.subtitles ?? [TrackChoice(id: nil, label: "Off", selected: true)]
        return rows.map { choice(.subtitle, $0, enabled: enabled) } + [
            .separator,
            TrackMenuItem(kind: .command, title: "Add Subtitle File…", enabled: enabled, action: .addSubtitle),
        ]
    }

    /// The items for `kind`.
    static func items(_ kind: TrackKind, _ view: TracksView?) -> [TrackMenuItem] {
        kind == .audio ? audioItems(view) : subtitleItems(view)
    }

    /// The delay and size rows of the subtitle popover, stepping like Z / Shift+Z and [ / ].
    static func adjustments(_ view: TracksView?) -> [SubtitleAdjustment] {
        guard let view else { return [] }
        return [
            SubtitleAdjustment(title: "Delay", value: view.subtitleDelay.label, enabled: view.adjustable,
                               decrease: .invoke(id: delayBack), increase: .invoke(id: delayForward),
                               decreaseHelp: "Show subtitles earlier", increaseHelp: "Show subtitles later"),
            SubtitleAdjustment(title: "Size", value: view.subtitleScale.label, enabled: view.adjustable,
                               decrease: .invoke(id: sizeDown), increase: .invoke(id: sizeUp),
                               decreaseHelp: "Make subtitles smaller", increaseHelp: "Make subtitles larger"),
        ]
    }

    /// The session command `action` sends; `nil` for the file panel, which the shell opens.
    static func command(for action: TrackMenuAction) -> Command? {
        switch action {
        case .select(let kind, let id): return .selectTrack(kind: kind, id: id)
        case .invoke(let id): return .invokeCommand(id: id)
        case .addSubtitle: return nil
        }
    }

    /// The on-screen readout after the registry command `id` ran; `nil` for a
    /// command without one.
    static func readout(after id: String, in view: TracksView) -> String? {
        switch id {
        case delayForward, delayBack: return "Subtitle delay: \(view.subtitleDelay.label)"
        case sizeUp, sizeDown: return "Subtitle size: \(view.subtitleScale.label)"
        default: return nil
        }
    }

    private static func choice(_ kind: TrackKind, _ row: TrackChoice, enabled: Bool) -> TrackMenuItem {
        TrackMenuItem(kind: .choice, title: row.label, checked: row.selected, enabled: enabled,
                      action: .select(kind: kind, id: row.id))
    }
}

/// The subtitle files the file panel offers: core `media_formats::SUBTITLE_EXTENSIONS`.
/// The session refuses any other file, whatever the panel lets through.
enum SubtitleFiles {
    static let extensions = ["srt", "ass", "ssa", "sub", "vtt", "idx", "sup"]
}
