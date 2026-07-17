//! Adaptive OSC (on-screen controls) overflow policy (issue #328).
//!
//! The Linux player's bottom control bar must keep its primary transport, the
//! timeline, volume, and the `…` overflow entry usable at every supported
//! window width. Lower-priority actions have to collapse into the overflow
//! menu *before* any two controls overlap — never by clipping, negative
//! margins, or scaling a glyph down to illegibility.
//!
//! This module is the pure, testable policy. Given the available content width
//! and the ordered list of control slots (each carrying the minimum width it
//! measured), it decides which slots stay in the bar and the exact horizontal
//! band each visible slot occupies. The GTK shell only performs the mechanical
//! allocate + hide, so the collapse decision is deterministic and unit-tested
//! away from any display server.

/// Every control slot the OSC bar can present, in canonical left-to-right
/// order. The variants that never collapse form the *floor*: primary transport,
/// the timeline, volume, and the overflow entry. Everything else — including the
/// time labels — folds into the overflow menu as width tightens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OscControlId {
    Play,
    Previous,
    Next,
    Elapsed,
    Timeline,
    Duration,
    Volume,
    Speed,
    Subtitles,
    Audio,
    Chapters,
    Screenshot,
    Fullscreen,
    /// The persistent `…` entry point. Always the final visible action and
    /// never collapses — it *is* where collapsed actions live.
    Overflow,
}

impl OscControlId {
    /// Canonical bar order. The shell appends its widgets in this order and the
    /// policy preserves it, so visual order and collapse math never diverge.
    pub const CANONICAL_ORDER: [OscControlId; 14] = [
        OscControlId::Play,
        OscControlId::Previous,
        OscControlId::Next,
        OscControlId::Elapsed,
        OscControlId::Timeline,
        OscControlId::Duration,
        OscControlId::Volume,
        OscControlId::Speed,
        OscControlId::Subtitles,
        OscControlId::Audio,
        OscControlId::Chapters,
        OscControlId::Screenshot,
        OscControlId::Fullscreen,
        OscControlId::Overflow,
    ];

    /// Collapse priority. `0` is the floor and never collapses. A higher number
    /// collapses earlier when the bar cannot fit every control, so the ordering
    /// below reads as "first to fold" (screenshot) down to "last to fold"
    /// (subtitles). The floor keeps the primary transport, timeline, volume,
    /// and overflow usable at every width, per the issue contract.
    pub fn collapse_priority(self) -> u16 {
        match self {
            // The mandated floor per the issue: primary transport, the
            // timeline, volume, and the overflow entry stay usable at every
            // supported width.
            OscControlId::Play
            | OscControlId::Previous
            | OscControlId::Next
            | OscControlId::Timeline
            | OscControlId::Volume
            | OscControlId::Overflow => 0,
            OscControlId::Screenshot => 8,
            OscControlId::Chapters => 7,
            OscControlId::Duration => 6,
            OscControlId::Speed => 5,
            OscControlId::Fullscreen => 4,
            OscControlId::Audio => 3,
            OscControlId::Subtitles => 2,
            // The elapsed clock is informational (the timeline already conveys
            // position) but cheap, so it is the last to fold — kept until only
            // the mandated floor can fit.
            OscControlId::Elapsed => 1,
        }
    }

    /// Whether the slot absorbs horizontal slack. Exactly the timeline grows
    /// past its minimum to fill the bar; every other slot renders at its
    /// measured minimum so the layout stays tight and predictable.
    pub fn is_flexible(self) -> bool {
        matches!(self, OscControlId::Timeline)
    }

    /// Whether the slot is part of the never-collapsing floor.
    pub fn is_floor(self) -> bool {
        self.collapse_priority() == 0
    }
}

/// One measured control handed to [`plan`]: its identity and the minimum width
/// it needs to render without clipping. Widths come from the live GTK measure
/// in production and from fixtures in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OscSlot {
    pub id: OscControlId,
    pub min_width: i32,
}

impl OscSlot {
    pub fn new(id: OscControlId, min_width: i32) -> Self {
        Self { id, min_width }
    }
}

/// The computed placement for a single slot. Collapsed slots report
/// `visible == false` with a zeroed band so the shell can hide them without a
/// second lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotPlacement {
    pub id: OscControlId,
    pub visible: bool,
    /// Left edge of the slot inside the bar, in px (content coordinates,
    /// i.e. already past the leading padding). Zero when collapsed.
    pub x: i32,
    /// Allocated width in px. Zero when collapsed.
    pub width: i32,
}

impl SlotPlacement {
    /// Right edge of the visible band (`x + width`). Meaningless when collapsed.
    pub fn right(&self) -> i32 {
        self.x + self.width
    }
}

