# Linux Continue-watching shelf layout

Issue #702 evidence: the welcome shelf sized by its text, and the same shelf laid out on a
grid. Both captures render the same fixture history — a very long title carrying emoji and
hashtags, a short title, a deep path with a long file name, and one item with no poster —
built by [`scripts/make-linux-recents-fixture.py`](../../../scripts/make-linux-recents-fixture.py).

## Captures

| State | Xvfb / X11, 1120x680 | Installed GNOME/Wayland, 1.5x scale |
|---|---|---|
| Before | [Shelf sized by its text](gtk-recents-shelf-before-1120x680.png) | [Operator's case](gtk-recents-shelf-before-gnome-wayland.png) |
| After | [Shelf on a grid](gtk-recents-shelf-after-1120x680.png) | [Operator's case, fixed](gtk-recents-shelf-after-gnome-wayland.png) |

## Measured card rectangles

Taken from the per-plane rectangles the interaction geometry diagnostic publishes under
`OKP_DEBUG_INTERACTIONS` (#690), which is also what
[`scripts/smoke-linux-recents-shelf.sh`](../../../scripts/smoke-linux-recents-shelf.sh)
asserts against.

| Surface | Before | After |
|---|---|---|
| Xvfb 1120x680 | 1056 / 1056 / 1056 px on three rows; History affordance 1036px wide, below the viewport | 194 / 194 / 194 px on one row at y=145; History affordance 36x36 |
| Xvfb narrowed to 700 | 636 / 636 / 636 px on three rows | 194 / 194 / 194 px, still one row |
| GNOME/Wayland 1280x691 | 904 / 298 / 904 px across two rows; History affordance 278x43 | 194 / 194 / 194 px on one row at y=154; History affordance 36x36 |

## Verification scope

The captures prove the rendered shelf geometry on a virtual X11 display and on an installed
GNOME/Wayland session with fractional scaling. They do not prove poster generation (the
fixture supplies posters by file stem through `OKP_POSTER_FIXTURE_DIR`), playback, or the
full History surface, whose rows are a separate layout.
