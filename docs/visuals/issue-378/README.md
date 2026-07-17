# Issue #378 — Linux companion-window acceptance

This evidence audits the Linux surfaces named by the issue against the shipped Windows composition,
the canonical Media Information reference from issue #263, the Settings shell reference from issue
#194, and the companion-window behavior in the compact-modes handoff.

## Surface classification

| Surface | Classification | Result |
|---|---|---|
| Media Information | Long-lived companion window | Non-modal, independently movable/resizable, one instance per player. |
| Settings, including About | Long-lived companion window | Uses the same construction, drag, resize, lifetime geometry, and owner cleanup path. |
| History | In-player idle-canvas takeover | Remains in the player by canonical design; it is not a companion window. |
| Chapter / bookmark editing | No long-lived GTK window exists | Current chapter and bookmark interactions remain in-player. |
| Open URL / Go to Time / subtitle search | Short-lived command dialogs | Remain modal. |
| Clear History and destructive confirmations | Confirmation dialogs | Remain modal. |
| File, folder, subtitle, and playlist choosers | Native file pickers | Remain modal. |

## References

- [Canonical Media Information — Streams](../issue-263/reference-streams-1120x680.png)
- [Canonical Media Information — Stats](../issue-263/reference-stats-1120x680.png)
- [Settings shell reference](../issue-194/reference-settings-shell.png)
- [Canonical History accounting](../issue-249/README.md)

## Full-window captures

| Surface / state | Capture |
|---|---|
| Media Information — natural `720×571`, Light | [Capture](media-info-natural-light.png) |
| Media Information — user-resized `838×659`, Light | [Capture](media-info-resized-light.png) |
| Media Information — natural, Auto-dark | [Capture](media-info-natural-dark.png) |
| Media Information — natural, High Contrast | [Capture](media-info-natural-high-contrast.png) |
| Settings — natural `760×560`, Light | [Capture](settings-natural-light.png) |
| Settings — user-resized `878×648`, Light | [Capture](settings-resized-light.png) |
| Settings — natural, Auto-dark | [Capture](settings-natural-dark.png) |
| Settings — natural, High Contrast | [Capture](settings-natural-high-contrast.png) |

## Redline and behavior accounting

| Area | Contract | Implementation evidence |
|---|---|---|
| Geometry | Preserve the canonical `720×571` Media Information and `760×560` Settings natural sizes; allow dense content to grow without clipping. | Natural captures are exact. Mapped south-east drags produce `838×659` and `878×648` windows, with scrollers/content expanding rather than a fixed card floating inside a larger shell. |
| Workarea | First/restored sizes stay inside the active monitor workarea. | Shared core geometry clamps natural or lifetime-restored sizes before mapping; the X11 smoke asserts every natural edge is inside the `1280×852` test workarea. Wayland leaves global placement to the compositor. |
| Window relationship | No modal grab, transient stacking, or forced above state. | X11 properties contain neither `_NET_WM_STATE_MODAL`, `_NET_WM_STATE_ABOVE`, nor `WM_TRANSIENT_FOR`. The player accepts play/pause, seek, volume, context-menu, drag, and fullscreen input while both companions are open. |
| Drag / resize | App-owned chrome moves the window; every edge/corner delegates resizing to the window manager. | The mapped smoke performs pointer drags through the title region and a real south-east resize gesture. Shared hit zones cover all eight `GdkSurfaceEdge` values and expose native resize cursors. |
| Single instance | Reopening raises the existing surface. | `I` and `Ctrl+,` are sent again while both windows are open; one window of each title remains and the runtime records the focus-existing path. |
| Lifetime geometry | A user-resized size survives close/reopen for the app lifetime. | The smoke closes and reopens each companion and asserts the exact resized dimensions are restored, clamped to the current workarea. No durable schema was introduced. |
| Ownership | Closing a companion does not stop playback; closing the player removes companions. | Parent interaction passes after individual companion closes. Closing the player exits the process and leaves no Media Information or Settings window. |
| Spacing / type | Preserve the established Media Information cards and Settings shell hierarchy. | Existing header, `280px` segmented control, card insets, two-column rows, footer, `192px` Settings rail, type ramp, and tabular diagnostics are unchanged. Resizing only adds usable content area. |
| Color / material | Keep themed surface material legible in Light, Auto-dark, and High Contrast. | The same content hierarchy is captured in all three modes. High Contrast replaces subtle card separation with explicit white borders; the custom chrome and resize cursors remain available. |
| Iconography / controls | Keep app identity and native window affordances; do not invent a new composition. | Media Information retains its app-owned identity, close, copy, and Done controls. Settings retains its existing minimize/maximize/close glyphs. |

## Automated interaction smoke

```text
scripts/smoke-linux-media-info.sh <binary> <output-directory>
```

The X11/Xvfb run proves mapped move/resize, non-modal window properties, parent input, fullscreen,
single-instance focus, lifetime geometry, owner cleanup, and deterministic Light/dark/high-contrast
rendering. Core and GTK unit tests separately pin the portable policy and ensure true command,
confirmation, subtitle-search, and file-picker dialogs remain modal.

## Verification limits

Xvfb proves package-equivalent X11 composition and actual WM gesture routing, not merely GTK
property values. It does not prove Mutter/Wayland placement, GNOME focus/raise behavior, compositor
shadows, portal/file-picker behavior, clipboard delivery, or live decoded playback continuity. A
GNOME Wayland operator must repeat move/resize on all edges/corners and the parent-interaction matrix
before removing the PR's WIP marker.
