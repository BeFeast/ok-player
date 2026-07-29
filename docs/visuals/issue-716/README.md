# Linux idle canvas at a narrow window

Issue #716 evidence: the idle Continue-watching surface cropped inside the window a portrait
clip fitted, and the same surface reflowing in that window with the pre-playback geometry
given back.

Every capture is an Xvfb session at **1366x768** — the smallest desktop in the Debian/Ubuntu
tester base, and the one on which the operator's report reproduces. A 9:16 clip fits into a
**381x679** window there (`okp-core` spends `WORK_AREA_FILL` 0.94 of the work area and
reserves `PLAYER_CHROME_RESERVE` 42 from its height), which is narrower than the idle canvas
could be laid out in. The history fixture is the same hostile one the shelf smoke uses,
built by [`scripts/make-linux-recents-fixture.py`](../../../scripts/make-linux-recents-fixture.py).

## Captures

| State | After leaving playback | Idle at 381px (the portrait fit) | Idle at 321px (one width below) |
|---|---|---|---|
| Before | [Window still 381x679, canvas cropped](gtk-idle-before-1366x768-portrait-fit-381px.png) | [Cropped](gtk-idle-before-381px.png) | [Cropped harder](gtk-idle-before-321px.png) |
| After | [Window back to 1120x680](gtk-idle-after-1366x768-geometry-restored.png) | [Reflowed](gtk-idle-after-381px.png) | [Reflowed](gtk-idle-after-321px.png) |

## Measured widths

Taken from the per-plane rectangles the interaction geometry diagnostic publishes under
`OKP_DEBUG_INTERACTIONS` (#690), which is also what
[`scripts/smoke-linux-idle-narrow-canvas.sh`](../../../scripts/smoke-linux-idle-narrow-canvas.sh)
asserts against.

| Measurement | Before | After |
|---|---|---|
| Idle canvas' own minimum width | 420px | 258px (one Continue-watching card plus the canvas gutter) |
| What set that minimum | the subtitle label: minimum 356px = its whole sentence, plus 2x32px padding | the shelf's fixed 194px card; every run of prose reflows below it |
| Canvas allocated in a 381px window | 420px — 39px of it outside the window | 381px |
| Canvas allocated in a 321px window | 420px — 99px outside | 321px |
| Window after leaving playback (idle 1120x680, then a 9:16 clip) | 381x679 | 1120x680 |
| Window after a 9:16 clip then a 16:9 clip | drifts with the last fit | 1120x680 |

## Verification scope

The captures prove the rendered idle canvas on a virtual X11 display at the geometry the
operator's report reproduces at, and the window geometry across two playback sessions. They
do not prove poster generation (the fixture supplies posters by file stem through
`OKP_POSTER_FIXTURE_DIR`), the History takeover, or the first-run welcome surface, which has
its own layout.