/// The full adaptive layout: one [`SlotPlacement`] per input slot, in input
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscLayout {
    pub placements: Vec<SlotPlacement>,
}

impl OscLayout {
    /// The placement for a specific control, if it was part of the plan.
    pub fn placement(&self, id: OscControlId) -> Option<&SlotPlacement> {
        self.placements.iter().find(|slot| slot.id == id)
    }

    /// Whether a control ended up visible in the bar.
    pub fn is_visible(&self, id: OscControlId) -> bool {
        self.placement(id).is_some_and(|slot| slot.visible)
    }

    /// The ids that collapsed into the overflow menu, in canonical order.
    pub fn collapsed(&self) -> Vec<OscControlId> {
        self.placements
            .iter()
            .filter(|slot| !slot.visible)
            .map(|slot| slot.id)
            .collect()
    }
}

/// The minimum content width the never-collapsing floor needs: the sum of the
/// floor slots' minimum widths plus one `spacing` between each. Slots not in
/// the floor are ignored. Used by the shell's `measure()` so GTK reports a low
/// minimum and actually hands the bar its narrow allocation instead of forcing
/// the full-width minimum and clipping the tail.
pub fn floor_min_width(slots: &[OscSlot], spacing: i32) -> i32 {
    let floor: Vec<&OscSlot> = slots.iter().filter(|slot| slot.id.is_floor()).collect();
    row_width(floor.iter().map(|slot| slot.min_width), spacing)
}

/// The natural content width with every slot visible.
pub fn natural_min_width(slots: &[OscSlot], spacing: i32) -> i32 {
    row_width(slots.iter().map(|slot| slot.min_width), spacing)
}

