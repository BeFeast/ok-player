//! The history documents: the `history` command, query and event groups.
//!
//! They follow the rules of [`crate::protocol`]: externally tagged `snake_case`
//! enums whose variants are all objects, `null` for absent optional values. The
//! golden fixtures in `fixtures/history/` pin every shape.

use serde::{Deserialize, Serialize};

use crate::protocol::MediaSource;

/// `{"history":{"action":{...}}}` inside a command envelope. Resume, progress
/// and per-file memory need no command: the session does them as media opens,
/// plays and changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryAction {
    /// Turn the private session on or off. While it is on, nothing new is
    /// recorded and nothing remembered is applied to an open; the history stays
    /// readable and entries can still be removed.
    SetPrivate { enabled: bool },
    /// Remove one entry. The media itself is untouched. Removing the media that
    /// is playing keeps it playing and stops recording it until it is opened
    /// again.
    Remove { source: MediaSource },
}

/// The answer to a history action, in `{"result":{"history":{"result":{...}}}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryActionResult {
    /// Carried out.
    Done {},
    /// There was nothing to change.
    Unchanged {},
    /// The history file could not be written; nothing changed.
    Failed { message: String },
}

/// `{"history":{"query":{...}}}` inside a query envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryQuery {
    /// The newest entries for the welcome screen's Continue watching shelf.
    WelcomeShelf {
        limit: u32,
        /// The shell's current offset from UTC in seconds. `when` labels are
        /// local to it.
        utc_offset_seconds: i32,
    },
    /// Every entry for the History page, newest first and grouped by day,
    /// keeping those whose title, folder or location contains `search`
    /// (ignoring case; empty keeps all).
    Page {
        #[serde(default)]
        search: String,
        /// The shell's current offset from UTC in seconds. Days and `when`
        /// labels are local to it.
        utc_offset_seconds: i32,
    },
}

/// The answer to a history query, in `{"result":{"history":{"result":{...}}}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryQueryResult {
    WelcomeShelf {
        private_session: bool,
        entries: Vec<HistoryEntry>,
    },
    Page {
        private_session: bool,
        /// The history file could not be read at startup; the page is empty
        /// and the next recorded change replaces the file.
        read_failed: bool,
        /// The line under the page title, e.g. "Everything you’ve opened · kept forever".
        summary: String,
        /// The line after the last entry.
        end_cap: String,
        /// Non-empty day groups, newest first.
        sections: Vec<HistorySection>,
    },
}

/// One day group of the History page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistorySection {
    pub bucket: HistoryBucket,
    /// The group header, e.g. "TODAY".
    pub header: String,
    pub entries: Vec<HistoryEntry>,
}

/// One remembered source, ready to render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// What to open to play it again.
    pub source: MediaSource,
    pub title: String,
    /// The folder breadcrumb, or the location itself when it has no folder.
    pub location: String,
    /// Last position (seconds); 0 once finished.
    pub position: f64,
    /// Duration (seconds); 0 when unknown.
    pub duration: f64,
    /// Watched fraction, 0 to 1.
    pub progress: f64,
    pub state: HistoryState,
    /// "Finished", "23m left", "2m in · 4%" or "Duration unknown".
    pub state_label: String,
    /// When it was last opened: "Today 21:14", "Yest. 16:40", "Tue 21:48", "12 Jun".
    pub when_label: String,
    /// When it was last opened, in Unix seconds.
    pub updated_at_unix: i64,
    /// Where opening it starts (seconds), or `null` for the start.
    pub resume_from: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryState {
    Finished,
    Progress,
    Barely,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryBucket {
    Today,
    Yesterday,
    EarlierThisWeek,
    Earlier,
}

/// A history event, in `{"history":{"event":{...}}}` inside an event batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryEvent {
    /// The media that just loaded started from its remembered position
    /// (seconds), as the resume rules allow.
    Resumed { position: f64 },
    /// What the history queries answer has changed; query again to refresh.
    Changed {},
    /// The private session was turned on or off.
    PrivateSession { enabled: bool },
    /// The history file could not be written. The change is kept and written
    /// with the next save; this is reported once until a save succeeds.
    SaveFailed { message: String },
}

impl From<okp_core::history_format::HistoryStateKind> for HistoryState {
    fn from(kind: okp_core::history_format::HistoryStateKind) -> Self {
        use okp_core::history_format::HistoryStateKind;
        match kind {
            HistoryStateKind::Finished => Self::Finished,
            HistoryStateKind::Progress => Self::Progress,
            HistoryStateKind::Barely => Self::Barely,
        }
    }
}

impl From<okp_core::history_format::HistoryBucket> for HistoryBucket {
    fn from(bucket: okp_core::history_format::HistoryBucket) -> Self {
        use okp_core::history_format::HistoryBucket as Core;
        match bucket {
            Core::Today => Self::Today,
            Core::Yesterday => Self::Yesterday,
            Core::EarlierThisWeek => Self::EarlierThisWeek,
            Core::Earlier => Self::Earlier,
        }
    }
}
