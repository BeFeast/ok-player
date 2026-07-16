# Linux chapter interval fallback acceptance

Issue #201 evidence at the canonical 1120x680 playback viewport. The applicable shipped
Windows reference and established GTK panel geometry are documented in
[`../issue-251/README.md`](../issue-251/README.md).

## Captures

| State | Capture |
|---|---|
| Real metadata-less media, interval fallback ready | [Interval fallback](intervals-loaded-1120x680.png) |
| Detect chapters selected, no engine available | [Unavailable state](intervals-unavailable-1120x680.png) |

## Redline accounting

| Area | Contract and implementation |
|---|---|
| Viewport and panel | `1120x680`; existing `x=804`, `y=44`, `316x556` panel bounds, 12px leading corners, 1px hairline, and 80px OSC clearance are unchanged. |
| Hierarchy and spacing | The explicit scene-detection action stays at the initial scroll position. Interval markers follow in their own labeled section; bookmarks remain a separate user-authored section. Existing 8px list insets, 6px row radii, and 10px icon/content gaps are reused. |
| Type | Existing 11px semibold section labels and 12.5px/11px row title/time hierarchy are reused. Tabular time readouts remain aligned. |
| Color and material | The canonical light panel material is unchanged. Detect uses the established teal action tint; interval rows use a quieter teal wash and neutral dashed border so they read as derived rather than embedded metadata. |
| Iconography | Search identifies on-demand detection; compact seek glyphs identify jumpable interval markers; bookmark iconography and embedded thumbnail cards are unchanged. |
| Control states | Idle detection is actionable and keyboard/mouse reachable. Without an engine, activation produces the existing dark toast and replaces the action with an inline unavailable note; no fake progress is shown. |
| Behavior | Embedded chapters remain authoritative and suppress interval generation. Metadata-less finite media opens on Chapters with round interval markers; short/unknown-duration media keeps the honest empty state. Detection is explicit and never runs during initial playback. |

## Verification scope

The captures use real libmpv playback under Xvfb/X11 and prove observed duration, absent
embedded metadata, interval rendering, panel activation, and the no-engine detection outcome.
They do not prove installed GNOME/Wayland compositor behavior, portal dialogs, desktop
drag/drop, clipboard integration, or cross-application focus behavior.
