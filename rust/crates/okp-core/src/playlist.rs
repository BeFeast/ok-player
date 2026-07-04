//! Playlist model — port of `src/OkPlayer.Core/Playlist.cs`; the C# suite in
//! `tests/OkPlayer.Tests/PlaylistTests.cs` is the executable spec. The port also absorbs the
//! Linux shell's playlist engine — queue insert modes, drag reorder, row removal, wrap-always
//! transport navigation and the auto-advance toggle — so the whole repeat/shuffle/queue/advance
//! state machine lives here and a shell only renders the list and drives the player.
//!
//! Navigation reads the neighbour in the active *play order* (shuffled or natural), wrapping
//! when Repeat=All; auto-advance additionally honours Repeat=One by replaying the current item.
//! The cursor moves only through [`Playlist::set_current`] / [`Playlist::set_current_index`]
//! (and the stepping helpers built on them), so a caller can peek-then-open without desyncing.
//! See `docs/core-compatibility.md` for the intentional divergences from the C# module.

use std::path::{Path, PathBuf};

use crate::media_formats;
use crate::natural_compare;

/// How the playlist behaves at its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    /// Stop at the last item.
    #[default]
    Off,
    /// Replay the current item on auto-advance.
    One,
    /// Wrap around (last → first, first → last).
    All,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }

    /// Stable identifier persisted in settings (never localized).
    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::One => "one",
            Self::All => "all",
        }
    }

    pub fn from_settings_value(value: &str) -> Self {
        match value {
            "one" => Self::One,
            "all" => Self::All,
            _ => Self::Off,
        }
    }
}

/// Where queued media lands relative to the current item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueInsertMode {
    /// Add to the end of the playlist, skipping entries already queued.
    Append,
    /// Insert right after the current item, moving already-queued entries there.
    PlayNext,
}

/// One playlist entry: a local media file or a stream URL.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum PlaylistItem {
    Local(PathBuf),
    Url(String),
}

impl PlaylistItem {
    /// Wrap a local path if it has a recognized media extension.
    pub fn local(path: PathBuf) -> Option<Self> {
        media_formats::is_media(&path).then_some(Self::Local(path))
    }

    /// Parse one already-resolved M3U entry (see [`crate::m3u::parse`]) into an item, dropping
    /// entries that are neither a playable URL nor a media file.
    pub fn from_m3u_entry(entry: &str) -> Option<Self> {
        if media_formats::is_playable_url(Some(entry)) {
            return Some(Self::Url(entry.to_owned()));
        }

        Self::local(PathBuf::from(entry))
    }

    /// Whether this entry is the loaded media, given what the player reports as current.
    pub fn is_current(&self, current_file: Option<&Path>, current_url: Option<&str>) -> bool {
        match self {
            Self::Local(path) => current_file.is_some_and(|current| current == path),
            Self::Url(url) => current_url.is_some_and(|current| current == url),
        }
    }

    /// Short human-readable label: the file name, or the last URL segment.
    pub fn display_name(&self) -> String {
        match self {
            Self::Local(path) => path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| path.display().to_string()),
            Self::Url(url) => url
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| url.to_owned()),
        }
    }

    /// Full path or URL, for tooltips and secondary labels.
    pub fn display_location(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Url(url) => url.to_owned(),
        }
    }

    /// The line this entry serializes to in an M3U document.
    pub fn m3u_entry(&self) -> String {
        match self {
            Self::Local(path) => path.to_string_lossy().into_owned(),
            Self::Url(url) => url.to_owned(),
        }
    }

    // Natural-sort key; matches the C# module sorting full path strings.
    fn order_key(&self) -> Option<&str> {
        match self {
            Self::Local(path) => path.to_str(),
            Self::Url(url) => Some(url.as_str()),
        }
    }
}

// Non-zero fallback so shuffle stays usable when a shell never seeds (xorshift locks on zero;
// next_shuffle_value guards that too, but a distinctive default keeps sequences well-mixed).
const DEFAULT_SHUFFLE_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// A playlist: the items in a stable identity order, a cursor, and the play modes (repeat,
/// shuffle, auto-advance). Pure and testable — no player, filesystem, or clock access; a shell
/// feeds loads/clicks in and reads which item to hand the player.
#[derive(Clone, Debug)]
pub struct Playlist {
    /// Items in identity order (natural-sorted for a folder, document order for an M3U).
    items: Vec<PlaylistItem>,
    /// Playback order: indices into `items` (identity unless shuffled, current kept first).
    order: Vec<usize>,
    /// Index into `items` of the playing entry.
    current_index: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    auto_advance: bool,
    shuffle_seed: u64,
}

