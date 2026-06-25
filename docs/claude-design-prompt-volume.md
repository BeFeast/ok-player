# Claude Design Prompt — OK Player · Volume control

You are designing **one component** for **OK Player**, a Windows-native media player: the **volume
control** and its interactions. Produce a focused UI/UX design with all states and a clear interaction
model. The product's design system and four pillars are the source of truth — read
[`claude-design-prompt.md`](./claude-design-prompt.md) and [`OK-Player-PRD.md`](./OK-Player-PRD.md);
match that system exactly (native **Fluent/Mica**, light + auto/dark, single teal accent, calm and
restrained, macOS-utility-grade polish — IINA/Elmedia feel, *not* macOS chrome).

## Where it lives

The volume control sits in the **floating OSC pill** that hovers, auto-hiding, **over the video** —
so it must stay legible over both very-bright (snow) and very-dark (near-black) frames using the
project's floating-control material recipe (scrim + Acrylic). It shares the OSC row with play/pause,
the seek bar, time, speed, subtitle/audio switchers and fullscreen, so its **resting footprint must
be small** and it should **not reflow the row** when it expands.

## The problem to solve

The current build ships only a minimal inline version (mute glyph + a thin 54px bar + a `%` chip).
It's ~1/3 of the way to a real design. Design the **complete** control. A promising direction to
explore (not a mandate): a **compact resting state that expands to full controls on hover/focus** —
but recommend what's most elegant and discoverable; justify the choice.

## Design these states

- **Resting** (playing, chrome visible): the smallest tasteful representation — what does the user
  see at a glance? Icon only, or icon + level?
- **Hover / active / keyboard-focus**: the expanded control with the full affordances below.
- **Muted**: unmistakable, and it must communicate that the **level is remembered** (un-mute returns
  to the prior level, not to 100%).
- **Boost above 100%**: volume ranges **0–130%**; above 100% is "boost" and reads **amber**
  (`#f0b840`). Show a **100% reference marker** so boost is legible as *beyond unity*, not just "more".
- Optional: a transient **OSD toast** treatment for volume changes when the OSC is hidden (shares the
  one toast style).

## Interactions to specify (behaviour + affordance)

- **Drag / click** the bar to set level; **scroll** to adjust, **Shift-scroll** for a fine step.
- **Click the readout to type an exact value** (e.g. `54.71%`, `132%`).
- **Mute toggle** (also the `M` key) — remembers the level.
- Define hover-in / hover-out and expand / collapse **motion** (durations + easing), animating
  **opacity *and* size/position** per the project's motion rules; it must hold frame cadence during
  4K playback.
- Mouse, trackpad-scroll, **and keyboard** paths; note focus-visual and hit-target sizing.

## Deliverables

1. The control in every state above (resting + expanded + muted + boost), at OSC scale, over both a
   bright and a dark frame.
2. An **interaction/redline spec**: sizes, spacing, the 0–130 mapping, the 100% marker, the amber
   boost ramp, type-to-set field, motion curves, and the exact tokens (colors, radii, durations)
   reusing the existing design system — no new primitives unless justified.
3. A short rationale for the resting↔expanded model you chose.

## Constraints

Windows 11 only. Restraint over decoration — no badges, no waveform, no skeuomorphism. It must feel
like part of the same OSC, not a bolt-on. Keep it quiet and elegant; the bar should disappear into
the design, not announce itself.
