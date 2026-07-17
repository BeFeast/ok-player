# Issue #229 — HDR diagnostics reservation

This evidence covers the compact, non-interactive HDR handling state in Linux Settings and the
matching diagnostic shown for HDR media. The applicable art direction is the existing Settings
shell/material reference from issue #194 and the canonical Media Information references from issue
#263. Issue #229 intentionally overrides the prototype's aspirational passthrough value: Linux does
not claim that HDR passthrough or a user-selected tone-mapping mode is available.

## References and implementation captures

- [Settings shell reference](../issue-194/reference-settings-shell.png)
- [Canonical Media Information Streams](../issue-263/reference-streams-1120x680.png)
- [Canonical Media Information Stats](../issue-263/reference-stats-1120x680.png)
- [GTK Video Settings](gtk-settings-video.png)
- [GTK HDR Streams](gtk-media-info-streams.png)
- [GTK engine diagnostics](gtk-media-info-stats.png)

## Redline and behavior accounting

| Area | Contract | Implementation evidence |
|---|---|---|
| Geometry | Preserve the established `760px` Settings shell and `720px` Media Information modal at the `1120x680` player viewport. | The Settings capture remains `760px` wide. Media Information remains centered at `720px`; no new section, window, or control group changes the modal bounds. |
| Hierarchy | Later capabilities may reserve a compact state, while diagnostics and controls remain distinct. | Settings adds one status row after Hardware decode: label, explanatory copy, and an `Automatic` pill. It has no switch, button, click handler, or persisted preference. Media Information adds `HDR Handling — Automatic · engine-managed` beside source HDR metadata. |
| Spacing and type | Reuse existing Settings inset rows and Media Information two-column cards. | No new spacing, font, radius, or material tokens. The row reuses the `42px` inset/status grammar; diagnostics reuse the existing `12.5px` label and `12px` value styles. |
| Color/material | Preserve the current themed Settings surface and canonical light Media Information card over video. | No new colors or material layers. The status pill uses the existing Settings accent treatment; HDR source metadata keeps the existing diagnostic highlight. |
| Iconography | Do not imply a new HDR action or mode. | No icon, toggle, disclosure, shader, upscaler, passthrough, or tone-map selector is added. |
| Wording | Report what is known without promising display passthrough. | Source metadata says HDR when detected. Handling says `Automatic · engine-managed`. The raw mpv property is labeled `Engine Tone Mapping`, making it an engine diagnostic rather than a user control. Settings states that tone-mapping and passthrough controls are unavailable. |
| Playback behavior | Keep existing safe defaults. | The change performs no mpv property writes and adds no settings schema field. Hardware decode and video adjustments retain their existing behavior. |

## Verification limits

The Xvfb captures prove deterministic GTK composition, exact window/modal geometry, compact labels,
and separation between the Settings status and Media Information diagnostics. They do not prove
HDR display activation, compositor color management, passthrough, tone-mapping correctness, or live
GNOME/Wayland output behavior. Those capabilities remain unimplemented and are not represented as
working controls.