impl Default for Playlist {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            current_index: None,
            repeat: RepeatMode::Off,
            shuffle: false,
            auto_advance: true,
            shuffle_seed: DEFAULT_SHUFFLE_SEED,
        }
    }
}

impl Playlist {
    /// Build a playlist with the cursor on `current` (if present). `sort` natural-sorts the
    /// items (the folder case); pass `false` to keep the given order (an M3U's order matters).
    pub fn from_items(
        items: Vec<PlaylistItem>,
        current: Option<&PlaylistItem>,
        sort: bool,
    ) -> Self {
        let mut playlist = Self {
            items,
            ..Self::default()
        };
        if sort {
            playlist.items.sort_by(|left, right| {
                natural_compare::compare(left.order_key(), right.order_key())
            });
        }
        playlist.current_index = current.and_then(|current| playlist.index_of(current));
        playlist.rebuild_order();
        playlist
    }

    /// Seed the shuffle RNG (a shell passes entropy such as the clock; tests pass a constant).
    pub fn reseed(&mut self, seed: u64) {
        self.shuffle_seed = seed;
    }

    pub fn items(&self) -> &[PlaylistItem] {
        &self.items
    }

    pub fn get(&self, index: usize) -> Option<&PlaylistItem> {
        self.items.get(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current(&self) -> Option<&PlaylistItem> {
        self.current_index.and_then(|index| self.items.get(index))
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Shuffle the play order (the current item stays first); turning it off restores the
    /// identity order.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle != shuffle {
            self.shuffle = shuffle;
            self.rebuild_order();
        }
    }

    pub fn auto_advance(&self) -> bool {
        self.auto_advance
    }

    /// Linux extension over the C# module: gates end-of-file advancement (but not Repeat=One,
    /// which replays regardless). Defaults to on, which is the fixed C# behavior.
    pub fn set_auto_advance(&mut self, auto_advance: bool) {
        self.auto_advance = auto_advance;
    }

    /// The next item in play order without moving the cursor (wraps when Repeat=All), or `None`
    /// at the end. Repeat=One does not affect manual next — see [`Self::auto_advance_target`].
    pub fn peek_next(&self) -> Option<&PlaylistItem> {
        self.neighbour(1).map(|index| &self.items[index])
    }

    pub fn peek_prev(&self) -> Option<&PlaylistItem> {
        self.neighbour(-1).map(|index| &self.items[index])
    }

    pub fn has_next(&self) -> bool {
        self.peek_next().is_some()
    }

    pub fn has_prev(&self) -> bool {
        self.peek_prev().is_some()
    }

    /// What to play when the current item ends: the same item when Repeat=One, nothing when
    /// auto-advance is off, otherwise [`Self::peek_next`].
    pub fn auto_advance_target(&self) -> Option<&PlaylistItem> {
        self.auto_advance_target_index()
            .map(|index| &self.items[index])
    }

    /// [`Self::auto_advance_target`] as an index into [`Self::items`] — unambiguous when the
    /// playlist repeats an entry, so a shell can load the item and then commit the cursor with
    /// [`Self::set_current_index`].
    pub fn auto_advance_target_index(&self) -> Option<usize> {
        if self.repeat == RepeatMode::One {
            return self.current_index;
        }
        if !self.auto_advance {
            return None;
        }
        self.neighbour(1)
    }

    /// What to try after the current item fails to load or decode: the next item in play order,
    /// skipping the one that just failed. Unlike [`Self::auto_advance_target_index`] it never
    /// replays the failed item (so a broken file under Repeat=One doesn't loop) and never wraps
    /// (so a fully broken queue terminates at the no-media surface instead of cycling forever). It
    /// also ignores auto-advance: a failure isn't a user-intended stop, so the queue is kept alive
    /// by moving on. `None` when the failed item is last in play order — the shell then clears to
    /// the welcome surface.
    pub fn advance_after_error_index(&self) -> Option<usize> {
        let current = self.current_index?;
        let position = self.order.iter().position(|&index| index == current)?;
        self.order.get(position + 1).copied()
    }

    /// Advance the cursor to the next item in play order and return it (`None` at the end).
    // Named for parity with the C# module; a playlist is not an iterator (prev, repeat, wrap).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<PlaylistItem> {
        self.step(1)
    }

    pub fn prev(&mut self) -> Option<PlaylistItem> {
        self.step(-1)
    }

    fn step(&mut self, direction: isize) -> Option<PlaylistItem> {
        let index = self.neighbour(direction)?;
        self.set_current_index(index);
        Some(self.items[index].clone())
    }

