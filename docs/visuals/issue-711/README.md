# Issue #711 — Settings/About opens whole and on one grid

Three defects the operator reported together on `0.11.0-beta.0.208`: the window opened at a
constant height and cut its own page off, the two hairline rules at the bottom of the two
columns sat a couple of dozen pixels apart, and the footer row declared no vertical
alignment. All captures are the light scheme on a 1920×1080 virtual display, taken with the
same harness the CI check uses.

## Captures

- [About as it opened, before](about-before-opened.png) — 760×560. The page needs 776 px and
  the desktop offers 1032, but the window takes the constant it was built with, so the
  reader arrives at a scrollbar on an almost empty screen.
- [About at the height the page wants, before](about-before-fit.png) — the same build,
  resized by hand to 776 px so both rules are visible at once. The rail rule sits at y=717
  and the page rule at y=694: near enough to read as a mistake, far enough to be one.
- [About as it opens, after](about-after.png) — 760×753, the height the page measures,
  inside a 1032 px work area. Nothing scrolls and both rules land on y=694.
- [Bottom band, before](footer-band-before.png) and [after](footer-band-after.png) — the same
  strip of both columns at 200 %.

## What the numbers say

Read off the `interaction: settings-geometry` record the window publishes under
`OKP_DEBUG_INTERACTIONS`, which is what `scripts/smoke-linux-settings-about.sh` subtracts:

| Measurement | Before | After |
|---|---|---|
| Window height on a 1080p desktop | 560 | 753 |
| Work area available | 1032 | 1032 |
| Page left out of view | 216 px | 0 |
| Rail rule y | 717 | 694 |
| Page rule y | 694 | 694 |
| Footer button / links centre | 730 / 730 | 723 / 723 |
| Footer left edge vs content column | 216 / 216 | 216 / 216 |

The two footer children already shared a centre before the change, but only because both
happened to be stretched to the same row height; nothing in the code said so, and the
negative control below shows how little it takes to lose it.

## Divergence from the Windows reference

`src/OkPlayer.App/SettingsWindow.xaml` keeps its About footer under the cards inside the
scroll viewer, so on Windows the rail rule and the footer rule are at plainly different
heights and never read as one line. This surface bottom-anchors the footer instead, which is
what makes the two rules share a baseline as the issue requires. Everything else follows the
reference: the same 760 px shell, the 192/568 columns, the 24/44 gutters, the 9 px rule inset
in the rail, and `VerticalAlignment="Center"` on every footer cell.

The Windows footer also carries a third cell — a copy-confirmation status between the button
and the links — that this shell reports through the status toast instead. That divergence is
older than this change and is left alone.

## Negative control

Every assertion in `scripts/smoke-linux-settings-about.sh` was checked by putting the defect
back and re-running it:

| Defect reintroduced | Reported failure |
|---|---|
| Window keeps its pre-content height | `About opened with 193px of the page out of view on a 1920x1080 screen` |
| About footer flows after the cards again | `the rail rule sits at y=717 and the page rule at y=694` |
| Footer children stop declaring an alignment | `the footer button centres on y=723 and its links on y=717` |
| Rules reported but painted transparent | `rail_rule: no rule is painted at y=694` |
| Shell stops noticing a size it did not ask for | `a page change resized a window sized by hand (height) from 620px to 1032px` |
| Shell watches only the height, so a sideways drag does not count | `a page change resized a window sized by hand (width) from 753px to 1956px` |

Two claims deliberately carry no assertion, because nothing in this harness can make them
fail: on a display smaller than the reference shell the window is held at 760x432 by GTK and
the window manager whatever the shell asks for, and a window dragged to a shorter monitor
needs a multi-head session this harness does not have. A gate that cannot fail is not a
gate.
