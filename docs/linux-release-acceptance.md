# Linux release acceptance

Linux release evidence is package-specific and has four levels:

1. `model-unit`: pure Rust model/schema tests.
2. `xvfb-render`: deterministic X11 rendering and scripted mpv interaction. This can prove geometry, pixels, playback state, screenshot file creation, and X11 fullscreen transitions.
3. `installed-package`: launch and version checks against the candidate `.deb` or AppImage.
4. `gnome-wayland-operator`: live GNOME/Wayland acceptance. Only this level may mark chooser, drag/drop, clipboard, portal, compositor, or focus rows `PASS`.

Issue-specific 4K60 presentation evidence is also operator-only. Run
`scripts/run-linux-acceptance-harness.sh --wayland-presentation <binary> <fixture> <output>` inside
the target GNOME Wayland session. It rejects X11/Xvfb, verifies the fixture is exactly 3840×2160
HEVC Main10 `yuv420p10le` at 60/1, and records both the native Wayland/EGL plane and the retained
`GtkGLArea` A/B path. Native acceptance requires at least 55 completed EGL swaps per second over a
continuous 15-second post-warmup window, `hwdec-current=vaapi`, zero decoder-drop growth, no
sustained VO-drop growth, and a 1x playback clock within five percent. The generated private logs
and comparison JSON are evidence artifacts, not repository fixtures.

Generate deterministic media and captures:

```bash
./scripts/run-linux-acceptance-harness.sh ok-player artifacts/linux-acceptance
```

The runner generates media, captures all fixed states, writes `xvfb-rows.json`, and exits non-zero when any canonical redline fails. First-run evidence comes from the canonical dark empty-state suite, which also verifies the light variant; the obsolete pre-recovery main-window smoke is not part of the release harness. The required state names are defined by `okp_core::acceptance_evidence::REQUIRED_XVFB_STATES`. References use public logical IDs such as `windows-player-redlines`, `history-handoff`, and `about-handoff`; local source locations must never be written into evidence.

Generated fixtures include dark and moving-bright 30-second H.264 media, a 60-second buffered-playback source, chapter metadata, and a `natural-queue` folder containing `Episode 1`, `Episode 2`, and `Episode 10` plus a non-media file. The playback harness serves media from localhost to induce a delayed real load, a throttled partial demuxer cache, and a real HTTP 404/retry path. Xvfb also exercises direct file open, playback/duration, panel actions, screenshot file creation, X11 fullscreen, and the EWMH Always-on-top state. The live GNOME folder-chooser row uses the generated queue and records its natural order; the headless run must not mark that chooser row `PASS`.

The playback harness also launches the binary with `--resume 12` and requires the explicit one-shot
seek to be accepted by libmpv. This proves process-argument parsing and seek dispatch in the packaged
shell; model-unit tests prove explicit-over-remembered precedence, zero/near-end handling, watched
thresholds, and private-session report suppression. It does not prove a future companion IPC
transport, because the MVP report sink is deliberately local and no-op.

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
| Fullscreen | real `1280x900` X11 transition with titlebar and OSC fully clear at idle |
| Always on top | selected pin plus actual `_NET_WM_STATE_ABOVE`; unsupported Wayland result remains operator-only |

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
