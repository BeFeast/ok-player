# Linux release acceptance

Fedora has its own acceptance contract — stock vs RPM Fusion codecs, enforcing
SELinux with AVC collection, Flatpak/RPM/COPR states, and virtual-GPU skip
evidence — documented in [`fedora-acceptance.md`](fedora-acceptance.md). This
document covers the general Debian/AppImage evidence levels.

> **Candidate channel:** QA candidates for explicitly enrolled installs are published to the rolling
> `linux-candidate` pre-release, not to a permanent `linux-v*` Release (issue #339). A candidate is
> published from one exact native-builder bundle. The scheduled path marks a bundle `accepted` only
> after its required build gates pass; an operator may use manual dispatch to publish `pending` or
> `rejected` while completing additional evidence. The public feed is untouched throughout. See
> [linux-candidate-channel.md](linux-candidate-channel.md).

The installed public predecessor -> defective candidate -> fixed candidate N -> candidate N+1
gate, failure/recovery matrix, retained migration anchors, and machine-readable cleanup
authorization are defined in
[linux-candidate-upgrade-acceptance.md](linux-candidate-upgrade-acceptance.md). This is a live
GNOME/Wayland operator gate; Xvfb cannot mark it complete.

Linux release evidence is package-specific and has four levels:

1. `model-unit`: pure Rust model/schema tests.
2. `xvfb-render`: deterministic X11 rendering and scripted mpv interaction. This can prove geometry, pixels, playback state, screenshot file creation, and X11 fullscreen transitions.
3. `installed-package`: launch and version checks against the candidate `.deb` or AppImage.
4. `gnome-wayland-operator`: live GNOME/Wayland acceptance. Only this level may mark chooser, drag/drop, clipboard, portal, compositor, or focus rows `PASS`.

## Screenshot acceptance

Every candidate bundle includes `acceptance/deb-screenshot.png` plus
`acceptance/deb-screenshot.txt`. The builder extracts the exact Debian payload,
starts real local playback under Xvfb, invokes the shared Screenshot shortcut,
creates a previously missing default destination, requires a non-empty readable
image, and records its SHA-256. This is `xvfb-render` evidence only.

Before a screenshot regression is accepted on hardware, install that exact
candidate `.deb` and record all of the following from a real GNOME/Wayland
session:

- the exact path and SHA-256 of a saved frame while paused;
- the exact path and SHA-256 of a saved frame while playing;
- separate frame-only and with-subtitles captures from media with visible subtitles;
- a pasted Wayland clipboard image from Copy frame, with no retained screenshot file;
- creation of a missing default screenshot directory;
- an invalid or unwritable configured destination failing promptly with the destination and cause, and without a success message.

The saved images must be non-empty and decodable. Xvfb, package extraction, or
the presence of a toast cannot mark the Wayland clipboard or native compositor
rows as passing.

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

## Gapless playback capability

Linux deliberately reports gapless playback as **Deferred**. The GTK shell owns queue order and
waits for libmpv's end-of-file event before issuing a new `loadfile` command. libmpv's gapless-audio
mode only provides a safe basis when the engine owns the playlist transition, so enabling it on the
current path would claim continuity the player cannot guarantee and could disturb the existing
auto-advance, repeat, shuffle, and per-file restore lifecycle.

The capability decision and effective-preference gate are unit-tested in `okp-core`; the canonical
settings schema preserves the optional `playback.gapless` preference for a future engine-managed
path. Settings → Playback renders the current state as a disabled **Deferred / Unavailable** row,
and `scripts/smoke-linux-settings.sh <binary> <output> playback` verifies that the packaged shell
reports that state while rendering the page.

## Shift-locked interactive resize

Holding **Shift** while dragging any window edge or corner locks the current video/client aspect
ratio; releasing Shift returns to ordinary freeform resize and keeps the size reached so far
(issue #331). The geometry and the deterministic mid-drag Shift transitions are pure and unit-tested
in `okp_core::aspect_resize` (all edges/corners, landscape/portrait/square media, minimum-OSC and
workarea clamps, scale invariance, and Shift press/release mid-drag). The GTK shell keeps the
compositor-native `begin_resize` path and enforces the lock only through the toplevel `compute-size`
negotiation, so there is no per-motion `set_default_size` and no configure/resize feedback loop.

Continuous, non-jittering resize and a stable aspect ratio within a small rounding tolerance are a
`gnome-wayland-operator` row: they depend on the live Mutter build surfacing the compositor's
proposed size at `compute-size` time and cannot be proven under Xvfb. The operator confirms, in a
real GNOME/Wayland session, that a Shift-drag from every edge/corner follows the pointer smoothly,
holds the aspect, never snaps back or drifts off-screen, and that a normal (no-Shift) drag, initial
fit, fullscreen, maximize/restore, and window drag remain unaffected.

## Rarely-used video geometry

Aspect, zoom/pan, quarter-turn rotation, fill-screen crop, and deinterlace live only in the
player-wide right-click **Advanced commands → Video** group. The primary OSC and its curated More
popover must not contain geometry commands. Pan actions are available only after zooming above
100%; bounded zoom/pan actions and Reset disable at their current limits rather than becoming dead
commands.

`okp_core::video_geometry` unit tests prove action transitions, bounds, menu eligibility, and the
linear-to-libmpv zoom mapping. Shared-history tests prove local-file geometry round-trips through
`preferences.video_geometry`, survives progress saves, and normalizes before restore. Real-libmpv
tests prove the `video-zoom`, `video-pan-x/y`, `video-rotate`, `panscan`, and `deinterlace` command
path. `scripts/smoke-linux-context-menu.sh <binary> <output>` captures the `1280×900` context-menu
state and confirms the primary More popover remains a separate surface.

Xvfb can prove deterministic menu composition, disabled/selected rendering, scrolling, and the X11
command path. It does not prove GNOME/Wayland compositor placement, fractional scaling, or desktop
focus quality; those remain operator QA boundaries and do not change the geometry state contract.

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

Packaging writes `package-identity.json` and `acceptance-template.json`. The model-unit packaging
contract also runs the real Velopack CLI for both `linux-candidate` and public `linux` channels,
then verifies the generated feed, Full nupkg, standalone AppImage, and atomically staged versioned
AppImage identities. This proves package naming and byte identity, not installed desktop behavior.
Fill the template without changing its package identity. A publish run accepts the candidate
workflow run ID plus the base64-encoded completed manifest, validates every required row, and
publishes the exact artifacts from that candidate run. Rebuilding after operator acceptance is
intentionally not allowed because it would change the package hash.

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