    /// Move the cursor one step in play order, wrapping at both ends regardless of the repeat
    /// mode — the transport-button behavior (Repeat only governs what happens at end of file).
    /// `None` when there is nothing to move to (fewer than two items).
    pub fn step_wrapping(&mut self, direction: isize) -> Option<PlaylistItem> {
        let target = self.peek_wrapping_index(direction)?;
        self.set_current_index(target);
        Some(self.items[target].clone())
    }

    /// The [`Self::step_wrapping`] target as an index into [`Self::items`], without moving the
    /// cursor — a shell can attempt the load and commit the cursor only on success.
    pub fn peek_wrapping_index(&self, direction: isize) -> Option<usize> {
        if self.items.len() < 2 {
            return None;
        }

        let current = self.current_index.unwrap_or(0);
        let position = self.order.iter().position(|&index| index == current)? as isize;
        let len = self.order.len() as isize;
        Some(self.order[(position + direction).rem_euclid(len) as usize])
    }

    /// Re-point the cursor at `item` if present. A sequential step keeps the order; jumping
    /// elsewhere while shuffled re-shuffles the remaining order (new current first) so a click
    /// never skips the items between, and a wrap reshuffles the next cycle.
    pub fn set_current(&mut self, item: &PlaylistItem) -> bool {
        let Some(index) = self.index_of(item) else {
            return false;
        };
        self.set_current_index(index)
    }

    /// [`Self::set_current`] addressed by index — what a click on the visible list means, and
    /// unambiguous when an M3U repeats an entry.
    pub fn set_current_index(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        if Some(index) == self.current_index {
            return true; // already current → no-op
        }

        if self.shuffle && self.current_index.is_some() {
            let old_position = self
                .order
                .iter()
                .position(|&order_index| Some(order_index) == self.current_index);
            let sequential = old_position.is_some_and(|position| {
                self.order.get(position + 1) == Some(&index)
                    || (position > 0 && self.order.get(position - 1) == Some(&index))
            });
            self.current_index = Some(index);
            if !sequential {
                self.rebuild_order();
            }
        } else {
            self.current_index = Some(index);
        }
        true
    }

    /// Adopt the playlist built for a newly loaded item, cursor on `current`. Identical items
    /// keep the current play order (a same-folder load is just a cursor move); new contents
    /// rebuild it.
    pub fn reset(&mut self, items: Vec<PlaylistItem>, current: &PlaylistItem) {
        if self.items == items {
            // The cursor may already sit on a later occurrence of `current` (set by index when
            // the playlist repeats an entry); re-finding by equality would snap it back to the
            // first duplicate and desync navigation from the visible position.
            if self.current() != Some(current) {
                self.set_current(current);
            }
            return;
        }

        self.items = items;
        self.current_index = self.index_of(current);
        self.rebuild_order();
    }

