//! Pure layout math for the welcome "Continue watching" shelf — port of
//! `src/OkPlayer.Core/RecentsShelf.cs`; the C# suite in
//! `tests/OkPlayer.Tests/RecentsShelfTests.cs` is the executable spec. How many fixed-width
//! cards fit a given row width: the shelf shows exactly this many so it never needs a
//! horizontal scrollbar (the design's elegance bar); any remaining resumable files stay
//! reachable via History. Kept here, engine- and UI-agnostic, so the fit rule is unit-tested
//! rather than buried in the view.

/// The C# default for `unmeasured_default` — what the shelf shows before its first measure.
pub const DEFAULT_UNMEASURED: usize = 3;

/// How many cards to show: as many whole cards as fit `row_width`, capped by how many are
/// actually `available`.
///
/// n cards laid out with (n-1) gaps need n*card + (n-1)*spacing ≤ width, i.e.
/// n ≤ (width + spacing) / (card + spacing); we take the floor. This is 0 when not even one
/// card fits — important because the row no longer scrolls, so on a side-snapped or very
/// narrow window we must show nothing (and route the items to the overflow control) rather
/// than clip a full-width card. Before the row is measured (`row_width` ≤ 0) we fall back to
/// `unmeasured_default` so the first paint is sensible; a size-changed then corrects it. The
/// result is always clamped to [0, available].
pub fn visible_count(
    row_width: f64,
    available: usize,
    card_width: f64,
    spacing: f64,
    unmeasured_default: usize,
) -> usize {
    if available == 0 {
        return 0;
    }
    let fit = if row_width <= 0.0 {
        unmeasured_default
    } else {
        // 0 when one card doesn't fit -> no clipping
        ((row_width + spacing) / (card_width + spacing)) as usize
    };
    fit.min(available)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped card geometry: 194px card, 14px gap.
    const CARD: f64 = 194.0;
    const GAP: f64 = 14.0;

    #[test]
    fn visible_count_fits_as_many_whole_cards_as_there_is_room_for() {
        // n cards need n*194 + (n-1)*14 px. 772 is the welcome column's content width
        // (860 MaxWidth - 88 padding): 3 cards = 610, 4 cards = 818 > 772, so exactly 3 fit.
        let cases = [
            (772.0, 3),
            (610.0, 3), // exactly three cards wide -> three
            (609.0, 2), // one px short of three -> two
            (832.0, 4), // a wider column fits four
            (208.0, 1), // one card + one gap
            (194.0, 1), // exactly one card wide -> one
            (193.0, 0), // one px short of a card -> none (the row no longer scrolls, so don't clip)
            (120.0, 0), // far narrower than a card -> none; the overflow control holds them instead
        ];
        for (width, expected) in cases {
            assert_eq!(
                visible_count(width, 20, CARD, GAP, DEFAULT_UNMEASURED),
                expected,
                "width {width}"
            );
        }
    }

    #[test]
    fn visible_count_is_capped_by_what_is_available() {
        assert_eq!(visible_count(2000.0, 2, CARD, GAP, DEFAULT_UNMEASURED), 2);
    }

    #[test]
    fn visible_count_is_zero_when_nothing_is_available() {
        assert_eq!(visible_count(772.0, 0, CARD, GAP, DEFAULT_UNMEASURED), 0);
    }

    #[test]
    fn visible_count_falls_back_to_the_default_before_the_row_is_measured() {
        let cases = [(10, 3), (2, 2)]; // unmeasured -> the default, capped by availability
        for (available, expected) in cases {
            assert_eq!(visible_count(0.0, available, CARD, GAP, 3), expected);
        }
    }
}
