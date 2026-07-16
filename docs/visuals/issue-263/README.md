# Linux Media Information visual acceptance

Issue #263 evidence renders the canonical root Main Player prototype and the GTK implementation at the same `1120x680` player viewport. The narrow acceptance state is an exact `480x540` GTK capture.

## Canonical references

| State | Capture |
|---|---|
| Streams | [Canonical Streams](reference-streams-1120x680.png) |
| Stats for nerds | [Canonical Stats](reference-stats-1120x680.png) |

## GTK implementation

| State | Capture |
|---|---|
| Streams over dark video | [GTK Streams dark](gtk-streams-dark-1120x680.png) |
| Stats for nerds | [GTK Stats](gtk-stats-dark-1120x680.png) |
| Long title, path, and metadata | [GTK long metadata](gtk-long-metadata-1120x680.png) |
| Missing path, tracks, and diagnostics | [GTK missing fields](gtk-missing-fields-1120x680.png) |
| Streams scrolled to the bottom | [GTK scroll bottom](gtk-scroll-bottom-1120x680.png) |
| Narrow player | [GTK narrow](gtk-narrow-480x540.png) |
| Streams over bright video | [GTK Streams bright](gtk-streams-bright-1120x680.png) |

## Exact redline

| Area | Canonical contract | GTK accounting |
|---|---:|---|
| Player viewport | `1120x680` | Exact in all desktop captures. |
| Modal width | `720px`, maximum `92%` | `720px` at desktop; `441px` at a `480px` player width. |
| Modal height | maximum `84%` | Streams outer height `571px` at desktop and `453px` narrow; Stats and missing-data states shrink below the maximum. |
| Placement | Centered over the player canvas | Desktop `x=200`, `y=54`; narrow `x=19/20`, `y=43`. No second top-level window. |
| Scrim | `rgba(0,0,0,.50)` plus `3px` backdrop blur | Exact alpha scrim. GTK4 CSS has no portable backdrop-filter, so Linux uses the explicit solid-alpha fallback. |
| Surface | `#f7f7f5`, `11px` radius, 1px 8% black border, deep shadow | Exact material, radius, hairline, and elevation family. |
| Header | `17/20/15px` padding; `38px` identity; `13px` gap; `17px` title; `30px` close target | Exact geometry and hierarchy. The identity mark uses deterministic app-owned Cairo geometry; functional close and copy actions retain native symbolic icons. |
| Tabs | `280px` segmented control; `3px` inset; `8px` outer and `6px` selected radii | Exact. Streams and Stats remain keyboard reachable and preserve visible focus. |
| Body | `16/20/20px` inset; cards separated by `12px` | Exact. The scroller is bounded by the `84%` modal ceiling. |
| Cards | `13/16px` padding; `8px` radius; two columns; `28px` column gap; `9px` row gap | Exact desktop composition. Narrow mode keeps the same two-column surface and wraps values instead of inventing another layout. |
| Track rows | `9/11px` padding; `7px` radius; selected teal tint and status tag | Exact geometry and state hierarchy for audio, subtitles, default, active, and external tracks. |
| Footer | `12/20px` padding; path left; Copy all and Done right | Exact hierarchy. Long paths ellipsize without moving the actions. |
| Behavior | Escape/backdrop close, focus trap/return, keyboard tabs, copy, no playback interruption | Implemented on the player overlay. The player shortcut controller yields while the modal owns focus. |
| Window ownership | Modal remains inside the player | Smoke asserts one visible `OK Player` window and rejects the old `680x820` transient geometry. |

## Data accounting

The GTK shell reuses the existing observed `MediaInfo` snapshot. Stream sections exclude only the duplicated full path, which remains in the footer. Existing `Playback` diagnostics are presented as Decode/Render, Live/Performance, and Display/Output cards; no new mpv reads or portable parsing were added. The curated More menu and the video right-click Advanced commands menu both route through the same in-player modal entry point.

## Verification limits

The Xvfb captures prove deterministic rendering, exact player/modal geometry, responsive clamping, tab state, scrolling, missing/long data, bright/dark scrim contrast, and single-window composition. They do not prove compositor blur, real GNOME/Wayland focus return, clipboard delivery, backdrop pointer routing, or uninterrupted decoded playback on a live desktop. Those remain operator acceptance items, so the pull request stays a deliberate `WIP:` draft.
