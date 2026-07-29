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
//! band each visible slot occupies. It has three levers, spent in the order the
//! pillars rank them (#729): fold the secondary controls, tighten the gaps and
//! the pill inset, and only then let the seek bar give width back — down to its
//! own floor, which is narrow enough that the whole row fits inside any window a
//! portrait clip can fit itself to. The GTK shell only performs the mechanical
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

    /// Marker CSS class the shell stamps on the slot's widget. It carries no
    /// style; it is what lets the geometry diagnostic (#690) name the control
    /// behind a reported rectangle, so a headless check can say *which* control
    /// left the window rather than only that something did.
    pub fn slot_css_class(self) -> &'static str {
        match self {
            OscControlId::Play => "okp-osc-slot-play",
            OscControlId::Previous => "okp-osc-slot-previous",
            OscControlId::Next => "okp-osc-slot-next",
            OscControlId::Elapsed => "okp-osc-slot-elapsed",
            OscControlId::Timeline => "okp-osc-slot-timeline",
            OscControlId::Duration => "okp-osc-slot-duration",
            OscControlId::Volume => "okp-osc-slot-volume",
            OscControlId::Speed => "okp-osc-slot-speed",
            OscControlId::Subtitles => "okp-osc-slot-subtitles",
            OscControlId::Audio => "okp-osc-slot-audio",
            OscControlId::Chapters => "okp-osc-slot-chapters",
            OscControlId::Screenshot => "okp-osc-slot-screenshot",
            OscControlId::Fullscreen => "okp-osc-slot-fullscreen",
            OscControlId::Overflow => "okp-osc-slot-overflow",
        }
    }
}

/// Marker CSS class every OSC slot carries, whatever it is.
pub const SLOT_CSS_CLASS: &str = "okp-osc-slot";

/// The gap the bar tightens to once folding every collapsible control is no
/// longer enough to fit the row. Gaps and padding are the cheapest width in the
/// bar: taking them back keeps a control usable, where spilling past the window
/// edge does not (#729).
pub const COMPACT_SPACING: i32 = 8;

/// The horizontal pill inset the bar tightens to under the same pressure.
pub const COMPACT_PADDING: i32 = 8;

/// The width the seek bar is held at while there is still a secondary control
/// that could fold instead.
///
/// The pillars rank playback control above secondary affordances, so the bar
/// folds a screenshot button before it shortens the scrubber. Only once nothing
/// is left to fold does the seek bar start giving width back, down to its own
/// measured floor. The value is the width the timeline used to refuse to go
/// below, so the widths at which controls fold are unchanged by #729.
pub const FLEXIBLE_COMFORT_WIDTH: i32 = 144;