fn row_width(widths: impl Iterator<Item = i32>, spacing: i32) -> i32 {
    let mut total = 0;
    let mut count = 0;
    for width in widths {
        total += width.max(0);
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    total + spacing.max(0) * (count - 1)
}

/// Compute the adaptive bar layout.
///
/// `available_width` is the bar's outer allocation; `pad_start`/`pad_end` inset
/// the content (the pill's horizontal padding) and `spacing` is the gap between
/// adjacent visible controls. The returned placements are, by construction,
/// pairwise disjoint: each visible slot begins at least `spacing` px past the
/// previous slot's right edge, so no two controls ever share bounds and the
/// overflow entry always keeps an exclusive hit target.
pub fn plan(
    slots: &[OscSlot],
    available_width: i32,
    spacing: i32,
    pad_start: i32,
    pad_end: i32,
) -> OscLayout {
    let spacing = spacing.max(0);
    let content_width = (available_width - pad_start.max(0) - pad_end.max(0)).max(0);

    // Start with everything visible, then fold the highest-priority collapsible
    // slots one at a time until the remaining row fits — or until only the
    // floor is left. Ties break toward the later slot in canonical order so the
    // rightmost of an equal pair folds first, keeping the collapse visually
    // stable from the trailing edge inward.
    let mut visible: Vec<bool> = vec![true; slots.len()];
    loop {
        if row_fits(slots, &visible, spacing, content_width) {
            break;
        }
        let Some(victim) = next_collapse_victim(slots, &visible) else {
            // Only the floor remains; it cannot be narrowed further. Placing it
            // still yields disjoint bands (they extend past the content edge at
            // pathological widths, which no supported window reaches).
            break;
        };
        visible[victim] = false;
    }

    // Distribute leftover slack to the flexible slot(s). The timeline is the
    // only flexible control, so in practice it absorbs the entire remainder.
    let fixed_total: i32 = slots
        .iter()
        .zip(&visible)
        .filter(|(slot, vis)| **vis && !slot.id.is_flexible())
        .map(|(slot, _)| slot.min_width.max(0))
        .sum();
    let flexible_count = slots
        .iter()
        .zip(&visible)
        .filter(|(slot, vis)| **vis && slot.id.is_flexible())
        .count() as i32;
    let visible_count = visible.iter().filter(|vis| **vis).count() as i32;
    let gaps = (visible_count - 1).max(0) * spacing;
    let flexible_min_total: i32 = slots
        .iter()
        .zip(&visible)
        .filter(|(slot, vis)| **vis && slot.id.is_flexible())
        .map(|(slot, _)| slot.min_width.max(0))
        .sum();
    let slack = (content_width - fixed_total - flexible_min_total - gaps).max(0);
    let per_flexible_extra = if flexible_count > 0 {
        slack / flexible_count
    } else {
        0
    };
    let mut flexible_remainder = if flexible_count > 0 {
        slack % flexible_count
    } else {
        0
    };

    let mut placements = Vec::with_capacity(slots.len());
    let mut cursor = 0;
    let mut placed_any = false;
    for (slot, vis) in slots.iter().zip(&visible) {
        if !*vis {
            placements.push(SlotPlacement {
                id: slot.id,
                visible: false,
                x: 0,
                width: 0,
            });
            continue;
        }
        if placed_any {
            cursor += spacing;
        }
        let mut width = slot.min_width.max(0);
        if slot.id.is_flexible() {
            width += per_flexible_extra;
            if flexible_remainder > 0 {
                width += 1;
                flexible_remainder -= 1;
            }
        }
        placements.push(SlotPlacement {
            id: slot.id,
            visible: true,
            x: cursor,
            width,
        });
        cursor += width;
        placed_any = true;
    }

    OscLayout { placements }
}

fn row_fits(slots: &[OscSlot], visible: &[bool], spacing: i32, content_width: i32) -> bool {
    let widths = slots
        .iter()
        .zip(visible)
        .filter(|(_, vis)| **vis)
        .map(|(slot, _)| slot.min_width);
    row_width(widths, spacing) <= content_width
}

/// The index of the next slot to fold: the visible, collapsible slot with the
/// highest priority, breaking ties toward the later canonical position.
fn next_collapse_victim(slots: &[OscSlot], visible: &[bool]) -> Option<usize> {
    let mut best: Option<(usize, u16)> = None;
    for (index, (slot, vis)) in slots.iter().zip(visible).enumerate() {
        if !*vis {
            continue;
        }
        let priority = slot.id.collapse_priority();
        if priority == 0 {
            continue;
        }
        match best {
            // `>=` so a later slot with an equal priority wins the tie.
            Some((_, best_priority)) if priority >= best_priority => {
                best = Some((index, priority));
            }
            None => best = Some((index, priority)),
            _ => {}
        }
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Representative measured minimums (px) for the canonical bar, close to the
    /// GTK CSS floors: 32 px icon buttons, a 50 px speed chip, ~46 px time
    /// clocks, a 32 px resting volume, and a timeline that never shrinks below
    /// 90 px so the scrubber stays grabbable.
    fn canonical_slots() -> Vec<OscSlot> {
        OscControlId::CANONICAL_ORDER
            .into_iter()
            .map(|id| {
                let min = match id {
                    OscControlId::Elapsed | OscControlId::Duration => 46,
                    OscControlId::Timeline => 90,
                    OscControlId::Speed => 50,
                    _ => 32,
                };
                OscSlot::new(id, min)
            })
            .collect()
    }

    const SPACING: i32 = 16;
    const PAD: i32 = 14;

    fn assert_disjoint(layout: &OscLayout) {
        let mut previous_right: Option<i32> = None;
        for slot in layout.placements.iter().filter(|slot| slot.visible) {
            assert!(slot.width > 0, "visible slot {:?} has zero width", slot.id);
            if let Some(right) = previous_right {
                assert!(
                    slot.x >= right + SPACING,
                    "slot {:?} at x={} overlaps previous right edge {}",
                    slot.id,
                    slot.x,
                    right,
                );
            }
            previous_right = Some(slot.right());
        }
    }

    fn visible_ids(layout: &OscLayout) -> Vec<OscControlId> {
        layout
            .placements
            .iter()
            .filter(|slot| slot.visible)
            .map(|slot| slot.id)
            .collect()
    }

    #[test]
    fn wide_width_keeps_every_control() {
        let layout = plan(&canonical_slots(), 1120, SPACING, PAD, PAD);
        for id in OscControlId::CANONICAL_ORDER {
            assert!(
                layout.is_visible(id),
                "{id:?} should stay visible when wide"
            );
        }
        assert!(layout.collapsed().is_empty());
        assert_disjoint(&layout);
    }

    #[test]
    fn overflow_and_floor_survive_every_width() {
        for width in [1120, 900, 640, 520, 480, 420, 360, 300] {
            let layout = plan(&canonical_slots(), width, SPACING, PAD, PAD);
            for id in OscControlId::CANONICAL_ORDER
                .into_iter()
                .filter(|id| id.is_floor())
            {
                assert!(
                    layout.is_visible(id),
                    "floor control {id:?} collapsed at width {width}"
                );
            }
            assert!(
                layout.is_visible(OscControlId::Overflow),
                "overflow collapsed at width {width}"
            );
            assert_disjoint(&layout);
        }
    }

    #[test]
    fn overflow_is_the_final_visible_action() {
        for width in [1120, 640, 480, 360] {
            let layout = plan(&canonical_slots(), width, SPACING, PAD, PAD);
            let last_visible = layout
                .placements
                .iter()
                .rfind(|slot| slot.visible)
                .expect("at least the floor is visible");
            assert_eq!(
                last_visible.id,
                OscControlId::Overflow,
                "overflow must anchor the trailing edge at width {width}"
            );
        }
    }

    #[test]
    fn collapse_is_monotonic_from_wide_to_narrow() {
        let widths = [1120, 900, 760, 640, 560, 520, 480, 420, 360];
        let mut previous: Option<Vec<OscControlId>> = None;
        for width in widths {
            let layout = plan(&canonical_slots(), width, SPACING, PAD, PAD);
            let visible = visible_ids(&layout);
            if let Some(previous) = &previous {
                for id in &visible {
                    assert!(
                        previous.contains(id),
                        "{id:?} became visible at the narrower width {width}"
                    );
                }
            }
            previous = Some(visible);
        }
    }

    #[test]
    fn screenshot_folds_before_subtitles() {
        // A width that forces exactly one collapse should drop the highest
        // priority (screenshot) and keep the lowest (subtitles).
        let slots = canonical_slots();
        let natural = natural_min_width(&slots, SPACING) + 2 * PAD;
        let layout = plan(&slots, natural - 1, SPACING, PAD, PAD);
        assert!(!layout.is_visible(OscControlId::Screenshot));
        assert!(layout.is_visible(OscControlId::Subtitles));
        assert_disjoint(&layout);
    }

    #[test]
    fn narrow_floor_fits_within_content_width() {
        // At the documented narrow smoke floor (480 px) the floor must fit
        // inside the content box without spilling past the trailing padding.
        let slots = canonical_slots();
        let layout = plan(&slots, 480, SPACING, PAD, PAD);
        let content_right = 480 - 2 * PAD;
        let last = layout.placements.iter().rfind(|slot| slot.visible).unwrap();
        assert!(
            last.right() <= content_right,
            "floor spilled past content: right={} content_right={}",
            last.right(),
            content_right,
        );
    }

    #[test]
    fn overflow_keeps_an_exclusive_hit_target_beside_its_neighbour() {
        // The P0 regression: at a narrow width the overflow container and the
        // control beside it occluded each other. At every width the overflow
        // band must begin strictly past its left neighbour's right edge, so the
        // two never share bounds and the `…` hit target is unobstructed.
        for width in [900, 640, 520, 480, 420, 360] {
            let layout = plan(&canonical_slots(), width, SPACING, PAD, PAD);
            let visible: Vec<&SlotPlacement> = layout
                .placements
                .iter()
                .filter(|slot| slot.visible)
                .collect();
            let overflow_index = visible
                .iter()
                .position(|slot| slot.id == OscControlId::Overflow)
                .expect("overflow is always visible");
            assert_eq!(
                overflow_index,
                visible.len() - 1,
                "overflow must be the trailing action at width {width}"
            );
            let overflow = visible[overflow_index];
            let neighbour = visible[overflow_index - 1];
            assert!(
                overflow.x >= neighbour.right() + SPACING,
                "overflow at x={} overlaps neighbour {:?} ending at {} (width {width})",
                overflow.x,
                neighbour.id,
                neighbour.right(),
            );
        }
    }

    #[test]
    fn exactly_one_overflow_entry_exists() {
        // There is a single persistent entry point — never a second Settings
        // gear painted beside or below it.
        let layout = plan(&canonical_slots(), 480, SPACING, PAD, PAD);
        let overflow_slots = layout
            .placements
            .iter()
            .filter(|slot| slot.id == OscControlId::Overflow)
            .count();
        assert_eq!(overflow_slots, 1);
    }

    #[test]
    fn timeline_absorbs_slack() {
        let slots = canonical_slots();
        let narrow = plan(&slots, 700, SPACING, PAD, PAD);
        let wide = plan(&slots, 1120, SPACING, PAD, PAD);
        let narrow_timeline = narrow.placement(OscControlId::Timeline).unwrap().width;
        let wide_timeline = wide.placement(OscControlId::Timeline).unwrap().width;
        assert!(
            wide_timeline > narrow_timeline,
            "timeline should grow with available width: {wide_timeline} vs {narrow_timeline}"
        );
    }

    #[test]
    fn floor_min_width_is_below_the_natural_width() {
        let slots = canonical_slots();
        let floor = floor_min_width(&slots, SPACING);
        let natural = natural_min_width(&slots, SPACING);
        assert!(floor < natural);
        // Mandated floor: play+prev+next+timeline+volume+overflow (no clock).
        let expected = 32 + 32 + 32 + 90 + 32 + 32 + SPACING * 5;
        assert_eq!(floor, expected);
    }
}
