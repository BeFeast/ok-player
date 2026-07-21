# Issue #224 — reserved online subtitle search

This evidence compares the canonical design-system board's subtitle quick-switcher with the Linux
reserved implementation. The reference state has a selected text subtitle and the permanent Add,
Scribe, and online-search actions; the GTK state uses a selected external SRT so the existing local
cue-search action is also honestly available.

## Captures

- [Canonical subtitle switcher](reference-subtitle-switcher.png)
- [GTK reserved online-search state](gtk-online-reserved-popover.png)

## Redline and behavior accounting

| Area | Contract | Implementation evidence |
|---|---|---|
| Geometry | The canonical switcher is 288 px wide and places the reserved online row in the action block between the track list and quick adjustments. | GTK keeps the established 262 px requested width (264 px including borders), with a measured 0 px OSC-anchor delta. The 516 px height remains below the 520 px popover cap. Its additional height comes from already-shipped local cue search, Size, and preset-applicability rows that postdate the static board. |
| Hierarchy | Track selection first; file/search/generation actions next; quick subtitle adjustments and the Settings footer last. | The online row follows Add, local cue search, and Scribe, then a divider separates actions from Delay, Size, Style, and the footer. It remains present with or without subtitle tracks. |
| Spacing | The reference uses 8 px action padding, 10 px icon-to-label rhythm, 15 px glyphs, and compact 9 px badges. | GTK translates that into its established 32 px track rows with 5×7 px padding, 8 px row spacing, 16 px glyph boxes, and 8.5 px badges. Existing 5×4 px divider margins and quick-control spacing are unchanged. |
| Type | Action labels use the switcher's body ramp; state tags stay subordinate. | GTK retains 12 px action labels and 8.5 px semibold tags. The disabled online label and `SOON` tag remain visually quieter than the title, selected track, and Scribe action. |
| Color/material | One light elevated popover; teal is reserved for selection and Scribe, while unavailable online search is neutral and muted. | The existing light popover material is unchanged. Scribe uses the established teal treatment; the online row uses disabled text plus a neutral 6.5% black badge background. No new surface or accent color is introduced. |
| Iconography | Plus for local import, sparkle/Scribe identity for generation, magnifier for both local and online search. | The action rows use 16 px native-drawn plus and magnifier glyphs; the existing Scribe glyph remains. The online magnifier is muted with the disabled row. |
| Control states | The reserved build must show an honest unavailable state without implying that a provider call can run. Future policy states must distinguish disabled, private, no-media, provider-missing, implementation-unavailable, and available. | The shipped row is disabled, carries `SOON`, has no click handler, and exposes the full explanation as tooltip and accessible description. `okp-core` owns all six states; every non-available state rejects network authorization. |
| Result flow | A future downloaded SRT must enter the same external-subtitle load path as a local sidecar. | Local files and future online results both become `ExternalSubtitleImport`; GTK's existing `load_subtitle_path` now delegates to the shared `load_subtitle_import` path before calling libmpv. Provider/result provenance remains in the in-memory import model and does not trigger I/O. |

## Deterministic checks

```text
OKP_COMMAND_CAPTURE_SET=subtitles scripts/smoke-linux-track-popovers.sh <binary> <out>
CC=/usr/bin/cc cargo test -p okp-core online_subtitles
CC=/usr/bin/cc cargo test -p okp-core subtitle_import
```

The targeted Xvfb smoke measured `264×516`, a 0 px anchor delta, material mean `0.940217`, edge
mean `0.057133`, and preference-band standard deviation `0.102967`.

## Evidence limits

Xvfb proves deterministic GTK composition, the stable disabled row, its native popover geometry,
and the selected-external-SRT state. Network safety and future import routing are established by
pure model tests and the absence of a GTK click handler, not by a screenshot. This reserved action
does not open a chooser, portal, or network flow, so the capture does not claim live GNOME/Wayland
behavior for those unrelated facilities.