    /// Drop all items (media closed). Play modes and the RNG state survive.
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.current_index = None;
    }

    /// Queue `additions` around `current_file` per `mode`, returning how many were queued, or
    /// `None` (playlist untouched) when nothing new would be added. `current_file` is inserted
    /// first if missing so the queue always builds around the playing file, and the cursor lands
    /// on it.
    pub fn queue_insert(
        &mut self,
        current_file: &Path,
        additions: Vec<PathBuf>,
        mode: QueueInsertMode,
    ) -> Option<usize> {
        let mut items = self.items.clone();
        if items.is_empty() {
            items.push(PlaylistItem::Local(current_file.to_path_buf()));
        }
        if !items
            .iter()
            .any(|item| matches!(item, PlaylistItem::Local(path) if path.as_path() == current_file))
        {
            items.insert(0, PlaylistItem::Local(current_file.to_path_buf()));
        }

        let additions = additions
            .into_iter()
            .filter(|path| path.as_path() != current_file)
            .collect::<Vec<_>>();
        if additions.is_empty() {
            return None;
        }

        let count = match mode {
            QueueInsertMode::Append => {
                let additions = additions
                    .into_iter()
                    .filter(|path| {
                        !items.iter().any(
                            |item| matches!(item, PlaylistItem::Local(item_path) if item_path == path),
                        )
                    })
                    .map(PlaylistItem::Local)
                    .collect::<Vec<_>>();
                if additions.is_empty() {
                    return None;
                }

                let count = additions.len();
                items.extend(additions);
                count
            }
            QueueInsertMode::PlayNext => {
                items.retain(|item| {
                    !additions.iter().any(
                        |addition| matches!(item, PlaylistItem::Local(path) if path == addition),
                    )
                });
                let current_index = items
                    .iter()
                    .position(
                        |item| matches!(item, PlaylistItem::Local(path) if path.as_path() == current_file),
                    )
                    .unwrap_or(0);
                let count = additions.len();
                items.splice(
                    current_index + 1..current_index + 1,
                    additions.into_iter().map(PlaylistItem::Local),
                );
                count
            }
        };

        self.current_index = items.iter().position(
            |item| matches!(item, PlaylistItem::Local(path) if path.as_path() == current_file),
        );
        self.items = items;
        self.rebuild_order();
        Some(count)
    }

    /// Move the item at `from` so it sits at `to` (positions after the removal), keeping the
    /// cursor on the item it was on. `false` for out-of-range or no-op moves.
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || from == to {
            return false;
        }

        let item = self.items.remove(from);
        let target = to.min(self.items.len());
        self.items.insert(target, item);
        self.current_index = self.current_index.map(|current| {
            if current == from {
                return target;
            }
            let current = if current > from { current - 1 } else { current };
            if current >= target {
                current + 1
            } else {
                current
            }
        });
        self.rebuild_order();
        true
    }

    /// Remove the item at `index`. Refuses to remove the playing item or the last one left.
    pub fn remove(&mut self, index: usize) -> bool {
        if self.items.len() <= 1 || index >= self.items.len() {
            return false;
        }
        if Some(index) == self.current_index {
            return false;
        }

        self.items.remove(index);
        self.current_index = self.current_index.map(|current| {
            if current > index {
                current - 1
            } else {
                current
            }
        });
        self.rebuild_order();
        true
    }

    fn neighbour(&self, step: isize) -> Option<usize> {
        let current = self.current_index?;
        if self.order.is_empty() {
            return None;
        }

        let position = self.order.iter().position(|&index| index == current)? as isize + step;
        let len = self.order.len() as isize;
        let position = if position < 0 || position >= len {
            if self.repeat != RepeatMode::All {
                return None;
            }
            position.rem_euclid(len) // wrap
        } else {
            position
        };
        Some(self.order[position as usize])
    }

    fn rebuild_order(&mut self) {
        self.order = (0..self.items.len()).collect();
        if !self.shuffle {
            return;
        }

        for index in (1..self.order.len()).rev() {
            // Fisher–Yates
            let swap_with = (next_shuffle_value(&mut self.shuffle_seed) as usize) % (index + 1);
            self.order.swap(index, swap_with);
        }
        if let Some(current) = self.current_index {
            // Keep the playing item at the front so it isn't skipped.
            self.order.retain(|&index| index != current);
            self.order.insert(0, current);
        }
    }

    fn index_of(&self, item: &PlaylistItem) -> Option<usize> {
        self.items.iter().position(|existing| existing == item)
    }
}

