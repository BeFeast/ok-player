//! Shared Settings information architecture and search routing.
//!
//! Shells own their native controls and page composition. This module owns the
//! portable page IDs, deterministic rail order, and the searchable destination
//! index so deep links and Settings search cannot drift between surfaces.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsPage {
    Appearance,
    Playback,
    Subtitles,
    Video,
    Audio,
    Shortcuts,
    Integration,
    Updates,
    Advanced,
    About,
}

pub const SETTINGS_RAIL_ORDER: [SettingsPage; 9] = [
    SettingsPage::Appearance,
    SettingsPage::Playback,
    SettingsPage::Subtitles,
    SettingsPage::Video,
    SettingsPage::Audio,
    SettingsPage::Shortcuts,
    SettingsPage::Integration,
    SettingsPage::Updates,
    SettingsPage::Advanced,
];

impl SettingsPage {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Playback => "playback",
            Self::Subtitles => "subtitles",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Shortcuts => "shortcuts",
            Self::Integration => "integration",
            Self::Updates => "updates",
            Self::Advanced => "advanced",
            Self::About => "about",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Playback => "Playback",
            Self::Subtitles => "Subtitles",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Shortcuts => "Shortcuts",
            Self::Integration => "Integration",
            Self::Updates => "Updates",
            Self::Advanced => "Advanced",
            Self::About => "About",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "appearance" => Some(Self::Appearance),
            "playback" => Some(Self::Playback),
            "subtitles" => Some(Self::Subtitles),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            "shortcuts" => Some(Self::Shortcuts),
            "integration" => Some(Self::Integration),
            "updates" => Some(Self::Updates),
            "advanced" => Some(Self::Advanced),
            "about" => Some(Self::About),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsSearchResult {
    pub label: &'static str,
    pub page: SettingsPage,
}

const SETTINGS_SEARCH_INDEX: &[SettingsSearchResult] = &[
    SettingsSearchResult {
        label: "Appearance",
        page: SettingsPage::Appearance,
    },
    SettingsSearchResult {
        label: "Playback",
        page: SettingsPage::Playback,
    },
    SettingsSearchResult {
        label: "Subtitles",
        page: SettingsPage::Subtitles,
    },
    SettingsSearchResult {
        label: "Video",
        page: SettingsPage::Video,
    },
    SettingsSearchResult {
        label: "Audio",
        page: SettingsPage::Audio,
    },
    SettingsSearchResult {
        label: "Shortcuts",
        page: SettingsPage::Shortcuts,
    },
    SettingsSearchResult {
        label: "Integration",
        page: SettingsPage::Integration,
    },
    SettingsSearchResult {
        label: "Private session",
        page: SettingsPage::Integration,
    },
    SettingsSearchResult {
        label: "History retention",
        page: SettingsPage::Integration,
    },
    SettingsSearchResult {
        label: "Keep history for",
        page: SettingsPage::Integration,
    },
    SettingsSearchResult {
        label: "Clear watch history",
        page: SettingsPage::Integration,
    },
    SettingsSearchResult {
        label: "Updates",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Current version",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Update channel",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Update feed",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Install mode",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Current update status",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Automatic checks",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Check for updates",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Open Releases",
        page: SettingsPage::Updates,
    },
    SettingsSearchResult {
        label: "Advanced",
        page: SettingsPage::Advanced,
    },
    SettingsSearchResult {
        label: "About",
        page: SettingsPage::About,
    },
];

pub fn search_settings(query: &str) -> Vec<SettingsSearchResult> {
    let terms = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Vec::new();
    }

    SETTINGS_SEARCH_INDEX
        .iter()
        .copied()
        .filter(|result| {
            let label = result.label.to_ascii_lowercase();
            let page = result.page.title().to_ascii_lowercase();
            terms
                .iter()
                .all(|term| label.contains(term) || page.contains(term))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_precedes_advanced_in_the_exact_rail_order() {
        assert_eq!(
            SETTINGS_RAIL_ORDER.map(SettingsPage::id),
            [
                "appearance",
                "playback",
                "subtitles",
                "video",
                "audio",
                "shortcuts",
                "integration",
                "updates",
                "advanced",
            ]
        );
    }

    #[test]
    fn updates_deep_link_and_major_controls_are_searchable() {
        assert_eq!(
            SettingsPage::from_id(" Updates "),
            Some(SettingsPage::Updates)
        );
        for query in [
            "updates",
            "current version",
            "channel",
            "feed",
            "install mode",
            "status",
            "automatic checks",
            "check updates",
            "open releases",
        ] {
            assert!(
                search_settings(query)
                    .iter()
                    .any(|result| result.page == SettingsPage::Updates),
                "missing Updates search route for {query}"
            );
        }
    }

    #[test]
    fn privacy_controls_route_to_integration() {
        for query in [
            "private session",
            "history retention",
            "keep history",
            "clear watch history",
        ] {
            assert!(
                search_settings(query)
                    .iter()
                    .any(|result| result.page == SettingsPage::Integration),
                "missing Integration search route for {query}"
            );
        }
    }
}
