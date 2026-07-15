# Issue 252: Linux Settings and About

The paired captures use a 760x560 viewport at 100% scale. About pairs show the canonical Claude Design reference on the left and the GTK implementation on the right. Representative Settings pairs show Light on the left and Auto resolving to dark on the right.

## Redline accounting

| Area | Canonical | GTK implementation |
|---|---:|---:|
| Window | 760x560 | 760x560, non-resizable |
| App-owned title strip | 42px | 42px including the 1px divider |
| Body | 760x518 | 760x518 |
| Navigation rail | 192px | 192px with 1px trailing stroke |
| Content pane | 568px | 568px |
| Content padding | 28 top / 44 right / 28 bottom / 24 left | Exact |
| Search | 171x30, radius 7 | Exact |
| Navigation row | 171x36, radius 7 | Exact; selected row uses a 3px teal inset |
| Illustration | 118x94 container, 116x90 art | Exact; five graduated ticks and fixed teal triangle |
| Identity gap | 22px | Exact |
| Cards | radius 8, padding 14x16, 12px group gap | Exact |
| Spec rows | 10px vertical rhythm, label/value split | Exact; technical values use the platform monospace fallback |
| Footer | 8px top margin, 17px top padding | Exact; copy action left, GitHub/License right |

## Semantic treatment

- Light uses `#F7F7F5` for the fused shell, a 1.5% black rail tint, white cards, and 6-9% neutral strokes.
- Auto-dark uses `#1F1F1F`, a 2% white rail tint, 3.5% white cards, and 7-9% neutral strokes.
- The illustration triangle remains the fixed brand gradient in both themes. Only frame ticks switch from light teal to dark teal.
- Accent is limited to selected navigation, focus/active controls, status tags, and links.
- High-contrast forces opaque black/white surfaces and strokes. All normal Settings surfaces are already opaque, so reduced-transparency operation does not depend on compositor blur.

## Controls and scrolling

- Appearance persists Light or Auto and updates the open Settings window immediately. Auto follows the GTK desktop dark preference.
- Every page remains inside the fixed-height vertical scroller. The About Host card/footer and longer Playback/Shortcuts content are reachable without changing window geometry.
- Update controls remain in Advanced -> Updates; About contains only App, Engine, and Host.

## Evidence

- `about-light-reference-implementation.png`
- `about-dark-reference-implementation.png`
- `appearance-light-auto-dark.png`
- `playback-light-auto-dark.png`
- `shortcuts-light-auto-dark.png`

The captures are deterministic X11/Xvfb evidence for composition, sizing, theme states, and scroll containment. They do not validate a live GNOME/Wayland compositor, desktop color-scheme notifications, focus routing, clipboard integration, or external URL launching. Those remain operator QA before release.
