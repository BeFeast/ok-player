# Linux playback OSC at a narrow window

Issue #729 evidence: the control bar drawn wider than the window a portrait clip fitted
itself to, and the same bar reflowing inside it.

Captures are an Xvfb session at 1920x1080 playing the 9:16 fixture from
[`scripts/make-linux-portrait-fixture.py`](../../../scripts/make-linux-portrait-fixture.py),
paused so the chrome stays up, then resized. The two narrow widths are **381px** — the
window a 9:16 clip fits itself to on a 1366x768 laptop, the smallest desktop in the
Debian/Ubuntu tester base, derived exactly as in
[issue #716's evidence](../issue-716/README.md) — and **360px**, one width below it.

The bar's clipping threshold is a width, not a display: on this 1920x1080 screen the same
clip fits to 547px, which is above the threshold, so that capture is included only to show
the wide case is untouched.

## Captures

| State | 381px (the portrait fit on the smallest supported desktop) | 360px (one width below) | 547px (this display's portrait fit) |
|---|---|---|---|
| Before | [Volume cut in half, `…` off screen](gtk-osc-before-381px.png) | [Clipped harder](gtk-osc-before-360px.png) | unchanged |
| After | [Reflowed](gtk-osc-after-381px.png) | [Reflowed](gtk-osc-after-360px.png) | [Unchanged](gtk-osc-after-547px-portrait-fit.png) |

## Measured widths

Taken from the per-plane rectangles the interaction geometry diagnostic publishes under
`OKP_DEBUG_INTERACTIONS` (#690), which is also what
[`scripts/smoke-linux-osc-narrow-bar.sh`](../../../scripts/smoke-linux-osc-narrow-bar.sh)
asserts against.

| Measurement | Before | After |
|---|---|---|
| The bar's reported horizontal minimum | 422px | 300px |
| What set that minimum | the seek bar's own 144px floor, plus five 34px controls, five 16px gaps and 28px of pill inset | the seek bar's 72px floor, plus the same controls at 8px gaps and 16px of inset |
| Narrowest window the bar fits inside | 454px — wider than any portrait fit on a 1366x768 laptop | 332px |
| Pill in a 381px window | 424px wide, spanning 16..440 — 59px outside | 349px, spanning 16..365 |
| Volume in a 381px window | 341..375, against the window edge | 280..314 |
| `…` overflow entry in a 381px window | 391..425 — entirely outside, unreachable | 322..356 |
| Pill in a 360px window | 424px, spanning 16..440 — 80px outside | 328px, spanning 16..344 |
| Seek bar at 720px / 640px / 480px | 188px / 158px / 168px | unchanged |

The last row is the collapse order holding: the widths at which secondary controls fold are
unchanged, because the seek bar is still measured at its old 144px floor for as long as
anything else can fold instead.

## Verification scope

The captures prove the rendered OSC on a virtual X11 display at the widths listed. They do
not prove the popover surfaces, fullscreen chrome, or compact mode, which have their own
layouts, and they say nothing about a display scale other than 1.
