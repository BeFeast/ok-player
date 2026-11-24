//! Presentation of the shared playlist for an Up Next panel.

use okp_core::playlist::PlaylistItem;
use serde::{Deserialize, Serialize};

use crate::protocol::MediaSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueRowState {
    Current,
    Next,
    Behind,
    Upcoming,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRow {
    pub index: usize,
    pub source: MediaSource,
    pub title: String,
    pub state: QueueRowState,
    pub can_play_next: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueView {
    pub rows: Vec<QueueRow>,
    pub current_index: Option<usize>,
    pub heading: Option<String>,
    pub scanning: bool,
}

impl QueueView {
    /// Project identity order, retaining the indices accepted by playlist commands.
    /// The caller resolves the current source and the playlist's next play target.
    pub fn project(
        items: &[PlaylistItem],
        current_index: Option<usize>,
        next_index: Option<usize>,
        media_title: Option<&str>,
        scanning: bool,
    ) -> Self {
        let current_index = current_index.filter(|index| *index < items.len());
        let rows = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let is_current = current_index == Some(index);
                let state = if is_current {
                    QueueRowState::Current
                } else if current_index.is_some() && next_index == Some(index) {
                    QueueRowState::Next
                } else if current_index.is_some_and(|current| index < current) {
                    QueueRowState::Behind
                } else {
                    QueueRowState::Upcoming
                };
                let title = media_title
                    .filter(|title| is_current && !title.is_empty())
                    .map_or_else(|| item.display_name(), str::to_owned);
                QueueRow {
                    index,
                    source: MediaSource::from_item(item),
                    title,
                    state,
                    can_play_next: current_index
                        .is_some_and(|current| index != current && index != current + 1),
                }
            })
            .collect();
        let heading = (items.len() > 1).then(|| {
            let current_folder = current_index.and_then(|index| match &items[index] {
                PlaylistItem::Local(path) => path.parent(),
                PlaylistItem::Url(_) => None,
            });
            let from_folder = current_folder.is_some_and(|folder| {
                items.iter().all(
                    |item| matches!(item, PlaylistItem::Local(path) if path.parent() == Some(folder)),
                )
            });
            let label = if from_folder { "FROM THIS FOLDER" } else { "QUEUE" };
            format!("{label} · {}", items.len())
        });
        Self {
            rows,
            current_index,
            heading,
            scanning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_projects_identity_order_and_the_supplied_next_target() {
        let items =
            ["one.mp4", "two.mp4", "three.mp4"].map(|path| PlaylistItem::Local(path.into()));
        let view = QueueView::project(&items, Some(1), Some(0), Some("Episode Two"), false);
        assert_eq!(view.heading.as_deref(), Some("FROM THIS FOLDER · 3"));
        assert_eq!(view.current_index, Some(1));
        assert_eq!(
            view.rows.iter().map(|row| row.state).collect::<Vec<_>>(),
            [
                QueueRowState::Next,
                QueueRowState::Current,
                QueueRowState::Upcoming,
            ]
        );
        assert_eq!(view.rows[1].title, "Episode Two");
        assert!(view.rows[0].can_play_next);
        assert!(!view.rows[1].can_play_next);
        assert!(!view.rows[2].can_play_next);
        assert_eq!(view.rows[0].index, 0);
    }

    #[test]
    fn a_stream_with_local_additions_has_a_queue_heading() {
        let items = [
            PlaylistItem::Url("http://example.com/stream.mp4".into()),
            PlaylistItem::Local("extra.mp4".into()),
        ];
        let view = QueueView::project(&items, Some(0), Some(1), None, false);
        assert_eq!(view.heading.as_deref(), Some("QUEUE · 2"));
        assert_eq!(view.rows[1].state, QueueRowState::Next);
    }

    #[test]
    fn single_item_is_pinned_without_a_folder_heading_or_next_badge() {
        let items = [PlaylistItem::Local("single.mp4".into())];
        let view = QueueView::project(&items, Some(0), Some(0), Some(""), true);
        assert_eq!(view.heading, None);
        assert!(view.scanning);
        assert_eq!(view.rows[0].state, QueueRowState::Current);
        assert_eq!(view.rows[0].title, "single.mp4");
    }
}