/// The width a slot occupies in the fit test: its measured minimum, except for
/// the flexible seek bar, which is held at [`FLEXIBLE_COMFORT_WIDTH`] for as
/// long as something else can fold instead.
fn fit_width(slot: &OscSlot, hold_flexible: bool) -> i32 {
    if hold_flexible && slot.id.is_flexible() {
        slot.min_width.max(FLEXIBLE_COMFORT_WIDTH)
    } else {
        slot.min_width
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
    /// Leading pill inset the placements were laid out against. The bar tightens
    /// it at narrow widths, so the shell must offset children by this rather
    /// than by its own design constant.
    pub pad_start: i32,
    /// Trailing pill inset, used for the same reason when the row is mirrored
    /// for a right-to-left locale.
    pub pad_end: i32,
    /// Gap between adjacent visible controls in this layout.
    pub spacing: i32,
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

/// The natural content width with every slot visible and the seek bar at its
/// comfortable width.
pub fn natural_min_width(slots: &[OscSlot], spacing: i32) -> i32 {
    row_width(slots.iter().map(|slot| fit_width(slot, true)), spacing)
}

/// The narrowest *outer* width the bar can lay itself out inside: the floor row
/// at the tightened metrics, padding included.
///
/// This is the number the shell must report as its horizontal minimum. GTK
/// never allocates a widget less than the minimum it reports, so a minimum the
/// bar cannot honour is handed straight back to it inside a narrower window and
/// the tail of the row is clipped instead of reflowed — which is exactly the
/// defect in #729 and, on the idle canvas, in #716. Reporting the compact floor
/// keeps the reported minimum and the honoured minimum the same number.
pub fn compact_floor_width(slots: &[OscSlot]) -> i32 {
    floor_min_width(slots, COMPACT_SPACING) + COMPACT_PADDING * 2
}

/// The gaps and padding the row is laid out with at `available_width`.
///
/// The bar keeps its roomy design metrics for as long as the never-collapsing
/// floor fits between them, and tightens to the compact pair below that. It is
/// deliberately a step rather than a slide: two stable looks are easier to
/// recognise than a bar whose gaps drift with every pixel of a drag.
fn metrics_for(
    slots: &[OscSlot],
    available_width: i32,
    spacing: i32,
    pad_start: i32,
    pad_end: i32,
) -> (i32, i32, i32) {
    let (spacing, pad_start, pad_end) = (spacing.max(0), pad_start.max(0), pad_end.max(0));
    if floor_min_width(slots, spacing) + pad_start + pad_end <= available_width {
        return (spacing, pad_start, pad_end);
    }
    (
        spacing.min(COMPACT_SPACING),
        pad_start.min(COMPACT_PADDING),
        pad_end.min(COMPACT_PADDING),
    )
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
/// `available_width` is the bar's outer allocation; `pad_start`/`pad_end` are
/// the pill's design padding and `spacing` the design gap between adjacent
/// visible controls. All three are upper bounds: below the width where the
/// never-collapsing floor still fits between them the row is laid out on the
/// compact metrics instead, and the layout reports back the pair it used. The
/// returned placements are, by construction, pairwise disjoint: each visible
/// slot begins at least `spacing` px past the previous slot's right edge, so no
/// two controls ever share bounds and the overflow entry always keeps an
/// exclusive hit target.
pub fn plan(
    slots: &[OscSlot],
    available_width: i32,
    spacing: i32,
    pad_start: i32,
    pad_end: i32,
) -> OscLayout {
    let (spacing, pad_start, pad_end) =
        metrics_for(slots, available_width, spacing, pad_start, pad_end);
    let content_width = (available_width - pad_start - pad_end).max(0);

    // Start with everything visible, then fold the highest-priority collapsible
    // slots one at a time until the remaining row fits — or until only the
    // floor is left. Ties break toward the later slot in canonical order so the
    // rightmost of an equal pair folds first, keeping the collapse visually
    // stable from the trailing edge inward.
    let mut visible: Vec<bool> = vec![true; slots.len()];
    loop {
        // While something can still fold, the seek bar is measured at its
        // comfortable width, so a secondary control folds before the scrubber
        // is shortened.
        let hold_flexible = next_collapse_victim(slots, &visible).is_some();
        if row_fits(slots, &visible, spacing, content_width, hold_flexible) {
            break;
        }
        let Some(victim) = next_collapse_victim(slots, &visible) else {
            // Only the floor remains, laid out on the compact metrics. The shell
            // reports exactly this width as its minimum and GTK never allocates
            // a widget less than the minimum it reported, so a real bar is never
            // planned below it; the bands stay disjoint either way.
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

    OscLayout {
        placements,
        pad_start,
        pad_end,
        spacing,
    }
}

fn row_fits(
    slots: &[OscSlot],
    visible: &[bool],
    spacing: i32,
    content_width: i32,
    hold_flexible: bool,
) -> bool {
    let widths = slots
        .iter()
        .zip(visible)
        .filter(|(_, vis)| **vis)
        .map(|(slot, _)| fit_width(slot, hold_flexible));
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
    /// clocks, a 32 px resting volume, and the seek bar's own hard floor, which
    /// only binds once every collapsible control has already folded.
    fn canonical_slots() -> Vec<OscSlot> {
        OscControlId::CANONICAL_ORDER
            .into_iter()
            .map(|id| {
                let min = match id {
                    OscControlId::Elapsed | OscControlId::Duration => 46,
                    OscControlId::Timeline => 72,
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
                // The gap is whatever the layout used: it tightens to the
                // compact metrics at narrow widths.
                assert!(
                    slot.x >= right + layout.spacing,
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
                overflow.x >= neighbour.right() + layout.spacing,
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

    /// The defect in #729: the bar was laid out wider than the window it sits in,
    /// so the trailing controls — the volume and the `…` entry — were drawn past
    /// the right edge, half-visible and unreachable. Nothing the policy plans may
    /// leave the content box, at any width the bar can actually be given.
    #[test]
    fn no_control_is_ever_planned_outside_the_bar() {
        let slots = canonical_slots();
        let floor = compact_floor_width(&slots);
        for width in (floor..=1200).step_by(7) {
            let layout = plan(&slots, width, SPACING, PAD, PAD);
            let content_right = width - layout.pad_start - layout.pad_end;
            for slot in layout.placements.iter().filter(|slot| slot.visible) {
                assert!(
                    slot.x >= 0 && slot.right() <= content_right,
                    "{:?} spans {}..{} outside a {content_right}px content box (bar {width}px)",
                    slot.id,
                    slot.x,
                    slot.right(),
                );
            }
        }
    }

    /// Playback control outranks secondary affordances: the bar folds a
    /// screenshot or a chapter button before it shortens the scrubber, and only
    /// starts giving seek width back once nothing else can fold.
    #[test]
    fn the_seek_bar_shortens_last() {
        let slots = canonical_slots();
        for width in (compact_floor_width(&slots)..=1200).step_by(3) {
            let layout = plan(&slots, width, SPACING, PAD, PAD);
            let timeline = layout.placement(OscControlId::Timeline).unwrap();
            let anything_left_to_fold = layout
                .placements
                .iter()
                .any(|slot| slot.visible && !slot.id.is_floor());
            if anything_left_to_fold {
                assert!(
                    timeline.width >= FLEXIBLE_COMFORT_WIDTH,
                    "the seek bar shrank to {}px at {width}px while {:?} was still foldable",
                    timeline.width,
                    layout
                        .placements
                        .iter()
                        .find(|slot| slot.visible && !slot.id.is_floor())
                        .map(|slot| slot.id),
                );
            }
        }
    }

    /// The number the shell reports to GTK has to be one the policy can honour.
    /// GTK never allocates below a reported minimum, so any gap between the two
    /// is clipping waiting to happen.
    #[test]
    fn the_reported_minimum_is_a_width_the_row_fits_inside() {
        let slots = canonical_slots();
        let floor = compact_floor_width(&slots);
        let layout = plan(&slots, floor, SPACING, PAD, PAD);
        let content_right = floor - layout.pad_start - layout.pad_end;
        let last = layout.placements.iter().rfind(|slot| slot.visible).unwrap();
        assert_eq!(last.id, OscControlId::Overflow);
        assert!(
            last.right() <= content_right,
            "the floor row needs {}px inside the {content_right}px the reported minimum leaves",
            last.right(),
        );
        for id in OscControlId::CANONICAL_ORDER
            .into_iter()
            .filter(|id| id.is_floor())
        {
            assert!(
                layout.is_visible(id),
                "{id:?} collapsed at the reported floor"
            );
        }
    }

    /// Gaps and padding are the first width the bar gives back, and only once
    /// folding every collapsible control has stopped being enough.
    #[test]
    fn metrics_tighten_only_under_pressure() {
        let slots = canonical_slots();
        let roomy = plan(&slots, 1120, SPACING, PAD, PAD);
        assert_eq!(
            (roomy.spacing, roomy.pad_start, roomy.pad_end),
            (SPACING, PAD, PAD)
        );

        let tight = plan(&slots, compact_floor_width(&slots), SPACING, PAD, PAD);
        assert_eq!(
            (tight.spacing, tight.pad_start, tight.pad_end),
            (COMPACT_SPACING, COMPACT_PADDING, COMPACT_PADDING)
        );
        assert!(
            compact_floor_width(&slots) < floor_min_width(&slots, SPACING) + PAD * 2,
            "the compact floor has to be narrower than the roomy one to be worth having"
        );
    }

    #[test]
    fn floor_min_width_is_below_the_natural_width() {
        let slots = canonical_slots();
        let floor = floor_min_width(&slots, SPACING);
        let natural = natural_min_width(&slots, SPACING);
        assert!(floor < natural);
        // Mandated floor: play+prev+next+timeline+volume+overflow (no clock).
        let expected = 32 + 32 + 32 + 72 + 32 + 32 + SPACING * 5;
        assert_eq!(floor, expected);
    }
}
