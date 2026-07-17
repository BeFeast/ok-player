# Issue #194 — Linux subtitle presentation

This evidence covers the curated Linux subtitle presentation controls and their deterministic
rendering. The applicable art direction is the canonical design-system board's Subtitle
quick-switcher and three live-preview preset cards, plus the current Windows Settings → Subtitles
implementation. The available Settings reference capture shows the shared shell/material rather
than the Subtitles content state; the content geometry below is therefore accounted directly
against the Windows XAML and the canonical board.

## Captures

- [Settings shell reference](reference-settings-shell.png)
- [GTK Settings — light](gtk-settings-light.png)
- [GTK Settings — dark](gtk-settings-dark.png)
- [GTK subtitle quick-switcher](gtk-subtitle-popover.png)
- [GTK default subtitle](gtk-default-standard.png)
- [GTK Classic subtitle](gtk-classic-standard.png)
- [GTK boxed, large, raised subtitle](gtk-boxed-large-raised.png)
- [GTK narrow player](gtk-narrow.png)

## Redline and behavior accounting

| Area | Contract | Implementation evidence |
|---|---|---|
| Window geometry | Existing Settings shell: 760 px wide, 192 px rail, content pane with 24 px left / 44 px right padding. | Both GTK Settings captures remain exactly 760 px wide. The Presentation card occupies the 500 px inner content column without widening the window. |
| Hierarchy | Preset-only presentation; no raw property editor. Size, position, and style are separate curated choices. | Presentation is the first card: Size (Small / Normal / Large), Position (Standard / Raised), Style (Default / Bold / Classic / High contrast). Current-media delay and track selection remain separate below it. |
| Spacing | Existing Settings cards use 14×16 px padding, 34 px rows, 12 px page/card rhythm, 8 px radii. | Reuses the existing `okp-info-section`, Settings row, segmented-control, and page-padding rules. The four style choices use a compact 52 px minimum plus 6 px horizontal padding so the longest label fits the 500 px column. |
| Type | Existing Settings ramp: 12.5 px labels, 11.5 px supporting text, tabular numeric values. | Reuses the existing semantic label/value styles; the explanatory line is 11.5 px and wraps inside the card. |
| Color/material | Settings uses the existing light/dark themed surface and live accent; controls over video stay on the existing invariant dark OSC material. | No new palette values or material layers. Selected segments use the existing Settings selection treatment in light/dark. The quick-switcher keeps the established light popover material over both video substrates. |
| Iconography | No new subtitle icon family. Compact adjustment controls use the established minus/reset/plus glyph treatment. | The quick-switcher adds Size beside Delay with the same 26 px controls, then a compact Style cycle button and the canonical Settings footer. |
| Style behavior | Text-subtitle presets apply live. High contrast supplies a semi-transparent background box; ASS/SSA retains authored styling. | All presets write the same seven managed fields, so switching clears prior state. `Contrast` requests `background-box` with 72% black. The engine adapter uses the explicit property on mpv 0.39+ and mpv 0.37's equivalent implicit back-color behavior; real-libmpv tests cover both capability paths. |
| Size/position behavior | Global defaults persist; current-file size adjustments survive via per-file preferences. OSC visibility subtracts a transient lift from the configured baseline using `sub-pos`. | Default/Classic captions measure 387×22 px at global y=516. Large/Raised measures 543×32 px at global y=429: 40.3% wider, 45.5% taller, and 87 px higher. Its bottom is y=461, clear of the OSC band beginning near y=602. |
| Narrow behavior | Controls must not clip at reduced player width. | The existing 480×540 narrow smoke passes with the full OSC visible and no panel overlap. The subtitle popover remains independently pinned to 262 px content width. |

## Deterministic checks

```text
scripts/smoke-linux-settings.sh <binary> <out> subtitles light
scripts/smoke-linux-settings.sh <binary> <out> subtitles dark
scripts/smoke-linux-track-popovers.sh <binary> <out>
scripts/smoke-linux-subtitle-style.sh <binary> <out>
scripts/smoke-linux-narrow-width.sh <binary> <out>
```

The subtitle-style smoke renders a real embedded SRT through libmpv. It verifies the Classic
yellow fill by channel measurement, verifies Large/Raised geometry, and verifies OSC clearance.
The boxed-background option is additionally asserted through a real-libmpv property test.

## Evidence limits

Xvfb proves deterministic GTK composition, persisted settings input, libmpv subtitle pixels, and
measured layout. On this host the accelerated video substrate is not stable enough for a reliable
bright-footage background-box screenshot, so the committed boxed capture uses the black fallback
video plane; it proves size/position but not box contrast. Live GNOME/Wayland operator QA should
confirm the semi-transparent box over busy bright footage and the position transition while the
OSC reveals/hides. Xvfb also does not attest compositor timing, portal behavior, focus integration,
or other live-desktop behavior.