// xorshift64: deterministic given the seed, never yields zero (a zero seed is bumped to one).
fn next_shuffle_value(seed: &mut u64) -> u64 {
    let mut value = (*seed).max(1);
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *seed = value;
    value
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn local(path: &str) -> PlaylistItem {
        PlaylistItem::Local(PathBuf::from(path))
    }

    fn url(url: &str) -> PlaylistItem {
        PlaylistItem::Url(url.to_owned())
    }

    // Intentionally unsorted input, as in the C# suite.
    fn folder() -> Vec<PlaylistItem> {
        vec![
            local(r"C:\v\ep10.mkv"),
            local(r"C:\v\ep1.mkv"),
            local(r"C:\v\ep2.mkv"),
        ]
    }

    fn folder_playlist(current: &str) -> Playlist {
        Playlist::from_items(folder(), Some(&local(current)), true)
    }

    #[test]
    fn construct_sorts_and_lands_on_current() {
        let playlist = folder_playlist(r"C:\v\ep2.mkv");

        assert_eq!(playlist.len(), 3);
        assert_eq!(
            playlist.items(),
            [
                local(r"C:\v\ep1.mkv"),
                local(r"C:\v\ep2.mkv"),
                local(r"C:\v\ep10.mkv"),
            ]
        );
        assert_eq!(playlist.current_index(), Some(1));
        assert_eq!(playlist.current(), Some(&local(r"C:\v\ep2.mkv")));
    }

    #[test]
    fn next_prev_walk_the_list() {
        let mut playlist = folder_playlist(r"C:\v\ep1.mkv");

        assert!(playlist.has_next());
        assert!(!playlist.has_prev());
        assert_eq!(playlist.next(), Some(local(r"C:\v\ep2.mkv")));
        assert_eq!(playlist.next(), Some(local(r"C:\v\ep10.mkv")));
        assert!(!playlist.has_next());
        assert_eq!(playlist.next(), None); // at the end
        assert_eq!(playlist.prev(), Some(local(r"C:\v\ep2.mkv")));
        assert_eq!(playlist.prev(), Some(local(r"C:\v\ep1.mkv")));
        assert_eq!(playlist.prev(), None); // at the start
    }

    #[test]
    fn current_not_in_folder_has_no_neighbours() {
        let playlist = folder_playlist(r"C:\other\x.mkv");

        assert_eq!(playlist.current_index(), None);
        assert!(!playlist.has_next());
        assert!(!playlist.has_prev());
        assert_eq!(playlist.current(), None);
    }

    #[test]
    fn set_current_repoints_when_present_and_ignores_misses() {
        let mut playlist = folder_playlist(r"C:\v\ep1.mkv");

        assert!(playlist.set_current(&local(r"C:\v\ep10.mkv")));
        assert_eq!(playlist.current(), Some(&local(r"C:\v\ep10.mkv")));
        assert!(!playlist.set_current(&local(r"C:\v\missing.mkv")));
        assert_eq!(playlist.current(), Some(&local(r"C:\v\ep10.mkv"))); // unchanged on miss
    }

    #[test]
    fn peek_returns_neighbours_without_moving_cursor() {
        let mut playlist = folder_playlist(r"C:\v\ep1.mkv");

        assert_eq!(playlist.peek_next(), Some(&local(r"C:\v\ep2.mkv")));
        assert_eq!(playlist.peek_prev(), None); // at the start
        assert_eq!(playlist.current_index(), Some(0)); // peeking did not move the cursor

        playlist.set_current(&local(r"C:\v\ep10.mkv")); // last item
        assert_eq!(playlist.peek_next(), None);
        assert_eq!(playlist.peek_prev(), Some(&local(r"C:\v\ep2.mkv")));
        assert_eq!(playlist.current_index(), Some(2));
    }

    #[test]
    fn repeat_off_stops_at_end() {
        let playlist = folder_playlist(r"C:\v\ep10.mkv"); // last item, Repeat=Off (default)

        assert_eq!(playlist.peek_next(), None);
        assert_eq!(playlist.auto_advance_target(), None);
    }

    #[test]
    fn repeat_all_wraps_at_both_ends() {
        let mut last = folder_playlist(r"C:\v\ep10.mkv");
        last.set_repeat(RepeatMode::All);
        assert_eq!(last.peek_next(), Some(&local(r"C:\v\ep1.mkv"))); // end → first

        let mut first = folder_playlist(r"C:\v\ep1.mkv");
        first.set_repeat(RepeatMode::All);
        assert_eq!(first.peek_prev(), Some(&local(r"C:\v\ep10.mkv"))); // start → last
    }

    #[test]
    fn repeat_one_replays_on_auto_advance_but_manual_next_still_moves() {
        let mut playlist = folder_playlist(r"C:\v\ep2.mkv");
        playlist.set_repeat(RepeatMode::One);

        // EOF replays the current item.
        assert_eq!(
            playlist.auto_advance_target(),
            Some(&local(r"C:\v\ep2.mkv"))
        );
        // A manual hop still advances.
        assert_eq!(playlist.peek_next(), Some(&local(r"C:\v\ep10.mkv")));
    }

    #[test]
    fn shuffle_visits_every_file_once_starting_from_current() {
        let mut playlist = folder_playlist(r"C:\v\ep2.mkv");
        playlist.set_repeat(RepeatMode::All);
        playlist.set_shuffle(true);

        let mut seen = vec![playlist.current().cloned().expect("current should be set")];
        for _ in 0..playlist.len() - 1 {
            playlist.next();
            seen.push(playlist.current().cloned().expect("current should be set"));
        }

        assert_eq!(seen[0], local(r"C:\v\ep2.mkv")); // the playing item stays first
        let distinct = seen.iter().collect::<HashSet<_>>();
        assert_eq!(distinct.len(), 3); // a full permutation, no repeats
    }

    #[test]
    fn shuffle_off_restores_natural_order() {
        let mut playlist = folder_playlist(r"C:\v\ep1.mkv");
        playlist.set_shuffle(true);
        playlist.set_shuffle(false);

        // Back to natural ep1 → ep2 → ep10.
        assert_eq!(playlist.peek_next(), Some(&local(r"C:\v\ep2.mkv")));
    }

    #[test]
    fn shuffle_direct_jump_strands_no_file_under_repeat_off() {
        let items = (1..=6)
            .map(|n| local(&format!(r"C:\v\{n}.mkv")))
            .collect::<Vec<_>>();
        let mut playlist = Playlist::from_items(items.clone(), Some(&items[0]), true);
        playlist.reseed(3);
        playlist.set_shuffle(true);

        // A direct jump (clicking an Up-Next row), not a sequential step.
        playlist.set_current(&items[5]);
        let mut seen = HashSet::new();
        seen.insert(playlist.current().cloned().expect("current should be set"));
        while let Some(next) = playlist.next() {
            seen.insert(next); // Repeat=Off — walk to the end of the cycle
        }

        // The jump re-shuffled, so nothing is stranded this cycle.
        assert_eq!(seen.len(), items.len());
    }

    // ---- Absorbed Linux shell engine (queue / reorder / remove / transport wrap) ----

    #[test]
    fn queue_append_adds_new_media_to_the_end() {
        let mut playlist = Playlist::from_items(
            vec![
                local("/media/current.mkv"),
                url("https://example.test/stream"),
                local("/media/queued.mkv"),
            ],
            Some(&local("/media/current.mkv")),
            false,
        );
        let additions = vec![
            PathBuf::from("/media/current.mkv"),
            PathBuf::from("/media/queued.mkv"),
            PathBuf::from("/media/new.mp4"),
            PathBuf::from("/media/album.flac"),
        ];

        let count = playlist
            .queue_insert(
                Path::new("/media/current.mkv"),
                additions,
                QueueInsertMode::Append,
            )
            .expect("new media should append");

        assert_eq!(count, 2);
        assert_eq!(
            playlist.items(),
            [
                local("/media/current.mkv"),
                url("https://example.test/stream"),
                local("/media/queued.mkv"),
                local("/media/new.mp4"),
                local("/media/album.flac"),
            ]
        );
        assert_eq!(playlist.current(), Some(&local("/media/current.mkv")));
    }

    #[test]
    fn queue_play_next_inserts_after_current_and_moves_existing_items() {
        let mut playlist = Playlist::from_items(
            vec![
                local("/media/previous.mkv"),
                local("/media/current.mkv"),
                url("https://example.test/stream"),
                local("/media/later.mkv"),
                local("/media/final.mkv"),
            ],
            Some(&local("/media/current.mkv")),
            false,
        );
        let additions = vec![
            PathBuf::from("/media/later.mkv"),
            PathBuf::from("/media/new.mp4"),
        ];

        let count = playlist
            .queue_insert(
                Path::new("/media/current.mkv"),
                additions,
                QueueInsertMode::PlayNext,
            )
            .expect("play next should insert");

        assert_eq!(count, 2);
        assert_eq!(
            playlist.items(),
            [
                local("/media/previous.mkv"),
                local("/media/current.mkv"),
                local("/media/later.mkv"),
                local("/media/new.mp4"),
                url("https://example.test/stream"),
                local("/media/final.mkv"),
            ]
        );
        assert_eq!(playlist.current_index(), Some(1));
    }

    #[test]
    fn queue_rejects_current_only_selection() {
        let mut playlist = Playlist::from_items(
            vec![local("/media/current.mkv")],
            Some(&local("/media/current.mkv")),
            false,
        );

        assert_eq!(
            playlist.queue_insert(
                Path::new("/media/current.mkv"),
                vec![PathBuf::from("/media/current.mkv")],
                QueueInsertMode::Append,
            ),
            None
        );
        assert_eq!(playlist.items(), [local("/media/current.mkv")]);
    }

    #[test]
    fn reorder_moves_item_to_target_slot_after_removal() {
        let items = vec![
            local("/media/a.mkv"),
            local("/media/b.mkv"),
            url("https://example.test/c.mp4"),
            local("/media/d.mkv"),
        ];

        let mut playlist = Playlist::from_items(items.clone(), None, false);
        assert!(playlist.reorder(0, 2));
        assert_eq!(
            playlist.items(),
            [
                local("/media/b.mkv"),
                url("https://example.test/c.mp4"),
                local("/media/a.mkv"),
                local("/media/d.mkv"),
            ]
        );

        let mut playlist = Playlist::from_items(items, None, false);
        assert!(playlist.reorder(3, 1));
        assert_eq!(
            playlist.items(),
            [
                local("/media/a.mkv"),
                local("/media/d.mkv"),
                local("/media/b.mkv"),
                url("https://example.test/c.mp4"),
            ]
        );
    }

    #[test]
    fn reorder_rejects_noop_or_out_of_range_moves() {
        let items = vec![local("/media/a.mkv"), local("/media/b.mkv")];
        let mut playlist = Playlist::from_items(items, None, false);

        assert!(!playlist.reorder(0, 0));
        assert!(!playlist.reorder(3, 0));
    }

    #[test]
    fn reorder_keeps_the_cursor_on_the_current_item() {
        let mut playlist = folder_playlist(r"C:\v\ep2.mkv");

        assert!(playlist.reorder(0, 2)); // ep1 moves behind ep10
        assert_eq!(playlist.current(), Some(&local(r"C:\v\ep2.mkv")));
        assert!(playlist.reorder(0, 1)); // the current item itself moves
        assert_eq!(playlist.current(), Some(&local(r"C:\v\ep2.mkv")));
        assert_eq!(playlist.current_index(), Some(1));
    }

    #[test]
    fn remove_keeps_at_least_one_item() {
        let mut playlist = Playlist::from_items(
            vec![
                local("/media/a.mkv"),
                url("https://example.test/b.mp4"),
                local("/media/c.mkv"),
            ],
            Some(&local("/media/a.mkv")),
            false,
        );

        assert!(playlist.remove(1));
        assert_eq!(
            playlist.items(),
            [local("/media/a.mkv"), local("/media/c.mkv")]
        );

        let mut solo = Playlist::from_items(vec![local("/media/a.mkv")], None, false);
        assert!(!solo.remove(0));
    }

    #[test]
    fn remove_refuses_the_playing_item_and_tracks_the_cursor() {
        let mut playlist = folder_playlist(r"C:\v\ep2.mkv");

        assert!(!playlist.remove(1)); // the playing row stays
        assert!(playlist.remove(0)); // removing before the cursor shifts it
        assert_eq!(playlist.current_index(), Some(0));
        assert_eq!(playlist.current(), Some(&local(r"C:\v\ep2.mkv")));
    }

    #[test]
    fn step_wrapping_wraps_at_both_ends_regardless_of_repeat() {
        let mut playlist = folder_playlist(r"C:\v\ep10.mkv"); // Repeat=Off

        assert_eq!(playlist.step_wrapping(1), Some(local(r"C:\v\ep1.mkv")));
        assert_eq!(playlist.step_wrapping(-1), Some(local(r"C:\v\ep10.mkv")));

        let mut solo = Playlist::from_items(vec![local("/media/a.mkv")], None, false);
        assert_eq!(solo.step_wrapping(1), None); // nothing to move to
    }

    #[test]
    fn auto_advance_toggle_gates_eof_advance_but_not_repeat_one() {
        let mut playlist = folder_playlist(r"C:\v\ep1.mkv");
        playlist.set_auto_advance(false);

        assert_eq!(playlist.auto_advance_target(), None);

        playlist.set_repeat(RepeatMode::One);
        assert_eq!(
            playlist.auto_advance_target(),
            Some(&local(r"C:\v\ep1.mkv"))
        );
    }

    #[test]
    fn reset_keeps_the_shuffle_cycle_for_same_folder_loads() {
        let mut playlist = folder_playlist(r"C:\v\ep1.mkv");
        playlist.set_repeat(RepeatMode::All);
        playlist.set_shuffle(true);

        let next = playlist.peek_next().cloned().expect("shuffled next exists");
        playlist.reset(playlist.items().to_vec(), &next); // sequential step: same items, next item
        assert_eq!(playlist.current(), Some(&next));

        playlist.reset(vec![local("/media/x.mkv")], &local("/media/x.mkv"));
        assert_eq!(playlist.items(), [local("/media/x.mkv")]);
        assert_eq!(playlist.current_index(), Some(0));
    }

    #[test]
    fn reset_with_identical_items_keeps_the_cursor_on_a_duplicate_occurrence() {
        let items = vec![
            local("/media/a.mkv"),
            local("/media/b.mkv"),
            local("/media/a.mkv"), // an M3U may repeat an entry
            local("/media/c.mkv"),
        ];
        let mut playlist = Playlist::from_items(items.clone(), Some(&items[0]), false);
        playlist.set_current_index(2);

        playlist.reset(items.clone(), &local("/media/a.mkv"));

        assert_eq!(playlist.current_index(), Some(2)); // stays on the later occurrence
        assert_eq!(playlist.peek_next(), Some(&local("/media/c.mkv"))); // neighbours follow it

        playlist.reset(items, &local("/media/b.mkv")); // cursor elsewhere → re-find by equality
        assert_eq!(playlist.current_index(), Some(1));
    }

    #[test]
    fn peek_wrapping_index_reports_the_target_without_moving_the_cursor() {
        let playlist = folder_playlist(r"C:\v\ep10.mkv"); // last item, Repeat=Off

        assert_eq!(playlist.peek_wrapping_index(1), Some(0)); // wraps to the first item
        assert_eq!(playlist.peek_wrapping_index(-1), Some(1));
        assert_eq!(playlist.current_index(), Some(2)); // peeking did not move the cursor

        let solo = Playlist::from_items(vec![local("/media/a.mkv")], None, false);
        assert_eq!(solo.peek_wrapping_index(1), None); // nothing to move to
    }

    #[test]
    fn step_wrapping_navigates_duplicate_entries_by_position() {
        let items = vec![
            local("/media/a.mkv"),
            local("/media/b.mkv"),
            local("/media/a.mkv"),
        ];
        let mut playlist = Playlist::from_items(items.clone(), Some(&items[0]), false);

        assert_eq!(playlist.step_wrapping(-1), Some(local("/media/a.mkv")));
        assert_eq!(playlist.current_index(), Some(2)); // wrapped to the last duplicate
        assert_eq!(playlist.step_wrapping(1), Some(local("/media/a.mkv")));
        assert_eq!(playlist.current_index(), Some(0)); // and back to the first
    }

    #[test]
    fn auto_advance_target_index_distinguishes_consecutive_duplicates() {
        let items = vec![
            local("/media/a.mkv"),
            local("/media/b.mkv"),
            local("/media/b.mkv"),
            local("/media/c.mkv"),
        ];
        let mut playlist = Playlist::from_items(items.clone(), Some(&items[1]), false);

        assert_eq!(playlist.auto_advance_target_index(), Some(2)); // the second b, not a replay
        playlist.set_current_index(2);
        assert_eq!(playlist.auto_advance_target_index(), Some(3)); // the chain reaches c

        playlist.set_repeat(RepeatMode::One);
        assert_eq!(playlist.auto_advance_target_index(), Some(2)); // Repeat=One replays in place
    }

    #[test]
    fn advance_after_error_index_skips_the_failed_item_without_replaying_or_wrapping() {
        let items = vec![
            local("/media/a.mkv"),
            local("/media/b.mkv"),
            local("/media/c.mkv"),
        ];
        let mut playlist = Playlist::from_items(items.clone(), Some(&items[0]), false);

        assert_eq!(playlist.advance_after_error_index(), Some(1)); // a fails -> try b

        // Repeat=One must not replay the broken item, otherwise a failure loops forever.
        playlist.set_repeat(RepeatMode::One);
        assert_eq!(playlist.advance_after_error_index(), Some(1));

        // The last item has no successor even under Repeat=All: a fully broken queue must
        // terminate at the welcome surface rather than cycling.
        playlist.set_repeat(RepeatMode::All);
        playlist.set_current_index(2);
        assert_eq!(playlist.advance_after_error_index(), None);
    }

    #[test]
    fn advance_after_error_index_ignores_auto_advance_being_off() {
        let items = vec![local("/media/a.mkv"), local("/media/b.mkv")];
        let mut playlist = Playlist::from_items(items.clone(), Some(&items[0]), false);
        playlist.set_auto_advance(false);

        // A failure is not a user-intended stop, so the queue is kept alive regardless.
        assert_eq!(playlist.advance_after_error_index(), Some(1));
    }

    #[test]
    fn repeat_mode_settings_values_round_trip() {
        for mode in [RepeatMode::Off, RepeatMode::One, RepeatMode::All] {
            assert_eq!(RepeatMode::from_settings_value(mode.settings_value()), mode);
        }
        assert_eq!(RepeatMode::from_settings_value("garbage"), RepeatMode::Off);
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::Off);
    }

    #[test]
    fn playlist_item_parses_m3u_entries_and_formats_labels() {
        assert_eq!(
            PlaylistItem::from_m3u_entry("https://example.test/live/stream.mp3"),
            Some(url("https://example.test/live/stream.mp3"))
        );
        assert_eq!(
            PlaylistItem::from_m3u_entry("/media/a.mkv"),
            Some(local("/media/a.mkv"))
        );
        assert_eq!(PlaylistItem::from_m3u_entry("/media/readme.txt"), None);

        let item = local("/media/a.mkv");
        assert_eq!(item.display_name(), "a.mkv");
        assert_eq!(item.display_location(), "/media/a.mkv");
        assert_eq!(item.m3u_entry(), "/media/a.mkv");
        assert!(item.is_current(Some(Path::new("/media/a.mkv")), None));
        assert!(!item.is_current(None, Some("/media/a.mkv")));

        let stream = url("https://example.test/live/stream.mp3");
        assert_eq!(stream.display_name(), "stream.mp3");
        assert_eq!(stream.m3u_entry(), "https://example.test/live/stream.mp3");
        assert!(stream.is_current(None, Some("https://example.test/live/stream.mp3")));
    }
}
