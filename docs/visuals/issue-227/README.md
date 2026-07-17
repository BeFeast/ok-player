# Issue #227 — ASS/SSA native-style boundary

This evidence compares the existing issue #194 subtitle quick-switcher baseline with the new
native-style and preset-applicable states at the same 262 px requested popover width. The
applicable art direction remains the PRD subtitle quick-switcher, the Windows Settings →
Subtitles note that ASS/SSA keeps built-in styling, and the issue #194 presentation geometry.

## Captures

- [ASS baseline before the applicability state](reference-ass-before.png)
- [GTK ASS native-style state](gtk-ass-native-style.png)
- [GTK SRT preset-applicable state](gtk-srt-preset-applies.png)

## Redline and behavior accounting

| Area | Contract | Implementation evidence |
|---|---|---|
| Geometry | Preserve the canonical 262 px content width and OSC anchor. | Both new captures measure 264 px including the two 1 px borders, with a 2 px anchor delta. SRT is 411 px high; the three-track ASS/PGS preview reaches the 522 px popover cap and remains scrollable, with the existing unavailable-search explanation retained. |
| Spacing | Keep the existing 34 px preference rows and 7 px horizontal label inset. | Delay, Size, Style, footer, dividers, and track rows retain their established spacing. The new applicability line uses the same 7 px inset and adds only the wrapped status height required by the state. |
| Type | Supporting state copy must stay below control labels in the existing compact type ramp. | Applicability copy is 10.5 px, regular weight, while the selected track and Style value keep their existing 11.5 px/semibold hierarchy. |
| Color/material | No new palette or material layer. | The invariant light popover material, live teal selection, tertiary supporting text, and existing disabled-control opacity are reused. |
| Iconography | Do not add a new rich-subtitle glyph. | The selected-track check and existing delay/size icons are unchanged; the format and state are communicated in text. |
| Track labels | ASS/SSA must be identifiable before selection. | Rich tracks include `ASS` or `SSA` plus `Native style`; external `.ssa` wins over mpv's commonly reported `ass` codec so it is not mislabeled. |
| Control states | Presets must not imply that they repaint authored ASS/SSA. | ASS/SSA shows the explicit sentence “OK Player preset is not applied” and a disabled `Native` Style value. SRT shows “preset applies” and retains the live `Default ›` cycle control. |
| Engine behavior | Preserve authored styling while retaining deliberate size/position behavior. | libmpv is pinned to `sub-ass-override=scale` and `secondary-sub-ass-override=scale`; authored fonts, colors, inline layout, and signs remain native while curated presets continue to write only the normal text-renderer fields. Raw configuration cannot override this boundary. |
| Fallbacks | No active, image-based, and unknown formats must degrade honestly. | Core applicability states map these cases to “select a track” or “unavailable” copy instead of claiming preset support. |

## Deterministic checks

```text
scripts/smoke-linux-track-popovers.sh <binary> <out>
CC=/usr/bin/cc cargo test -p okp-core subtitle_tracks
CC=/usr/bin/cc cargo test -p okp-mpv ass_override_boundary_preserves_authored_styles_in_both_slots
```

The popover smoke also waits for an actually visible window before clicking. This removes a
pre-existing Xvfb race where `xdotool search` could return an unmapped startup window.

## Evidence limits

Xvfb proves deterministic GTK composition, exact popover geometry, control sensitivity, and the
preview-state labels. It does not prove live GNOME/Wayland compositor behavior, portal flows,
focus integration, or rendering of a real authored ASS/SSA script. The real-libmpv unit test
proves the primary and secondary override modes; authored-script rendering remains mpv/libass's
native path.
