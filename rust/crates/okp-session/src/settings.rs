use okp_core::settings::AppearanceTheme;
use okp_core::settings_navigation::{
    SETTINGS_RAIL_ORDER, SettingsPage, SettingsSearchResult, search_settings_with,
};
use okp_core::settings_store::SettingsStore;
use serde::{Deserialize, Serialize};

use crate::protocol::{DispatchResult, RejectReason};

/// The controls offered by this shell. A previously stored Dark theme can
/// still be read and displayed through `SettingsView`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceSelection {
    Light,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsView {
    pub theme: AppearanceTheme,
    pub sections: Vec<SettingsDestination>,
    pub about: SettingsDestination,
}

impl SettingsView {
    pub(crate) fn new(store: Option<&SettingsStore>) -> Self {
        Self {
            theme: store.map_or(AppearanceTheme::Auto, SettingsStore::appearance_theme),
            sections: SETTINGS_RAIL_ORDER.into_iter().map(Into::into).collect(),
            about: SettingsPage::About.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsDestination {
    pub id: SettingsPage,
    pub title: String,
    pub available: bool,
}

impl From<SettingsPage> for SettingsDestination {
    fn from(page: SettingsPage) -> Self {
        Self {
            id: page,
            title: page.title().to_owned(),
            available: matches!(page, SettingsPage::Appearance | SettingsPage::Updates | SettingsPage::About),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSearchHit {
    pub label: String,
    pub destination: SettingsDestination,
}

pub(crate) fn search(text: &str) -> Vec<SettingsSearchHit> {
    let appearance = [
        "App theme",
        "Player surface",
        "Window chrome",
        "Fullscreen caption",
        "Accent",
    ]
    .map(|label| SettingsSearchResult {
        label,
        page: SettingsPage::Appearance,
    });
    search_settings_with(text, &appearance)
        .into_iter()
        .map(|hit| SettingsSearchHit {
            label: hit.label.to_owned(),
            destination: hit.page.into(),
        })
        .collect()
}

pub(crate) fn save_theme(
    store: Option<&mut SettingsStore>,
    theme: AppearanceSelection,
) -> DispatchResult {
    let Some(store) = store else {
        return DispatchResult::Rejected {
            reason: RejectReason::Unavailable,
            message: "No settings profile is available.".to_owned(),
        };
    };
    let theme = match theme {
        AppearanceSelection::Light => AppearanceTheme::Light,
        AppearanceSelection::Auto => AppearanceTheme::Auto,
    };
    match SettingsStore::edit_appearance_at(store.path().to_owned(), theme) {
        Ok((updated, changed)) => {
            *store = updated;
            if changed {
                DispatchResult::Applied {}
            } else {
                DispatchResult::NoOp {}
            }
        }
        Err(error) => DispatchResult::Rejected {
            reason: RejectReason::Unavailable,
            message: format!("Could not save the app theme: {error}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        Command, CommandEnvelope, Query, QueryEnvelope, QueryReply, QueryResult, SCHEMA_VERSION,
    };

    #[test]
    fn settings_queries_and_command_match_the_swift_fixtures() {
        let command = CommandEnvelope {
            schema_version: SCHEMA_VERSION,
            command: Command::SetAppearanceTheme {
                theme: AppearanceSelection::Light,
            },
        };
        let fixture = include_str!("../fixtures/settings/command-theme.json");
        assert_eq!(
            serde_json::from_str::<CommandEnvelope>(fixture).unwrap(),
            command
        );
        assert_eq!(
            serde_json::to_value(&command).unwrap(),
            serde_json::from_str::<serde_json::Value>(fixture).unwrap()
        );
        let queries = [
            (
                Query::Settings {},
                include_str!("../fixtures/settings/query-settings.json"),
            ),
            (
                Query::SettingsSearch {
                    text: "history retention".into(),
                },
                include_str!("../fixtures/settings/query-search.json"),
            ),
        ];
        for (query, fixture) in queries {
            let expected = QueryEnvelope {
                schema_version: SCHEMA_VERSION,
                query,
            };
            assert_eq!(
                serde_json::from_str::<QueryEnvelope>(fixture).unwrap(),
                expected
            );
        }
        let replies = [
            (
                QueryResult::Settings {
                    settings: SettingsView::new(None),
                },
                include_str!("../fixtures/settings/reply-settings.json"),
            ),
            (
                QueryResult::SettingsSearch {
                    hits: search("history retention"),
                },
                include_str!("../fixtures/settings/reply-search.json"),
            ),
        ];
        for (result, fixture) in replies {
            let expected = QueryReply {
                schema_version: SCHEMA_VERSION,
                result,
            };
            assert_eq!(
                serde_json::to_value(&expected).unwrap(),
                serde_json::from_str::<serde_json::Value>(fixture).unwrap()
            );
        }
        assert!(search("no such preference").is_empty());
        assert!(search("app theme")[0].destination.available);
    }
}
