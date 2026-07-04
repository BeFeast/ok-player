//! Primary/secondary subtitle slot resolution — the shared rule for reading
//! which subtitle track occupies mpv's primary (`sid`) and secondary
//! (`secondary-sid`) slots. Port of the Windows `PlayerViewModel.ReadTracks`
//! rule (`isPrimary = selected && !isSecondary`); there is no C# core module to
//! mirror, so this module is the executable spec.
//!
//! mpv reports BOTH the primary-sid track and the secondary-sid track as
//! `track-list/N/selected = yes`, because both are decoded. A raw `selected`
//! flag therefore cannot tell the two apart: the secondary is identified by
//! matching its id against `secondary-sid` (mpv sets that to a concrete id,
//! never "auto"), and the primary is the remaining `selected` track. Without
//! this the primary picker would show a stray second checkmark on the secondary
//! track, the primary "Off" row would lose its check, and Media Info would flag
//! the secondary caption as the current primary.
//!
//! Pure / UI-free for headless tests.

/// Which slot a subtitle track occupies, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleSlot {
    /// The main caption (`sid`): a `selected` track that is not the secondary.
    Primary,
    /// The second simultaneous caption (`secondary-sid`), matched by id.
    Secondary,
}

/// Whether the track with `track_id` is the secondary caption. `secondary_sid`
/// is the normalized `secondary-sid` (`None` when the secondary slot is off).
pub fn is_secondary(track_id: i64, secondary_sid: Option<i64>) -> bool {
    secondary_sid == Some(track_id)
}

/// Whether the track with `track_id` is the primary caption. `selected` is
/// mpv's raw `track-list/N/selected`; the secondary track also reports
/// `selected`, so it is excluded here.
pub fn is_primary(track_id: i64, selected: bool, secondary_sid: Option<i64>) -> bool {
    selected && !is_secondary(track_id, secondary_sid)
}

/// The slot a single subtitle track occupies given its raw `selected` flag and
/// the active `secondary-sid`. `None` when the track drives neither slot.
pub fn slot_for(track_id: i64, selected: bool, secondary_sid: Option<i64>) -> Option<SubtitleSlot> {
    if is_secondary(track_id, secondary_sid) {
        Some(SubtitleSlot::Secondary)
    } else if selected {
        Some(SubtitleSlot::Primary)
    } else {
        None
    }
}

/// The id of the primary subtitle track, resolved from `(id, selected)` pairs
/// and the active `secondary-sid`. `None` when no track drives the primary slot
/// (the primary "Off" state). The first matching track wins; real mpv only ever
/// marks one non-secondary track `selected`.
pub fn primary_id<I>(tracks: I, secondary_sid: Option<i64>) -> Option<i64>
where
    I: IntoIterator<Item = (i64, bool)>,
{
    tracks
        .into_iter()
        .find(|&(id, selected)| is_primary(id, selected, secondary_sid))
        .map(|(id, _)| id)
}

/// Whether the secondary-subtitle picker should be offered at all. Mirrors the
/// Windows gate `CanUseSecondarySubtitle = subs.Count >= 2 || secondary active`:
/// a single subtitle track cannot fill both slots at once, but if mpv already
/// carries a secondary into a one-track file the user must still be able to
/// clear it — so an active secondary keeps the picker available regardless of
/// the track count.
pub fn can_use_secondary(subtitle_track_count: usize, secondary_active: bool) -> bool {
    subtitle_track_count >= 2 || secondary_active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_track_is_not_reported_as_primary() {
        // mpv marks both the primary (id 1) and the secondary (id 2) selected.
        assert!(is_primary(1, true, Some(2)));
        assert!(!is_secondary(1, Some(2)));
        assert!(!is_primary(2, true, Some(2)));
        assert!(is_secondary(2, Some(2)));
    }

    #[test]
    fn slot_for_classifies_each_track() {
        assert_eq!(slot_for(1, true, Some(2)), Some(SubtitleSlot::Primary));
        assert_eq!(slot_for(2, true, Some(2)), Some(SubtitleSlot::Secondary));
        // An unselected track that is neither slot.
        assert_eq!(slot_for(3, false, Some(2)), None);
        // The secondary is matched by id even if mpv had not flagged it selected
        // yet (freshly set), so the picker always reflects the request.
        assert_eq!(slot_for(2, false, Some(2)), Some(SubtitleSlot::Secondary));
    }

    #[test]
    fn primary_id_excludes_the_secondary_track() {
        let tracks = [(1_i64, true), (2, true), (3, false)];
        assert_eq!(primary_id(tracks, Some(2)), Some(1));
        // With no secondary the sole selected track is the primary.
        assert_eq!(primary_id([(1_i64, false), (2, true)], None), Some(2));
        // A file where only the secondary is active has no primary.
        assert_eq!(primary_id([(2_i64, true)], Some(2)), None);
        // Nothing selected → primary is off.
        assert_eq!(primary_id([(1_i64, false), (2, false)], None), None);
    }

    #[test]
    fn secondary_off_when_sid_is_none() {
        assert!(!is_secondary(1, None));
        assert_eq!(slot_for(1, true, None), Some(SubtitleSlot::Primary));
    }

    #[test]
    fn can_use_secondary_needs_two_tracks_or_an_active_secondary() {
        assert!(!can_use_secondary(0, false));
        assert!(!can_use_secondary(1, false));
        assert!(can_use_secondary(2, false));
        assert!(can_use_secondary(5, false));
        // A lone track still exposes the picker while a secondary is carried in,
        // so the user can turn it back off.
        assert!(can_use_secondary(1, true));
        assert!(can_use_secondary(0, true));
    }
}
