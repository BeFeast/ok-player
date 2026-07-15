# Linux release acceptance

Linux release evidence is package-specific and has four levels:

1. `model-unit`: pure Rust model/schema tests.
2. `xvfb-render`: deterministic X11 rendering and scripted mpv interaction. This can prove geometry, pixels, playback state, screenshot file creation, and X11 fullscreen transitions.
3. `installed-package`: launch and version checks against the candidate `.deb` or AppImage.
4. `gnome-wayland-operator`: live GNOME/Wayland acceptance. Only this level may mark chooser, drag/drop, clipboard, portal, compositor, or focus rows `PASS`.

Generate deterministic media and captures:

```bash
./scripts/run-linux-acceptance-harness.sh ok-player artifacts/linux-acceptance
```

The runner generates media, captures all fixed states, writes `xvfb-rows.json`, and exits non-zero when any canonical redline fails. The required state names are defined by `okp_core::acceptance_evidence::REQUIRED_XVFB_STATES`. References use public logical IDs such as `windows-player-redlines`, `history-handoff`, and `about-handoff`; local source locations must never be written into evidence.

Generated fixtures include dark and bright 30-second H.264 media, chapter metadata, and a `natural-queue` folder containing `Episode 1`, `Episode 2`, and `Episode 10` plus a non-media file. Xvfb exercises direct file open, playback/duration, panel actions, screenshot file creation, and X11 fullscreen. The live GNOME folder-chooser row uses the generated queue and records its natural order; the headless run must not mark that chooser row `PASS`.

The encoded redlines include:

| Surface | Bounds / region contract |
|---|---|
| Player | `1120×680` |
| Narrow player | `480×540` |
| OSC | canonical bottom band with `16px` side and `18px` bottom insets |
| Playback states | paused, buffering/loading, in-canvas error, OSD, buffered timeline, and chapter-context captures at `1120x680` |
| Chapters / Up Next | `316px` panel, `24px` right/top inset, clear video strip to its left |
| Settings/About | `760×560`, `192px` rail |
| History | canvas state at the player viewport, not a mismatched standalone window |
| Playing idle | bottom chrome band fully clear after the canonical timeout |
| Bright/dark fixtures | actual frame luminance plus visible OSC material |

When canonical reference captures are available, name them identically to the implementation captures and create exact-size sheets:

```bash
./scripts/run-linux-acceptance-harness.sh ok-player artifacts/linux-acceptance references
```

Packaging writes `package-identity.json` and `acceptance-template.json`. Fill the template without changing its package identity. A publish run accepts the candidate workflow run ID plus the base64-encoded completed manifest, validates every required row, and publishes the exact artifacts from that candidate run. Rebuilding after operator acceptance is intentionally not allowed because it would change the package hash.

Merge deterministic rows before recording installed/live results:

```bash
./scripts/merge-linux-acceptance-evidence.sh acceptance-template.json xvfb-rows.json acceptance-manifest.json
```

Required live rows are:

- GNOME file chooser
- GNOME folder chooser
- Wayland drag/drop
- Wayland clipboard
- desktop portal behavior
- Wayland compositor fullscreen behavior
- keyboard focus navigation

Headless evidence must leave all of these `not-run`.
