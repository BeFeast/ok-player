# Issue 331 — Shift-resize accounting

This change does not alter the player composition, CSS, icons, typography, or chrome geometry. It
changes only how the existing invisible edge/corner resize zones translate pointer motion into
window geometry.

| Contract area | Reference | Implementation accounting |
|---|---|---|
| Geometry | Current player proportions; compact handoff uses the real media ratio and a usable shorter-edge floor | Shift captures the client ratio at the moment it engages. All eight existing zones use signed logical pointer deltas; straight edges lead on their dragged axis and corners use the dominant fractional delta. |
| Spacing | Existing approximately 8px invisible edge/corner interaction zones | No CSS or hit-zone dimensions change. The current 6px edges and 16px corner overlays remain intact. |
| Type, color, material, iconography | Current Windows/GTK player chrome | Unchanged; resize adds no visible ornament or invented state. Existing native resize cursors remain the only affordance. |
| Control states | Shift held = locked; Shift released = freeform | Aggregate modifier-mask tracking keeps the lock active when either left or right Shift remains down. A transition rebases at the size already reached, so the next motion cannot snap to the drag origin. |
| Bounds | Monitor workarea, usable OSC floor, logical scaling | The compositor-reported logical workarea is the ceiling; `320×180` remains the standard-player floor, with the workarea winning on conflict. Rounding is performed once in logical pixels and tightened after projection so the integer result cannot cross the ceiling. |
| Behavior | Smooth live lock, final-size retention, no configure oscillation | Each changed pointer result emits one size request. Configure events never correct or re-enter the geometry state. X11 applies the opposite-edge position delta; Wayland uses stable size-only anchoring because clients cannot position normal toplevels. |

## Evidence and remaining operator gate

Pure tests cover every edge/corner, landscape/portrait/square ratios, inward and outward motion,
minimum/workarea conflict, post-rounding ceiling safety, fractional logical offsets, and mid-drag
Shift transitions. `scripts/smoke-linux-shift-resize.sh` exercises a mapped X11 window and records
monotonic samples plus lock/release results.

Neither that smoke nor an Xvfb screenshot can prove real GNOME/Wayland pointer feel, Mutter
placement, focus/modifier delivery, fractional scaling, or compositor workarea policy. The PR must
remain a deliberate work in progress until the installed candidate is exercised on real
GNOME/Wayland at the same player state and the operator records all-edge/corner traces with aspect
error at or below `0.01`, no reversal/snap-back, and retained final sizes.
