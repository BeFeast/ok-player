import AppKit

enum AppearanceSelection: String, Codable {
    case light, auto
}

enum AppearanceTheme: String, Codable {
    case light, dark, auto

    var appearanceOverride: NSAppearance? {
        switch self {
        case .light: return NSAppearance(named: .aqua)
        case .dark: return NSAppearance(named: .darkAqua)
        case .auto: return nil
        }
    }
}

enum SettingsPage: String, Codable {
    case appearance, playback, subtitles, video, audio, shortcuts, integration, updates, advanced, about

    var symbol: String {
        switch self {
        case .appearance: return "sun.max"
        case .playback: return "play"
        case .subtitles: return "captions.bubble"
        case .video: return "display"
        case .audio: return "speaker.wave.2"
        case .shortcuts: return "keyboard"
        case .integration: return "link"
        case .updates: return "square.and.arrow.down"
        case .advanced: return "curlybraces"
        case .about: return "info.circle"
        }
    }
}

struct SettingsDestination: Codable, Equatable {
    let id: SettingsPage
    let title: String
    let available: Bool
}

struct SettingsSearchHit: Codable, Equatable {
    let label: String
    let destination: SettingsDestination
}

struct SettingsState: Codable, Equatable {
    let theme: AppearanceTheme
    let sections: [SettingsDestination]
    let about: SettingsDestination
}
