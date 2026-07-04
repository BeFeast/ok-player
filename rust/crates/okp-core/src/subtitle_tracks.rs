//! Subtitle-track selection roles: which subtitle track fills the *primary*
//! slot (mpv `sid`) and which the *secondary* slot (mpv `secondary-sid`), plus
//! whether the secondary picker should be offered at all. This is the pure
//! classification the shells wrap around a raw mpv track list; it lives here
//! (freeze-boundary) so the Linux GTK shell and the Windows shell share one
//! rule instead of each re-deriving it in shell code.
//!
//! There is no `OkPlayer.Core` counterpart — on Windows the same logic lives in
//! the `PlayerViewModel.ReadTracks` shell method (its `isPrimary = selected &&
//! !isSecondary` guard and its `CanUseSecondarySubtitle` gate). This module is
//! the Linux-shell extraction that also captures that Windows rule; see the note
//! in `docs/core-compatibility.md`. Engine- and UI-free → unit-tested.
//!
//! The subtlety this exists to handle: mpv reports `track-list/N/selected = yes`
//! for BOTH the primary and the secondary caption, so the raw flag alone cannot
//! tell them apart. The secondary is identified by id against `secondary-sid`
//! (mpv always resolves it to a concrete id, never `auto`); everything else mpv
//! reports as selected — including an auto/default pick it makes for us — is the
//! primary. Without excluding the secondary, it would draw a stray checkmark in
//! the primary picker and make an "active secondary, no primary" file read as
//! though a primary were selected.

/// The slot a subtitle track occupies in the current selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleTrackRole {
    /// Shown in neither slot.
    Inactive,
    /// The primary caption (mpv `sid`), rendered at the bottom.
    Primary,
    /// The secondary caption (mpv `secondary-sid`), rendered at the top.
    Secondary,
}

/// Classify one subtitle track from its raw mpv `selected` flag and the current
/// `secondary-sid`. A track whose id matches `secondary_sid` is the secondary
/// caption; any *other* track mpv reports as selected is the primary; the rest
/// are inactive. The id match wins over `selected` so the secondary is never
/// mistaken for the primary.
pub fn subtitle_track_role(
    track_id: i64,
    selected: bool,
    secondary_sid: Option<i64>,
) -> SubtitleTrackRole {
    if secondary_sid == Some(track_id) {
        SubtitleTrackRole::Secondary
    } else if selected {
        SubtitleTrackRole::Primary
    } else {
        SubtitleTrackRole::Inactive
    }
}

/// Whether a track fills the primary slot. Excludes the secondary track, whose
/// raw `selected` flag mpv also reports as `yes`.
pub fn is_primary_subtitle(track_id: i64, selected: bool, secondary_sid: Option<i64>) -> bool {
    subtitle_track_role(track_id, selected, secondary_sid) == SubtitleTrackRole::Primary
}

/// Whether a track fills the secondary slot.
pub fn is_secondary_subtitle(track_id: i64, secondary_sid: Option<i64>) -> bool {
    secondary_sid == Some(track_id)
}

/// Whether any track fills the primary slot — the negation is the primary
/// picker's "Off" state. An active secondary with no primary correctly reads as
/// primary-off because the secondary is excluded. Each item is `(track_id,
/// selected)`.
pub fn has_primary_subtitle(
    tracks: impl IntoIterator<Item = (i64, bool)>,
    secondary_sid: Option<i64>,
) -> bool {
    tracks
        .into_iter()
        .any(|(id, selected)| is_primary_subtitle(id, selected, secondary_sid))
}

/// Whether to offer the secondary-subtitle picker at all. Dual subtitles only
/// make sense once there is a choice, so the picker appears from two subtitle
/// tracks upward — OR whenever a secondary is already active (mpv can carry
/// `secondary-sid` into a single-track file), so the user can always turn an
/// active secondary back off. Mirrors the Windows `CanUseSecondarySubtitle`
/// gate.
pub fn can_offer_secondary(subtitle_track_count: usize, secondary_active: bool) -> bool {
    subtitle_track_count >= 2 || secondary_active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_non_secondary_track_is_primary() {
        assert_eq!(
            subtitle_track_role(3, true, None),
            SubtitleTrackRole::Primary
        );
        assert_eq!(
            subtitle_track_role(3, true, Some(4)),
            SubtitleTrackRole::Primary
        );
    }

    #[test]
    fn secondary_id_match_wins_over_selected() {
        // mpv marks the secondary track selected too; the id match must classify
        // it as Secondary, never Primary.
        assert_eq!(
            subtitle_track_role(4, true, Some(4)),
            SubtitleTrackRole::Secondary
        );
        // Even if mpv had not flagged it selected, the id match still decides.
        assert_eq!(
            subtitle_track_role(4, false, Some(4)),
            SubtitleTrackRole::Secondary
        );
    }

    #[test]
    fn unselected_non_secondary_track_is_inactive() {
        assert_eq!(
            subtitle_track_role(2, false, Some(4)),
            SubtitleTrackRole::Inactive
        );
        assert_eq!(
            subtitle_track_role(2, false, None),
            SubtitleTrackRole::Inactive
        );
    }

    #[test]
    fn is_primary_excludes_the_secondary_track() {
        assert!(is_primary_subtitle(3, true, Some(4)));
        assert!(!is_primary_subtitle(4, true, Some(4)));
        assert!(!is_primary_subtitle(2, false, Some(4)));
    }

    #[test]
    fn is_secondary_matches_only_the_secondary_id() {
        assert!(is_secondary_subtitle(4, Some(4)));
        assert!(!is_secondary_subtitle(3, Some(4)));
        assert!(!is_secondary_subtitle(4, None));
    }

    #[test]
    fn has_primary_true_when_a_non_secondary_track_is_selected() {
        // Track 3 primary, track 4 secondary — both flagged selected by mpv.
        let tracks = [(3, true), (4, true)];
        assert!(has_primary_subtitle(tracks, Some(4)));
    }

    #[test]
    fn has_primary_false_when_only_the_secondary_is_active() {
        // The one selected track IS the secondary → the primary slot is off.
        let tracks = [(4, true)];
        assert!(!has_primary_subtitle(tracks, Some(4)));
    }

    #[test]
    fn has_primary_false_with_no_selection() {
        assert!(!has_primary_subtitle([(3, false), (4, false)], None));
        assert!(!has_primary_subtitle(std::iter::empty(), None));
    }

    #[test]
    fn can_offer_secondary_requires_a_choice_or_an_active_secondary() {
        assert!(!can_offer_secondary(0, false));
        assert!(!can_offer_secondary(1, false));
        assert!(can_offer_secondary(2, false));
        assert!(can_offer_secondary(3, false));
        // A single-track file still offers the picker while a secondary lingers,
        // so the user can switch it off.
        assert!(can_offer_secondary(1, true));
        assert!(can_offer_secondary(0, true));
    }
}
